//! NotificationChannelService — //! service: SMTP/webhook channel CRUD plus
//! SendTestEmail (delivery lands with the mailer phase; the request
//! validates the channel and answers per contract).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::notification_channel::service::v1::{
    CreateNotificationChannelRequest, DeleteNotificationChannelRequest,
    GetNotificationChannelRequest, ListNotificationChannelResponse, NotificationChannel,
    SendTestEmailRequest, UpdateNotificationChannelRequest,
};
use proto::proto::pagination::PagingRequest;

fn channel_proto(r: crate::data::sys_notification_channels::Model) -> NotificationChannel {
    NotificationChannel {
        id: Some(r.id),
        name: Some(r.name),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| if s == "WEBHOOK" { 1 } else { 0 }),
        smtp_host: r.smtp_host,
        smtp_port: r.smtp_port,
        smtp_username: r.smtp_username,
        has_password: r
            .smtp_password
            .as_deref()
            .is_some_and(|p| !p.is_empty())
            .then_some(true),
        smtp_from: r.smtp_from,
        smtp_tls: r.smtp_tls.as_deref().map(|s| match s {
            "NONE" => 0,
            "SSL" => 2,
            _ => 1,
        }),
        enabled: r.status.as_deref().map(|s| s != "OFF"),
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        // WEBHOOK 出站四联(上游 N 契约新增)——Rust 侧实体尚无对应列,真实
        // 出站移植(签名风格/载荷模板/密文)落地前一律应答 None。
        webhook_url: None,
        has_webhook_secret: None,
        webhook_sign_style: None,
        webhook_payload_template: None,
    }
}

pub struct NotificationChannelService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationChannelServiceHandlers for NotificationChannelService {
    async fn list_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationChannelResponse, StatusError> {
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationChannelResponse {
            items: rows.into_iter().map(channel_proto).collect(),
            total,
        })
    }

    async fn get_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationChannelRequest,
    ) -> Result<NotificationChannel, StatusError> {
        let row = crate::data::sys_notification_channels::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("notification channel"))?;
        Ok(channel_proto(row))
    }

    async fn create_notification_channel(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateNotificationChannelRequest,
    ) -> Result<NotificationChannel, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        let inserted = crate::data::sys_notification_channels::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            type_column: Set(Some(if data.r#type == Some(1) {
                "WEBHOOK".into()
            } else {
                "EMAIL".into()
            })),
            smtp_host: Set(data.smtp_host),
            smtp_port: Set(data.smtp_port),
            smtp_username: Set(data.smtp_username),
            smtp_password: Set(None), // secrets write via update with a password payload
            smtp_from: Set(data.smtp_from),
            smtp_tls: Set(Some(match data.smtp_tls.unwrap_or(1) {
                0 => "NONE".to_string(),
                2 => "SSL".to_string(),
                _ => "START_TLS".to_string(),
            })),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(channel_proto(inserted))
    }

    async fn update_notification_channel(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationChannelRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_notification_channels::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("notification channel"))?;
        let mut a: crate::data::sys_notification_channels::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.smtp_host {
                a.smtp_host = Set(Some(v.clone()));
            }
            if let Some(v) = data.smtp_port {
                a.smtp_port = Set(Some(v));
            }
            if let Some(v) = &data.smtp_username {
                a.smtp_username = Set(Some(v.clone()));
            }
            if let Some(v) = &data.smtp_from {
                a.smtp_from = Set(Some(v.clone()));
            }
            if let Some(v) = data.smtp_tls {
                a.smtp_tls = Set(Some(match v {
                    0 => "NONE".to_string(),
                    2 => "SSL".to_string(),
                    _ => "START_TLS".to_string(),
                }));
            }
            if let Some(v) = data.enabled {
                a.status = Set(Some(if v { "ON".into() } else { "OFF".into() }));
            }
        }
        a.updated_by = Set(Some(payload.user_id));
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationChannelRequest,
    ) -> Result<Empty, StatusError> {
        crate::data::sys_notification_channels::Entity::delete_by_id(req.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn send_test_email(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendTestEmailRequest,
    ) -> Result<Empty, StatusError> {
        // Delivery rides the mailer phase; validate the channel exists.
        let row = crate::data::sys_notification_channels::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("notification channel"))?;
        if row.smtp_host.as_deref().map_or(true, |h| h.is_empty()) {
            return Err(status_error(
                "BAD_REQUEST",
                "channel has no smtp host configured",
            ));
        }
        Ok(Empty {})
    }
}
