//! NotificationRuleService — 事件类型 → 渠道的路由规则 CRUD + 测试投递。
//!
//! 测试投递按规则走一遍"规则 → 渠道 → 台账"链:能落台账的落台账(状态按渠道
//! 出站能力如实标记,Rust 侧 SMTP/WEBHOOK 出站通道未移植前记 FAILED 并写明
//! 原因——与上游"SMS 路由到它 = 台账 FAILED"的验收语义同形)。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use crate::proto::notification::service::v1::{
    CreateNotificationRuleRequest, DeleteNotificationRuleRequest, GetNotificationRuleRequest,
    ListNotificationRuleResponse, NotificationRule, TestDispatchNotificationRequest,
    TestDispatchNotificationResponse, UpdateNotificationRuleRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

/// The test-dispatch verdict for a rule: a disabled rule (or one with
/// no/unknown channel) skips; a channel whose Rust-side outbound is
/// not ported records FAILED with the honest reason — the same
/// "SMS routes to it = ledger FAILED" acceptance semantics upstream
/// uses for its own unported outbounds.
fn rule_proto(r: crate::admin::data::sys_notification_rules::Model) -> NotificationRule {
    NotificationRule {
        id: Some(r.id),
        event_type: r
            .event_type
            .as_deref()
            .and_then(mapping::notification_event_type_of),
        channel: r
            .channel
            .as_deref()
            .and_then(mapping::notification_channel_kind_of),
        is_async: r.is_async,
        is_enabled: r.is_enabled,
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
    }
}

/// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
/// if-let-Some 全量提交语义不变）。全部可空列走 opt 形态：掩码选中 +
/// 载荷 None = 显式清列；无掩码 None 不动列。event_type/channel 的
/// 未知枚举拒绝保留原位（闭包无法短路返回 Err，走 touches + if-let）。
fn apply_fields(
    a: &mut crate::data::sys_notification_rules::ActiveModel,
    data: &NotificationRule,
    mask: &crate::fieldmask::FieldMask,
) -> Result<(), StatusError> {
    // event_type/channel 的未知枚举拒绝保留原位（闭包无法短路返回 Err）；
    // None 分支与 set_masked!(opt) 精确同形——掩码选中 + 载荷 None = 显式
    // 清列，无掩码 None 不动列。
    if mask.touches("event_type") {
        match data.event_type {
            Some(v) => {
                let s = mapping::notification_event_type_str(v)
                    .ok_or_else(|| status_error("BAD_REQUEST", "unknown event_type"))?;
                a.event_type = Set(Some(s.into()));
            }
            None => {
                if !mask.unrestricted() {
                    a.event_type = Set(None);
                }
            }
        }
    }
    if mask.touches("channel") {
        match data.channel {
            Some(v) => {
                let s = mapping::notification_channel_kind_str(v)
                    .ok_or_else(|| status_error("BAD_REQUEST", "unknown channel"))?;
                a.channel = Set(Some(s.into()));
            }
            None => {
                if !mask.unrestricted() {
                    a.channel = Set(None);
                }
            }
        }
    }
    crate::set_masked!(opt mask, a, is_async, data.is_async, |v: &bool| Some(*v));
    crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(
        *v
    ));
    crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
        v.clone()
    ));
    Ok(())
}

pub struct NotificationRuleService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::NotificationRuleServiceHandlers for NotificationRuleService {
    async fn list_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationRuleResponse, StatusError> {
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationRuleResponse {
            items: rows.into_iter().map(rule_proto).collect(),
            total: total as i64,
        })
    }

    async fn get_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationRuleRequest,
    ) -> Result<NotificationRule, StatusError> {
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        Ok(rule_proto(repo.get_by_id(req.id).await?))
    }

    async fn create_notification_rule(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateNotificationRuleRequest,
    ) -> Result<NotificationRule, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        let mut a = crate::admin::data::sys_notification_rules::ActiveModel {
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked())?;
        let event_type = match &a.event_type {
            Set(v) => v.clone().unwrap_or_default(),
            _ => String::new(),
        };
        if event_type.is_empty() {
            return Err(status_error("BAD_REQUEST", "event_type is required"));
        }
        if repo.find_by_event_type(&event_type).await?.is_some() {
            return Err(status_error("CONFLICT", "event_type already routed"));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(rule_proto(inserted))
    }

    async fn update_notification_rule(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationRuleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        let mut a: crate::data::sys_notification_rules::ActiveModel = row.into();
        // 字段写入统一过 update_mask 白名单门（无掩码即存量全量提交语义）；
        // event_type 唯一性冲突校验保留原位——门只管"载荷字段→列"的写面，
        // 掩码未选 event_type 时列保持 Unchanged，校验自然早退。
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
        apply_fields(&mut a, &data, &mask)?;
        if let sea_orm::ActiveValue::Set(v) = &a.event_type {
            let ev = v.clone().unwrap_or_default();
            if !ev.is_empty() {
                if let Some(other) = repo.find_by_event_type(&ev).await? {
                    if other.id != req.id {
                        return Err(status_error("CONFLICT", "event_type already routed"));
                    }
                }
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationRuleRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        repo.delete_by_id(req.id).await?;
        Ok(Empty {})
    }

    async fn test_dispatch_notification(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: TestDispatchNotificationRequest,
    ) -> Result<TestDispatchNotificationResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::admin::data::repos::NotificationRuleRepo::new(&self.state.db);
        let rule = repo.get_by_id(req.id).await?;
        if !rule.is_enabled.unwrap_or(false) {
            return Err(status_error("BAD_REQUEST", "rule is disabled"));
        }
        // 真投递（上游 RuleTestNotification 同语义）：规则的事件类型回显给
        // 被测路由，正文一个出口覆盖 EMAIL/WEBHOOK/SMS；显式不点名渠道
        // （channel_id=0 → sender 内按"第一个启用的同类型渠道"自选）。
        let target = req
            .target
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| status_error("BAD_REQUEST", "target is required"))?;
        let event_type = rule.event_type.clone().unwrap_or_default();
        let sreq = super::notification_sender::SendRequest {
            target,
            title: "RushWind 路由测试通知".into(),
            content: format!(
                "这是路由规则 [{}] 的测试投递。\n\n被测事件：{}\n触发人：用户 {}",
                rule.id, event_type, payload.user_id
            ),
            channel_id: 0,
            event_type,
            params: Default::default(),
        };
        let channel = rule.channel.clone().unwrap_or_default();
        let (_, outcome) = super::notification_sender::deliver_once(
            &self.state.db,
            &channel,
            &sreq,
            payload.user_id,
        )
        .await?;
        Ok(TestDispatchNotificationResponse {
            delivery_id: 0,
            status: mapping::notification_delivery_status_of(outcome.status()).unwrap_or(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_notification_rules as rules;
    use crate::proto::gen::services::NotificationRuleServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, event_type: &str) -> rules::Model {
        rules::ActiveModel {
            event_type: Set(Some(event_type.into())),
            channel: Set(Some("EMAIL".into())),
            is_enabled: Set(Some(true)),
            remark: Set(Some("原始备注".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> rules::Model {
        rules::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(
        id: i64,
        data: NotificationRule,
        paths: &[&str],
    ) -> UpdateNotificationRuleRequest {
        UpdateNotificationRuleRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 remark 时，载荷里
    /// 即便带了 is_enabled 也不许动——部分提交不再误写未选中列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = NotificationRuleService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "CONTACT_BIND_CODE").await;

        svc.update_notification_rule(
            ctx(0, 9),
            update_req(
                row.id,
                NotificationRule {
                    remark: Some("新备注".into()),
                    is_enabled: Some(false),
                    ..Default::default()
                },
                &["remark"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.remark.as_deref(),
            Some("新备注"),
            "masked remark lands"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = NotificationRuleService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "CHANNEL_TEST_EMAIL").await;

        // 无掩码：None 不动列。
        svc.update_notification_rule(
            ctx(0, 9),
            UpdateNotificationRuleRequest {
                id: row.id,
                data: Some(NotificationRule {
                    is_enabled: Some(false),
                    ..Default::default()
                }),
                update_mask: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.remark.as_deref(),
            Some("原始备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update_notification_rule(
            ctx(0, 9),
            update_req(row.id, NotificationRule::default(), &["remark"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
