//! The module's data layer: the tenant-scoped notice entity and the
//! repository derived from the exported `repo_shell!` shell — the
//! tenancy predicate, the listings, and the module's own predicates
//! beside the invocation.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// The notice table (`biz_notices` — business tables carry the module
/// prefix so the admin schema and business schemas never collide).
pub mod notices {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "biz_notices")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub title: String,
        pub content: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_published: Option<bool>,
        pub published_at: Option<chrono::NaiveDateTime>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
        pub created_by: Option<i64>,
        pub updated_by: Option<i64>,
        pub deleted_by: Option<i64>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// The repo shell from the host library: struct + tenancy predicate +
// list/paged_list in one line. The exported macro resolves its shared
// names through $crate / sea_orm (the expansion site keeps the usual
// trait imports — see the module list above). Repo-specific predicates
// live in the impl block beside the invocation, exactly like the admin
// tree's repos.
admin_api::repo_shell!(tenant NoticeRepo, notices);

impl NoticeRepo<'_> {
    /// The row by id under the viewer's tenancy predicate: the
    /// tenant-scoped get the shell deliberately leaves hand-written
    /// (a tenant-scoped get/delete must carry the viewer predicate).
    pub async fn get(
        &self,
        id: i64,
    ) -> Result<Option<notices::Model>, admin_api::state::StatusError> {
        notices::Entity::find_by_id(id)
            .filter(self.condition())
            .one(self.db)
            .await
            .map_err(admin_api::state::db_err)
    }
}
