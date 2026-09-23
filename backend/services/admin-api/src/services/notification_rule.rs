//! NotificationRuleService — 事件类型 → 渠道的路由规则 CRUD + 测试投递。
//!
//! 测试投递按规则走一遍"规则 → 渠道 → 台账"链:能落台账的落台账(状态按渠道
//! 出站能力如实标记,Rust 侧 SMTP/WEBHOOK 出站通道未移植前记 FAILED 并写明
//! 原因——与上游"SMS 路由到它 = 台账 FAILED"的验收语义同形)。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::notification::service::v1::{
    CreateNotificationRuleRequest, DeleteNotificationRuleRequest, GetNotificationRuleRequest,
    ListNotificationRuleResponse, NotificationRule, TestDispatchNotificationRequest,
    TestDispatchNotificationResponse, UpdateNotificationRuleRequest,
};
use proto::proto::pagination::PagingRequest;

fn event_type_str(v: i32) -> Option<&'static str> {
    Some(match v {
        1 => "PASSWORD_RESET_CODE",
        2 => "CONTACT_BIND_CODE",
        3 => "CHANNEL_TEST_EMAIL",
        4 => "INTERNAL_MESSAGE",
        _ => return None,
    })
}

fn channel_str(v: i32) -> Option<&'static str> {
    Some(match v {
        1 => "EMAIL",
        2 => "SMS",
        3 => "WEBHOOK",
        4 => "INTERNAL",
        _ => return None,
    })
}

fn status_i32(v: &str) -> i32 {
    match v {
        "SENT" => 2,
        "FAILED" => 3,
        "SKIPPED" => 4,
        _ => 1, // SENDING
    }
}

fn rule_proto(r: crate::data::sys_notification_rules::Model) -> NotificationRule {
    NotificationRule {
        id: Some(r.id),
        event_type: r.event_type.as_deref().and_then(event_type_i32_of),
        channel: r.channel.as_deref().and_then(channel_i32_of),
        is_async: r.is_async,
        is_enabled: r.is_enabled,
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

fn event_type_i32_of(s: &str) -> Option<i32> {
    Some(match s {
        "PASSWORD_RESET_CODE" => 1,
        "CONTACT_BIND_CODE" => 2,
        "CHANNEL_TEST_EMAIL" => 3,
        "INTERNAL_MESSAGE" => 4,
        _ => return None,
    })
}

fn channel_i32_of(s: &str) -> Option<i32> {
    Some(match s {
        "EMAIL" => 1,
        "SMS" => 2,
        "WEBHOOK" => 3,
        "INTERNAL" => 4,
        _ => return None,
    })
}

fn upsert_fields(
    a: &mut crate::data::sys_notification_rules::ActiveModel,
    data: &NotificationRule,
) -> Result<(), StatusError> {
    if let Some(v) = data.event_type {
        let s =
            event_type_str(v).ok_or_else(|| status_error("BAD_REQUEST", "unknown event_type"))?;
        a.event_type = Set(Some(s.into()));
    }
    if let Some(v) = data.channel {
        let s = channel_str(v).ok_or_else(|| status_error("BAD_REQUEST", "unknown channel"))?;
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
        Ok(rule_proto(repo.get_by_id(req.id).await?))
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
        let row = repo.get_by_id(req.id).await?;
        let mut a: crate::data::sys_notification_rules::ActiveModel = row.into();
        upsert_fields(&mut a, &data)?;
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
        a.updated_by = Set(Some(payload.user_id));
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_notification_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationRuleRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
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
        let repo = crate::data::repos::NotificationRuleRepo::new(&self.state.db);
        let rule = repo.get_by_id(req.id).await?;

        // 台账行:规则/渠道停用 → SKIPPED;Rust 侧出站通道未移植 → FAILED 并写明原因。
        let (status, last_error) = if !rule.is_enabled.unwrap_or(false) {
            ("SKIPPED", "规则已停用")
        } else {
            match rule.channel.as_deref().unwrap_or("") {
                "EMAIL" => ("FAILED", "Rust 侧 SMTP 出站未移植"),
                "SMS" => ("FAILED", "短信通道未实现"),
                "WEBHOOK" => ("FAILED", "Rust 侧 WEBHOOK 出站未移植"),
                "INTERNAL" => ("FAILED", "Rust 侧站内信出站未移植"),
                "" => ("SKIPPED", "规则未配置渠道"),
                _ => ("SKIPPED", "未知渠道"),
            }
        };

        let delivery = crate::data::repos::NotificationDeliveryRepo::new(&self.state.db)
            .insert(crate::data::sys_notification_deliveries::ActiveModel {
                event_type: Set(rule.event_type.clone()),
                channel: Set(rule.channel.clone()),
                channel_id: Set(None),
                recipient_user_id: Set(None),
                related_id: Set(None),
                target: Set(req.target),
                status: Set(Some(status.into())),
                last_error: Set(Some(last_error.into())),
                request_id: Set(Some(format!(
                    "test-{}-{}",
                    payload.user_id,
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ))),
                attempts: Set(Some(1)),
                created_by: Set(Some(payload.user_id)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await?;

        Ok(TestDispatchNotificationResponse {
            delivery_id: delivery.id,
            status: status_i32(status),
        })
    }
}
