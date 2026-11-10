//! PlanRepo — platform-global subscription-plan catalog rows.

//! Plan / PlanModule / PlanQuota repos — plan repos:
//! platform-global subscription-plan catalog rows.

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::data::{sys_plan_modules, sys_plan_quotas, sys_plans};
use crate::state::{db_err, StatusError};

pub struct PlanRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> PlanRepo<'a> {
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
    ) -> Result<(Vec<sys_plans::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            sys_plans::Entity::find()
                .filter(self.condition())
                .order_by_asc(sys_plans::Column::Id),
            req,
        )
        .await
    }

    pub async fn get_by_id(&self, id: i64) -> Result<sys_plans::Model, StatusError> {
        sys_plans::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| StatusError::new(404, "NOT_FOUND", "plan not found"))
    }

    pub async fn delete_cascade(&self, id: i64) -> Result<(), StatusError> {
        sys_plan_modules::Entity::delete_many()
            .filter(sys_plan_modules::Column::PlanId.eq(id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        sys_plan_quotas::Entity::delete_many()
            .filter(sys_plan_quotas::Column::PlanId.eq(id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        sys_plans::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
