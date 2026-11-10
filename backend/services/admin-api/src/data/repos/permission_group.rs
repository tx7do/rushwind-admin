//! PermissionGroupRepo — permission groups are platform-global catalog
//! rows; the plain lookup/delete surface over them (creates and field
//! merges stay with the service's ActiveModel builders).

use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};

use crate::data::sys_permission_groups as entity;
use crate::state::{db_err, StatusError};

pub struct PermissionGroupRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> PermissionGroupRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// Paged listing, id ascending: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<entity::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            entity::Entity::find().order_by_asc(entity::Column::Id),
            req,
        )
        .await
    }

    pub async fn find(&self, id: i64) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn delete_by_id(&self, id: i64) -> Result<(), StatusError> {
        entity::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
