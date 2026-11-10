//! InternalMessageCategoryRepo — the category CRUD surface
//! (admin-managed dictionary rows, tenant-scoped).

//! InternalMessage repos — message repos:
//! send inserts denormalized recipients (status RECEIVED immediately);
//! delete/revoke cascade recipient rows; the inbox is the user-scoped
//! recipient list with one IN-query backfill.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::state::{db_err, StatusError};

/// The category CRUD surface (admin-managed dictionary rows).
pub struct InternalMessageCategoryRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> InternalMessageCategoryRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// Paged tenant-scoped listing: returns (rows, total).
    pub async fn paged_list(
        &self,
        tenant_id: i64,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<crate::data::internal_message_categories::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            crate::data::internal_message_categories::Entity::find()
                .filter(crate::data::internal_message_categories::Column::TenantId.eq(tenant_id))
                .order_by_asc(crate::data::internal_message_categories::Column::SortOrder),
            req,
        )
        .await
    }

    /// The scoped row lookup — tenant viewers see own-tenant rows only,
    /// platform/system viewers see everything.
    pub async fn find_scoped(
        &self,
        id: i64,
        scope: Option<i64>,
    ) -> Result<Option<crate::data::internal_message_categories::Model>, StatusError> {
        let mut query = crate::data::internal_message_categories::Entity::find_by_id(id);
        if let Some(tid) = scope {
            query =
                query.filter(crate::data::internal_message_categories::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }

    /// The scoped delete — tenant viewers delete own-tenant rows only,
    /// platform/system viewers any named row.
    pub async fn delete_scoped(&self, id: i64, scope: Option<i64>) -> Result<(), StatusError> {
        let mut query = crate::data::internal_message_categories::Entity::delete_many()
            .filter(crate::data::internal_message_categories::Column::Id.eq(id));
        if let Some(tid) = scope {
            query =
                query.filter(crate::data::internal_message_categories::Column::TenantId.eq(tid));
        }
        query.exec(self.db).await.map_err(|_| {
            crate::state::internal_error("delete internal message categories failed")
        })?;
        Ok(())
    }
}
