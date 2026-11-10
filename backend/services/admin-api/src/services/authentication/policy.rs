//! The login-policy gate: per-tenant IP/TIME/DEVICE black/white
//! rules from `sys_login_policies`, evaluated fail-open.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::data::sys_login_policies as login_policies;

use super::AuthenticationService;

impl AuthenticationService {
    /// checkLoginPolicies — black-hit blocks, whitelist presence requires
    /// a hit, per IP/TIME/DEVICE in the checker's order. Fail-open.
    pub(super) async fn check_login_policies(
        &self,
        tenant_id: i64,
        user_id: i64,
        ip: &str,
        device_id: &str,
    ) -> bool {
        let rows = match login_policies::Entity::find()
            .filter(login_policies::Column::TenantId.eq(tenant_id))
            .all(&self.state.db)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return false,
        };
        for method in ["IP", "TIME", "DEVICE"] {
            let mut blacks = Vec::new();
            let mut whites = Vec::new();
            for p in &rows {
                if p.method.as_deref() != Some(method) {
                    continue;
                }
                let target = p.target_id.as_deref().and_then(|t| t.parse::<i64>().ok());
                if let Some(target) = target {
                    if target != 0 && target != user_id {
                        continue;
                    }
                }
                if p.type_column.as_deref() == Some("WHITELIST") {
                    whites.push(p);
                } else {
                    blacks.push(p);
                }
            }
            let matched = |value: &str| -> bool {
                match method {
                    "IP" => crate::policy::ip_matches(ip, value),
                    "TIME" => crate::policy::time_window_matches(value),
                    _ => !device_id.is_empty() && device_id == value,
                }
            };
            for p in &blacks {
                if p.value.as_deref().map(matched).unwrap_or(false) {
                    return true;
                }
            }
            if !whites.is_empty()
                && !whites
                    .iter()
                    .any(|p| p.value.as_deref().map(matched).unwrap_or(false))
            {
                return true;
            }
        }
        false
    }
}
