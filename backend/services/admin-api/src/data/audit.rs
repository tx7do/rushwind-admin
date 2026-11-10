//! Audit-log entities — the six append-only tables (AutoIncrementId +
//! CreatedAt + TenantID mixins).

pub mod sys_api_audit_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_api_audit_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub user_id: Option<i64>,
        pub username: Option<String>,
        pub ip_address: Option<String>,
        /// jsonb GeoLocation.
        pub geo_location: Option<Json>,
        /// jsonb DeviceInfo.
        pub device_info: Option<Json>,
        pub referer: Option<String>,
        pub app_version: Option<String>,
        pub http_method: Option<String>,
        pub path: Option<String>,
        pub request_uri: Option<String>,
        pub api_module: Option<String>,
        pub api_operation: Option<String>,
        pub api_description: Option<String>,
        pub request_id: Option<String>,
        pub trace_id: Option<String>,
        pub span_id: Option<String>,
        pub latency_ms: Option<u32>,
        pub success: Option<bool>,
        pub status_code: Option<u32>,
        pub reason: Option<String>,
        pub request_header: Option<String>,
        pub request_body: Option<String>,
        pub response: Option<String>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_operation_audit_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_operation_audit_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub user_id: Option<i64>,
        pub username: Option<String>,
        pub resource_type: Option<String>,
        pub resource_id: Option<String>,
        /// enum(CREATE,UPDATE,DELETE,READ,ASSIGN,UNASSIGN,EXPORT,IMPORT,OTHER).
        pub action: Option<String>,
        /// jsonb.
        pub before_data: Option<Json>,
        /// jsonb.
        pub after_data: Option<Json>,
        /// enum(PUBLIC,INTERNAL,CONFIDENTIAL,SECRET).
        pub sensitive_level: Option<String>,
        pub request_id: Option<String>,
        pub trace_id: Option<String>,
        pub success: Option<bool>,
        pub failure_reason: Option<String>,
        pub ip_address: Option<String>,
        /// jsonb GeoLocation.
        pub geo_location: Option<Json>,
        /// jsonb DeviceInfo.
        pub device_info: Option<Json>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_login_audit_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_login_audit_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub user_id: Option<i64>,
        pub username: Option<String>,
        pub ip_address: Option<String>,
        /// jsonb GeoLocation.
        pub geo_location: Option<Json>,
        pub session_id: Option<String>,
        /// jsonb DeviceInfo.
        pub device_info: Option<Json>,
        pub request_id: Option<String>,
        pub trace_id: Option<String>,
        /// enum(LOGIN,LOGOUT,SESSION_EXPIRED,KICKED_OUT,PASSWORD_RESET).
        pub action_type: Option<String>,
        /// enum(SUCCESS,FAILED,PARTIAL,LOCKED).
        pub status: Option<String>,
        /// enum(PASSWORD,SMS_CODE,QR_CODE,OIDC_SOCIAL,BIOMETRIC,FIDO2).
        pub login_method: Option<String>,
        pub failure_reason: Option<String>,
        pub mfa_status: Option<String>,
        pub risk_score: Option<u32>,
        /// enum(LOW,MEDIUM,HIGH).
        pub risk_level: Option<String>,
        /// jsonb []string.
        pub risk_factors: Option<Json>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_data_access_audit_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_data_access_audit_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub user_id: Option<i64>,
        pub username: Option<String>,
        pub ip_address: Option<String>,
        /// jsonb GeoLocation.
        pub geo_location: Option<Json>,
        /// jsonb DeviceInfo.
        pub device_info: Option<Json>,
        pub request_id: Option<String>,
        pub trace_id: Option<String>,
        pub data_source: Option<String>,
        pub table_name: Option<String>,
        pub data_id: Option<String>,
        /// enum(SELECT,INSERT,UPDATE,DELETE,VIEW,BULK_READ,EXPORT,IMPORT,…).
        pub access_type: Option<String>,
        pub sql_digest: Option<String>,
        pub sql_text: Option<String>,
        pub affected_rows: Option<u32>,
        pub latency_ms: Option<u32>,
        pub success: Option<bool>,
        /// enum(PUBLIC,INTERNAL,CONFIDENTIAL,SECRET).
        pub sensitive_level: Option<String>,
        pub data_masked: Option<bool>,
        pub masking_rules: Option<String>,
        pub business_purpose: Option<String>,
        pub data_category: Option<String>,
        pub db_user: Option<String>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_permission_audit_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_permission_audit_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub operator_id: Option<i64>,
        pub operator_name: Option<String>,
        pub target_type: Option<String>,
        pub target_id: Option<String>,
        pub target_name: Option<String>,
        /// enum(GRANT,REVOKE,UPDATE,RESET,CREATE,DELETE,ASSIGN,UNASSIGN,…).
        pub action: Option<String>,
        /// jsonb.
        pub old_value: Option<Json>,
        /// jsonb.
        pub new_value: Option<Json>,
        pub ip_address: Option<String>,
        pub request_id: Option<String>,
        pub reason: Option<String>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_policy_evaluation_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_policy_evaluation_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub tenant_id: Option<i64>,
        pub user_id: Option<i64>,
        pub membership_id: Option<i64>,
        pub permission_id: Option<i64>,
        pub policy_id: Option<i64>,
        pub request_path: Option<String>,
        pub request_method: Option<String>,
        pub result: Option<bool>,
        pub effect_details: Option<String>,
        pub scope_sql: Option<String>,
        pub ip_address: Option<String>,
        pub trace_id: Option<String>,
        pub evaluation_context: Option<String>,
        pub log_hash: Option<String>,
        pub signature: Option<Vec<u8>>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
