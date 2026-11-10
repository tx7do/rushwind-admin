//! AccessKeyRepo — tenant-scoped queries with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::sys_access_keys as entity;
use crate::data::Viewer;
use crate::state::{db_err, not_found, StatusError};

repo_shell!(tenant AccessKeyRepo, entity);

impl<'a> AccessKeyRepo<'a> {
    pub async fn get_by_id(&self, id: i64) -> Result<entity::Model, StatusError> {
        let mut query = entity::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = entity::Entity::find_by_id(id).filter(entity::Column::TenantId.eq(tid));
        }
        query
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("access key"))
    }

    pub async fn delete_by_id(&self, id: i64) -> Result<(), StatusError> {
        if self.viewer.tenant_scope().is_some() {
            entity::Entity::delete_many()
                .filter(entity::Column::TenantId.eq(self.viewer.tenant_scope().unwrap_or(0)))
                .filter(entity::Column::Id.eq(id))
                .exec(self.db)
                .await
                .map_err(db_err)?;
            return Ok(());
        }
        entity::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

impl<'a> AccessKeyRepo<'a> {
    /// The key lookup scoped to one tenant (Get's query_by=access_key).
    pub async fn find_by_access_key_for_tenant(
        &self,
        tenant_id: i64,
        ak: &str,
    ) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::TenantId.eq(tenant_id))
            .filter(entity::Column::AccessKey.eq(ak))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// The unscoped key lookup (IssueToken's credential path — auth-time
    /// lookups cross tenants by design).
    pub async fn find_by_access_key(&self, ak: &str) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::AccessKey.eq(ak))
            .one(self.db)
            .await
            .map_err(db_err)
    }
}
