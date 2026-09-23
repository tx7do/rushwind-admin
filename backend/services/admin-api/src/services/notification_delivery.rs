//! NotificationDeliveryService — 投递台账的平台级只读视图。
//!
//! 台账行由投递链写入(当前只有测试投递一条入口),本服务只回答"发生了什么"。

use std::sync::Arc;

use crate::state::{AppState, StatusError};
use proto::proto::notification::service::v1::{
    GetNotificationDeliveryRequest, ListNotificationDeliveryResponse, NotificationDelivery,
};
use proto::proto::pagination::PagingRequest;

fn status_i32(v: &str) -> i32 {
    match v {
        "SENT" => 2,
        "FAILED" => 3,
        "SKIPPED" => 4,
        _ => 1,
    }
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

fn event_type_i32_of(s: &str) -> Option<i32> {
    Some(match s {
        "PASSWORD_RESET_CODE" => 1,
        "CONTACT_BIND_CODE" => 2,
        "CHANNEL_TEST_EMAIL" => 3,
        "INTERNAL_MESSAGE" => 4,
        _ => return None,
    })
}

fn delivery_proto(r: crate::data::sys_notification_deliveries::Model) -> NotificationDelivery {
    NotificationDelivery {
        id: Some(r.id),
        event_type: r.event_type.as_deref().and_then(event_type_i32_of),
        channel: r.channel.as_deref().and_then(channel_i32_of),
        channel_id: r.channel_id,
        recipient_user_id: r.recipient_user_id,
        related_id: r.related_id,
        target: r.target,
        status: r.status.as_deref().map(status_i32),
        last_error: r.last_error,
        sent_at: None,
        request_id: r.request_id,
        attempts: r.attempts,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct NotificationDeliveryService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationServiceHandlers for NotificationDeliveryService {
    async fn list_notification_delivery(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationDeliveryResponse, StatusError> {
        let repo = crate::data::repos::NotificationDeliveryRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationDeliveryResponse {
            items: rows.into_iter().map(delivery_proto).collect(),
            total,
        })
    }

    async fn get_notification_delivery(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationDeliveryRequest,
    ) -> Result<NotificationDelivery, StatusError> {
        let repo = crate::data::repos::NotificationDeliveryRepo::new(&self.state.db);
        Ok(delivery_proto(repo.get_by_id(req.id).await?))
    }
}
