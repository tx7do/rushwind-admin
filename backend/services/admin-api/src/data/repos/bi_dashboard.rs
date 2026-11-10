//! BiDashboardRepo — tenant-scoped dashboard/screen/template rows;
//! code is unique per tenant, version is the optimistic-lock handle.
//! BiDashboardRevisionRepo — the version-snapshot thin repo: insert-only
//! (deletes cascade with the dashboard), three actions only.

use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::PaginatorTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::QuerySelect;

use crate::data::bi_dashboard_revisions as revision;
use crate::data::bi_dashboards as entity;
use crate::data::Viewer;
use crate::state::{db_err, StatusError};

repo_shell!(tenant BiDashboardRepo, entity);

impl<'a> BiDashboardRepo<'a> {
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

/// 版本快照薄仓储：只增不改不删（删除随主表级联），只需 快照落库 /
/// 按仪表盘分页拉取 / 取单行 三个动作，不走通用 CRUD 定式。
/// 查询一律带租户闸（快照行自带 tenant_id，比上游"经主表间接受权"更严）。
pub struct BiDashboardRevisionRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

impl<'a> BiDashboardRevisionRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    fn condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => Condition::all().add(revision::Column::TenantId.eq(tid)),
            None => Condition::all(),
        }
    }

    /// 落一行快照。调用方（service 层）已取好旧值；失败由调用方决定是否阻断。
    /// tenant_id 取快照来源仪表盘的租户（平台侧触发的保存也归到仪表盘租户）。
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        tenant_id: Option<i64>,
        dashboard_id: i64,
        version: Option<u32>,
        name: Option<String>,
        payload: Option<String>,
        source: &str,
        created_by: i64,
    ) -> Result<(), StatusError> {
        revision::ActiveModel {
            tenant_id: sea_orm::Set(tenant_id),
            dashboard_id: sea_orm::Set(Some(dashboard_id)),
            version: sea_orm::Set(version),
            name: sea_orm::Set(name),
            payload: sea_orm::Set(payload),
            source: sea_orm::Set(Some(source.to_string())),
            created_by: sea_orm::Set(Some(created_by)),
            created_at: sea_orm::Set(Some(crate::data::now())),
            updated_at: sea_orm::Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(self.db)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// 按仪表盘拉取快照分页（新快照在前：version DESC, id DESC；与上游
    /// Order(Desc(version), Desc(id)) 同序，分页默认 1/20 也同参）。
    pub async fn list_paged(
        &self,
        dashboard_id: i64,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<(Vec<revision::Model>, u64), StatusError> {
        let page_no = page.filter(|p| *p > 0).unwrap_or(1);
        let size = page_size.filter(|s| *s > 0).unwrap_or(20);
        let base = revision::Entity::find()
            .filter(self.condition())
            .filter(revision::Column::DashboardId.eq(dashboard_id));
        let total = base.clone().count(self.db).await.map_err(db_err)?;
        let rows = base
            .order_by_desc(revision::Column::Version)
            .order_by_desc(revision::Column::Id)
            .offset(((page_no - 1) as u64) * size as u64)
            .limit(size as u64)
            .all(self.db)
            .await
            .map_err(db_err)?;
        Ok((rows, total))
    }

    /// 取单个快照（恢复时校验归属）。
    pub async fn find_scoped(&self, id: i64) -> Result<Option<revision::Model>, StatusError> {
        revision::Entity::find_by_id(id)
            .filter(self.condition())
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// 主表删除时的级联清理（SeaORM 无库级 cascade，随 delete 流程显式执行）。
    pub async fn delete_for_dashboard(&self, dashboard_id: i64) -> Result<(), StatusError> {
        revision::Entity::delete_many()
            .filter(revision::Column::DashboardId.eq(dashboard_id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
