//! MyTenantUsageService — 租户自助用量：租户管理员查看本租户的套餐用量
//! 与配额。与 TenantService.GetUsage（平台管理员按租户 ID 查任意租户）
//! 互补——本服务把租户 ID 钉死为 operator 的租户，不接受客户端传参。
//!
//! 存在的意义：配额硬限制落地后，租户用户撞 403 plan quota exceeded 时
//! 需要有地方看到"用了多少/上限多少"，否则报错无解释。

use std::sync::Arc;

use crate::state::{operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::TenantUsage;

pub struct MyTenantUsageService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::MyTenantUsageServiceHandlers for MyTenantUsageService {
    /// 返回当前操作者所属租户的用量与配额。平台用户（tenantId==0）无套餐
    /// 语义，返回空 Usage（前端不展示该卡片）。
    async fn get_my_tenant_usage(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<TenantUsage, StatusError> {
        let payload = operator_of(&ctx)?;
        let tenant_id = payload.tenant_id;
        if tenant_id == 0 {
            return Ok(TenantUsage {
                tenant_id: 0,
                ..Default::default()
            });
        }
        super::tenant::usage_for(&self.state, tenant_id).await
    }
}
