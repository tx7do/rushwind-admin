//! BI entity table: designer-produced dashboards / screens / templates.

pub mod bi_dashboards {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "bi_dashboards")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        /// 编码，租户内唯一，业务侧按编码取仪表盘。
        pub code: String,
        /// enum(DASHBOARD,SCREEN,TEMPLATE).
        pub kind: Option<String>,
        pub remark: Option<String>,
        /// 仪表盘 JSON（设计器 schema 原文；服务端不解析只透传，与打印模板同口径）。
        pub payload: Option<String>,
        /// 图表数量冗余列（来自 payload.charts，列表页直显免解析）。
        #[sea_orm(default_value = 0)]
        pub chart_count: Option<u32>,
        /// 主题预设冗余列（来自 payload.theme.key）。
        pub theme_key: Option<String>,
        /// 缩略图 URL（设计器导出 PNG 上传，列表页预览）。
        pub thumbnail: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_built_in: Option<bool>,
        /// 是否租户内共享（false 仅创建者可见）。
        #[sea_orm(default_value = false)]
        pub is_public: Option<bool>,
        /// 乐观锁版本号：更新回传读到的版本，冲突拒绝；保存成功 +1。
        #[sea_orm(default_value = 1)]
        pub version: Option<u32>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
