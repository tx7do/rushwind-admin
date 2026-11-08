//! BI entity tables: designer-produced dashboards / screens / templates
//! and their revision snapshots.

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

/// BI 仪表盘版本快照表：每次成功保存把**保存前**的 payload/name/version
/// 留一行快照，恢复 = 把快照写回主表（恢复动作本身先快照当前版本，可逆）。
/// 只增不改不删（表级无更新入口），随主表级联删除。
pub mod bi_dashboard_revisions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_bi_dashboard_revisions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        /// 所属仪表盘 ID。
        pub dashboard_id: Option<u32>,
        /// 快照对应的仪表盘版本号（保存前的旧版本）。
        #[sea_orm(default_value = 0)]
        pub version: Option<u32>,
        /// 保存前的名称快照（恢复时一并写回）。
        pub name: Option<String>,
        /// 保存前的仪表盘 JSON 快照（服务端不解析只透传）。
        pub payload: Option<String>,
        /// 快照来源（SAVE=常规保存触发 RESTORE=恢复前留底）。
        #[sea_orm(default_value = "SAVE")]
        pub source: Option<String>,
        pub created_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
