//! LanguageRepo — platform-global rows with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};

use crate::admin::data::sys_languages as entity;
use crate::admin::state::{db_err, StatusError};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::pagination::PagingRequest;
    use crate::testsupport::{mem_db, schema};
    use sea_orm::Set;

    async fn row(db: &sea_orm::DatabaseConnection, code: &str, name: &str) -> entity::Model {
        repo(db)
            .insert(entity::ActiveModel {
                language_code: Set(code.into()),
                language_name: Set(name.into()),
                ..Default::default()
            })
            .await
            .expect("insert language")
    }

    fn repo(db: &sea_orm::DatabaseConnection) -> LanguageRepo<'_> {
        LanguageRepo::new(db)
    }

    /// get_by_id 命中与 404 文案（noun arm 的共享形状）。
    #[tokio::test]
    async fn get_by_id_hit_and_not_found_wording() {
        let db = mem_db().await;
        schema(&db).await;
        let inserted = row(&db, "zh-CN", "简体中文").await;

        let got = repo(&db).get_by_id(inserted.id).await.unwrap();
        assert_eq!(got.language_code, "zh-CN");

        let err = repo(&db).get_by_id(inserted.id + 100).await.unwrap_err();
        assert_eq!(err.reason, "NOT_FOUND");
        assert_eq!(err.message, "language not found");
    }

    /// delete_by_id 后行消失、find_id_by_code 编码寻路。
    #[tokio::test]
    async fn delete_by_id_and_code_lookup() {
        let db = mem_db().await;
        schema(&db).await;
        let zh = row(&db, "zh-CN", "简体中文").await;
        row(&db, "en-US", "English").await;
        let repo = repo(&db);

        assert_eq!(
            repo.find_id_by_code("en-US").await.unwrap(),
            Some((zh.id + 1) as u64)
        );

        repo.delete_by_id(zh.id).await.unwrap();
        assert!(repo.get_by_id(zh.id).await.is_err());
        // 编码寻路不受删除影响（独立谓词）。
        assert_eq!(repo.find_id_by_code("zh-CN").await.unwrap(), None);
    }

    /// paged_list：默认 page1/size10 全量在内、no_paging 返回全部且
    /// total 折叠为行数。
    #[tokio::test]
    async fn paged_list_defaults_and_no_paging() {
        let db = mem_db().await;
        schema(&db).await;
        for i in 0..12 {
            row(&db, &format!("lang-{i:02}"), &format!("Lang {i}")).await;
        }
        let repo = repo(&db);

        let (rows, total) = repo.paged_list(&PagingRequest::default()).await.unwrap();
        assert_eq!(rows.len(), 10, "default page size 10");
        assert_eq!(total, 12);

        let all = PagingRequest {
            no_paging: Some(true),
            ..Default::default()
        };
        let (rows, total) = repo.paged_list(&all).await.unwrap();
        assert_eq!(rows.len(), 12);
        assert_eq!(total, 12, "no_paging folds total to the row count");
    }
}
