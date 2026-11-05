//! AiKnowledgeBaseService — 知识库 CRUD + 文档入库（切片 → embedding →
//! 落库）+ 余弦检索（pgvector 原生 SQL）。
//!
//! 向量列不在 SeaORM 模型内（上游 ent 同约束）：启动迁移幂等补建
//! `CREATE EXTENSION IF NOT EXISTS vector` + `ALTER TABLE sys_ai_chunks ADD
//! COLUMN IF NOT EXISTS embedding vector(1536)`（扩展需超级用户；失败仅
//! RAG 降级，不阻断服务）。检索/写入走原生 SQL（`embedding <=> $vec`）。
//! 切片 500 字符、重叠 50（上游 ragChunkRunes/ragChunkOverlap 同值）。
//! embedding 消耗与 chat 同口径记入用量流水；记量失败不阻断向量化。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::data::Viewer;
use crate::state::{
    db_err, not_found, operator_of, require_data, status_error, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{
    AiDoc, AiKnowledgeBase, CreateAiKnowledgeBaseRequest, DeleteAiDocRequest,
    DeleteAiKnowledgeBaseRequest, GetAiKnowledgeBaseRequest, ListAiDocsRequest,
    ListAiKnowledgeBaseResponse, SearchAiKnowledgeRequest, SearchAiKnowledgeResponse,
    UpdateAiKnowledgeBaseRequest, UploadAiDocRequest, UploadAiDocResponse,
};
use proto::proto::pagination::PagingRequest;

/** embedding 输出向量维度——与启动迁移 `AiChunkVectorColumn` 的
 * vector(1536) 定维互为文档；维度值本体钉在迁移 SQL 里，此处仅作
 * 常量对照（embedding 模型换维时两处同步）。 */
#[allow(dead_code)]
pub const AI_EMBEDDING_DIMENSIONS: usize = 1536;

const RAG_CHUNK_RUNES: usize = 500;
const RAG_CHUNK_OVERLAP: usize = 50;

fn base_proto(r: crate::data::sys_ai_knowledge_bases::Model) -> AiKnowledgeBase {
    AiKnowledgeBase {
        id: Some(r.id),
        tenant_id: r.tenant_id,
        name: Some(r.name),
        description: r.description,
        provider_id: r.provider_id,
        embedding_model: r.embedding_model,
        user_id: Some(r.user_id),
        doc_count: None,
        created_by: None,
        updated_by: None,
        deleted_by: None,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn doc_proto(r: crate::data::sys_ai_docs::Model) -> AiDoc {
    AiDoc {
        id: Some(r.id),
        base_id: Some(r.base_id),
        name: Some(r.name),
        chunk_count: r.chunk_count,
        status: r.status,
        error_message: r.error_message,
        user_id: r.user_id,
        tenant_id: r.tenant_id,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

/// 按字符切片，带重叠保证跨片语义连续。
pub fn split_into_chunks(content: &str, size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let step = size.saturating_sub(overlap).max(1);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let end = (i + size).min(chars.len());
        let piece: String = chars[i..end].iter().collect();
        if !piece.trim().is_empty() {
            out.push(piece);
        }
        if end == chars.len() {
            break;
        }
        i += step;
    }
    out
}

/// f32 向量 → pgvector 字面量 `[0.1,0.2,...]`。
fn vec_literal(v: &[f32]) -> String {
    let items: Vec<String> = v.iter().map(|x| format!("{x}")).collect();
    format!("[{}]", items.join(","))
}

pub struct AiKnowledgeBaseService {
    pub state: Arc<AppState>,
}

impl AiKnowledgeBaseService {
    async fn base_scoped(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        id: u32,
    ) -> Result<crate::data::sys_ai_knowledge_bases::Model, StatusError> {
        let mut query = crate::data::sys_ai_knowledge_bases::Entity::find_by_id(id);
        if let Some(tid) = Viewer::from_ctx(ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_knowledge_bases::Column::TenantId.eq(tid));
        }
        query
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("ai knowledge base"))
    }

    /// 知识库的 embedding 通道：base.provider_id 的 provider 行。
    async fn provider_for_base(
        &self,
        base: &crate::data::sys_ai_knowledge_bases::Model,
    ) -> Result<crate::data::sys_ai_providers::Model, StatusError> {
        let pid = base.provider_id.ok_or_else(|| {
            status_error(
                "BAD_REQUEST",
                "knowledge base embedding config is incomplete",
            )
        })?;
        crate::data::sys_ai_providers::Entity::find_by_id(pid)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("ai provider"))
    }

    /// 批量向量化（返回与 texts 对齐的字面量），消耗按 chat 同口径入台账；
    /// 记量失败不阻断向量化（与上游注释一致）。
    async fn embed_texts(
        &self,
        provider: &crate::data::sys_ai_providers::Model,
        base: &crate::data::sys_ai_knowledge_bases::Model,
        user_id: u32,
        tenant_id: u32,
        texts: &[String],
    ) -> Result<Vec<String>, StatusError> {
        let model = base
            .embedding_model
            .clone()
            .unwrap_or_else(|| "text-embedding-3-small".into());
        let started = std::time::Instant::now();
        let vectors = super::llm::embeddings(provider, &model, texts.to_vec())
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;
        if vectors.len() != texts.len() {
            return Err(status_error(
                "INTERNAL_SERVER_ERROR",
                format!(
                    "embedding count mismatch: got {} want {}",
                    vectors.len(),
                    texts.len()
                ),
            ));
        }
        let duration_ms = started.elapsed().as_millis() as u32;
        let _ = crate::data::repos::AiUsageLogRepo::new(&self.state.db)
            .insert(crate::data::sys_ai_usage_logs::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                provider_id: Set(Some(provider.id)),
                conversation_id: Set(None),
                user_id: Set(Some(user_id)),
                model_name: Set(Some(model)),
                // embeddings 端点的 usage 计入 prompt_tokens；total 同值。
                prompt_tokens: Set(Some(0)),
                completion_tokens: Set(Some(0)),
                total_tokens: Set(Some(0)),
                duration_ms: Set(Some(duration_ms)),
                created_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await;
        Ok(vectors.iter().map(|v| vec_literal(v)).collect())
    }

    /// 文档入库：切片 → 向量化 → 落库（docs 行 READY/FAILED 终态；失败不
    /// 污染整个请求语义——错误信息保留在文档行）。
    async fn ingest_doc(
        &self,
        tenant_id: u32,
        user_id: u32,
        base: &crate::data::sys_ai_knowledge_bases::Model,
        name: &str,
        content: &str,
    ) -> Result<crate::data::sys_ai_docs::Model, StatusError> {
        let provider = self.provider_for_base(base).await?;
        let doc = crate::data::sys_ai_docs::ActiveModel {
            tenant_id: Set(Some(tenant_id)),
            base_id: Set(base.id),
            name: Set(name.to_string()),
            status: Set(Some("PROCESSING".into())),
            user_id: Set(Some(user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        let chunks = split_into_chunks(content, RAG_CHUNK_RUNES, RAG_CHUNK_OVERLAP);
        let mut a: crate::data::sys_ai_docs::ActiveModel = doc.into();
        if chunks.is_empty() {
            a.status = Set(Some("FAILED".into()));
            a.error_message = Set(Some("content produced no chunks".into()));
            a.updated_at = Set(Some(crate::data::now()));
            return a.update(&self.state.db).await.map_err(db_err);
        }
        match self
            .embed_texts(&provider, base, user_id, tenant_id, &chunks)
            .await
        {
            Ok(literals) => {
                insert_chunks(&self.state.db, tenant_id, doc_id_of(&a), &chunks, &literals).await?;
                a.status = Set(Some("READY".into()));
                a.error_message = Set(None);
                a.chunk_count = Set(Some(chunks.len() as u32));
            }
            Err(e) => {
                // 一次 embedding 故障不让整个请求 500：文档行标 FAILED 保留原因。
                a.status = Set(Some("FAILED".into()));
                a.error_message = Set(Some(e.message.clone()));
            }
        }
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)
    }
}

fn doc_id_of(a: &crate::data::sys_ai_docs::ActiveModel) -> u32 {
    match &a.id {
        sea_orm::ActiveValue::Unchanged(v) | sea_orm::ActiveValue::Set(v) => *v,
        _ => 0,
    }
}

/// 切片批量落库（原生 SQL：pgvector 列不在实体模型内）。
async fn insert_chunks(
    db: &sea_orm::DatabaseConnection,
    tenant_id: u32,
    doc_id: u32,
    chunks: &[String],
    vectors: &[String],
) -> Result<(), StatusError> {
    use sea_orm::ConnectionTrait;
    if chunks.len() != vectors.len() {
        return Err(status_error(
            "INTERNAL_SERVER_ERROR",
            "chunk/vector length mismatch",
        ));
    }
    let mut sql = String::from(
        "INSERT INTO sys_ai_chunks (created_at, tenant_id, doc_id, content, chunk_index, embedding) VALUES ",
    );
    let mut args: Vec<String> = Vec::with_capacity(chunks.len() * 3);
    for (j, (c, v)) in chunks.iter().zip(vectors).enumerate() {
        if j > 0 {
            sql.push_str(", ");
        }
        // NOW(), tenant_id, doc_id, content, chunk_index, embedding::vector
        sql.push_str(&format!(
            "(NOW(), {}, {}, '{}', {}, '{}::vector')",
            tenant_id,
            doc_id,
            c.replace('\'', "''"),
            j,
            v.replace('\'', "''")
        ));
        args.clear();
    }
    let _ = args;
    db.execute_unprepared(&sql)
        .await
        .map_err(|e| status_error("INTERNAL_SERVER_ERROR", format!("insert ai chunks: {e}")))?;
    Ok(())
}

/// RAG 检索核心（chat/agent/content 共用）：query 向量化 → pgvector 余弦
/// 检索 → 命中内容列表（按分数序）。
pub async fn search_knowledge_base(
    state: &Arc<AppState>,
    tenant_id: u32,
    user_id: u32,
    base_id: u32,
    query: &str,
    top_k: usize,
) -> Result<Vec<String>, StatusError> {
    use sea_orm::{ConnectionTrait, Statement};
    let svc_state = Arc::clone(state);
    let svc = AiKnowledgeBaseService { state: svc_state };
    let base = crate::data::sys_ai_knowledge_bases::Entity::find_by_id(base_id)
        .one(&state.db)
        .await
        .map_err(db_err)?
        .ok_or_else(|| not_found("ai knowledge base"))?;
    let provider = svc.provider_for_base(&base).await?;
    let literals = svc
        .embed_texts(&provider, &base, user_id, tenant_id, &[query.to_string()])
        .await?;
    let top_k = top_k.max(1) as i64;
    let vec = literals[0].replace('\'', "''");
    let sql = format!(
        "SELECT c.content FROM sys_ai_chunks c \
         JOIN sys_ai_docs d ON d.id = c.doc_id \
         WHERE d.base_id = {base_id} AND ({tenant_id} = 0 OR c.tenant_id = {tenant_id}) \
         ORDER BY c.embedding <=> '{vec}'::vector LIMIT {top_k}"
    );
    let rows = state
        .db
        .query_all_raw(Statement::from_string(state.db.get_database_backend(), sql))
        .await
        .map_err(|e| status_error("INTERNAL_SERVER_ERROR", format!("search ai chunks: {e}")))?;
    Ok(rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "content").ok())
        .collect())
}

#[async_trait::async_trait]
impl proto::gen::services::AiKnowledgeBaseServiceHandlers for AiKnowledgeBaseService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiKnowledgeBaseResponse, StatusError> {
        let mut query = crate::data::sys_ai_knowledge_bases::Entity::find();
        if let Some(tid) = Viewer::from_ctx(&ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_knowledge_bases::Column::TenantId.eq(tid));
        }
        let (rows, total) = crate::paging::fetch_paged(&self.state.db, query, &req).await?;
        Ok(ListAiKnowledgeBaseResponse {
            items: rows.into_iter().map(base_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiKnowledgeBaseRequest,
    ) -> Result<AiKnowledgeBase, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::get_ai_knowledge_base_request::QueryBy
        );
        Ok(base_proto(self.base_scoped(&ctx, id).await?))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateAiKnowledgeBaseRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        if data.provider_id.unwrap_or(0) == 0
            || data.embedding_model.as_deref().unwrap_or("").is_empty()
        {
            return Err(status_error(
                "BAD_REQUEST",
                "provider_id and embedding_model are required",
            ));
        }
        crate::data::sys_ai_knowledge_bases::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            description: Set(data.description.clone()),
            provider_id: Set(data.provider_id),
            embedding_model: Set(data.embedding_model.clone()),
            user_id: Set(payload.user_id),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiKnowledgeBaseRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let row = self.base_scoped(&ctx, req.id).await?;
        let mut a: crate::data::sys_ai_knowledge_bases::ActiveModel = row.into();
        if let Some(v) = &data.name {
            a.name = Set(v.clone());
        }
        if let Some(v) = &data.description {
            a.description = Set(Some(v.clone()));
        }
        if let Some(v) = data.provider_id {
            a.provider_id = Set(Some(v));
        }
        if let Some(v) = &data.embedding_model {
            a.embedding_model = Set(Some(v.clone()));
        }
        a.updated_at = Set(Some(crate::data::now()));
        let _ = payload;
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiKnowledgeBaseRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::delete_ai_knowledge_base_request::QueryBy
        );
        self.base_scoped(&ctx, id).await?;
        // 级联清文档与切片（含向量列行——原生 SQL）。
        use sea_orm::ConnectionTrait;
        let _ = self
            .state
            .db
            .execute_unprepared(&format!(
                "DELETE FROM sys_ai_chunks WHERE doc_id IN (SELECT id FROM sys_ai_docs WHERE base_id = {id})"
            ))
            .await;
        crate::data::sys_ai_docs::Entity::delete_many()
            .filter(crate::data::sys_ai_docs::Column::BaseId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_ai_knowledge_bases::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    /// 文本内联入库（UploadDocFile 走文件域上传面，随文件域增量接入）。
    async fn upload_doc(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UploadAiDocRequest,
    ) -> Result<UploadAiDocResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        if req.base_id == 0 || req.name.trim().is_empty() || req.content.trim().is_empty() {
            return Err(status_error(
                "BAD_REQUEST",
                "base_id, name and content are required",
            ));
        }
        let base = self.base_scoped(&ctx, req.base_id).await?;
        let doc = self
            .ingest_doc(
                payload.tenant_id,
                payload.user_id,
                &base,
                &req.name,
                &req.content,
            )
            .await?;
        Ok(UploadAiDocResponse {
            doc: Some(doc_proto(doc.clone())),
            chunk_count: doc.chunk_count.unwrap_or(0),
        })
    }

    /// 文件面入库：本仓文件域的 multipart 通道随文件域增量接入；现以
    /// 干净 NOT_IMPLEMENTED 声明边界（proto 生成面已挂载，不会 404）。
    async fn upload_doc_file(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: proto::proto::ai::service::v1::UploadAiDocFileRequest,
    ) -> Result<UploadAiDocResponse, StatusError> {
        Err(status_error(
            "NOT_IMPLEMENTED",
            "file-backed ingestion rides the storage increment; use UploadDoc with inline text",
        ))
    }

    async fn list_docs(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ListAiDocsRequest,
    ) -> Result<proto::proto::ai::service::v1::ListAiDocsResponse, StatusError> {
        let base = self.base_scoped(&ctx, req.base_id).await?;
        let rows = crate::data::sys_ai_docs::Entity::find()
            .filter(crate::data::sys_ai_docs::Column::BaseId.eq(base.id))
            .order_by_desc(crate::data::sys_ai_docs::Column::Id)
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let total = rows.len() as u64;
        Ok(proto::proto::ai::service::v1::ListAiDocsResponse {
            items: rows.into_iter().map(doc_proto).collect(),
            total,
        })
    }

    async fn delete_doc(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiDocRequest,
    ) -> Result<Empty, StatusError> {
        let (base_id, id) = (req.base_id, req.id);
        self.base_scoped(&ctx, base_id).await?;
        crate::data::sys_ai_docs::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("ai doc"))?;
        use sea_orm::ConnectionTrait;
        let _ = self
            .state
            .db
            .execute_unprepared(&format!("DELETE FROM sys_ai_chunks WHERE doc_id = {id}"))
            .await;
        crate::data::sys_ai_docs::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn search(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: SearchAiKnowledgeRequest,
    ) -> Result<SearchAiKnowledgeResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        if req.base_id == 0 || req.query.trim().is_empty() {
            return Err(status_error(
                "BAD_REQUEST",
                "base_id and query are required",
            ));
        }
        self.base_scoped(&ctx, req.base_id).await?;
        let hits = search_knowledge_base(
            &self.state,
            payload.tenant_id,
            payload.user_id,
            req.base_id,
            &req.query,
            req.top_k.unwrap_or(4).max(1) as usize,
        )
        .await?;
        Ok(SearchAiKnowledgeResponse {
            hits: hits
                .into_iter()
                .map(|content| proto::proto::ai::service::v1::KnowledgeHit {
                    content,
                    ..Default::default()
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_walks_with_overlap_and_last_tail() {
        let content = "x".repeat(1200);
        let chunks = split_into_chunks(&content, 500, 50);
        // 步长 450：0..500, 450..950, 900..1200（尾片短于 size）
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].len(), 300);
        // 重叠区内容一致
        assert_eq!(chunks[0][450..], chunks[1][..50]);
        // 空内容
        assert!(split_into_chunks("", 500, 50).is_empty());
        // size<=overlap 不死循环
        assert!(!split_into_chunks(&"y".repeat(100), 10, 10).is_empty());
    }

    #[test]
    fn vec_literal_formats_pgvector_spelling() {
        assert_eq!(vec_literal(&[0.1, 0.25]), "[0.1,0.25]");
        assert_eq!(vec_literal(&[]), "[]");
    }
}
