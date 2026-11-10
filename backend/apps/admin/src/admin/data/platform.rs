//! Platform entity tables: access keys, configs, login policies, files.

pub mod sys_access_keys {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_access_keys")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub name: String,
        pub access_key: String,
        /// hex(SHA-256(secret)).
        pub secret_hash: String,
        pub expires_at: Option<chrono::NaiveDateTime>,
        pub last_used_at: Option<chrono::NaiveDateTime>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
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
pub mod sys_configs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_configs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub key: String,
        pub value: Option<String>,
        /// enum(STRING,BOOL,INT) default STRING.
        pub value_type: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_built_in: Option<bool>,
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
pub mod sys_login_policies {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_login_policies")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub target_id: Option<String>,
        pub value: Option<String>,
        pub reason: Option<String>,
        /// enum(BLACKLIST,WHITELIST) default BLACKLIST.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        /// enum(IP,MAC,REGION,TIME,DEVICE) default IP.
        pub method: Option<String>,
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
pub mod files {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "files")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        /// enum(UNKNOWN,MINIO,…) default MINIO.
        pub provider: Option<String>,
        pub bucket_name: Option<String>,
        pub file_directory: Option<String>,
        pub file_guid: Option<String>,
        pub save_file_name: Option<String>,
        pub file_name: Option<String>,
        pub extension: Option<String>,
        pub size: Option<i64>,
        pub size_format: Option<String>,
        pub link_url: Option<String>,
        pub content_hash: Option<String>,
        pub remark: Option<String>,
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
