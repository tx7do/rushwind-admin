//! ScriptRepo — enabled-script listing and counting.

//! ScriptRepo / ScriptLogRepo — script repos:
//! enabled-script loading and the log purge (before-timestamp or all).

use sea_orm::{DatabaseConnection, EntityTrait, PaginatorTrait, QueryOrder};

use crate::admin::data::sys_scripts;
use crate::admin::state::{db_err, StatusError};

pub struct ScriptRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> ScriptRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<sys_scripts::Model>, StatusError> {
        sys_scripts::Entity::find()
            .order_by_asc(sys_scripts::Column::Id)
            .all(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn count(&self) -> u64 {
        sys_scripts::Entity::find()
            .count(self.db)
            .await
            .unwrap_or(0)
    }

    /// Paged listing over the PagingRequest contract: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &crate::proto::pagination::PagingRequest,
    ) -> Result<(Vec<sys_scripts::Model>, u64), StatusError> {
        crate::admin::paging::fetch_paged(
            self.db,
            sys_scripts::Entity::find().order_by_asc(sys_scripts::Column::Id),
            req,
        )
        .await
    }
}
