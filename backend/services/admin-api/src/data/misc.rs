//! Platform entities: dicts, i18n, languages, configs, tasks, login
//! policies, access keys, notification channels, scripts, files, internal
//! messages.

pub mod sys_dict_types {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_dict_types")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub type_code: String,
        pub type_name: String,
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
pub mod sys_dict_entries {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_dict_entries")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub type_id: Option<u32>,
        pub entry_value: Option<String>,
        pub numeric_value: Option<i32>,
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
pub mod sys_dict_entry_i18n {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_dict_entry_i18n")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub entry_id: Option<u32>,
        pub language_code: Option<String>,
        pub entry_label: Option<String>,
        pub description: Option<String>,
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
pub mod sys_languages {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_languages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub language_code: String,
        pub language_name: String,
        pub native_name: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_default: Option<bool>,
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
pub mod sys_configs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_configs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub name: String,
        pub key: String,
        pub value: Option<String>,
        /// enum(STRING,BOOL,INT) default STRING.
        pub value_type: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_built_in: Option<bool>,
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
pub mod sys_tasks {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_tasks")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
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
pub mod sys_login_policies {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_login_policies")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub target_id: Option<String>,
        pub value: Option<String>,
        pub reason: Option<String>,
        /// enum(BLACKLIST,WHITELIST) default BLACKLIST.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        /// enum(IP,MAC,REGION,TIME,DEVICE) default IP.
        pub method: Option<String>,
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
pub mod sys_access_keys {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_access_keys")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub access_key: String,
        /// hex(SHA-256(secret)).
        pub secret_hash: String,
        pub expires_at: Option<chrono::NaiveDateTime>,
        pub last_used_at: Option<chrono::NaiveDateTime>,
        /// enum(OFF,ON) default ON.
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
pub mod sys_notification_channels {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_notification_channels")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub name: String,
        /// enum(EMAIL,WEBHOOK) default EMAIL.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        pub smtp_host: Option<String>,
        pub smtp_port: Option<u32>,
        pub smtp_username: Option<String>,
        pub smtp_password: Option<String>,
        pub smtp_from: Option<String>,
        /// enum(NONE,START_TLS,SSL) default START_TLS.
        pub smtp_tls: Option<String>,
        pub webhook_url: Option<String>,
        pub webhook_secret: Option<String>,
        /// enum(CUSTOM,NONE,DINGTALK,FEISHU,WECOM).
        pub webhook_sign_style: Option<String>,
        pub webhook_payload_template: Option<String>,
        pub remark: Option<String>,
        /// enum(OFF,ON) default ON.
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
pub mod sys_scripts {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_scripts")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
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
pub mod sys_script_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_script_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub script_id: Option<u32>,
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
pub mod files {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "files")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
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
pub mod internal_messages {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "internal_messages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub title: Option<String>,
        pub content: Option<String>,
        pub sender_id: Option<u32>,
        pub category_id: Option<u32>,
        /// enum(DRAFT,PUBLISHED,SCHEDULED,REVOKED,ARCHIVED,DELETED) default DRAFT.
        pub status: Option<String>,
        /// enum(NOTIFICATION,PRIVATE,GROUP) default NOTIFICATION.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
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
pub mod internal_message_categories {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "internal_message_categories")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub code: String,
        pub icon_url: Option<String>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
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
pub mod internal_message_recipients {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "internal_message_recipients")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub message_id: Option<u32>,
        pub recipient_user_id: Option<u32>,
        /// enum(SENT,RECEIVED,READ,REVOKED,DELETED) default SENT.
        pub status: Option<String>,
        pub received_at: Option<chrono::NaiveDateTime>,
        pub read_at: Option<chrono::NaiveDateTime>,
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

pub mod sys_notification_rules {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_notification_rules")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        /// enum(PASSWORD_RESET_CODE,CONTACT_BIND_CODE,CHANNEL_TEST_EMAIL,INTERNAL_MESSAGE) unique.
        pub event_type: Option<String>,
        /// enum(EMAIL,SMS,WEBHOOK,INTERNAL).
        pub channel: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_async: Option<bool>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
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

pub mod sys_notification_deliveries {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_notification_deliveries")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        /// enum(PASSWORD_RESET_CODE,CONTACT_BIND_CODE,CHANNEL_TEST_EMAIL,INTERNAL_MESSAGE).
        pub event_type: Option<String>,
        /// enum(EMAIL,SMS,WEBHOOK,INTERNAL).
        pub channel: Option<String>,
        pub channel_id: Option<u32>,
        pub recipient_user_id: Option<u32>,
        pub related_id: Option<u32>,
        pub target: Option<String>,
        /// enum(SENDING,SENT,FAILED,SKIPPED) default SENDING.
        pub status: Option<String>,
        pub last_error: Option<String>,
        pub request_id: Option<String>,
        #[sea_orm(default_value = 0)]
        pub attempts: Option<u32>,
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
