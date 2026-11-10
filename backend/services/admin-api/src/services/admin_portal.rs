//! AdminPortalService: the
//! post-login surface (navigation tree from the roles' menus, permission
//! codes, and the combined initial context).

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use pbjson_types::Empty;
use proto::gen::services::AdminPortalServiceHandlers;
use proto::proto::admin::service::v1::{
    InitialContextResponse, ListPermissionCodeResponse, ListRouteResponse,
};
use proto::proto::permission::service::v1::MenuRouteItem;

use crate::state::{internal_error, AppState};

use super::menu::menu_meta_from_json;
use super::user::{load_user, user_to_proto};

pub struct AdminPortalService {
    pub state: Arc<AppState>,
}

impl AdminPortalService {
    /// The role-permitted menu ids → menu rows (non-BUTTON, status ON).
    async fn permitted_menus(
        &self,
        uid: i64,
    ) -> Result<Vec<crate::data::sys_menus::Model>, crate::state::StatusError> {
        let role_ids: Vec<i64> = crate::data::sys_user_roles::Entity::find()
            .filter(crate::data::sys_user_roles::Column::UserId.eq(uid))
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .iter()
            .filter_map(|r| r.role_id)
            .collect();
        let mut menu_ids: Vec<i64> = Vec::new();
        if !role_ids.is_empty() {
            let perm_ids: Vec<i64> = crate::data::sys_role_permissions::Entity::find()
                .filter(crate::data::sys_role_permissions::Column::RoleId.is_in(role_ids))
                .all(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?
                .iter()
                .filter_map(|r| r.permission_id)
                .collect();
            if !perm_ids.is_empty() {
                menu_ids = crate::data::sys_permission_menus::Entity::find()
                    .filter(crate::data::sys_permission_menus::Column::PermissionId.is_in(perm_ids))
                    .all(&self.state.db)
                    .await
                    .map_err(|e| internal_error(format!("db: {e}")))?
                    .iter()
                    .filter_map(|r| r.menu_id)
                    .collect();
            }
        }
        let mut query = crate::data::sys_menus::Entity::find()
            .filter(crate::data::sys_menus::Column::Status.eq("ON"));
        // Platform operators see everything; tenant scope narrows to the
        // permitted set (empty set for tenant users → empty routes).
        query = if menu_ids.is_empty() {
            return Ok(Vec::new());
        } else {
            query.filter(crate::data::sys_menus::Column::Id.is_in(menu_ids))
        };
        let mut rows = query
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        rows.sort_by_key(|m| m.id);
        Ok(rows)
    }

    fn build_route_tree(rows: &[crate::data::sys_menus::Model]) -> Vec<MenuRouteItem> {
        fn item(row: &crate::data::sys_menus::Model) -> MenuRouteItem {
            let meta = row.meta.as_ref().and_then(menu_meta_from_json);
            MenuRouteItem {
                path: row.path.clone(),
                redirect: row.redirect.clone(),
                alias: row.alias.clone(),
                name: Some(row.name.clone()),
                component: row.component.clone(),
                meta,
                children: Vec::new(),
            }
        }
        fn attach(
            rows: &[crate::data::sys_menus::Model],
            parent: Option<u64>,
        ) -> Vec<MenuRouteItem> {
            rows.iter()
                .filter(|r| r.parent_id == parent.map(|v| v as i64))
                .map(|r| {
                    let mut node = item(r);
                    node.children = attach(rows, Some(r.id as u64));
                    node
                })
                .collect()
        }
        attach(rows, None)
    }

    async fn permission_codes(&self, uid: i64) -> Result<Vec<String>, crate::state::StatusError> {
        let role_ids: Vec<i64> = crate::data::sys_user_roles::Entity::find()
            .filter(crate::data::sys_user_roles::Column::UserId.eq(uid))
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .iter()
            .filter_map(|r| r.role_id)
            .collect();
        if role_ids.is_empty() {
            return Ok(Vec::new());
        }
        let perm_ids: Vec<i64> = crate::data::sys_role_permissions::Entity::find()
            .filter(crate::data::sys_role_permissions::Column::RoleId.is_in(role_ids))
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .iter()
            .filter_map(|r| r.permission_id)
            .collect();
        if perm_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(crate::data::sys_permissions::Entity::find()
            .filter(crate::data::sys_permissions::Column::Id.is_in(perm_ids))
            .all(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .into_iter()
            .map(|p| p.code)
            .collect())
    }
}

#[async_trait::async_trait]
impl AdminPortalServiceHandlers for AdminPortalService {
    async fn get_navigation(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListRouteResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let rows = self.permitted_menus(payload.user_id).await?;
        Ok(ListRouteResponse {
            items: Self::build_route_tree(&rows),
        })
    }

    async fn get_my_permission_code(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListPermissionCodeResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let codes = self.permission_codes(payload.user_id).await?;
        Ok(ListPermissionCodeResponse {
            codes,
            hidden_fields: payload.hidden_fields,
        })
    }

    async fn get_initial_context(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<InitialContextResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let (user, role_codes) = load_user(&self.state, payload.user_id).await?;
        let rows = self.permitted_menus(payload.user_id).await?;
        let codes = self.permission_codes(payload.user_id).await?;
        let _ = user_to_proto(user, role_codes);
        Ok(InitialContextResponse {
            menus: Self::build_route_tree(&rows),
            permissions: codes,
            hidden_fields: payload.hidden_fields,
        })
    }
}
