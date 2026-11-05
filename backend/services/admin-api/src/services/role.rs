//! RoleService — service layer:
//! role CRUD with permission / org-unit / field-permission bindings; the
//! template (tenant:manager) role is copied when a tenant is provisioned
//! (TenantService.WithAdmin).

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::data::repos::RoleRepo;
use crate::data::Viewer;
use crate::mapping;
use crate::state::{
    db_err, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    CreateRoleRequest, DeleteRoleRequest, GetRoleRequest, ListRoleResponse, Role,
    RoleFieldPermission, UpdateRoleRequest,
};

fn scope_to_str(v: i32) -> String {
    match v {
        1 => "SELF".into(),
        2 => "UNIT_ONLY".into(),
        3 => "UNIT_AND_CHILD".into(),
        4 => "SELECTED_UNITS".into(),
        _ => "ALL".into(),
    }
}

/// Unknown rows read as the zero variant.
fn scope_to_proto(s: &str) -> i32 {
    mapping::role_scope_of(s).unwrap_or(0)
}

/// Unknown rows read as 3 (CUSTOM).
fn type_to_proto(s: &str) -> i32 {
    mapping::role_type_of(s).unwrap_or(3)
}

async fn role_proto(state: &AppState, r: crate::data::sys_roles::Model) -> Role {
    let perm_ids: Vec<u32> = crate::data::sys_role_permissions::Entity::find()
        .filter(crate::data::sys_role_permissions::Column::RoleId.eq(r.id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.permission_id)
        .collect();
    let org_units: Vec<u32> = crate::data::sys_role_org_units::Entity::find()
        .filter(crate::data::sys_role_org_units::Column::RoleId.eq(r.id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.org_unit_id)
        .collect();
    // The proto groups hidden fields per resource; the rows are flat.
    let mut field_permissions: Vec<RoleFieldPermission> = Vec::new();
    for row in crate::data::sys_role_field_permissions::Entity::find()
        .filter(crate::data::sys_role_field_permissions::Column::RoleId.eq(r.id))
        .all(&state.db)
        .await
        .unwrap_or_default()
    {
        let (Some(resource), Some(field)) = (row.resource, row.field_name) else {
            continue;
        };
        match field_permissions
            .iter_mut()
            .find(|fp| fp.resource == resource)
        {
            Some(fp) => fp.hidden_fields.push(field),
            None => field_permissions.push(RoleFieldPermission {
                resource,
                hidden_fields: vec![field],
            }),
        }
    }
    Role {
        id: Some(r.id),
        name: Some(r.name),
        code: Some(r.code),
        parent_id: None,
        parent_name: None,
        sort_order: r.sort_order,
        status: r.status.as_deref().map(crate::state::status_to_proto),
        description: r.description,
        is_protected: r.is_protected,
        r#type: r.type_column.as_deref().map(type_to_proto),
        data_scope: r.data_scope.as_deref().map(scope_to_proto),
        permissions: perm_ids,
        org_units,
        field_permissions,
        tenant_id: r.tenant_id,
        tenant_name: None,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct RoleService {
    pub state: Arc<AppState>,
}

impl RoleService {
    fn apply_fields(a: &mut crate::data::sys_roles::ActiveModel, data: &Role) {
        if let Some(v) = &data.name {
            a.name = Set(v.clone());
        }
        if let Some(v) = &data.code {
            a.code = Set(v.clone());
        }
        if let Some(v) = &data.description {
            a.description = Set(Some(v.clone()));
        }
        if let Some(v) = data.is_protected {
            a.is_protected = Set(Some(v));
        }
        if let Some(v) = data.status {
            a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
        }
        if let Some(v) = data.sort_order {
            a.sort_order = Set(Some(v));
        }
        if let Some(v) = data.data_scope {
            a.data_scope = Set(Some(scope_to_str(v)));
        }
    }

    fn type_to_str(v: i32) -> String {
        match v {
            1 => "SYSTEM".into(),
            2 => "TEMPLATE".into(),
            _ => "TENANT".into(),
        }
    }

    /// Sync the role's permission/org-unit/field-permission bindings.
    async fn sync_bindings(
        &self,
        tenant_id: u32,
        role_id: u32,
        data: &Role,
    ) -> Result<(), StatusError> {
        crate::data::sys_role_permissions::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::data::sys_role_permissions::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_role_permissions::Column::RoleId.eq(role_id)),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        for perm in &data.permissions {
            crate::data::sys_role_permissions::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                role_id: Set(Some(role_id)),
                permission_id: Set(Some(*perm)),
                effect: Set(Some("ALLOW".into())),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        crate::data::sys_role_org_units::Entity::delete_many()
            .filter(crate::data::sys_role_org_units::Column::RoleId.eq(role_id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        for unit in &data.org_units {
            crate::data::sys_role_org_units::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                role_id: Set(Some(role_id)),
                org_unit_id: Set(Some(*unit)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        crate::data::sys_role_field_permissions::Entity::delete_many()
            .filter(crate::data::sys_role_field_permissions::Column::RoleId.eq(role_id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        for fp in &data.field_permissions {
            for field in &fp.hidden_fields {
                crate::data::sys_role_field_permissions::ActiveModel {
                    tenant_id: Set(Some(tenant_id)),
                    role_id: Set(Some(role_id)),
                    resource: Set(Some(fp.resource.clone())),
                    field_name: Set(Some(field.clone())),
                    created_at: Set(Some(crate::data::now())),
                    updated_at: Set(Some(crate::data::now())),
                    ..Default::default()
                }
                .insert(&self.state.db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::RoleServiceHandlers for RoleService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListRoleResponse, StatusError> {
        // Listing rides the repo: tenancy predicates live in the data layer.
        let repo = RoleRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        let mut items = Vec::with_capacity(rows.len());
        for r in rows {
            items.push(role_proto(&self.state, r).await);
        }
        Ok(ListRoleResponse { items, total })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetRoleRequest,
    ) -> Result<Role, StatusError> {
        let tid = tenant_of(&ctx);
        let row = match req.query_by {
            Some(proto::proto::permission::service::v1::get_role_request::QueryBy::Id(id)) => {
                crate::data::sys_roles::Entity::find_by_id(id)
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
            }
            Some(proto::proto::permission::service::v1::get_role_request::QueryBy::Code(code)) => {
                crate::data::sys_roles::Entity::find()
                    .filter(
                        Condition::all()
                            .add(crate::data::sys_roles::Column::TenantId.eq(tid))
                            .add(crate::data::sys_roles::Column::Code.eq(code)),
                    )
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
            }
            Some(proto::proto::permission::service::v1::get_role_request::QueryBy::Name(name)) => {
                crate::data::sys_roles::Entity::find()
                    .filter(
                        Condition::all()
                            .add(crate::data::sys_roles::Column::TenantId.eq(tid))
                            .add(crate::data::sys_roles::Column::Name.eq(name)),
                    )
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
            }
            None => None,
        }
        .ok_or_else(|| not_found("role"))?;
        Ok(role_proto(&self.state, row).await)
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateRoleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        let mut a = crate::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
            type_column: Set(Some(Self::type_to_str(data.r#type.unwrap_or(3)))),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        Self::apply_fields(&mut a, &data);
        if a.status.clone().unwrap().is_none() {
            a.status = Set(Some("ON".into()));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        self.sync_bindings(payload.tenant_id, inserted.id, &data)
            .await?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateRoleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_roles::Entity::find_by_id(req.id)
            .filter(crate::data::sys_roles::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("role"))?;
        let mut a: crate::data::sys_roles::ActiveModel = row.into();
        if let Some(data) = &req.data {
            Self::apply_fields(&mut a, data);
            crate::stamp_update!(a, payload.user_id);
            a.update(&self.state.db).await.map_err(db_err)?;
            self.sync_bindings(payload.tenant_id, req.id, data).await?;
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteRoleRequest,
    ) -> Result<Empty, StatusError> {
        let tid = tenant_of(&ctx);
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::permission::service::v1::delete_role_request::QueryBy
        );
        let row = crate::data::sys_roles::Entity::find_by_id(id)
            .filter(crate::data::sys_roles::Column::TenantId.eq(tid))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("role"))?;
        if row.is_protected == Some(true) {
            return Err(status_error(
                "FORBIDDEN",
                "protected role cannot be deleted",
            ));
        }
        crate::data::sys_role_permissions::Entity::delete_many()
            .filter(crate::data::sys_role_permissions::Column::RoleId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_role_org_units::Entity::delete_many()
            .filter(crate::data::sys_role_org_units::Column::RoleId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_role_field_permissions::Entity::delete_many()
            .filter(crate::data::sys_role_field_permissions::Column::RoleId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_roles::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
