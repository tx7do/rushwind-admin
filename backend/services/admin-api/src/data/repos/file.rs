//! FileRepo — tenant-scoped queries with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::files as entity;
use crate::data::Viewer;
use crate::state::{db_err, not_found, StatusError};

repo_shell!(tenant FileRepo, entity);

impl<'a> FileRepo<'a> {
    pub async fn get_by_id(&self, id: i64) -> Result<entity::Model, StatusError> {
        let mut query = entity::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = entity::Entity::find_by_id(id).filter(entity::Column::TenantId.eq(tid));
        }
        query
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("file"))
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
