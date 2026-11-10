//! PrintTemplateRepo — tenant-scoped print templates; code is the
//! business-side reference anchor.

use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

use crate::data::print_templates as entity;
use crate::state::{db_err, StatusError};

repo_shell!(tenant PrintTemplateRepo, entity);

impl<'a> PrintTemplateRepo<'a> {
    /// The scoped row lookup — tenant viewers see own-tenant rows only,
    /// platform/system viewers any named row.
    pub async fn find_scoped(&self, id: i64) -> Result<Option<entity::Model>, StatusError> {
        let mut query = entity::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = query.filter(entity::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }

    /// Code uniqueness within the tenant (business-side lookup anchor).
    pub async fn find_by_code(&self, code: &str) -> Result<Option<entity::Model>, StatusError> {
        let mut query = entity::Entity::find().filter(entity::Column::Code.eq(code));
        if let Some(tid) = self.viewer.tenant_scope() {
            query = query.filter(entity::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }
}
