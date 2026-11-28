//! InternalMessageService — tenant messages: send (with the SSE
//! recipient fan-out), list/get/update/delete, and revoke with the
//! recipient cascade.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, tenant_of, AppState, StatusError};
use crate::proto::internal_message::service::v1::{
    GetInternalMessageRequest, InternalMessage, ListInternalMessageResponse, RevokeMessageRequest,
    SendMessageRequest, SendMessageResponse, UpdateInternalMessageRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

/// Unknown rows read as DRAFT.
fn message_status_to_proto(s: &str) -> i32 {
    mapping::internal_message_status_of(s).unwrap_or(0)
}

fn message_proto(r: crate::admin::data::internal_messages::Model) -> InternalMessage {
    InternalMessage {
        id: Some(r.id),
        title: r.title,
        content: r.content,
        status: r.status.as_deref().map(message_status_to_proto),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| mapping::internal_message_type_of(s).unwrap_or(0)),
        sender_id: r.sender_id,
        sender_name: None,
        category_id: r.category_id,
        category_name: None,
        tenant_id: r.tenant_id,
        tenant_name: None,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

/// 分类名回填（参照 enrichRelations 的 CategoryName 面；best-effort）。
async fn bind_category_names(state: &AppState, items: &mut [InternalMessage]) {
    let ids: std::collections::BTreeSet<i64> = items.iter().filter_map(|m| m.category_id).collect();
    if ids.is_empty() {
        return;
    }
    let categories: std::collections::HashMap<i64, String> =
        crate::admin::data::internal_message_categories::Entity::find()
            .filter(crate::admin::data::internal_message_categories::Column::Id.is_in(ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect();
    for m in items {
        if let Some(cid) = m.category_id {
            m.category_name = categories.get(&cid).cloned();
        }
    }
}

pub struct InternalMessageService {
    pub state: Arc<AppState>,
}

impl InternalMessageService {
    /// Insert recipients with the denormalized title/content (status
    /// RECEIVED immediately — contract for inbox reads).
    pub async fn insert_recipients(
        &self,
        tenant_id: i64,
        message_id: i64,
        title: &str,
        content: &str,
        recipient_ids: &[i64],
    ) -> Result<(), StatusError> {
        for uid in recipient_ids {
            crate::admin::data::internal_message_recipients::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                message_id: Set(Some(message_id)),
                recipient_user_id: Set(Some(*uid)),
                status: Set(Some("RECEIVED".into())),
                received_at: Set(Some(crate::admin::data::now())),
                created_at: Set(Some(crate::admin::data::now())),
                updated_at: Set(Some(crate::admin::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        let _ = (title, content);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::InternalMessageServiceHandlers for InternalMessageService {
    async fn list_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListInternalMessageResponse, StatusError> {
        let repo = crate::admin::data::repos::InternalMessageRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tenant_of(&ctx), &req).await?;
        let mut items: Vec<InternalMessage> = rows.into_iter().map(message_proto).collect();
        bind_category_names(&self.state, &mut items).await;
        Ok(ListInternalMessageResponse {
            items,
            total: total as i64,
        })
    }

    async fn get_message(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetInternalMessageRequest,
    ) -> Result<InternalMessage, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::internal_message::service::v1::get_internal_message_request::QueryBy
        );
        let row = crate::admin::data::internal_messages::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        let mut message = message_proto(row);
        bind_category_names(&self.state, std::slice::from_mut(&mut message)).await;
        Ok(message)
    }

    async fn update_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateInternalMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::internal_messages::Entity::find_by_id(req.id)
            .filter(crate::admin::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        let mut a: crate::admin::data::internal_messages::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
            // 存量 if-let-Some 全量提交语义不变）。掩码选中 + 载荷 None =
            // 显式清可空列；无掩码 None 不动列。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, a, title, data.title, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, content, data.content, |v: &String| Some(
                v.clone()
            ));
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::internal_message::service::v1::DeleteInternalMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::internal_message::service::v1::delete_internal_message_request::QueryBy
        );
        let row = crate::admin::data::internal_messages::Entity::find_by_id(id)
            .filter(crate::admin::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        // Transactional cascade of recipients (DeleteMessageWithRecipients).
        crate::admin::data::internal_message_recipients::Entity::delete_many()
            .filter(crate::admin::data::internal_message_recipients::Column::MessageId.eq(row.id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::admin::data::internal_messages::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn send_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let type_str = match req.r#type {
            1 => "PRIVATE",
            2 => "GROUP",
            _ => "NOTIFICATION",
        };
        let message = crate::admin::data::internal_messages::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            title: Set(req.title.clone()),
            content: Set(Some(req.content.clone())),
            sender_id: Set(Some(payload.user_id)),
            category_id: Set(req.category_id),
            status: Set(Some("PUBLISHED".into())),
            type_column: Set(Some(type_str.into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        // Recipients: explicit list (single or multi), else the whole
        // tenant fan-out (the broadcast rides the scheduler phase;
        // direct fan-out covers the API contract).
        let recipient_ids: Vec<i64> = if !req.target_user_ids.is_empty() {
            req.target_user_ids.to_vec()
        } else if let Some(uid) = req.recipient_user_id {
            vec![uid]
        } else if req.target_all == Some(true) {
            crate::admin::data::sys_users::Entity::find()
                .filter(crate::admin::data::sys_users::Column::TenantId.eq(payload.tenant_id))
                .all(&self.state.db)
                .await
                .map_err(db_err)?
                .iter()
                .map(|u| u.id)
                .collect()
        } else {
            Vec::new()
        };
        let received_at = crate::admin::state::datetime_to_ts(crate::admin::data::now());
        for chunk in recipient_ids.chunks(1000) {
            self.insert_recipients(
                payload.tenant_id,
                message.id,
                req.title.as_deref().unwrap_or(""),
                &req.content,
                chunk,
            )
            .await?;
            // SSE push per recipient (publishNotification: stream = userId,
            // event = notification, data = the recipient protojson).
            for uid in chunk {
                crate::admin::server::sse::publish_recipient(
                    &self.state.hub,
                    &crate::admin::server::sse::NotificationPayload {
                        id: message.id,
                        message_id: (message.id as i64),
                        recipient_user_id: *uid,
                        title: req.title.clone().unwrap_or_default(),
                        content: req.content.clone(),
                        received_at,
                    },
                );
            }
        }
        Ok(SendMessageResponse {
            message_id: message.id,
        })
    }

    async fn revoke_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RevokeMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::internal_messages::Entity::find_by_id(req.message_id)
            .filter(crate::admin::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        crate::admin::data::internal_message_recipients::Entity::delete_many()
            .filter(crate::admin::data::internal_message_recipients::Column::MessageId.eq(row.id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut a: crate::admin::data::internal_messages::ActiveModel = row.into();
        a.status = Set(Some("REVOKED".into()));
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::internal_messages as messages;
    use crate::proto::gen::services::InternalMessageServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(
        db: &sea_orm::DatabaseConnection,
        tenant: i64,
        title: &str,
        content: &str,
    ) -> messages::Model {
        messages::ActiveModel {
            tenant_id: Set(Some(tenant)),
            title: Set(Some(title.into())),
            content: Set(Some(content.into())),
            status: Set(Some("PUBLISHED".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> messages::Model {
        messages::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: i64, data: InternalMessage, paths: &[&str]) -> UpdateInternalMessageRequest {
        UpdateInternalMessageRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            ..Default::default()
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 title 时，载荷里
    /// 即便带了 content 也不许动——部分提交不再误写未选中列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = InternalMessageService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "旧标题", "旧内容").await;

        svc.update_message(
            ctx(1, 9),
            update_req(
                row.id,
                InternalMessage {
                    title: Some("新标题".into()),
                    content: Some("越权内容".into()),
                    ..Default::default()
                },
                &["title"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.title.as_deref(), Some("新标题"), "masked title lands");
        assert_eq!(
            fresh.content.as_deref(),
            Some("旧内容"),
            "unmasked content is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = InternalMessageService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "标题", "正文").await;

        // 无掩码：None 不动列。
        svc.update_message(
            ctx(1, 9),
            UpdateInternalMessageRequest {
                id: row.id,
                data: Some(InternalMessage {
                    title: Some("换个标题".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.content.as_deref(),
            Some("正文"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update_message(
            ctx(1, 9),
            update_req(row.id, InternalMessage::default(), &["content"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.content.is_none(),
            "masked None explicitly clears the column"
        );
    }

    /// 分类名回填（参照 enrichRelations 的 CategoryName 面）——此前恒 None。
    #[tokio::test]
    async fn get_message_backfills_category_name() {
        let state = test_state().await;
        let svc = InternalMessageService {
            state: state.clone(),
        };
        let db = &state.db;
        let category = crate::admin::data::internal_message_categories::ActiveModel {
            name: Set("通知类".into()),
            code: Set("notice".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let row = messages::ActiveModel {
            tenant_id: Set(Some(1)),
            title: Set(Some("带分类".into())),
            content: Set(Some("正文".into())),
            status: Set(Some("PUBLISHED".into())),
            category_id: Set(Some(category.id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let m = svc
            .get_message(
                ctx(1, 9),
                crate::proto::internal_message::service::v1::GetInternalMessageRequest {
                    view_mask: None,
                    query_by: Some(
                        crate::proto::internal_message::service::v1::get_internal_message_request::QueryBy::Id(
                            row.id,
                        ),
                    ),
                },
            )
            .await
            .unwrap();
        assert_eq!(m.category_name.as_deref(), Some("通知类"));
    }
}
