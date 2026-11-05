//! AiQueryService — 自然语言查询（NL→SQL）。
//!
//! 上游 Ask 的完整链路是安全关键面：NL→SQL 生成（带对话历史消解指代）+
//! 四重护栏（只读语句 / 表白名单 / LIMIT 钳制 / 二次正则复核，租户侧另加
//! tenant_id 后置校验 fail-closed）+ 只读事务执行。护栏的严谨性就是注入
//! 防线的本体——本仓以显式 NOT_IMPLEMENTED 声明边界，待独立里程碑按上游
//! ai_query_service.go（generateSQL / sanitizeSQLFor / executeReadOnly /
//! aiQueryTableWhitelist 四件套）逐件移植；上传面同款边界语义
//! （UploadDocFile）。proto 生成面已挂载，调用得到干净的 501 而非 404。

use std::sync::Arc;

use crate::state::{status_error, AppState, StatusError};
use proto::proto::ai::service::v1::{AskAiQueryRequest, AskAiQueryResponse};

pub struct AiQueryService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::AiQueryServiceHandlers for AiQueryService {
    async fn ask(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: AskAiQueryRequest,
    ) -> Result<AskAiQueryResponse, StatusError> {
        Err(status_error(
            "NOT_IMPLEMENTED",
            "nl2sql rides its own milestone: the read-only guard chain (whitelist tables / \
             limit clamp / statement read-only check / tenant post-filter) must land whole \
             before any generated sql executes",
        ))
    }
}
