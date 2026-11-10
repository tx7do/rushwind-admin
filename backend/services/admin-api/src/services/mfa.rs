//! MfaService — `internal/service/module`:
//! TOTP enrollment (secret cached server-side until confirm), status
//! listing, disable/revoke, and the unauthenticated challenge completion
//! that issues the token pair after a successful code.

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::state::{internal_error, status_error, AppState, StatusError};
use crate::token::{new_jwt_id, SessionMeta, UserTokenPayload};
use proto::gen::services::MfaServiceHandlers;
use proto::proto::authentication::service::v1::{
    confirm_enroll_method_request, disable_mfa_request, start_enroll_method_response,
    verify_mfa_challenge_request, ConfirmEnrollMethodRequest, ConfirmEnrollMethodResponse,
    DisableMfaRequest, EnrolledMethod, GetMfaStatusRequest, GetMfaStatusResponse,
    ListEnrolledMethodsRequest, ListEnrolledMethodsResponse, LoginResponse, RevokeMfaDeviceRequest,
    StartEnrollMethodRequest, StartEnrollMethodResponse, TotpResult, VerifyMfaChallengeRequest,
};
use rushwind_authn::Authenticator as _;

pub struct MfaService {
    pub state: Arc<AppState>,
}

const ENROLL_TTL: u64 = 300;

impl MfaService {
    fn ctx_of(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
    ) -> Result<(i64, i64, String), StatusError> {
        let payload = crate::state::operator_of(ctx)?;
        Ok((payload.user_id, payload.tenant_id, payload.username))
    }

    /// 启用中的 TOTP 因子（step-up 校验用——上游 FindEnabledTotpForUser 语义，
    /// status=ENABLED 才可参与验证）。
    async fn enabled_totp_for(
        &self,
        tenant_id: i64,
        user_id: i64,
    ) -> Option<crate::data::sys_user_mfa_factors::Model> {
        crate::data::sys_user_mfa_factors::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_mfa_factors::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(user_id))
                    .add(crate::data::sys_user_mfa_factors::Column::Method.eq("TOTP"))
                    .add(crate::data::sys_user_mfa_factors::Column::Status.eq("ENABLED")),
            )
            .one(&self.state.db)
            .await
            .ok()
            .flatten()
    }

    /// 同码重放防线（P3 加固）：记录 (uid, code) 组合，90 秒内（TOTP ±skew
    /// 窗口）重复出现即拒绝。Redis 异常放行（防御性优化，不阻断正常流程；
    /// 主防线是挑战/因子校验本体）。
    async fn try_acquire_used_totp_code(&self, user_id: i64, code: &str) -> bool {
        let mut conn = self.state.redis.clone();
        let key = format!("mfa:usedcode:{user_id}:{code}");
        match redis::cmd("SET")
            .arg(&key)
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(90)
            .query_async::<Option<()>>(&mut conn)
            .await
        {
            Err(_) => true,       // Redis 异常放行
            Ok(Some(())) => true, // 首次出现，获取成功
            Ok(None) => false,    // 90 秒内重复出现 → 重放拒绝
        }
    }

    async fn factor_for(
        &self,
        tenant_id: i64,
        user_id: i64,
    ) -> Option<crate::data::sys_user_mfa_factors::Model> {
        crate::data::sys_user_mfa_factors::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_mfa_factors::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(user_id))
                    .add(crate::data::sys_user_mfa_factors::Column::Method.eq("TOTP")),
            )
            .one(&self.state.db)
            .await
            .ok()
            .flatten()
    }

    async fn issue_after_verify(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        payload: &UserTokenPayload,
    ) -> Result<LoginResponse, StatusError> {
        let payload = UserTokenPayload {
            jti: new_jwt_id(),
            ..payload.clone()
        };
        let now = chrono::Utc::now().timestamp();
        let access = self
            .state
            .jwt
            .create_identity(&rushwind_authn::AuthClaims(
                payload.to_access_claims(now + self.state.tokens.access_expires_secs),
            ))
            .map_err(|_| internal_error("create access token failed"))?;
        let refresh = self
            .state
            .jwt
            .create_identity(&rushwind_authn::AuthClaims(
                payload.to_refresh_claims(now + self.state.tokens.refresh_expires_secs),
            ))
            .map_err(|_| internal_error("create refresh token failed"))?;
        self.state
            .tokens
            .add_token_pair(payload.user_id, &payload.jti, &access, &refresh)
            .await
            .map_err(internal_error)?;
        let meta = SessionMeta {
            ip: ctx.ip.clone(),
            user_agent: ctx.user_agent.clone(),
            username: payload.username.clone(),
            tenant_id: payload.tenant_id,
            device: payload.device_id.clone(),
            login_at: chrono::Local::now().naive_local().to_string(),
        };
        let _ = self
            .state
            .tokens
            .set_session_meta(payload.user_id, &payload.jti, &meta)
            .await;
        let secure = ctx
            .headers
            .get("x-forwarded-proto")
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        let (rt_cookie, exp_cookie) = self.state.tokens.refresh_cookie_values(&refresh, secure);
        ctx.add_reply_header("Set-Cookie", rt_cookie);
        ctx.add_reply_header("Set-Cookie", exp_cookie);
        Ok(LoginResponse {
            token_type: 0,
            access_token: access,
            expires_in: self.state.tokens.access_expires_secs,
            scope: None,
            id_token: None,
            mfa_operation_id: None,
        })
    }
}

#[async_trait::async_trait]
impl MfaServiceHandlers for MfaService {
    async fn get_m_f_a_status(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetMfaStatusRequest,
    ) -> Result<GetMfaStatusResponse, crate::state::StatusError> {
        let (uid, tid, _) = self.ctx_of(&ctx)?;
        let _ = &req; // keys off the operator identity
        let factor = self.factor_for(tid, uid).await;
        let mut enrolled = Vec::new();
        if let Some(f) = &factor {
            enrolled.push(EnrolledMethod {
                id: f.id.to_string(),
                method: 0, // MfaMethod::TOTP
                display: f.display_name.clone().unwrap_or_else(|| "TOTP".into()),
                enabled: f.status.as_deref() == Some("ENABLED"),
                created_at: f.created_at.and_then(crate::state::naive_to_ts),
                last_used_at: f.last_used_at.and_then(crate::state::naive_to_ts),
            });
        }
        Ok(GetMfaStatusResponse {
            enabled: factor
                .map(|f| f.status.as_deref() == Some("ENABLED"))
                .unwrap_or(false),
            enrolled,
            enforcement: 0, // OFF
        })
    }

    async fn list_enrolled_methods(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: ListEnrolledMethodsRequest,
    ) -> Result<ListEnrolledMethodsResponse, crate::state::StatusError> {
        let (uid, tid, _) = self.ctx_of(&ctx)?;
        let mut items = Vec::new();
        if let Some(f) = self.factor_for(tid, uid).await {
            items.push(EnrolledMethod {
                id: f.id.to_string(),
                method: 0,
                display: f.display_name.clone().unwrap_or_else(|| "TOTP".into()),
                enabled: f.status.as_deref() == Some("ENABLED"),
                created_at: f.created_at.and_then(crate::state::naive_to_ts),
                last_used_at: f.last_used_at.and_then(crate::state::naive_to_ts),
            });
        }
        Ok(ListEnrolledMethodsResponse { items })
    }

    async fn start_enroll_method(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: StartEnrollMethodRequest,
    ) -> Result<StartEnrollMethodResponse, crate::state::StatusError> {
        let (uid, tid, _) = self.ctx_of(&ctx)?;
        // Only TOTP is enrollable end-to-end; SMS/WebAuthn
        // branches need external providers).
        if req.method != 0 {
            return Err(status_error("BAD_REQUEST", "unsupported mfa method"));
        }
        if self.factor_for(tid, uid).await.is_some() {
            return Err(status_error("BAD_REQUEST", "mfa method already enrolled"));
        }
        let op_id = new_jwt_id();
        let secret_bytes = rand::random::<[u8; 20]>();
        let secret = crate::crypto::base32_encode(&secret_bytes);
        let account = format!("uid:{uid}");
        let otp_auth_url = format!("otpauth://totp/RushWindAdmin:{account}?secret={secret}&issuer=RushWindAdmin&algorithm=SHA1&digits=6&period=30");
        // QR as a PNG data URI.
        // SVG rendering needs no native image stack; the data URI rides
        // the same wire field fills with a PNG.
        let qr =
            qrcode::QrCode::new(otp_auth_url.as_bytes()).map_err(|_| internal_error("qr code"))?;
        let svg = qr
            .render::<qrcode::render::svg::Color>()
            .quiet_zone(false)
            .build();
        use base64::Engine as _;
        let qr_data_uri = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        );
        let mut conn = self.state.redis.clone();
        let _: Result<(), _> = redis::AsyncCommands::set_ex(
            &mut conn,
            format!("mfa:enroll:{op_id}"),
            serde_json::json!({ "userId": uid, "tenantId": tid, "secret": secret }).to_string(),
            ENROLL_TTL,
        )
        .await;
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ENROLL_TTL as i64);
        Ok(StartEnrollMethodResponse {
            expires_at: Some(crate::state::naive_to_ts(expires_at.naive_utc()).unwrap()),
            operation_id: op_id,
            result: Some(start_enroll_method_response::Result::Totp(TotpResult {
                secret,
                otp_auth_url,
                qr_code_data_uri: qr_data_uri,
            })),
        })
    }

    async fn confirm_enroll_method(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ConfirmEnrollMethodRequest,
    ) -> Result<ConfirmEnrollMethodResponse, crate::state::StatusError> {
        let (uid, tid, _) = self.ctx_of(&ctx)?;
        let mut conn = self.state.redis.clone();
        let raw: Option<String> =
            redis::AsyncCommands::get(&mut conn, format!("mfa:enroll:{}", req.operation_id))
                .await
                .ok()
                .flatten();
        let Some(raw) = raw else {
            return Err(status_error("BAD_REQUEST", "enroll operation expired"));
        };
        let data: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| internal_error(e.to_string()))?;
        let secret = data["secret"]
            .as_str()
            .ok_or_else(|| internal_error("enroll payload"))?
            .to_string();
        let stored_uid = data["userId"].as_i64().unwrap_or(0);
        let stored_tid = data["tenantId"].as_i64().unwrap_or(0);
        if stored_uid != uid || stored_tid != tid {
            return Err(status_error("FORBIDDEN", "enroll operation owner mismatch"));
        }
        let code = match &req.credential {
            Some(confirm_enroll_method_request::Credential::TotpCode(code)) => code.clone(),
            _ => String::new(),
        };
        if !crate::crypto::totp_verify(&secret, &code) {
            return Err(status_error("FORBIDDEN", "invalid mfa code"));
        }
        let factor = crate::data::sys_user_mfa_factors::ActiveModel {
            tenant_id: Set(Some(tid)),
            user_id: Set(Some(uid)),
            method: Set(Some("TOTP".into())),
            secret_hash: Set(crate::crypto::encrypt_if_needed(&secret)),
            display_name: Set(Some("TOTP".into())),
            status: Set(Some("ENABLED".into())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        let inserted = factor
            .insert(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        let _: Result<i64, _> =
            redis::AsyncCommands::del(&mut conn, format!("mfa:enroll:{}", req.operation_id)).await;
        Ok(ConfirmEnrollMethodResponse {
            success: true,
            credential_id: inserted.id.to_string(),
        })
    }

    async fn disable_m_f_a(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DisableMfaRequest,
    ) -> Result<pbjson_types::Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let (uid, tid, _) = self.ctx_of(&ctx)?;

        // 管理端救援路径：目标用户由 user_id 指定（≠ 本人），仅平台管理员，
        // 用于用户认证器丢失时的救援重置（按 credential_id 精确删，或按
        // method 清空该用户该方法全部因子），留告警日志。
        let target = req.user_id.map(|v| v as i64).unwrap_or(0);
        if target != 0 && target != uid {
            if payload.is_platform_admin != Some(true) {
                return Err(status_error(
                    "FORBIDDEN",
                    "only platform admin can reset mfa for others",
                ));
            }
            if let Some(id) = req
                .credential_id
                .as_deref()
                .and_then(|v| v.parse::<u64>().ok())
            {
                // 凭 credential_id 定位因子行（目标用户可能在任意租户）
                let factor = crate::data::sys_user_mfa_factors::Entity::find_by_id(id as i64)
                    .one(&self.state.db)
                    .await
                    .map_err(|e| internal_error(format!("db: {e}")))?;
                let factor = match factor {
                    Some(f) if f.user_id == Some(target) => f,
                    _ => return Err(status_error("NOT_FOUND", "mfa credential not found")),
                };
                crate::data::sys_user_mfa_factors::Entity::delete_by_id(factor.id)
                    .exec(&self.state.db)
                    .await
                    .map_err(|e| internal_error(format!("db: {e}")))?;
            } else {
                if req.method.unwrap_or(0) != 0 {
                    return Err(status_error("BAD_REQUEST", "unsupported mfa method"));
                }
                // 以平台管理员视角全量查找其因子行定位 tenant
                let first = crate::data::sys_user_mfa_factors::Entity::find()
                    .filter(
                        Condition::all()
                            .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(target))
                            .add(crate::data::sys_user_mfa_factors::Column::Method.eq("TOTP")),
                    )
                    .one(&self.state.db)
                    .await
                    .map_err(|e| internal_error(format!("db: {e}")))?;
                let first = match first {
                    Some(f) => f,
                    None => return Err(status_error("NOT_FOUND", "mfa credential not found")),
                };
                crate::data::sys_user_mfa_factors::Entity::delete_many()
                    .filter(
                        Condition::all()
                            .add(
                                crate::data::sys_user_mfa_factors::Column::TenantId
                                    .eq(first.tenant_id.unwrap_or(0)),
                            )
                            .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(target))
                            .add(crate::data::sys_user_mfa_factors::Column::Method.eq("TOTP")),
                    )
                    .exec(&self.state.db)
                    .await
                    .map_err(|e| internal_error(format!("db: {e}")))?;
            }
            eprintln!("[mfa] admin [{uid}] reset mfa for user [{target}]");
            return Ok(pbjson_types::Empty {});
        }

        // 本人路径 step-up（P3-1 修复）：拆掉第二因子是高价值目标——会话被
        // 劫持的攻击者可先 Disable 再绑定自己的 TOTP 持久化。proto 的
        // verifier oneof 此前从未被消费：现强制要求当前有效 TOTP 码（劫持
        // 会话不含 TOTP secret，无法生成有效码）。刻意只接受 TOTP：口令可能
        // 已随会话被劫持者掌握，TOTP 码才是劫持者缺失的一环（第二因子的本义）。
        // 管理端救援路径不受限（场景即"认证器丢失"，且已有平台管理员闸门）。
        let code = match req.verifier.as_ref() {
            Some(disable_mfa_request::Verifier::TotpCode(code)) if !code.is_empty() => code.clone(),
            _ => {
                return Err(status_error(
                    "BAD_REQUEST",
                    "step-up verification required: current totp_code is required to disable mfa",
                ));
            }
        };
        let Some(factor) = self.enabled_totp_for(tid, uid).await else {
            return Err(status_error(
                "FORBIDDEN",
                "no enabled totp factor to verify",
            ));
        };
        let secret =
            crate::crypto::decrypt_if_needed(&factor.secret_hash).map_err(internal_error)?;
        if !crate::crypto::totp_verify(&secret, &code) {
            return Err(status_error("FORBIDDEN", "invalid mfa code"));
        }
        // 同码重放防线与登录挑战同款
        if !self.try_acquire_used_totp_code(uid, &code).await {
            return Err(status_error("FORBIDDEN", "invalid mfa code"));
        }

        // 本人路径：按 credential_id 精确解绑；未传时清空本人该方法全部因子
        // （兑现 proto 注释"指定凭证 id 或仅按方法禁用全部"的契约）
        let mut delete = crate::data::sys_user_mfa_factors::Entity::delete_many().filter(
            Condition::all()
                .add(crate::data::sys_user_mfa_factors::Column::TenantId.eq(tid))
                .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(uid)),
        );
        if let Some(id) = req
            .credential_id
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
        {
            delete = delete.filter(crate::data::sys_user_mfa_factors::Column::Id.eq(id));
        }
        delete
            .exec(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(pbjson_types::Empty {})
    }

    async fn revoke_m_f_a_device(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RevokeMfaDeviceRequest,
    ) -> Result<pbjson_types::Empty, crate::state::StatusError> {
        let (uid, tid, _) = self.ctx_of(&ctx)?;
        let id: i64 = req
            .credential_id
            .parse()
            .map_err(|_| status_error("BAD_REQUEST", "invalid credential id"))?;
        crate::data::sys_user_mfa_factors::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_mfa_factors::Column::TenantId.eq(tid))
                    .add(crate::data::sys_user_mfa_factors::Column::UserId.eq(uid))
                    .add(crate::data::sys_user_mfa_factors::Column::Id.eq(id)),
            )
            .exec(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(pbjson_types::Empty {})
    }

    async fn verify_m_f_a_challenge(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: VerifyMfaChallengeRequest,
    ) -> Result<LoginResponse, crate::state::StatusError> {
        let op_id = req.operation_id.clone();
        // Peek the challenge (deleted only on success).
        let payload: UserTokenPayload = {
            let mut conn = self.state.redis.clone();
            let raw: Option<String> =
                redis::AsyncCommands::get(&mut conn, format!("mfa:login:{op_id}"))
                    .await
                    .ok()
                    .flatten();
            match raw {
                Some(raw) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&raw).map_err(|e| internal_error(e.to_string()))?;
                    let username = value["payload"]["username"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    ctx.add_reply_header("X-Audit-Username", username);
                    serde_json::from_value(value["payload"].clone())
                        .map_err(|e| internal_error(e.to_string()))?
                }
                None => {
                    return Err(status_error(
                        "FORBIDDEN",
                        "invalid or expired mfa challenge",
                    ));
                }
            }
        };

        let code = match req.response.as_ref() {
            Some(verify_mfa_challenge_request::Response::TotpCode(code)) => code.clone(),
            _ => String::new(),
        };

        let factor = self
            .factor_for(payload.tenant_id, payload.user_id)
            .await
            .ok_or_else(|| status_error("FORBIDDEN", "mfa is not enrolled"))?;
        let secret =
            crate::crypto::decrypt_if_needed(&factor.secret_hash).map_err(internal_error)?;
        if !crate::crypto::totp_verify(&secret, &code) {
            // Failure counting: 3 strikes kill the challenge.
            let mut conn = self.state.redis.clone();
            let fail_key = format!("mfa:loginfail:{op_id}");
            let fails: i64 = redis::AsyncCommands::incr(&mut conn, &fail_key, 1i64)
                .await
                .unwrap_or(1);
            let _: Result<(), _> = redis::AsyncCommands::expire(&mut conn, &fail_key, 300i64).await;
            if fails >= 3 {
                let _: Result<i64, _> =
                    redis::AsyncCommands::del(&mut conn, format!("mfa:login:{op_id}")).await;
                let _: Result<i64, _> = redis::AsyncCommands::del(&mut conn, &fail_key).await;
                return Err(status_error(
                    "FORBIDDEN",
                    "too many invalid mfa attempts, please login again",
                ));
            }
            return Err(status_error("FORBIDDEN", "invalid mfa code"));
        }

        // P3 加固：同码重放防线——TOTP 在 ±skew 窗口内同一码值有效 ~90s，
        // 攻击者截获一次提交可在窗口内对多个 operation_id 重放同码。SETNX
        // 记录 (uid, code)，已存在即拒绝。挑战消耗后码值不再可用，窗口期
        // 天然封死。
        if !self
            .try_acquire_used_totp_code(payload.user_id, &code)
            .await
        {
            return Err(status_error("FORBIDDEN", "mfa code already used"));
        }

        // Success: single-use (delete challenge + failures), last-used stamp,
        // rate limiter reset, then the token pair.
        let mut conn = self.state.redis.clone();
        let _: Result<i64, _> =
            redis::AsyncCommands::del(&mut conn, format!("mfa:login:{op_id}")).await;
        let _: Result<i64, _> =
            redis::AsyncCommands::del(&mut conn, format!("mfa:loginfail:{op_id}")).await;
        {
            let mut active: crate::data::sys_user_mfa_factors::ActiveModel = factor.into();
            active.last_used_at = Set(Some(crate::data::now()));
            let _ = active.update(&self.state.db).await;
        }
        crate::ratelimit::reset(&self.state.redis, &ctx.ip, &payload.username).await;
        self.issue_after_verify(&ctx, &payload).await
    }
}
