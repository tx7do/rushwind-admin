//! LanguageRepo — platform-global rows with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::sys_languages as entity;
use crate::state::{db_err, not_found, StatusError};

repo_shell!(global LanguageRepo, entity, "language");

impl<'a> LanguageRepo<'a> {
    /// The id behind a language-code lookup (Get's query_by=code).
    pub async fn find_id_by_code(&self, code: &str) -> Result<Option<u64>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::LanguageCode.eq(code))
            .one(self.db)
            .await
            .map_err(db_err)
            .map(|row| row.map(|row| row.id as u64))
    }

    /// One insert, shared by create and batch_create.
    pub async fn insert(&self, row: entity::ActiveModel) -> Result<entity::Model, StatusError> {
        ActiveModelTrait::insert(row, self.db).await.map_err(db_err)
    }
}
