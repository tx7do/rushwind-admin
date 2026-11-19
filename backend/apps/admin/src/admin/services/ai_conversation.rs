//! AiConversationService — 会话 CRUD（读面按操作者钉定，平台用户看全量）。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, Set};

use crate::admin::data::Viewer;
use crate::admin::state::{db_err, not_found, operator_of, AppState, StatusError};
use crate::proto::ai::service::v1::{
    AiConversation, DeleteAiConversationRequest, GetAiConversationRequest,
    ListAiConversationResponse, UpdateAiConversationRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn conv_proto(r: crate::admin::data::sys_ai_conversations::Model) -> AiConversation {
    AiConversation {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        title: r.title,
        provider_id: r.provider_id.map(|v| v as u64),
        user_id: Some(r.user_id as u64),
        last_message_at: r.last_message_at.and_then(crate::admin::state::naive_to_ts),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct AiConversationService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::AiConversationServiceHandlers for AiConversationService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiConversationResponse, StatusError> {
        let repo = crate::admin::data::repos::AiConversationRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiConversationResponse {
            items: rows.into_iter().map(conv_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiConversationRequest,
    ) -> Result<AiConversation, StatusError> {
        let repo = crate::admin::data::repos::AiConversationRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::ai::service::v1::get_ai_conversation_request::QueryBy
        );
        let row = repo
            .find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        Ok(conv_proto(row))
    }

    /// 会话改名（标题）。字段级更新：写入过 update_mask 白名单门（无掩码
    /// 即存量全量提交语义）；会话归属校验（repo 按操作者租户钉定）原位保留。
    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiConversationRequest,
    ) -> Result<Empty, StatusError> {
        let _payload = operator_of(&ctx)?;
        let repo = crate::admin::data::repos::AiConversationRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        let row = repo
            .find_scoped(req.id as i64)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        let mut a: crate::admin::data::sys_ai_conversations::ActiveModel = row.into();
        if let Some(data) = &req.data {
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, a, title, data.title, |v: &String| Some(v.clone()));
        }
        // 会话表无 updated_by 冗余列；updated_at 手动盖章。
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiConversationRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::ai::service::v1::delete_ai_conversation_request::QueryBy
        );
        let repo = crate::admin::data::repos::AiConversationRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        repo.find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        // 会话删除级联消息行（上游 delete cascade 同语义）。
        use sea_orm::QueryFilter as _;
        crate::admin::data::sys_ai_messages::Entity::delete_many()
            .filter(crate::admin::data::sys_ai_messages::Column::ConversationId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_ai_conversations::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_ai_conversations as convs;
    use crate::testsupport::{ctx, test_state};
    use crate::proto::gen::services::AiConversationServiceHandlers as _;
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, title: &str) -> convs::Model {
        convs::ActiveModel {
            tenant_id: Set(Some(1)),
            title: Set(Some(title.into())),
            provider_id: Set(Some(3)),
            user_id: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> convs::Model {
        convs::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: u64, data: AiConversation, paths: &[&str]) -> UpdateAiConversationRequest {
        UpdateAiConversationRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码不含 title 时，载荷里
    /// 的标题不许动；掩码选中 title 才落列。本 handler 的写面只有 title，
    /// 门外的 provider_id 本就不可写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = AiConversationService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "旧标题").await;

        // 掩码不含 title：改名请求被门挡下。
        svc.update(
            me,
            update_req(
                row.id as u64,
                AiConversation {
                    title: Some("劫持标题".into()),
                    ..Default::default()
                },
                &["provider_id"],
            ),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.title.as_deref(),
            Some("旧标题"),
            "unmasked title is not writable through the gate"
        );

        // 掩码选中 title：改名落列。
        svc.update(
            ctx(0, 1),
            update_req(
                row.id as u64,
                AiConversation {
                    title: Some("新标题".into()),
                    ..Default::default()
                },
                &["title"],
            ),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.title.as_deref(), Some("新标题"), "masked title lands");
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = AiConversationService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "旧标题").await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateAiConversationRequest {
                id: row.id as u64,
                data: Some(AiConversation::default()),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.title.as_deref(),
            Some("旧标题"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            me,
            update_req(row.id as u64, AiConversation::default(), &["title"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.title.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
