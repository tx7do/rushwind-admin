//! Identity & access entities: users, credentials, MFA factors, org
//! units, positions, tenants, plans.

pub mod sys_users {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub username: String,
        pub nickname: Option<String>,
        pub realname: Option<String>,
        pub email: Option<String>,
        #[sea_orm(default_value = "")]
        pub mobile: Option<String>,
        #[sea_orm(default_value = "")]
        pub telephone: Option<String>,
        pub avatar: Option<String>,
        #[sea_orm(default_value = "")]
        pub address: Option<String>,
        #[sea_orm(default_value = "")]
        pub region: Option<String>,
        /// 偏好语言（zh-CN/en-US，空 = 跟随请求 Accept-Language）：事务性
        /// 邮件（找回密码/换绑验证码）按此渲染。
        pub locale: Option<String>,
        pub description: Option<String>,
        /// enum(SECRET,MALE,FEMALE) default SECRET.
        pub gender: Option<String>,
        pub last_login_at: Option<chrono::NaiveDateTime>,
        pub last_login_ip: Option<String>,
        pub locked_until: Option<chrono::NaiveDateTime>,
        /// enum(NORMAL,DISABLED,PENDING,LOCKED,EXPIRED,CLOSED) default NORMAL.
        pub status: Option<String>,
        pub remark: Option<String>,
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
pub mod sys_user_credentials {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_user_credentials")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub user_id: Option<u32>,
        /// enum(USERNAME,USERID,EMAIL,PHONE,… ) default USERNAME.
        pub identity_type: Option<String>,
        pub identifier: String,
        /// enum(PASSWORD_HASH,API_KEY,…) default PASSWORD_HASH.
        pub credential_type: Option<String>,
        pub credential: String,
        #[sea_orm(default_value = false)]
        pub is_primary: Option<bool>,
        /// enum(DISABLED,ENABLED,…) default ENABLED.
        pub status: Option<String>,
        /// jsonb: {"password_history": ["<bcrypt>", …]}.
        pub extra_info: Option<Json>,
        pub provider: Option<String>,
        pub provider_account_id: Option<String>,
        pub activate_token_hash: Option<String>,
        pub activate_token_expires_at: Option<chrono::NaiveDateTime>,
        pub activate_token_used_at: Option<chrono::NaiveDateTime>,
        pub reset_token_hash: Option<String>,
        pub reset_token_expires_at: Option<chrono::NaiveDateTime>,
        pub reset_token_used_at: Option<chrono::NaiveDateTime>,
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
pub mod sys_user_mfa_factors {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_user_mfa_factors")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub user_id: Option<u32>,
        /// enum(TOTP,SMS,EMAIL,WEBAUTHN) default TOTP.
        pub method: Option<String>,
        /// AES-GCM ciphertext (`enc:` prefix) or plaintext.
        pub secret_hash: String,
        pub display_name: Option<String>,
        /// enum(DISABLED,ENABLED) default ENABLED.
        pub status: Option<String>,
        pub last_used_at: Option<chrono::NaiveDateTime>,
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
pub mod sys_user_roles {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_user_roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub user_id: Option<u32>,
        pub role_id: Option<u32>,
        pub start_at: Option<chrono::NaiveDateTime>,
        pub end_at: Option<chrono::NaiveDateTime>,
        pub assigned_at: Option<chrono::NaiveDateTime>,
        pub assigned_by: Option<u32>,
        #[sea_orm(default_value = false)]
        pub is_primary: Option<bool>,
        /// enum(PENDING,ACTIVE,DISABLED,EXPIRED) default ACTIVE.
        pub status: Option<String>,
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
pub mod sys_org_units {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_org_units")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub parent_id: Option<u32>,
        pub name: String,
        pub code: Option<String>,
        pub leader_id: Option<u32>,
        /// enum(COMPANY,DIVISION,DEPARTMENT,TEAM,…) default DEPARTMENT.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        /// jsonb []string.
        pub business_scopes: Option<Json>,
        pub external_id: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_legal_entity: Option<bool>,
        pub registration_number: Option<String>,
        pub tax_id: Option<String>,
        pub legal_entity_org_id: Option<u32>,
        pub address: Option<String>,
        pub phone: Option<String>,
        pub email: Option<String>,
        pub timezone: Option<String>,
        pub country: Option<String>,
        pub latitude: Option<f64>,
        pub longitude: Option<f64>,
        pub start_at: Option<chrono::NaiveDateTime>,
        pub end_at: Option<chrono::NaiveDateTime>,
        pub contact_user_id: Option<u32>,
        /// jsonb []string.
        pub permission_tags: Option<Json>,
        pub path: Option<String>,
        pub remark: Option<String>,
        pub description: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
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
pub mod sys_positions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_positions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub code: Option<String>,
        pub org_unit_id: Option<u32>,
        pub reports_to_position_id: Option<u32>,
        pub description: Option<String>,
        pub job_family: Option<String>,
        pub job_grade: Option<String>,
        pub level: Option<i32>,
        #[sea_orm(default_value = 0)]
        pub headcount: Option<u32>,
        #[sea_orm(default_value = false)]
        pub is_key_position: Option<bool>,
        /// enum(REGULAR,MANAGER,LEAD,INTERN,CONTRACT,OTHER) default REGULAR.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        pub start_at: Option<chrono::NaiveDateTime>,
        pub end_at: Option<chrono::NaiveDateTime>,
        pub remark: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
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
pub mod sys_tenants {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_tenants")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub name: String,
        pub code: String,
        pub logo_url: Option<String>,
        pub domain: Option<String>,
        pub industry: Option<String>,
        pub admin_user_id: Option<u32>,
        /// enum(ON,OFF,EXPIRED,FREEZE) default ON.
        pub status: Option<String>,
        /// enum(TRIAL,PAID,INTERNAL,PARTNER,CUSTOM) default PAID.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        /// enum(PENDING,APPROVED,REJECTED) default PENDING.
        pub audit_status: Option<String>,
        pub subscription_at: Option<chrono::NaiveDateTime>,
        pub unsubscribe_at: Option<chrono::NaiveDateTime>,
        pub subscription_plan: Option<String>,
        pub expired_at: Option<chrono::NaiveDateTime>,
        pub plan_id: Option<u32>,
        pub remark: Option<String>,
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
pub mod sys_plans {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_plans")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub name: String,
        /// enum(FREE,STANDARD,ENTERPRISE) default FREE.
        pub version: Option<String>,
        /// enum(READONLY,BLOCK_LOGIN,FREEZE) default READONLY.
        pub expiry_policy: Option<String>,
        pub data_retention_days: Option<u32>,
        pub description: Option<String>,
        pub remark: Option<String>,
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
pub mod sys_plan_modules {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_plan_modules")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub plan_id: u32,
        /// enum(DASHBOARD,OPM,SYSTEM,DICT,TENANT,PERMISSION,LOG,INTERNAL_MESSAGE,FILE,TASK).
        pub module: Option<String>,
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
pub mod sys_plan_quotas {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_plan_quotas")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub plan_id: u32,
        /// enum(USER_LIMIT,STORAGE,API_CALL).
        pub quota_type: Option<String>,
        pub quota_value: Option<i64>,
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
