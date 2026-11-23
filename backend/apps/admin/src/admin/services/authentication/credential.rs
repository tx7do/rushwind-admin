//! Credential gates: `sys_configs` integer reads, the login
//! identifier → username resolution, and the AES+bcrypt password
//! verify with its dummy-hash timing equalizer.

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use rushwind_http_binding::envelope::StatusError;

use crate::admin::state::{internal_error, status_error};

use crate::admin::data::sys_configs as configs;
use crate::admin::data::sys_user_credentials as credentials;
use crate::admin::data::sys_users as users;

use super::AuthenticationService;

impl AuthenticationService {
    /// The `sys_configs` integer read (string value, parse-or-default).
    pub(super) async fn config_int(&self, key: &str, default: i64) -> i64 {
        let row = configs::Entity::find()
            .filter(configs::Column::Key.eq(key))
            .one(&self.state.db)
            .await
            .ok()
            .flatten();
        match row.and_then(|r| r.value) {
            Some(v) => v.parse().unwrap_or(default),
            None => default,
        }
    }

    /// FindUsernameByIdentifier: `@` → email lookup, all-digits → mobile
    /// lookup (ambiguous → 500), miss → input unchanged.
    pub(super) async fn resolve_identifier(
        &self,
        tenant_id: i64,
        input: &str,
    ) -> Result<String, StatusError> {
        if input.is_empty() {
            return Ok(input.to_string());
        }
        let column = if input.contains('@') {
            users::Column::Email
        } else if input.bytes().all(|b| b.is_ascii_digit()) {
            let rows = users::Entity::find()
                .filter(
                    Condition::all()
                        .add(users::Column::TenantId.eq(tenant_id))
                        .add(users::Column::Mobile.eq(input)),
                )
                .all(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?;
            if rows.len() > 1 {
                return Err(internal_error("ambiguous account identifier"));
            }
            return Ok(rows
                .first()
                .map(|u| u.username.clone())
                .unwrap_or_else(|| input.to_string()));
        } else {
            return Ok(input.to_string());
        };
        let row = users::Entity::find()
            .filter(
                Condition::all()
                    .add(users::Column::TenantId.eq(tenant_id))
                    .add(column.eq(input)),
            )
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(row.map(|u| u.username).unwrap_or_else(|| input.to_string()))
    }

    /// FindUserCredential: decrypt → lookup → dummy-verify paths →
    /// bcrypt → password-age policy. Returns the matched user id.
    pub(super) async fn verify_credential(
        &self,
        tenant_id: i64,
        identifier: &str,
        encrypted_password: &str,
    ) -> Result<i64, StatusError> {
        let plain = crate::admin::crypto::decrypt_transfer_password(encrypted_password)
            .map_err(|_| status_error("BAD_REQUEST", "invalid credential format"))?;

        let row = credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(credentials::Column::TenantId.eq(tenant_id))
                    .add(credentials::Column::IdentityType.eq("USERNAME"))
                    .add(credentials::Column::Identifier.eq(identifier)),
            )
            .one(&self.state.db)
            .await;
        let row = match row {
            Ok(Some(row)) => row,
            Ok(None) => {
                crate::admin::crypto::dummy_verify();
                return Err(status_error("USER_NOT_FOUND", "user not found"));
            }
            Err(_) => {
                crate::admin::crypto::dummy_verify();
                return Err(internal_error("db error"));
            }
        };
        let (cred, cred_user_id) = (row.credential.clone(), row.user_id);
        let (Some(cred_user_id), Some(status)) = (cred_user_id, row.status.clone()) else {
            crate::admin::crypto::dummy_verify();
            return Err(status_error("USER_NOT_FOUND", "user not found"));
        };
        if status != "ENABLED" {
            crate::admin::crypto::dummy_verify();
            return Err(status_error("USER_NOT_FOUND", "user not found"));
        }
        if !crate::admin::crypto::verify_password(&plain, &cred) {
            return Err(status_error("INVALID_PASSWORD", "incorrect password"));
        }
        // Password-age policy (sys.password.maxAgeDays, ≤0 disables).
        // 过期信号归一成 INVALID_PASSWORD：过期分支只有"密码正确"的请求
        // 才能到达，任何可区分应答（原 "password expired"）都构成"密码
        // 正确"的 oracle——其他失败路径全部归一，此处是唯一漏网。恢复
        // 入口是 ForgotPassword 流程。
        let max_age = self.config_int("sys.password.maxAgeDays", 90).await;
        if max_age > 0 && row.credential_type.as_deref() == Some("PASSWORD_HASH") {
            if let Some(updated_at) = row.updated_at {
                let age = crate::admin::data::now() - updated_at;
                if age > chrono::Duration::days(max_age) {
                    return Err(status_error("INVALID_PASSWORD", "incorrect password"));
                }
            }
        }
        Ok(cred_user_id)
    }
}
