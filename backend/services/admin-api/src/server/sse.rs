//! The SSE notification server — the deployment face. The wire
//! contract (preflight, token extraction, the error line, the stream
//! filtering, the transport headers) lives in
//! `rushwind-transport-sse`; this file carries the admin authorize
//! sequence as a Gate adapter, the notification payload assembler, and
//! the factory wiring with the deployment's listener defaults.

use std::sync::Arc;

use crate::state::{status_error, AppState, StatusError};
use rushwind_transport_sse::{Failure, Gate, SseApp};

pub use rushwind_transport_sse::Hub;

/// The admin authorize sequence: signature + expiry via the engine,
/// whitelist + blacklist via the store — the HandleAuthorize
/// ValidateTokenRequest(ACCESS) sequence.
struct AdminGate {
    authenticator: Arc<dyn rushwind_authn::Authenticator>,
    tokens: crate::token::TokenStore,
}

impl Gate for AdminGate {
    async fn authorize(&self, token: &str) -> Result<u64, Failure> {
        let Ok(claims) = self.authenticator.authenticate_token(token) else {
            return Err(Failure::new("UNAUTHORIZED", "invalid token"));
        };
        let Some(uid) = claims.0.get("uid").and_then(|v| v.as_u64()) else {
            return Err(Failure::new("UNAUTHORIZED", "invalid token"));
        };
        let Ok(jti) = claims.get_jwt_id() else {
            return Err(Failure::new("UNAUTHORIZED", "invalid token"));
        };
        if !self
            .tokens
            .is_valid_access_token(uid as i64, &jti, token)
            .await
        {
            return Err(Failure::new(
                "UNAUTHORIZED",
                "access token is revoked or expired",
            ));
        }
        if self.tokens.is_blocked_access_token(&jti).await {
            return Err(Failure::new("FORBIDDEN", "token is blocked"));
        }
        Ok(uid)
    }
}

/// The status-table constructor the transport calls for every failure
/// envelope (reason → the admin error table's HTTP status).
fn sse_status_error(reason: &'static str, message: String) -> StatusError {
    status_error(reason, message)
}

/// The sse transport factory: the framework router under the wire's
/// address and path, with the deployment's listener defaults.
pub fn factory(
    state: std::sync::Arc<AppState>,
) -> impl Fn(
    serde_json::Value,
    rushwind_bootstrap::RouteInput,
) -> rushwind_bootstrap::BoxFuture<
    'static,
    Result<std::sync::Arc<dyn rushwind_transport::Server>, rushwind_bootstrap::BootstrapError>,
> + Send
       + Sync
       + 'static {
    let app = Arc::new(SseApp {
        hub: state.hub.clone(),
        gate: Arc::new(AdminGate {
            authenticator: Arc::clone(&state.authenticator),
            tokens: state.tokens.clone(),
        }),
        status_error: sse_status_error,
    });
    rushwind_transport_sse::factory(
        app,
        rushwind_transport_sse::SseWire {
            addr: Some(rushwind_bootstrap::BindWire(std::net::SocketAddr::from((
                [0, 0, 0, 0],
                7789,
            )))),
            path: Some("/events".to_string()),
        },
    )
}

/// Publishes a notification for one recipient — called by SendMessage
/// after the recipient rows land. Payload = the recipient protojson,
/// assembled field-by-field (the proto types carry no serde derives).
pub struct NotificationPayload {
    pub id: i64,
    pub message_id: i64,
    pub recipient_user_id: i64,
    pub title: String,
    pub content: String,
    pub received_at: Option<pbjson_types::Timestamp>,
}

pub fn publish_recipient(hub: &Hub, payload: &NotificationPayload) {
    let json = serde_json::json!({
        "id": payload.id,
        "messageId": payload.message_id,
        "recipientUserId": payload.recipient_user_id,
        "status": "RECEIVED",
        "title": payload.title,
        "content": payload.content,
        "receivedAt": payload.received_at.map(|ts| serde_json::json!({
            "seconds": ts.seconds,
            "nanos": ts.nanos,
        })),
    });
    hub.publish(payload.recipient_user_id as u64, json.to_string());
}
