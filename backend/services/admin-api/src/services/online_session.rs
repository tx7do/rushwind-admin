//! OnlineSessionService — //! internal/service/service: the Redis `us:*` session
//! catalog (admin listing with keyword filter, force logout, my-session
//! list/revoke).

use std::sync::Arc;

use crate::state::{AppState, StatusError};
use crate::token::SessionMeta;
use proto::proto::online_session::service::v1::{
    ForceLogoutSessionRequest, ForceLogoutSessionResponse, ListMyOnlineSessionRequest,
    ListOnlineSessionRequest, ListOnlineSessionResponse, OnlineSession,
    RevokeMyOnlineSessionRequest, RevokeMyOnlineSessionResponse,
};

fn session_proto(jti: &str, uid: i64, meta: &SessionMeta, current_uid: i64) -> OnlineSession {
    let login_at = chrono::NaiveDateTime::parse_from_str(&meta.login_at, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&meta.login_at, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .and_then(crate::state::naive_to_ts);
    OnlineSession {
        current: Some(uid == current_uid),
        jti: Some(jti.to_string()),
        user_id: Some(uid as u64),
        username: Some(meta.username.clone()),
        tenant_id: Some(meta.tenant_id as u64),
        client_type: Some(0),
        ip_address: Some(meta.ip.clone()),
        user_agent: Some(meta.user_agent.clone()),
        device_id: meta.device.clone(),
        login_at,
    }
}

pub struct OnlineSessionService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::OnlineSessionServiceHandlers for OnlineSessionService {
    async fn list_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ListOnlineSessionRequest,
    ) -> Result<ListOnlineSessionResponse, StatusError> {
        let me = crate::state::operator_of(&ctx)?;
        let sessions = self.state.tokens.list_all_sessions().await;
        let keyword = req.keyword.clone().unwrap_or_default().to_lowercase();
        let mut items: Vec<OnlineSession> = sessions
            .iter()
            .filter(|(_, _, _, meta)| {
                keyword.is_empty()
                    || meta.username.to_lowercase().contains(&keyword)
                    || meta.ip.to_lowercase().contains(&keyword)
            })
            .map(|(_, uid, jti, meta)| session_proto(jti, *uid as i64, meta, me.user_id))
            .collect();
        items.sort_by(|a, b| {
            b.login_at
                .as_ref()
                .map(|t| t.seconds)
                .cmp(&a.login_at.as_ref().map(|t| t.seconds))
        });
        let total = items.len() as u64;
        let page = req.page.unwrap_or(1).max(1) as usize;
        let size = req.page_size.unwrap_or(10).max(1) as usize;
        let page_items: Vec<OnlineSession> = items
            .into_iter()
            .skip((page - 1) * size)
            .take(size)
            .collect();
        Ok(ListOnlineSessionResponse {
            items: page_items,
            total,
        })
    }

    async fn force_logout_session(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: ForceLogoutSessionRequest,
    ) -> Result<ForceLogoutSessionResponse, StatusError> {
        let uid = req.user_id.unwrap_or(0);
        if uid == 0 {
            return Err(crate::state::status_error(
                "BAD_REQUEST",
                "user_id required",
            ));
        }
        match req.jti.as_deref() {
            Some(jti) if !jti.is_empty() => {
                self.state.tokens.revoke_token_by_jti(uid as i64, jti).await
            }
            _ => self.state.tokens.revoke_user_token(uid as i64).await,
        }
        Ok(ForceLogoutSessionResponse {})
    }

    async fn list_my_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: ListMyOnlineSessionRequest,
    ) -> Result<ListOnlineSessionResponse, StatusError> {
        let me = crate::state::operator_of(&ctx)?;
        let sessions = self.state.tokens.list_user_sessions(me.user_id).await;
        let items: Vec<OnlineSession> = sessions
            .iter()
            .map(|(jti, meta)| session_proto(jti, me.user_id, meta, me.user_id))
            .collect();
        Ok(ListOnlineSessionResponse {
            total: items.len() as u64,
            items,
        })
    }

    async fn revoke_my_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RevokeMyOnlineSessionRequest,
    ) -> Result<RevokeMyOnlineSessionResponse, StatusError> {
        let me = crate::state::operator_of(&ctx)?;
        let Some(jti) = req.jti.clone().filter(|j| !j.is_empty()) else {
            return Err(crate::state::status_error("BAD_REQUEST", "jti required"));
        };
        let owned = self
            .state
            .tokens
            .list_user_sessions(me.user_id)
            .await
            .iter()
            .any(|(j, _)| *j == jti);
        if !owned {
            return Err(crate::state::status_error(
                "FORBIDDEN",
                "session does not belong to the operator",
            ));
        }
        self.state
            .tokens
            .revoke_token_by_jti(me.user_id, &jti)
            .await;
        Ok(RevokeMyOnlineSessionResponse {})
    }
}
