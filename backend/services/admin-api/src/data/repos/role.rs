//! RoleRepo — role CRUD,
//! permission bindings, role codes by ids, and the template copy used by
//! tenant provisioning.

use sea_orm::sea_query::Condition;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};

use crate::data::Viewer;
use crate::data::{sys_role_permissions, sys_roles as roles};
use crate::state::{db_err, status_error, StatusError};

pub struct RoleRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

impl<'a> RoleRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    fn condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => Condition::all().add(roles::Column::TenantId.eq(tid)),
            None => Condition::all(),
        }
    }

    pub async fn list(&self) -> Result<Vec<roles::Model>, StatusError> {
        roles::Entity::find()
            .filter(self.condition())
            .order_by_asc(roles::Column::SortOrder)
            .all(self.db)
            .await
            .map_err(db_err)
    }

    /// Paged listing over the PagingRequest contract: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<roles::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            roles::Entity::find()
                .filter(self.condition())
                .order_by_asc(roles::Column::SortOrder),
            req,
        )
        .await
    }

    pub async fn get_by_id(&self, id: i64) -> Result<roles::Model, StatusError> {
        let mut query = roles::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = roles::Entity::find_by_id(id).filter(roles::Column::TenantId.eq(tid));
        }
        query
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| status_error("NOT_FOUND", "role not found"))
    }

    pub async fn get_by_code(&self, code: &str) -> Result<Option<roles::Model>, StatusError> {
        roles::Entity::find()
            .filter(self.condition().add(roles::Column::Code.eq(code)))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn list_codes_by_ids(&self, ids: &[u32]) -> Vec<String> {
        if ids.is_empty() {
            return Vec::new();
        }
        roles::Entity::find()
            .filter(roles::Column::Id.is_in(ids.to_vec()))
            .all(self.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.code)
            .collect()
    }

    pub async fn create(
        &self,
        name: &str,
        code: &str,
        role_type: &str,
        operator_id: i64,
    ) -> Result<roles::Model, StatusError> {
        roles::ActiveModel {
            tenant_id: Set(self.viewer.stamp_tenant(None)),
            name: Set(name.to_string()),
            code: Set(code.to_string()),
            type_column: Set(Some(role_type.to_string())),
            status: Set(Some("ON".into())),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(self.db)
        .await
        .map_err(db_err)
    }

    /// Copy the template role's permission bindings into a fresh tenant role.
    pub async fn copy_permissions(
        &self,
        from_role: u64,
        to_role: u64,
        to_tenant: u64,
        _operator_id: i64,
    ) -> Result<(), StatusError> {
        let perms = sys_role_permissions::Entity::find()
            .filter(sys_role_permissions::Column::RoleId.eq(from_role))
            .all(self.db)
            .await
            .map_err(db_err)?;
        for perm in perms {
            sys_role_permissions::ActiveModel {
                tenant_id: Set(Some(to_tenant as i64)),
                role_id: Set(Some(to_role as i64)),
                permission_id: Set(perm.permission_id),
                effect: Set(Some("ALLOW".into())),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(self.db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub async fn sync_permissions(
        &self,
        tenant_id: i64,
        role_id: i64,
        permission_ids: &[u64],
        _operator_id: i64,
    ) -> Result<(), StatusError> {
        sys_role_permissions::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(sys_role_permissions::Column::TenantId.eq(tenant_id))
                    .add(sys_role_permissions::Column::RoleId.eq(role_id)),
            )
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for pid in permission_ids {
            sys_role_permissions::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                role_id: Set(Some(role_id)),
                permission_id: Set(Some(*pid as i64)),
                effect: Set(Some("ALLOW".into())),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(self.db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<(), StatusError> {
        sys_role_permissions::Entity::delete_many()
            .filter(sys_role_permissions::Column::RoleId.eq(id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        roles::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
