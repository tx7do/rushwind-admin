//! Print entity table: designer print templates.

pub mod print_templates {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "print_templates")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub name: String,
        /// 编码（业务侧引用锚）。
        pub code: String,
        /// 资源键（模板在存储侧的资源定位）。
        pub resource_key: Option<String>,
        /// 模板 JSON（设计器 schema 原文；服务端不解析只透传，与 BI 同口径）。
        pub payload: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_built_in: Option<bool>,
        /// 纸张尺寸名（A4/A5/Letter…；渲染端解析为毫米宽高）。
        pub paper_size: Option<String>,
        /// enum(PORTRAIT,LANDSCAPE).
        pub orientation: Option<String>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
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
