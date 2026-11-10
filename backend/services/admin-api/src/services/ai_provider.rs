//! AiProviderService — LLM 提供商 CRUD。api_key 明文经全局加密器
//! （AES-256-GCM，enc: 前缀）落库并刷新脱敏 hint；更新时 key 为空 = 本次
//! 不改 key（hint 与 has 面照常回显），与上游 service 层语义一致。
//! is_default 唯一化：置默认时清掉同租户其他默认行。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::data::Viewer;
use crate::state::{db_err, not_found, operator_of, require_data, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{
    AiProvider, CreateAiProviderRequest, DeleteAiProviderRequest, GetAiProviderRequest,
    ListAiProviderResponse, UpdateAiProviderRequest,
};
use proto::proto::pagination::PagingRequest;

/// ModelType proto 枚举 ↔ ent 列值（与上游枚举名逐字一致）。
fn model_type_str(v: i32) -> Option<&'static str> {
    Some(match v {
        1 => "OPENAI",
        2 => "AZURE_OPENAI",
        3 => "OLLAMA",
        4 => "OPENAI_COMPATIBLE",
        5 => "MOONSHOT",
        6 => "ZHIPU_AI",
        7 => "QWEN",
        8 => "DEEPSEEK",
        _ => return None,
    })
}

fn model_type_proto(s: &str) -> i32 {
    match s {
        "OPENAI" => 1,
        "AZURE_OPENAI" => 2,
        "OLLAMA" => 3,
        "OPENAI_COMPATIBLE" => 4,
        "MOONSHOT" => 5,
        "ZHIPU_AI" => 6,
        "QWEN" => 7,
        "DEEPSEEK" => 8,
        _ => 0,
    }
}

fn hint_of(key: &str) -> String {
    let t = key.trim();
    if t.is_empty() {
        return String::new();
    }
    match t.len() {
        n if n <= 8 => format!("sk-***{t}"),
        _ => format!("sk-***{}", &t[t.len() - 4..]),
    }
}

fn provider_proto(r: crate::data::sys_ai_providers::Model) -> AiProvider {
    AiProvider {
        id: Some(r.id as u64),
        name: r.name,
        model_type: r.model_type.as_deref().map(model_type_proto),
        model_name: r.model_name,
        base_url: r.base_url,
        organization: r.organization,
        // api_key 明文永不出库（proto 字段仅承载写入载荷）；hint 回显。
        api_key: String::new().into(),
        api_key_hint: r.api_key_hint,
        local_host: r.local_host,
        local_port: r.local_port,
        timeout_seconds: r.timeout_seconds,
        system_prompt: r.system_prompt,
        is_default: r.is_default,
        remark: r.remark,
        is_enabled: r.is_enabled,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(a: &mut crate::data::sys_ai_providers::ActiveModel, data: &AiProvider) {
    if let Some(v) = &data.name {
        a.name = Set(Some(v.clone()));
    }
    if let Some(v) = data.model_type.and_then(model_type_str) {
        a.model_type = Set(Some(v.into()));
    }
    if let Some(v) = &data.model_name {
        a.model_name = Set(Some(v.clone()));
    }
    if let Some(v) = &data.base_url {
        a.base_url = Set(Some(v.clone()));
    }
    if let Some(v) = &data.organization {
        a.organization = Set(Some(v.clone()));
    }
    if let Some(v) = &data.local_host {
        a.local_host = Set(Some(v.clone()));
    }
    if let Some(v) = data.local_port {
        a.local_port = Set(Some(v));
    }
    if let Some(v) = data.timeout_seconds {
        a.timeout_seconds = Set(Some(v));
    }
    if let Some(v) = &data.system_prompt {
        a.system_prompt = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_default {
        a.is_default = Set(Some(v));
    }
    if let Some(v) = &data.remark {
        a.remark = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
}

pub struct AiProviderService {
    pub state: Arc<AppState>,
}

impl AiProviderService {
    async fn clear_other_defaults(&self, tenant_id: i64, keep_id: i64) -> Result<(), StatusError> {
        let rows = crate::data::sys_ai_providers::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(crate::data::sys_ai_providers::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_ai_providers::Column::IsDefault.eq(true)),
            )
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        for row in rows {
            if row.id == keep_id {
                continue;
            }
            let mut a: crate::data::sys_ai_providers::ActiveModel = row.into();
            a.is_default = Set(Some(false));
            a.update(&self.state.db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::AiProviderServiceHandlers for AiProviderService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiProviderResponse, StatusError> {
        let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiProviderResponse {
            items: rows.into_iter().map(provider_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiProviderRequest,
    ) -> Result<AiProvider, StatusError> {
        let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::get_ai_provider_request::QueryBy
        );
        let row = repo
            .find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        Ok(provider_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let _repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        // api_key 明文非空 → 加密落库并刷新 hint；为空 → 不设 key。
        let (api_key, api_key_hint) = match data.api_key.as_deref().map(str::trim) {
            Some(k) if !k.is_empty() => {
                (Some(crate::crypto::encrypt_if_needed(k)), Some(hint_of(k)))
            }
            _ => (None, None),
        };
        let mut a = crate::data::sys_ai_providers::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            api_key: Set(api_key),
            api_key_hint: Set(api_key_hint),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data);
        if a.is_enabled.clone().unwrap().is_none() {
            a.is_enabled = Set(Some(true));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        if inserted.is_default == Some(true) {
            self.clear_other_defaults(payload.tenant_id, inserted.id as u64 as i64)
                .await?;
        }
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        let mut a: crate::data::sys_ai_providers::ActiveModel = row.into();
        upsert_fields(&mut a, &data);
        // key 为空 → 本次请求不改 key：api_key/api_key_hint 摘出（保留原值）。
        match data.api_key.as_deref().map(str::trim) {
            Some(k) if !k.is_empty() => {
                a.api_key = Set(Some(crate::crypto::encrypt_if_needed(k)));
                a.api_key_hint = Set(Some(hint_of(k)));
            }
            _ => {}
        }
        crate::stamp_update!(a, payload.user_id);
        a.updated_at = Set(Some(crate::data::now()));
        let is_default = a.is_default.clone().unwrap() == Some(true);
        a.update(&self.state.db).await.map_err(db_err)?;
        if is_default {
            self.clear_other_defaults(payload.tenant_id, req.id as i64)
                .await?;
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::delete_ai_provider_request::QueryBy
        );
        let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        repo.find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        crate::data::sys_ai_providers::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_hint_masks_tail_and_keeps_short_keys() {
        // 长 key：尾 4 位
        assert_eq!(hint_of("sk-abcdefghijklmnop"), "sk-***mnop");
        // 短 key（≤8）：全量展示（没有可遮的尾）
        assert_eq!(hint_of("abc12345"), "sk-***abc12345");
        // 空白归一
        assert_eq!(hint_of("  xyz  "), "sk-***xyz");
        assert_eq!(hint_of(""), "");
        assert_eq!(hint_of("   "), "");
    }

    #[test]
    fn model_type_maps_round_trip_across_the_enum() {
        // 全部 8 个枚举值双向 round-trip；未知值 None 落 0
        for (i, name) in [
            (1, "OPENAI"),
            (2, "AZURE_OPENAI"),
            (3, "OLLAMA"),
            (4, "OPENAI_COMPATIBLE"),
            (5, "MOONSHOT"),
            (6, "ZHIPU_AI"),
            (7, "QWEN"),
            (8, "DEEPSEEK"),
        ] {
            assert_eq!(model_type_str(i), Some(name), "proto {i}");
            assert_eq!(model_type_proto(name), i, "ent {name}");
        }
        assert_eq!(model_type_str(0), None);
        assert_eq!(model_type_str(99), None);
        assert_eq!(model_type_proto("UNKNOWN"), 0);
    }
}
