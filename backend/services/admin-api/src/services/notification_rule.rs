//! NotificationRuleService — 事件类型 → 渠道的路由规则 CRUD + 测试投递。
//!
//! 测试投递按规则走一遍"规则 → 渠道 → 台账"链:能落台账的落台账(状态按渠道
//! 出站能力如实标记,Rust 侧 SMTP/WEBHOOK 出站通道未移植前记 FAILED 并写明
//! 原因——与上游"SMS 路由到它 = 台账 FAILED"的验收语义同形)。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::mapping;
use crate::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::notification::service::v1::{
    CreateNotificationRuleRequest, DeleteNotificationRuleRequest, GetNotificationRuleRequest,
    ListNotificationRuleResponse, NotificationRule, TestDispatchNotificationRequest,
    TestDispatchNotificationResponse, UpdateNotificationRuleRequest,
};
use proto::proto::pagination::PagingRequest;

/// The test-dispatch verdict for a rule: a disabled rule (or one with
/// no/unknown channel) skips; a channel whose Rust-side outbound is
/// not ported records FAILED with the honest reason — the same
/// "SMS routes to it = ledger FAILED" acceptance semantics upstream
/// uses for its own unported outbounds.
fn rule_proto(r: crate::data::sys_notification_rules::Model) -> NotificationRule {
    NotificationRule {
        id: Some(r.id as u64),
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
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(
    a: &mut crate::data::sys_notification_rules::ActiveModel,
    data: &NotificationRule,
) -> Result<(), StatusError> {
    if let Some(v) = data.event_type {
        let s = mapping::notification_event_type_str(v)
            .ok_or_else(|| status_error("BAD_REQUEST", "unknown event_type"))?;
        a.event_type = Set(Some(s.into()));
    }
    if let Some(v) = data.channel {
        let s = mapping::notification_channel_kind_str(v)
            .ok_or_else(|| status_error("BAD_REQUEST", "unknown channel"))?;
        a.channel = Set(Some(s.into()));
    }
    if let Some(v) = data.is_async {
        a.is_async = Set(Some(v));
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
    if let Some(v) = &data.remark {
        a.remark = Set(Some(v.clone()));
    }
    Ok(())
}

pub struct NotificationRuleService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationRuleServiceHandlers for NotificationRuleService {
    async fn list_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationRuleResponse, StatusError> {
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationRuleResponse {
            items: rows.into_iter().map(rule_proto).collect(),
            total,
        })
    }

    async fn get_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationRuleRequest,
    ) -> Result<NotificationRule, StatusError> {
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        Ok(rule_proto(repo.get_by_id(req.id as i64).await?))
    }

    async fn create_notification_rule(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateNotificationRuleRequest,
    ) -> Result<NotificationRule, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        let mut a = crate::data::sys_notification_rules::ActiveModel {
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data)?;
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
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id as i64).await?;
        let mut a: crate::data::sys_notification_rules::ActiveModel = row.into();
        upsert_fields(&mut a, &data)?;
        if let sea_orm::ActiveValue::Set(v) = &a.event_type {
            let ev = v.clone().unwrap_or_default();
            if !ev.is_empty() {
                if let Some(other) = repo.find_by_event_type(&ev).await? {
                    if other.id != req.id as i64 {
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
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        repo.get_by_id(req.id as i64).await?;
        repo.delete_by_id(req.id as i64).await?;
        Ok(Empty {})
    }

    async fn test_dispatch_notification(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: TestDispatchNotificationRequest,
    ) -> Result<TestDispatchNotificationResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        let rule = repo.get_by_id(req.id as i64).await?;
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
