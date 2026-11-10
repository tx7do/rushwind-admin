//! NotificationDeliveryService — 投递台账的平台级只读视图。
//!
//! 台账行由投递链写入(当前只有测试投递一条入口),本服务只回答"发生了什么"。

use std::sync::Arc;

use crate::mapping;
use crate::state::{AppState, StatusError};
use proto::proto::notification::service::v1::{
    GetNotificationDeliveryRequest, ListNotificationDeliveryResponse, NotificationDelivery,
};
use proto::proto::pagination::PagingRequest;

fn delivery_proto(r: crate::data::sys_notification_deliveries::Model) -> NotificationDelivery {
    NotificationDelivery {
        id: Some(r.id as u64),
        event_type: r
            .event_type
            .as_deref()
            .and_then(mapping::notification_event_type_of),
        channel: r
            .channel
            .as_deref()
            .and_then(mapping::notification_channel_kind_of),
        channel_id: r.channel_id.map(|v| v as u64),
        recipient_user_id: r.recipient_user_id.map(|v| v as u64),
        related_id: r.related_id.map(|v| v as u64),
        target: r.target,
        status: r
            .status
            .as_deref()
            .map(|s| mapping::notification_delivery_status_of(s).unwrap_or(1)),
        last_error: r.last_error,
        sent_at: None,
        request_id: r.request_id,
        attempts: r.attempts,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
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
        Ok(delivery_proto(repo.get_by_id(req.id as i64).await?))
    }
}
