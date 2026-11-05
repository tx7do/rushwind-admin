//! AI entity tables: providers, agent apps/runs, conversations, messages,
//! knowledge bases, docs, chunks, usage logs.
//!
//! The pgvector embedding column does not ride the SeaORM model (matching
//! the upstream ent constraint): it is created by hand-written startup SQL
//! (CREATE EXTENSION vector + dimension-aware ALTER) and read/written via
//! raw SQL on the RAG lane only.

pub mod sys_ai_providers {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_providers")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: Option<String>,
        /// enum(OPENAI,AZURE_OPENAI,OLLAMA,OPENAI_COMPATIBLE,…) — the wire
        /// spelling the reference's ent enum carries.
        pub model_type: Option<String>,
        pub model_name: Option<String>,
        /// 云端 OpenAI 兼容 API 地址。
        pub base_url: Option<String>,
        pub organization: Option<String>,
        /// API Key（AES-GCM 加密落库，enc: 前缀；GOWIND/RUSHWIND crypto key）。
        pub api_key: Option<String>,
        /// 脱敏提示（如 sk-***abcd）。
        pub api_key_hint: Option<String>,
        /// 本地模型（Ollama 等）主机与端口。
        pub local_host: Option<String>,
        pub local_port: Option<i32>,
        pub timeout_seconds: Option<i32>,
        /// 系统提示词。
        pub system_prompt: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_default: Option<bool>,
        pub remark: Option<String>,
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

pub mod sys_ai_agent_apps {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_agent_apps")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub description: Option<String>,
        /// 画布图（节点编排 JSON 原文；服务端不解析只透传）。
        pub graph: Option<String>,
        #[sea_orm(default_value = true)]
        pub enabled: Option<bool>,
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

pub mod sys_ai_agent_runs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_agent_runs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub app_id: u32,
        /// enum(PENDING,RUNNING,SUCCESS,FAILED)（引擎回写）。
        pub status: Option<String>,
        pub input: Option<String>,
        pub output: Option<String>,
        pub error: Option<String>,
        /// 节点执行轨迹（JSON 原文）。
        pub trace: Option<String>,
        #[sea_orm(default_value = 0)]
        pub prompt_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub completion_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub total_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub duration_ms: Option<u32>,
        pub user_id: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sys_ai_conversations {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_conversations")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub title: Option<String>,
        pub provider_id: Option<u32>,
        pub user_id: u32,
        pub last_message_at: Option<chrono::NaiveDateTime>,
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

pub mod sys_ai_messages {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_messages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub conversation_id: u32,
        /// enum(USER,ASSISTANT,SYSTEM).
        pub role: Option<String>,
        pub content: Option<String>,
        pub model_name: Option<String>,
        #[sea_orm(default_value = 0)]
        pub prompt_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub completion_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub duration_ms: Option<u32>,
        pub error_message: Option<String>,
        pub user_id: u32,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sys_ai_knowledge_bases {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_knowledge_bases")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub description: Option<String>,
        pub provider_id: Option<u32>,
        /// embedding 模型名（按 provider 的 /v1/embeddings 调用）。
        pub embedding_model: Option<String>,
        pub user_id: u32,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sys_ai_docs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_docs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub base_id: u32,
        pub name: String,
        #[sea_orm(default_value = 0)]
        pub chunk_count: Option<u32>,
        /// enum(PENDING,PROCESSING,READY,FAILED).
        pub status: Option<String>,
        pub error_message: Option<String>,
        pub user_id: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sys_ai_chunks {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_chunks")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub doc_id: Option<u32>,
        pub content: Option<String>,
        pub chunk_index: Option<u32>,
        /// embedding 向量列（pgvector vector(1536)）不进本模型：
        /// 启动期手写 SQL 补建，读写走 RAG 通道的原生 SQL。
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sys_ai_usage_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_ai_usage_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub provider_id: Option<u32>,
        pub conversation_id: Option<u32>,
        pub user_id: Option<u32>,
        pub model_name: Option<String>,
        #[sea_orm(default_value = 0)]
        pub prompt_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub completion_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub total_tokens: Option<u32>,
        #[sea_orm(default_value = 0)]
        pub duration_ms: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
