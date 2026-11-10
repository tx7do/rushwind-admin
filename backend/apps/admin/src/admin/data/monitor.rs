//! Monitor-alert entity table: threshold rules with evaluator state.

pub mod sys_monitor_alert_rules {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "monitor_alert_rules")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        /// enum(GO_GOROUTINES,GO_MEM_ALLOC_MB,DB_OPEN_CONNECTIONS,DB_PING_FAIL,REDIS_DB_SIZE).
        pub metric: Option<String>,
        /// enum(GE,LE)；DB_PING_FAIL 忽略 op/threshold（布尔指标）。
        pub op: Option<String>,
        pub threshold: Option<f64>,
        /// 重复告警冷却（分钟），默认 30。
        #[sea_orm(default_value = 30)]
        pub cooldown_minutes: Option<u32>,
        /// enum(EMAIL,SMS,WEBHOOK,INTERNAL) — 显式点对点，不经路由表。
        pub channel: Option<String>,
        /// EMAIL 收件地址 / WEBHOOK 回调 URL / SMS 手机号。
        pub target: String,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
        pub remark: Option<String>,
        /// —— 以下为评估器回写状态（只读面） ——
        #[sea_orm(default_value = false)]
        pub last_firing: Option<bool>,
        pub last_value: Option<f64>,
        pub last_alerted_at: Option<chrono::NaiveDateTime>,
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
