//! TenantRepo — tenants are
//! platform-global (no tenant column); the exists gate is code OR name
//! ; usage counts ride the tenant-scoped tables.

use sea_orm::sea_query::Condition;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};

use crate::admin::data::sys_tenants as tenants;
use crate::admin::data::sys_users as users;
use crate::admin::state::{db_err, StatusError};

pub struct TenantRepo<'a> {
    pub db: &'a DatabaseConnection,
}

/// Tenants park soft (`deleted_at` set, status OFF) on delete — every
/// read face here excludes parked rows, so the list no longer shows them
/// and the exists gates allow re-creating a parked tenant's code/name.
fn alive() -> Condition {
    Condition::all().add(tenants::Column::DeletedAt.is_null())
}

impl<'a> TenantRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find_by_id(id)
            .filter(alive())
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find()
            .filter(alive())
            .filter(tenants::Column::Code.eq(code))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn find_by_name(&self, name: &str) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find()
            .filter(alive())
            .filter(tenants::Column::Name.eq(name))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// Paged listing, id ascending: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &crate::proto::pagination::PagingRequest,
    ) -> Result<(Vec<tenants::Model>, u64), StatusError> {
        crate::admin::paging::fetch_paged(
            self.db,
            tenants::Entity::find()
                .filter(alive())
                .order_by_asc(tenants::Column::Id),
            req,
        )
        .await
    }

    /// The OR-semantics exists probe (code OR name), skipping empty
    /// sides; both empty answers false.
    pub async fn exists_any(&self, code: &str, name: &str) -> Result<bool, StatusError> {
        let mut query = tenants::Entity::find().filter(alive());
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
            .filter(alive())
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

    /// The raw row by id, parked rows included — the recycle-bin face's
    /// lookup (issue #3): callers decide parked-ness by inspecting
    /// `deleted_at`.
    pub async fn find_by_id_unfiltered(
        &self,
        id: i64,
    ) -> Result<Option<tenants::Model>, StatusError> {
        tenants::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// 回收站列表：停车行（deleted_at 非空），删除时间降序（issue #3）。
    /// 与 paged_list 的 alive 域互斥，同一行的两副面孔各走各的门。
    pub async fn paged_deleted(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<tenants::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            tenants::Entity::find()
                .filter(tenants::Column::DeletedAt.is_not_null())
                .order_by_desc(tenants::Column::DeletedAt),
            req,
        )
        .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::{mem_db, schema};
    use sea_orm::{ActiveModelTrait, Set};

    async fn seed(db: &sea_orm::DatabaseConnection) -> Vec<i64> {
        let mut ids = Vec::new();
        for (name, code) in [("acme", "acme"), ("globex", "globex")] {
            let row = tenants::ActiveModel {
                name: Set(name.into()),
                code: Set(code.into()),
                status: Set(Some("ON".into())),
                audit_status: Set(Some("APPROVED".into())),
                ..Default::default()
            }
            .insert(db)
            .await
            .expect("insert tenant");
            ids.push(row.id);
        }
        ids
    }

    /// 删除是停车（deleted_at + OFF）不是消失：停车行对全部读面不可见，
    /// 且腾出 code/name 让重建同名租户通过 exists 闸（issue #6b）。
    #[tokio::test]
    async fn parked_tenants_leave_every_read_face() {
        let db = mem_db().await;
        schema(&db).await;
        let ids = seed(&db).await;
        let parked = ids[0];
        tenants::ActiveModel {
            id: Set(parked),
            status: Set(Some("OFF".into())),
            deleted_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .update(&db)
        .await
        .expect("park tenant");

        let repo = TenantRepo::new(&db);
        assert!(repo.find_by_id(parked).await.unwrap().is_none());
        assert!(repo.find_by_code("acme").await.unwrap().is_none());
        assert!(repo.find_by_name("acme").await.unwrap().is_none());
        let (rows, total) = repo.paged_list(&Default::default()).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].code, "globex");
        assert!(!repo.exists_any("acme", "acme").await.unwrap());
        assert!(!repo.exists_values_any("acme", "acme").await.unwrap());
        assert!(repo.exists_any("", "globex").await.unwrap());
    }

    /// 回收站列表面（issue #3）：停车行在 paged_deleted 可见（删除时间
    /// 降序），alive 行不可见——两副面孔互斥；unfiltered find 是回收站
    /// 操作的寻址面。
    #[tokio::test]
    async fn trash_face_shows_only_parked_rows() {
        let db = mem_db().await;
        schema(&db).await;
        let ids = seed(&db).await;
        tenants::ActiveModel {
            id: Set(ids[0]),
            deleted_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .update(&db)
        .await
        .expect("park tenant");

        let repo = TenantRepo::new(&db);
        let (rows, total) = repo.paged_deleted(&Default::default()).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].id, ids[0]);
        assert!(rows[0].deleted_at.is_some());

        let raw = repo
            .find_by_id_unfiltered(ids[0])
            .await
            .unwrap()
            .expect("parked row visible unfiltered");
        assert!(raw.deleted_at.is_some());
    }
}
