//! PositionRepo — tenant-scoped queries with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::admin::data::sys_positions as entity;
use crate::admin::state::{db_err, not_found, StatusError};

repo_shell!(tenant PositionRepo, entity);

impl<'a> PositionRepo<'a> {
    pub async fn get_by_id(&self, id: i64) -> Result<entity::Model, StatusError> {
        let mut query = entity::Entity::find_by_id(id).filter(entity::Column::DeletedAt.is_null());
        if let Some(tid) = self.viewer.tenant_scope() {
            query = entity::Entity::find_by_id(id)
                .filter(entity::Column::TenantId.eq(tid))
                .filter(entity::Column::DeletedAt.is_null());
        }
        query
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("position"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::scope::ViewerKind;
    use crate::data::Viewer;
    use crate::proto::proto::pagination::PagingRequest;
    use crate::testsupport::{mem_db, schema};
    use sea_orm::{ActiveModelTrait, DatabaseBackend, Set};

    fn viewer(tenant_id: i64) -> Viewer {
        Viewer {
            kind: ViewerKind::User,
            user_id: 1,
            tenant_id,
        }
    }

    fn system() -> Viewer {
        Viewer {
            kind: ViewerKind::System,
            user_id: 0,
            tenant_id: 0,
        }
    }

    async fn seed_rows(db: &sea_orm::DatabaseConnection) {
        for (tid, name) in [(7, "eng-a"), (7, "eng-b"), (9, "sales-a")] {
            entity::ActiveModel {
                tenant_id: Set(Some(tid)),
                name: Set(name.into()),
                ..Default::default()
            }
            .insert(db)
            .await
            .expect("insert position");
        }
    }

    /// tenant viewer 只见本租；平台（tid=0）与 system 全见。
    #[tokio::test]
    async fn tenant_predicate_follows_the_viewer() {
        let db = mem_db().await;
        schema(&db).await;
        seed_rows(&db).await;

        let mine = PositionRepo::new(&db, viewer(7)).list().await.unwrap();
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().all(|r| r.tenant_id == Some(7)));

        let platform = PositionRepo::new(&db, viewer(0)).list().await.unwrap();
        assert_eq!(platform.len(), 3, "platform viewer is wide");

        let sys = PositionRepo::new(&db, system()).list().await.unwrap();
        assert_eq!(sys.len(), 3, "system viewer is wide");
    }

    /// 租户视角的 id 寻址读写受租户谓词约束：跨租读 404、跨租删不动，
    /// 平台视角可删任意行。
    #[tokio::test]
    async fn id_addressed_reads_and_writes_carry_the_scope() {
        let db = mem_db().await;
        schema(&db).await;
        seed_rows(&db).await;
        let foreign_id = 3; // sales-a（租户 9，第三条插入）

        let mine = PositionRepo::new(&db, viewer(7));
        let err = mine.get_by_id(foreign_id).await.unwrap_err();
        assert_eq!(err.reason, "NOT_FOUND");
        assert_eq!(err.message, "position not found");
        mine.delete_by_id(foreign_id).await.unwrap();
        assert_eq!(
            PositionRepo::new(&db, viewer(9))
                .list()
                .await
                .unwrap()
                .len(),
            1,
            "another tenant's delete must not touch the row"
        );

        let platform = PositionRepo::new(&db, viewer(0));
        platform.delete_by_id(foreign_id).await.unwrap();
        assert_eq!(
            PositionRepo::new(&db, system()).list().await.unwrap().len(),
            2,
            "platform delete removes the row"
        );
    }

    /// paged_list：page/page_size 切片、total 与谓词一致、offset/limit
    /// 显式出现时优先于 page 切片。
    #[tokio::test]
    async fn paged_list_slices_and_counts() {
        let db = mem_db().await;
        schema(&db).await;
        seed_rows(&db).await;
        let repo = PositionRepo::new(&db, viewer(7));

        let page1 = PagingRequest {
            page: Some(1),
            page_size: Some(1),
            ..Default::default()
        };
        let (rows, total) = repo.paged_list(&page1).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 2, "total counts all matching rows");

        let page2 = PagingRequest {
            page: Some(2),
            page_size: Some(1),
            ..Default::default()
        };
        let (rows, _) = repo.paged_list(&page2).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_ne!(
            rows[0].id,
            rows_first(&repo, &page1).await,
            "page 2 differs"
        );

        let explicit = PagingRequest {
            offset: Some(1),
            limit: Some(5),
            ..Default::default()
        };
        let (rows, total) = repo.paged_list(&explicit).await.unwrap();
        assert_eq!(rows.len(), 1, "offset 1 over two rows leaves one");
        assert_eq!(total, 2);
        assert_eq!(
            rows[0].id,
            rows_first(&repo, &page2).await,
            "offset slice rides the id order"
        );
    }

    async fn rows_first(repo: &PositionRepo<'_>, req: &PagingRequest) -> i64 {
        repo.paged_list(req).await.unwrap().0[0].id
    }

    /// sqlite 装配面烟测：seed + 条件查询跑通（本仓 sqlite 测试面的
    /// 装配等价性由 migration 测试钉住，这里只验谓词组合不炸）。
    #[tokio::test]
    async fn sqlite_backend_is_assembled() {
        let db = mem_db().await;
        schema(&db).await;
        assert_eq!(db.get_database_backend(), DatabaseBackend::Sqlite);
        seed_rows(&db).await;
        let repo = PositionRepo::new(&db, system());
        assert_eq!(repo.list().await.unwrap().len(), 3);
    }

    /// 软删（deleted_at 非空）行对宏的两臂读面统一不可见：list、
    /// paged_list、get_by_id 都不再返回（回收站读面是未来的独立特性）。
    #[tokio::test]
    async fn soft_deleted_rows_are_invisible() {
        let db = mem_db().await;
        schema(&db).await;
        seed_rows(&db).await;
        entity::ActiveModel {
            id: Set(1),
            deleted_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .update(&db)
        .await
        .expect("park row 1");

        let repo = PositionRepo::new(&db, viewer(7));
        assert_eq!(
            repo.list().await.unwrap().len(),
            1,
            "parked row hidden from the tenant list"
        );
        let err = repo.get_by_id(1).await.unwrap_err();
        assert_eq!(err.reason, "NOT_FOUND");

        let wide = PositionRepo::new(&db, system());
        let (rows, total) = wide.paged_list(&PagingRequest::default()).await.unwrap();
        assert_eq!(total, 2, "parked row hidden from the wide paging face");
        assert!(rows.iter().all(|r| r.id != 1));
    }
}
