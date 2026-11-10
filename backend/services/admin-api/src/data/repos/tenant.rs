//! TenantRepo — tenants are
//! platform-global (no tenant column); the exists gate is code OR name
//! ; usage counts ride the tenant-scoped tables.

use sea_orm::sea_query::Condition;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};

use crate::data::sys_tenants as tenants;
use crate::data::sys_users as users;
use crate::state::{db_err, StatusError};

pub struct TenantRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> TenantRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find()
            .filter(tenants::Column::Code.eq(code))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn find_by_name(&self, name: &str) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find()
            .filter(tenants::Column::Name.eq(name))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// Paged listing, id ascending: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<tenants::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            tenants::Entity::find().order_by_asc(tenants::Column::Id),
            req,
        )
        .await
    }

    /// The OR-semantics exists probe (code OR name), skipping empty
    /// sides; both empty answers false.
    pub async fn exists_any(&self, code: &str, name: &str) -> Result<bool, StatusError> {
        let mut query = tenants::Entity::find();
        if !code.is_empty() && !name.is_empty() {
            query = query.filter(
                Condition::any()
                    .add(tenants::Column::Code.eq(code))
                    .add(tenants::Column::Name.eq(name)),
            );
        } else if !code.is_empty() {
            query = query.filter(tenants::Column::Code.eq(code));
        } else if !name.is_empty() {
            query = query.filter(tenants::Column::Name.eq(name));
        } else {
            return Ok(false);
        }
        Ok(query.one(self.db).await.map_err(db_err)?.is_some())
    }

    /// The provisioning exists probe: OR over the raw values, empty
    /// sides included.
    pub async fn exists_values_any(&self, code: &str, name: &str) -> Result<bool, StatusError> {
        Ok(tenants::Entity::find()
            .filter(
                Condition::any()
                    .add(tenants::Column::Code.eq(code))
                    .add(tenants::Column::Name.eq(name)),
            )
            .one(self.db)
            .await
            .map_err(db_err)?
            .is_some())
    }

    /// Best-effort tenant user count (dashboard/usage surfaces).
    pub async fn user_count(&self, tenant_id: i64) -> u64 {
        users::Entity::find()
            .filter(users::Column::TenantId.eq(tenant_id))
            .count(self.db)
            .await
            .unwrap_or(0)
    }
}
