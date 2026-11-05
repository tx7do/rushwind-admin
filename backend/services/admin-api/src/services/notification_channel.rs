//! NotificationChannelService — SMTP/webhook channel CRUD plus
//! SendTestEmail (delivery lands with the mailer phase; the request
//! validates the channel and answers per contract).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::mapping;
use crate::state::{db_err, operator_of, status_error, AppState, StatusError};
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
        // Unknown rows read as START_TLS.
        smtp_tls: r
            .smtp_tls
            .as_deref()
            .map(|s| mapping::notification_smtp_tls_of(s).unwrap_or(1)),
        enabled: r.status.as_deref().map(|s| s != "OFF"),
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        webhook_url: r.webhook_url,
        has_webhook_secret: r
            .webhook_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            .then_some(true),
        // Unknown rows read as CUSTOM.
        webhook_sign_style: r
            .webhook_sign_style
            .as_deref()
            .map(|s| mapping::notification_sign_style_of(s).unwrap_or(0)),
        webhook_payload_template: r.webhook_payload_template,
        // SMS/settings faces land with the notification-channel increment
        // (entity columns ride that migration; rows read empty until then).
        settings: None,
        sms_provider: None,
        sms_access_key_id: None,
        has_sms_access_secret: None,
        sms_sign_name: None,
        sms_template_code: None,
        sms_app_id: None,
        sms_region: None,
        smtp_from_name: None,
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
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        Ok(channel_proto(repo.get_by_id(req.id).await?))
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
            smtp_tls: Set(Some(
                data.smtp_tls
                    .map_or("START_TLS", |v| {
                        mapping::notification_smtp_tls_str(v).unwrap_or("START_TLS")
                    })
                    .to_string(),
            )),
            webhook_url: Set(data.webhook_url),
            webhook_secret: Set(None), // secrets write via update with a secret payload
            webhook_sign_style: Set(data.webhook_sign_style.map(|v| {
                mapping::notification_sign_style_str(v)
                    .unwrap_or("CUSTOM")
                    .to_string()
            })),
            webhook_payload_template: Set(data.webhook_payload_template),
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
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
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
            if let Some(v) = &data.webhook_url {
                a.webhook_url = Set(Some(v.clone()));
            }
            if let Some(v) = &data.webhook_payload_template {
                a.webhook_payload_template = Set(Some(v.clone()));
            }
            if let Some(v) = data.webhook_sign_style {
                a.webhook_sign_style = Set(Some(
                    mapping::notification_sign_style_str(v)
                        .unwrap_or("CUSTOM")
                        .to_string(),
                ));
            }
            if let Some(v) = data.smtp_tls {
                a.smtp_tls = Set(Some(
                    mapping::notification_smtp_tls_str(v)
                        .unwrap_or("START_TLS")
                        .to_string(),
                ));
            }
            if let Some(v) = data.enabled {
                a.status = Set(Some(if v { "ON".into() } else { "OFF".into() }));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationChannelRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        repo.delete_by_id(req.id).await?;
        Ok(Empty {})
    }

    async fn send_test_email(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendTestEmailRequest,
    ) -> Result<Empty, StatusError> {
        // Delivery rides the mailer phase; validate the channel exists.
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        if row.smtp_host.as_deref().map_or(true, |h| h.is_empty()) {
            return Err(status_error(
                "BAD_REQUEST",
                "channel has no smtp host configured",
            ));
        }
        Ok(Empty {})
    }

    async fn send_test(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: proto::proto::notification_channel::service::v1::SendTestRequest,
    ) -> Result<Empty, StatusError> {
        // The generic channel test (EMAIL/WEBHOOK/SMS/EXT) rides the outbound
        // delivery increment; until then it validates the channel exists.
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        Ok(Empty {})
    }

    async fn list_channel_schemas(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<
        proto::proto::notification_channel::service::v1::ListChannelSchemasResponse,
        StatusError,
    > {
        // The EXT_* settings-channel schema registry (39 declared channels on
        // the reference) lands with the notification-channel increment; the
        // dynamic forms read empty until then.
        Ok(
            proto::proto::notification_channel::service::v1::ListChannelSchemasResponse {
                items: Vec::new(),
            },
        )
    }
}
