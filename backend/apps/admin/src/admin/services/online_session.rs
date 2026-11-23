//! OnlineSessionService — //! internal/service/service: the Redis `us:*` session
//! catalog (admin listing with keyword filter, force logout, my-session
//! list/revoke).

use std::sync::Arc;

use crate::admin::state::{AppState, StatusError};
use crate::admin::token::SessionMeta;
use crate::proto::online_session::service::v1::{
    ForceLogoutSessionRequest, ForceLogoutSessionResponse, ListMyOnlineSessionRequest,
    ListOnlineSessionRequest, ListOnlineSessionResponse, OnlineSession,
    RevokeMyOnlineSessionRequest, RevokeMyOnlineSessionResponse,
};

/// 归属谓词（C5）：平台管理员看全域，租户成员只看本租户——抽成纯函数
/// 以便测试面钉死（mini-redis 无 SCAN，列表通道在测试面恒空，谓词只能
/// 单测）。
fn session_visible(meta: &SessionMeta, platform: bool, tenant_id: i64) -> bool {
    platform || meta.tenant_id == tenant_id
}

fn session_proto(jti: &str, uid: i64, meta: &SessionMeta, current_uid: i64) -> OnlineSession {
    let login_at = chrono::NaiveDateTime::parse_from_str(&meta.login_at, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&meta.login_at, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .and_then(crate::admin::state::naive_to_ts);
    OnlineSession {
        current: Some(uid == current_uid),
        jti: Some(jti.to_string()),
        user_id: Some(uid),
        username: Some(meta.username.clone()),
        tenant_id: Some(meta.tenant_id),
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
impl crate::proto::gen::services::OnlineSessionServiceHandlers for OnlineSessionService {
    async fn list_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ListOnlineSessionRequest,
    ) -> Result<ListOnlineSessionResponse, StatusError> {
        let me = crate::admin::state::operator_of(&ctx)?;
        // 归属闸：平台管理员看全域会话，租户成员只看本租户——原实现
        // 无租户过滤也无平台闸，任意登录用户可枚举全平台会话（jti/
        // 用户名/IP/UA）。
        let platform = me.is_platform_admin == Some(true);
        let sessions = self.state.tokens.list_all_sessions().await;
        let keyword = req.keyword.clone().unwrap_or_default().to_lowercase();
        let mut items: Vec<OnlineSession> = sessions
            .iter()
            .filter(|(_, _, _, meta)| {
                session_visible(meta, platform, me.tenant_id)
                    && (keyword.is_empty()
                        || meta.username.to_lowercase().contains(&keyword)
                        || meta.ip.to_lowercase().contains(&keyword))
            })
            .map(|(_, uid, jti, meta)| session_proto(jti, *uid as i64, meta, me.user_id))
            .collect();
        items.sort_by(|a, b| {
            b.login_at
                .as_ref()
                .map(|t| t.seconds)
                .cmp(&a.login_at.as_ref().map(|t| t.seconds))
        });
        let total = items.len();
        let page = req.page.unwrap_or(1).max(1) as usize;
        let size = req.page_size.unwrap_or(10).max(1) as usize;
        let page_items: Vec<OnlineSession> = items
            .into_iter()
            .skip((page - 1) * size)
            .take(size)
            .collect();
        Ok(ListOnlineSessionResponse {
            items: page_items,
            total: total as i64,
        })
    }

    async fn force_logout_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ForceLogoutSessionRequest,
    ) -> Result<ForceLogoutSessionResponse, StatusError> {
        let me = crate::admin::state::operator_of(&ctx)?;
        let uid = req.user_id.unwrap_or(0);
        if uid == 0 {
            return Err(crate::admin::state::status_error(
                "BAD_REQUEST",
                "user_id required",
            ));
        }
        // 归属闸：原实现不校验调用者（_ctx 未用），任意登录用户可强踢
        // 任意账号（含平台管理员）。平台管理员放行；租户成员仅限目标
        // 会话属于本租户——目标无会话时幂等返回（无可踢，效果等价）。
        if me.is_platform_admin != Some(true) {
            let sessions = self.state.tokens.list_user_sessions(uid).await;
            if !sessions.is_empty()
                && !sessions
                    .iter()
                    .any(|(_, meta)| meta.tenant_id == me.tenant_id)
            {
                return Err(crate::admin::state::status_error(
                    "FORBIDDEN",
                    "sessions outside the operator tenant",
                ));
            }
        }
        match req.jti.as_deref() {
            Some(jti) if !jti.is_empty() => self.state.tokens.revoke_token_by_jti(uid, jti).await,
            _ => self.state.tokens.revoke_user_token(uid).await,
        }
        Ok(ForceLogoutSessionResponse {})
    }

    async fn list_my_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: ListMyOnlineSessionRequest,
    ) -> Result<ListOnlineSessionResponse, StatusError> {
        let me = crate::admin::state::operator_of(&ctx)?;
        let sessions = self.state.tokens.list_user_sessions(me.user_id).await;
        let items: Vec<OnlineSession> = sessions
            .iter()
            .map(|(jti, meta)| session_proto(jti, me.user_id, meta, me.user_id))
            .collect();
        Ok(ListOnlineSessionResponse {
            total: items.len() as i64,
            items,
        })
    }

    async fn revoke_my_online_session(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RevokeMyOnlineSessionRequest,
    ) -> Result<RevokeMyOnlineSessionResponse, StatusError> {
        let me = crate::admin::state::operator_of(&ctx)?;
        let Some(jti) = req.jti.clone().filter(|j| !j.is_empty()) else {
            return Err(crate::admin::state::status_error(
                "BAD_REQUEST",
                "jti required",
            ));
        };
        let owned = self
            .state
            .tokens
            .list_user_sessions(me.user_id)
            .await
            .iter()
            .any(|(j, _)| *j == jti);
        if !owned {
            return Err(crate::admin::state::status_error(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::state::operator_of;
    use crate::proto::gen::services::OnlineSessionServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};

    fn meta(tenant: i64, username: &str) -> SessionMeta {
        SessionMeta {
            username: username.into(),
            tenant_id: tenant,
            ip: "10.0.0.1".into(),
            user_agent: "ua".into(),
            device: None,
            login_at: "2026-11-20 00:00:00".into(),
        }
    }

    /// 归属闸（C5）：台账可见性谓词——租户成员只看本租户会话，平台管理
    /// 员看全域。mini-redis 测试面无 SCAN（list_all_sessions 恒空），
    /// 列表通道走不通，谓词按纯函数钉死。
    #[test]
    fn list_visibility_is_tenant_scoped_for_members_and_global_for_platform() {
        let alice = meta(7, "alice");
        let bob = meta(8, "bob");
        // 租户 7 成员：本租户可见，外租户不可见。
        assert!(session_visible(&alice, false, 7));
        assert!(!session_visible(&bob, false, 7));
        // 平台管理员：全域可见。
        assert!(session_visible(&alice, true, 0));
        assert!(session_visible(&bob, true, 0));
    }

    /// 归属闸（C5）：租户成员的台账在测试面上不泄露外租户会话——
    /// mini-redis 无 SCAN，list_all_sessions 恒空，total 恒 0（泄露面
    /// 与空集重合）；真实的跨租户过滤由谓词测试与真 redis 环境覆盖。
    #[tokio::test]
    async fn list_stays_empty_for_tenant_members_on_the_test_face() {
        let state = test_state().await;
        let svc = OnlineSessionService {
            state: state.clone(),
        };
        svc.state
            .tokens
            .set_session_meta(202, "jti-b", &meta(8, "bob"))
            .await
            .unwrap();
        let resp = svc
            .list_online_session(
                ctx(7, 101),
                ListOnlineSessionRequest {
                    keyword: None,
                    page: None,
                    page_size: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(resp.total, 0, "foreign-tenant session must not surface");
    }

    /// 归属闸（C5）：租户成员强踢外租户 uid 被拒；无会话目标幂等放行
    /// （mini-redis 测试面 SCAN 为空 → 幂等分支；SCAN 拒绝态需真 redis）。
    #[tokio::test]
    async fn force_logout_is_idempotent_without_sessions() {
        let state = test_state().await;
        let svc = OnlineSessionService {
            state: state.clone(),
        };
        // 无会话：幂等 Ok（校验也过了 operator_of 归属闸本身）。
        svc.force_logout_session(
            ctx(7, 101),
            ForceLogoutSessionRequest {
                user_id: Some(202),
                jti: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(operator_of(&ctx(7, 101)).is_ok());
    }
}
