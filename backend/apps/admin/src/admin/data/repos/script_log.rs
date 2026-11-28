//! ScriptLogRepo — script-log counting, paged listing, and the purge.

//! ScriptRepo / ScriptLogRepo — script repos:
//! enabled-script loading and the log purge (before-timestamp or all).

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};

use crate::admin::data::sys_script_logs;
use crate::admin::state::{db_err, StatusError};

pub struct ScriptLogRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> ScriptLogRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn count(&self) -> u64 {
        sys_script_logs::Entity::find()
            .count(self.db)
            .await
            .unwrap_or(0)
    }

    /// Paged listing, newest first: returns (rows, total).
    pub async fn paged_list(
        &self,
        req: &crate::proto::pagination::PagingRequest,
    ) -> Result<(Vec<sys_script_logs::Model>, u64), StatusError> {
        crate::admin::paging::fetch_paged(
            self.db,
            sys_script_logs::Entity::find().order_by_desc(sys_script_logs::Column::CreatedAt),
            req,
        )
        .await
    }

    /// Purge: before-timestamp or everything.
    pub async fn purge(
        &self,
        before: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    ) -> Result<u64, StatusError> {
        let mut query = sys_script_logs::Entity::delete_many();
        if let Some(cutoff) = before {
            query = query.filter(sys_script_logs::Column::CreatedAt.lt(cutoff));
        }
        let result = query.exec(self.db).await.map_err(db_err)?;
        Ok(result.rows_affected)
    }
}
