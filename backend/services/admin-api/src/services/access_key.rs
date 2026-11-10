//! AccessKeyService — CRUD over `sys_access_keys` plus the
//! unauthenticated AK/SK→token exchange (`IssueToken`): SHA-256 secret
//! verification with constant-time compare, rate limiting keyed by IP+AK,
//! and a machine-token mint (uid 0, roles ["machine"]) whitelisted in
//! Redis.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use pbjson_types::Empty;
use proto::gen::services::AccessKeyServiceHandlers;
use proto::proto::access_key::service::v1::{
    AccessKey, CreateAccessKeyRequest, CreateAccessKeyResponse, DeleteAccessKeyRequest,
    GetAccessKeyRequest, IssueTokenRequest, IssueTokenResponse, ListAccessKeyResponse,
    ResetAccessKeySecretRequest, UpdateAccessKeyRequest,
};
use proto::proto::pagination::PagingRequest;

use crate::state::{internal_error, status_error, AppState};
use crate::token::{new_jwt_id, UserTokenPayload};
use rushwind_authn::Authenticator as _;

pub struct AccessKeyService {
    pub state: Arc<AppState>,
}

fn new_access_key() -> String {
    format!("ak-{}", hex::encode(rand::random::<[u8; 8]>()))
}

fn new_secret() -> String {
    format!("sk-{}", hex::encode(rand::random::<[u8; 32]>()))
}

impl AccessKeyService {
    fn to_proto(&self, row: crate::data::sys_access_keys::Model) -> AccessKey {
        AccessKey {
            id: Some(row.id as u64),
            name: Some(row.name),
            access_key: Some(row.access_key),
            status: Some(if row.status.as_deref() == Some("OFF") {
                0
            } else {
                1
            }),
            expires_at: row.expires_at.and_then(crate::state::naive_to_ts),
            last_used_at: row.last_used_at.and_then(crate::state::naive_to_ts),
            tenant_id: row.tenant_id.map(|v| v as u64),
            created_by: row.created_by.map(|v| v as u64),
            updated_by: row.updated_by.map(|v| v as u64),
            deleted_by: row.deleted_by.map(|v| v as u64),
            created_at: row.created_at.and_then(crate::state::naive_to_ts),
            updated_at: row.updated_at.and_then(crate::state::naive_to_ts),
            deleted_at: row.deleted_at.and_then(crate::state::naive_to_ts),
        }
    }
}

#[async_trait::async_trait]
impl AccessKeyServiceHandlers for AccessKeyService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAccessKeyResponse, crate::state::StatusError> {
        let _payload = crate::state::operator_of(&ctx)?;
        let repo = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAccessKeyResponse {
            items: rows.into_iter().map(|r| self.to_proto(r)).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAccessKeyRequest,
    ) -> Result<AccessKey, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let repo = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let row = match req.query_by {
            Some(proto::proto::access_key::service::v1::get_access_key_request::QueryBy::Id(
                id,
            )) => Some(repo.get_by_id(id as i64).await?),
            Some(
                proto::proto::access_key::service::v1::get_access_key_request::QueryBy::AccessKey(
                    ak,
                ),
            ) => {
                repo.find_by_access_key_for_tenant(payload.tenant_id, &ak)
                    .await?
            }
            None => None,
        }
        .ok_or_else(|| status_error("ACCESS_KEY_NOT_FOUND", "access key not found"))?;
        Ok(self.to_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateAccessKeyRequest,
    ) -> Result<CreateAccessKeyResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let data = req.data.unwrap_or_default();
        let secret = new_secret();
        let row = crate::data::sys_access_keys::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.unwrap_or_else(|| "unnamed".into())),
            access_key: Set(new_access_key()),
            secret_hash: Set(crate::crypto::sha256_hex(secret.as_bytes())),
            expires_at: Set(data.expires_at.and_then(|t| crate::state::ts_to_naive(&t))),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        let inserted = row
            .insert(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(CreateAccessKeyResponse {
            data: Some(self.to_proto(inserted)),
            secret,
        })
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAccessKeyRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let repo = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let mut active: crate::data::sys_access_keys::ActiveModel =
            repo.get_by_id(req.id as i64).await?.into();
        if let Some(data) = &req.data {
            if let Some(name) = &data.name {
                active.name = Set(name.clone());
            }
            if let Some(expires_at) = &data.expires_at {
                active.expires_at = Set(crate::state::ts_to_naive(expires_at));
            }
            if let Some(status) = data.status {
                active.status = Set(Some(if status == 0 {
                    "OFF".into()
                } else {
                    "ON".into()
                }));
            }
        }
        crate::stamp_update!(active, payload.user_id);
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAccessKeyRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        let _payload = crate::state::operator_of(&ctx)?;
        let repo = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        repo.get_by_id(req.id as i64).await?;
        repo.delete_by_id(req.id as i64).await?;
        Ok(Empty {})
    }

    async fn reset_secret(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ResetAccessKeySecretRequest,
    ) -> Result<CreateAccessKeyResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let repo = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let secret = new_secret();
        let mut active: crate::data::sys_access_keys::ActiveModel =
            repo.get_by_id(req.id as i64).await?.into();
        active.secret_hash = Set(crate::crypto::sha256_hex(secret.as_bytes()));
        crate::stamp_update!(active, payload.user_id);
        let updated = active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(CreateAccessKeyResponse {
            data: Some(self.to_proto(updated)),
            secret,
        })
    }

    async fn issue_token(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: IssueTokenRequest,
    ) -> Result<IssueTokenResponse, crate::state::StatusError> {
        if req.access_key.is_empty() || req.secret.is_empty() {
            return Err(status_error("BAD_REQUEST", "invalid access key or secret"));
        }
        // Rate limit dimensioned by IP + AK.
        if crate::ratelimit::is_locked(&self.state.redis, &ctx.ip, &req.access_key).await {
            return Err(status_error(
                "BAD_REQUEST",
                "too many failures, please try again later",
            ));
        }
        let row = crate::data::repos::AccessKeyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        )
        .find_by_access_key(&req.access_key.clone())
        .await?;
        let Some(row) = row else {
            crate::ratelimit::check_and_incr(&self.state.redis, &ctx.ip, &req.access_key).await;
            return Err(status_error("BAD_REQUEST", "invalid access key or secret"));
        };
        if row.status.as_deref() != Some("ON") {
            return Err(status_error("BAD_REQUEST", "invalid access key or secret"));
        }
        if let Some(expires_at) = row.expires_at {
            if crate::data::now() >= expires_at {
                return Err(status_error("BAD_REQUEST", "invalid access key or secret"));
            }
        }
        // Constant-time compare of SHA-256 hex digests.
        let digest = crate::crypto::sha256_hex(req.secret.as_bytes());
        if !constant_time_eq(digest.as_bytes(), row.secret_hash.as_bytes()) {
            crate::ratelimit::check_and_incr(&self.state.redis, &ctx.ip, &req.access_key).await;
            return Err(status_error("BAD_REQUEST", "invalid access key or secret"));
        }

        let payload = UserTokenPayload {
            username: format!("ak:{}", row.access_key),
            user_id: 0,
            tenant_id: (row.tenant_id.unwrap_or(0) as u64 as i64),
            roles: vec!["machine".into()],
            ..Default::default()
        };
        let payload = UserTokenPayload {
            jti: new_jwt_id(),
            ..payload
        };
        let now = chrono::Utc::now().timestamp();
        let access = self
            .state
            .jwt
            .create_identity(&rushwind_authn::AuthClaims(
                payload.to_access_claims(now + self.state.tokens.access_expires_secs),
            ))
            .map_err(|_| internal_error("create access token failed"))?;
        // Machine tokens carry uid 0 in the whitelist key (at:0:0:{jti}).
        self.state
            .tokens
            .add_access_token(0, &payload.jti, &access)
            .await
            .map_err(internal_error)?;
        let _ = ctx;
        Ok(IssueTokenResponse {
            access_token: access,
            expires_in: self.state.tokens.access_expires_secs as u32,
            token_type: "bearer".into(),
        })
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
