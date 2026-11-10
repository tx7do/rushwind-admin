//! Authority resolution: turn a user's active roles into the token
//! claims — permission-code check (`system:access_backend`), admin
//! flags, data scopes (`dss`/`dsu`) and hidden fields (`hfs`).

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use rushwind_http_binding::envelope::StatusError;

use crate::state::{internal_error, status_error};
use crate::token::UserTokenPayload;

use crate::data::sys_role_field_permissions as role_field_permissions;
use crate::data::sys_role_org_units as role_org_units;
use crate::data::sys_role_permissions as role_permissions;
use crate::data::sys_roles as roles;
use crate::data::sys_user_roles as user_roles;
use crate::data::sys_users as users;

use super::AuthenticationService;

/// The permission code every backend-capable user must hold
/// (`constants.SystemAccessBackendPermissionCode`).
const SYSTEM_ACCESS_BACKEND: &str = "sys:access_backend";
const PLATFORM_ADMIN_ROLE: &str = "platform:admin";
const TENANT_ADMIN_ROLE: &str = "tenant:manager";

impl AuthenticationService {
    /// resolveUserAuthority + authorizeAndEnrich (OneToOne relation): the
    /// user's roles → permission codes must contain
    /// `system:access_backend`; roles/admin-flags/data-scope/hidden-field
    /// claims aggregate from the valid roles.
    pub(super) async fn resolve_authority(
        &self,
        payload: &mut UserTokenPayload,
    ) -> Result<(), StatusError> {
        let uid = payload.user_id;
        let _tid = payload.tenant_id;

        let role_rows = user_roles::Entity::find()
            .filter(
                Condition::all()
                    .add(user_roles::Column::UserId.eq(uid))
                    .add(user_roles::Column::Status.eq("ACTIVE")),
            )
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        let role_ids: Vec<i64> = role_rows.iter().filter_map(|r| r.role_id).collect();

        let perm_rows = if role_ids.is_empty() {
            Vec::new()
        } else {
            role_permissions::Entity::find()
                .filter(role_permissions::Column::RoleId.is_in(role_ids.clone()))
                .all(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?
        };
        let perm_ids: Vec<i64> = perm_rows.iter().filter_map(|r| r.permission_id).collect();

        let codes: Vec<String> = if perm_ids.is_empty() {
            Vec::new()
        } else {
            crate::data::sys_permissions::Entity::find()
                .filter(crate::data::sys_permissions::Column::Id.is_in(perm_ids))
                .all(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?
                .into_iter()
                .map(|p| p.code)
                .collect()
        };

        if !codes.iter().any(|c| c == SYSTEM_ACCESS_BACKEND) {
            return Err(status_error("FORBIDDEN", "insufficient authority"));
        }

        let role_rows = roles::Entity::find()
            .filter(roles::Column::Id.is_in(role_ids))
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;

        payload.roles = role_rows.iter().map(|r| r.code.clone()).collect();
        for code in &payload.roles {
            if code == PLATFORM_ADMIN_ROLE {
                payload.is_platform_admin = Some(true);
            }
            if code == TENANT_ADMIN_ROLE {
                payload.is_tenant_admin = Some(true);
            }
        }

        // dss: the roles' data-scope enum names (unique, order stable).
        let mut dss = Vec::new();
        let mut dsu_units: Vec<i64> = Vec::new();
        for role in &role_rows {
            if let Some(scope) = &role.data_scope {
                if !dss.iter().any(|s| s == scope) {
                    dss.push(scope.clone());
                }
            }
        }
        payload.data_scopes = dss;
        // dsu: the unit targets of UNIT_* scopes (union).
        let unit_roles: Vec<i64> = role_rows
            .iter()
            .filter(|r| {
                matches!(
                    r.data_scope.as_deref(),
                    Some("UNIT_ONLY") | Some("UNIT_AND_CHILD") | Some("SELECTED_UNITS")
                )
            })
            .map(|r| r.id)
            .collect();
        if !unit_roles.is_empty() {
            let units = role_org_units::Entity::find()
                .filter(role_org_units::Column::RoleId.is_in(unit_roles))
                .all(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?;
            for u in units {
                if let Some(oid) = u.org_unit_id {
                    if !dsu_units.contains(&oid) {
                        dsu_units.push(oid);
                    }
                }
            }
            payload.data_scope_unit_ids = Some(
                dsu_units
                    .iter()
                    .map(|u| u.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }

        // hfs: "resource.field" hidden-field entries of the valid roles.
        let hfs = role_field_permissions::Entity::find()
            .filter(
                role_field_permissions::Column::RoleId
                    .is_in(role_rows.iter().map(|r| r.id).collect::<Vec<_>>()),
            )
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        payload.hidden_fields = hfs
            .iter()
            .filter_map(|r| {
                let resource = r.resource.as_ref()?;
                let field = r.field_name.as_ref()?;
                Some(format!("{resource}.{field}"))
            })
            .collect();

        Ok(())
    }

    /// Builds the token payload for an authenticated user (fresh roles,
    /// scopes and hidden fields) — shared by login and refresh.
    pub(super) async fn payload_for_user(
        &self,
        user: users::Model,
        client_id: Option<String>,
        device_id: Option<String>,
    ) -> Result<UserTokenPayload, StatusError> {
        let mut payload = UserTokenPayload {
            user_id: (user.id as u64 as i64),
            tenant_id: (user.tenant_id.unwrap_or(0) as u64 as i64),
            username: user.username.clone(),
            client_id,
            device_id,
            ..Default::default()
        };
        self.resolve_authority(&mut payload).await?;
        Ok(payload)
    }
}
