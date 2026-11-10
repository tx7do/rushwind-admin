//! PlanQuotaRepo — platform-global plan-quota rows.

//! Plan / PlanModule / PlanQuota repos — plan repos:
//! platform-global subscription-plan catalog rows.

use sea_orm::sea_query::Condition;
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::data::sys_plan_quotas;
use crate::state::{db_err, StatusError};

pub struct PlanQuotaRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> PlanQuotaRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    // Platform-global table: no tenant predicate applies.
    fn condition(&self) -> Condition {
        Condition::all()
    }

    /// Paged listing over the PagingRequest contract: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<sys_plan_quotas::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            sys_plan_quotas::Entity::find()
                .filter(self.condition())
                .order_by_asc(sys_plan_quotas::Column::Id),
            req,
        )
        .await
    }

    pub async fn get_by_id(&self, id: i64) -> Result<sys_plan_quotas::Model, StatusError> {
        sys_plan_quotas::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| StatusError::new(404, "NOT_FOUND", "plan quota not found"))
    }

    pub async fn delete_by_id(&self, id: i64) -> Result<(), StatusError> {
        sys_plan_quotas::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
