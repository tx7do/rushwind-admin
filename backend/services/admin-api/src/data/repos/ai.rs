//! AI repos — provider（tenant 面）与 usage-log（platform 台账只读）。

use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QuerySelect;

use crate::data::sys_ai_providers as provider;
use crate::data::sys_ai_usage_logs as usage;
use crate::data::Viewer;
use crate::state::{db_err, StatusError};

pub struct AiProviderRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

impl<'a> AiProviderRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    fn condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => Condition::all().add(provider::Column::TenantId.eq(tid)),
            None => Condition::all(),
        }
    }

    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<provider::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            provider::Entity::find().filter(self.condition()),
            req,
        )
        .await
    }

    pub async fn find_scoped(&self, id: u32) -> Result<Option<provider::Model>, StatusError> {
        let mut query = provider::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = query.filter(provider::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }

    /// 默认提供商（发送链路取数锚）。
    pub async fn find_default(&self) -> Result<Option<provider::Model>, StatusError> {
        let mut query = provider::Entity::find().filter(provider::Column::IsDefault.eq(true));
        if let Some(tid) = self.viewer.tenant_scope() {
            query = query.filter(provider::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }
}

pub struct AiUsageLogRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> AiUsageLogRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// 台账写入（发送链路挂账）。
    pub async fn insert(&self, row: usage::ActiveModel) -> Result<usage::Model, StatusError> {
        use sea_orm::ActiveModelTrait;
        row.insert(self.db).await.map_err(db_err)
    }

    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<usage::Model>, u64), StatusError> {
        crate::paging::fetch_paged(self.db, usage::Entity::find(), req).await
    }

    /// 当月（本地时区）用量汇总：tokens 与调用次数。
    pub async fn month_summary(&self) -> Result<(u32, u32), StatusError> {
        use chrono::Datelike as _;
        let month_start = crate::data::now()
            .date()
            .with_day(1)
            .unwrap_or_default()
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default();
        let row = usage::Entity::find()
            .filter(usage::Column::CreatedAt.gte(month_start))
            .select_only()
            .column_as(usage::Column::TotalTokens.sum(), "tokens")
            .column_as(usage::Column::Id.count(), "calls")
            .into_tuple::<(Option<i64>, i64)>()
            .one(self.db)
            .await
            .map_err(db_err)?;
        Ok(match row {
            Some((tokens, calls)) => (tokens.unwrap_or(0).max(0) as u32, calls.max(0) as u32),
            None => (0, 0),
        })
    }

    /// 租户 AI_TOKENS 月度配额上限：tenant.plan_id → 套餐的 AI_TOKENS
    /// quota_value。None = 未配置（平台租户 / 无套餐 / 未配该维度 → 不限）。
    pub async fn tenant_token_quota_limit(
        &self,
        tenant_id: u32,
    ) -> Result<Option<i64>, StatusError> {
        use sea_orm::QueryOrder as _;
        if tenant_id == 0 {
            return Ok(None);
        }
        let plan_id = crate::data::sys_tenants::Entity::find_by_id(tenant_id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .and_then(|t| t.plan_id);
        let Some(plan_id) = plan_id else {
            return Ok(None);
        };
        let quotas = crate::data::sys_plan_quotas::Entity::find()
            .filter(crate::data::sys_plan_quotas::Column::PlanId.eq(plan_id))
            .filter(crate::data::sys_plan_quotas::Column::QuotaType.eq("AI_TOKENS"))
            .order_by_asc(crate::data::sys_plan_quotas::Column::Id)
            .all(self.db)
            .await
            .map_err(db_err)?;
        Ok(quotas.into_iter().find_map(|q| q.quota_value))
    }

    /// 某租户自 monthStart 起的 tokens 总和（配额判定用；上游
    /// SumTokensByTenantSince 同型）。
    pub async fn sum_tokens_by_tenant_since(
        &self,
        tenant_id: u32,
        month_start: chrono::NaiveDateTime,
    ) -> Result<u32, StatusError> {
        let row = usage::Entity::find()
            .filter(usage::Column::CreatedAt.gte(month_start))
            .filter(usage::Column::TenantId.eq(tenant_id))
            .select_only()
            .column_as(usage::Column::TotalTokens.sum(), "tokens")
            .into_tuple::<Option<i64>>()
            .one(self.db)
            .await
            .map_err(db_err)?;
        Ok(row.flatten().unwrap_or(0).max(0) as u32)
    }
}

pub struct AiConversationRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

impl<'a> AiConversationRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    fn condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => {
                Condition::all().add(crate::data::sys_ai_conversations::Column::TenantId.eq(tid))
            }
            None => Condition::all(),
        }
    }

    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<crate::data::sys_ai_conversations::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            crate::data::sys_ai_conversations::Entity::find().filter(self.condition()),
            req,
        )
        .await
    }

    pub async fn find_scoped(
        &self,
        id: u32,
    ) -> Result<Option<crate::data::sys_ai_conversations::Model>, StatusError> {
        let mut query = crate::data::sys_ai_conversations::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = query.filter(crate::data::sys_ai_conversations::Column::TenantId.eq(tid));
        }
        query.one(self.db).await.map_err(db_err)
    }
}

pub struct AiMessageRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

impl<'a> AiMessageRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    /// 消息经会话归属租户（消息表自身无 tenant 列）。
    fn condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => {
                Condition::all().add(crate::data::sys_ai_conversations::Column::TenantId.eq(tid))
            }
            None => Condition::all(),
        }
    }

    pub async fn paged_list(
        &self,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<crate::data::sys_ai_messages::Model>, u64), StatusError> {
        let conv_ids = crate::data::sys_ai_conversations::Entity::find()
            .filter(self.condition())
            .select_only()
            .column(crate::data::sys_ai_conversations::Column::Id)
            .into_tuple::<u32>()
            .all(self.db)
            .await
            .map_err(db_err)?;
        crate::paging::fetch_paged(
            self.db,
            crate::data::sys_ai_messages::Entity::find()
                .filter(crate::data::sys_ai_messages::Column::ConversationId.is_in(conv_ids)),
            req,
        )
        .await
    }

    pub async fn find_scoped(
        &self,
        id: u32,
    ) -> Result<Option<crate::data::sys_ai_messages::Model>, StatusError> {
        crate::data::sys_ai_messages::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)
    }
}
