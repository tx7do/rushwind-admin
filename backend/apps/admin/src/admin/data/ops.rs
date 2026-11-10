//! Automation entity tables: tasks and the script/log pair.

pub mod sys_tasks {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_tasks")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        /// enum(PERIODIC,DELAY,WAIT_RESULT) default PERIODIC.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        pub type_name: String,
        /// jsonb (string payload).
        pub task_payload: Option<Json>,
        pub cron_spec: Option<String>,
        /// jsonb TaskOption.
        pub task_options: Option<Json>,
        #[sea_orm(default_value = false)]
        pub enable: Option<bool>,
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
pub mod sys_scripts {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_scripts")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        /// enum(LUA,JAVASCRIPT) default LUA.
        pub language: Option<String>,
        pub hook_point: Option<String>,
        pub source: Option<String>,
        #[sea_orm(default_value = 0)]
        pub priority: Option<i32>,
        pub description: Option<String>,
        #[sea_orm(default_value = false)]
        pub critical: Option<bool>,
        #[sea_orm(default_value = 1)]
        pub version: Option<u32>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
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
pub mod sys_script_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_script_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub script_id: Option<i64>,
        pub script_name: Option<String>,
        /// enum(LUA,JAVASCRIPT).
        pub language: Option<String>,
        pub trigger_type: Option<String>,
        pub hook_point: Option<String>,
        pub version: Option<u32>,
        pub success: Option<bool>,
        pub duration_ms: Option<i64>,
        pub error: Option<String>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
