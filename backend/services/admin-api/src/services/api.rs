//! ApiService — service layer:
//! `sys_apis` CRUD plus SyncApis (the (path, method)-keyed MERGE against the
//! vendored OpenAPI document — ids stable, bindings endpoint-snapshotted) and
//! GetWalkRouteData.

use std::sync::Arc;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, Set,
};

use crate::mapping;
use crate::state::{db_err, internal_error, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    Api, CreateApiRequest, DeleteApiRequest, GetApiRequest, ListApiResponse, UpdateApiRequest,
};

/// The `ServiceTagToBusinessModule` map
/// (pkg/constants/module): tag → module string.
fn tag_to_module(tag: &str) -> &'static str {
    match tag {
        "AdminPortalService" | "DashboardService" | "AuthenticationService" => "DASHBOARD",
        "UserService" | "OrgUnitService" | "PositionService" | "UserProfileService"
        | "RoleService" => "OPM",
        "MenuService" | "ApiService" | "PermissionService" | "PermissionGroupService" => {
            "PERMISSION"
        }
        "DictTypeService" | "DictEntryService" => "DICT",
        "LanguageService"
        | "LoginPolicyService"
        | "ConfigService"
        | "AccessKeyService"
        | "ServerMonitorService"
        | "NotificationChannelService"
        | "OnlineSessionService" => "SYSTEM",
        "FileService" | "FileTransferService" => "FILE",
        "TaskService" => "TASK",
        "TenantService"
        | "PlanService"
        | "PlanQuotaService"
        | "PlanModuleService"
        | "MyTenantUsageService" => "TENANT",
        "ApiAuditLogService"
        | "LoginAuditLogService"
        | "OperationAuditLogService"
        | "DataAccessAuditLogService"
        | "PermissionAuditLogService"
        | "PolicyEvaluationLogService"
        | "RedisCacheMonitorService" => "LOG",
        "InternalMessageService"
        | "InternalMessageCategoryService"
        | "InternalMessageRecipientService"
        | "NotificationPreferenceService" => "INTERNAL_MESSAGE",
        "AiProviderService"
        | "AiConversationService"
        | "AiMessageService"
        | "AiChatService"
        | "AiUsageLogService"
        | "AiKnowledgeBaseService"
        | "AiQueryService"
        | "AiAgentService"
        | "AiContentService" => "AI",
        "TaskMonitorService" => "TASK",
        // 显式化（此前走 SYSTEM 兜底）：无专属 Module 枚举值的移植域
        "MonitorAlertService"
        | "ScriptService"
        | "ScriptLogService"
        | "BiDashboardService"
        | "MfaService" => "SYSTEM",
        // PRINT 模块（2026-11-09 上游：打印模板对租户开放）
        "PrintTemplateService" => "PRINT",
        _ => "SYSTEM",
    }
}

fn api_proto(r: crate::data::sys_apis::Model) -> Api {
    Api {
        id: Some(r.id),
        operation: r.operation,
        path: r.path,
        method: r.method,
        module: r.module,
        module_description: r.module_description,
        business_module: r
            .business_module
            .as_deref()
            .map(|m| mapping::menu_module_of(m).unwrap_or(0)),
        description: r.description,
        scope: r.scope.as_deref().map(|s| if s == "APP" { 2 } else { 1 }),
        status: r.status.as_deref().map(crate::state::status_to_proto),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct ApiService {
    pub state: Arc<AppState>,
}

impl ApiService {
    /// MERGE 主体（单事务内调用）：派生集（OpenAPI 端点清单）按 (path, method)
    /// 与库存对齐——命中→原位更新（保 id，授权绑定零漂移）/ 缺失→插入（id 自增）
    /// / 库存多余→删行并级联清权限绑定（api_id 与端点快照双口径）。删除段放
    /// 最后：中途失败残留的是"本该删的旧端点"，重跑同步即自愈，好过删多了。
    pub(crate) async fn sync_merge_in_txn(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        rows: Vec<super::api::DerivedApiRow>,
        operator_id: u32,
    ) -> Result<(), StatusError> {
        let existing = crate::data::sys_apis::Entity::find()
            .all(txn)
            .await
            .map_err(db_err)?;
        let mut existing_by_key: std::collections::HashMap<(String, String), u32> =
            std::collections::HashMap::with_capacity(existing.len());
        let mut row_by_id: std::collections::HashMap<u32, &crate::data::sys_apis::Model> =
            std::collections::HashMap::with_capacity(existing.len());
        for row in &existing {
            let (Some(path), Some(method)) = (row.path.clone(), row.method.clone()) else {
                continue;
            };
            existing_by_key.insert((path, method.to_uppercase()), row.id);
            row_by_id.insert(row.id, row);
        }

        let mut spec_keys = std::collections::HashSet::with_capacity(rows.len());
        for (path, method, operation, module, _) in &rows {
            let key = (path.clone(), method.clone());
            if !spec_keys.insert(key) {
                continue; // spec 内重复键防御性跳过
            }
            let business = tag_to_module_by_operation(operation);
            match existing_by_key.get(&(path.clone(), method.clone())) {
                Some(&exist_id) => {
                    let exist = row_by_id[&exist_id];
                    let next_business = Some(business.to_string());
                    if exist.operation.as_deref() == Some(operation.as_str())
                        && exist.module.as_deref() == Some(module.as_str())
                        && exist.business_module == next_business
                    {
                        continue; // 元数据一致零操作
                    }
                    let mut a: crate::data::sys_apis::ActiveModel = exist.clone().into();
                    a.operation = Set(Some(operation.clone()));
                    a.module = Set(Some(module.clone()));
                    a.business_module = Set(next_business);
                    crate::stamp_update!(a, operator_id);
                    a.update(txn).await.map_err(db_err)?;
                }
                None => {
                    crate::data::sys_apis::ActiveModel {
                        path: Set(Some(path.clone())),
                        method: Set(Some(method.clone())),
                        operation: Set(Some(operation.clone())),
                        module: Set(Some(module.clone())),
                        business_module: Set(Some(business.to_string())),
                        scope: Set(Some("ADMIN".into())),
                        status: Set(Some("ON".into())),
                        created_by: Set(Some(operator_id)),
                        created_at: Set(Some(crate::data::now())),
                        updated_at: Set(Some(crate::data::now())),
                        ..Default::default()
                    }
                    .insert(txn)
                    .await
                    .map_err(db_err)?;
                }
            }
        }

        // 移除段：库存有、派生集没有（含 NULL/空身份行——它们不是任何端点）。
        let gone: Vec<&crate::data::sys_apis::Model> = existing
            .iter()
            .filter(|row| {
                let (Some(path), Some(method)) = (row.path.clone(), row.method.clone()) else {
                    return true;
                };
                !spec_keys.contains(&(path, method.to_uppercase()))
            })
            .collect();
        if !gone.is_empty() {
            let gone_ids: Vec<u32> = gone.iter().map(|r| r.id).collect();
            for row in &gone {
                let path = row.path.clone().unwrap_or_default();
                let method = row.method.clone().unwrap_or_default();
                if !path.is_empty() && !method.is_empty() {
                    crate::data::sys_permission_apis::Entity::delete_many()
                        .filter(
                            Condition::all()
                                .add(
                                    crate::data::sys_permission_apis::Column::EndpointPath
                                        .eq(&path),
                                )
                                .add(
                                    crate::data::sys_permission_apis::Column::EndpointMethod
                                        .eq(method.to_uppercase()),
                                ),
                        )
                        .exec(txn)
                        .await
                        .map_err(db_err)?;
                }
            }
            crate::data::sys_permission_apis::Entity::delete_many()
                .filter(crate::data::sys_permission_apis::Column::ApiId.is_in(gone_ids.clone()))
                .exec(txn)
                .await
                .map_err(db_err)?;
            crate::data::sys_apis::Entity::delete_many()
                .filter(crate::data::sys_apis::Column::Id.is_in(gone_ids))
                .exec(txn)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::ApiServiceHandlers for ApiService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListApiResponse, StatusError> {
        let repo = crate::data::repos::ApiRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListApiResponse {
            items: rows.into_iter().map(api_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetApiRequest,
    ) -> Result<Api, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::permission::service::v1::get_api_request::QueryBy
        );
        let row = crate::data::sys_apis::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("api"))?;
        Ok(api_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateApiRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_apis::ActiveModel {
            operation: Set(data.operation),
            path: Set(data.path),
            method: Set(data.method),
            module: Set(data.module),
            module_description: Set(data.module_description),
            business_module: Set(data.business_module.map(|m| {
                match m {
                    1 => "DASHBOARD",
                    2 => "OPM",
                    3 => "SYSTEM",
                    4 => "DICT",
                    5 => "TENANT",
                    6 => "PERMISSION",
                    7 => "LOG",
                    8 => "INTERNAL_MESSAGE",
                    9 => "FILE",
                    10 => "TASK",
                    11 => "AI",
                    _ => "SYSTEM",
                }
                .to_string()
            })),
            description: Set(data.description),
            scope: Set(Some(if data.scope == Some(2) {
                "APP".into()
            } else {
                "ADMIN".into()
            })),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateApiRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_apis::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("api"))?;
        let mut a: crate::data::sys_apis::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = &data.path {
                a.path = Set(Some(v.clone()));
            }
            if let Some(v) = &data.method {
                a.method = Set(Some(v.clone()));
            }
            if let Some(v) = &data.operation {
                a.operation = Set(Some(v.clone()));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteApiRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::permission::service::v1::delete_api_request::QueryBy
        );
        // 先取行拿端点身份：删行后按 api_id 与 (path, method) 快照双口径
        // 级联清理权限绑定，消灭孤儿绑定（孤儿绑定会在 id 重编号后指到无关
        // 端点造成越权放行——上游 CascadeDeleteBindings 同语义）。
        let row = crate::data::sys_apis::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_permission_apis::Entity::delete_many()
            .filter(crate::data::sys_permission_apis::Column::ApiId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        if let Some(row) = row {
            let path = row.path.clone().unwrap_or_default();
            let method = row.method.clone().unwrap_or_default();
            if !path.is_empty() && !method.is_empty() {
                crate::data::sys_permission_apis::Entity::delete_many()
                    .filter(
                        Condition::all()
                            .add(crate::data::sys_permission_apis::Column::EndpointPath.eq(&path))
                            .add(
                                crate::data::sys_permission_apis::Column::EndpointMethod
                                    .eq(method.to_uppercase()),
                            ),
                    )
                    .exec(&self.state.db)
                    .await
                    .map_err(db_err)?;
            }
        }
        crate::data::sys_apis::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn sync_apis(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let rows = derived_api_rows().map_err(internal_error)?;

        // MERGE 按自然键 (path, method) 单事务增量对齐（上游 SyncMerge 同教义，
        // 仓内不再有删除重建型同步）：旧的 Truncate + 顺序重编号造成过两类事故
        // ——同步窗口内租户闸 fail-closed 全租户 403；id 全表洗牌让 api_id 绑定
        // 漂移（授权悄悄换到别的端点）。现 id 稳定、绑定存端点快照，授权零漂移；
        // 移除段同事务级联清绑定。
        use sea_orm::TransactionTrait as _;
        let txn = self.state.db.begin().await.map_err(db_err)?;
        let result = self.sync_merge_in_txn(&txn, rows, payload.user_id).await;
        match result {
            Ok(()) => txn.commit().await.map_err(db_err)?,
            Err(_) => {
                let _ = txn.rollback().await;
            }
        }
        result.map(|_| Empty {})
    }

    async fn get_walk_route_data(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListApiResponse, StatusError> {
        // The debug surface: the live route table (id-path-method only).
        let rows = crate::data::sys_apis::Entity::find()
            .order_by_asc(crate::data::sys_apis::Column::Id)
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(ListApiResponse {
            items: rows.into_iter().map(api_proto).collect(),
            total: 0,
        })
    }
}

fn tag_to_module_by_operation(operation: &str) -> &'static str {
    let service = operation.split('_').next().unwrap_or("");
    tag_to_module(service)
}

/// 派生端点行：(path, method, operation, module, 预留)。
pub(crate) type DerivedApiRow = (String, String, String, String, Option<u32>);

/// 派生端点清单：解析打进二进制的 OpenAPI 文档（不含 id）——(path, method)
/// 即身份。启动期同步与 SyncApis RPC 共用。
pub(crate) fn derived_api_rows() -> Result<Vec<DerivedApiRow>, String> {
    let doc: serde_yaml::Value = serde_yaml::from_str(crate::assets::OPENAPI_DATA)
        .map_err(|e| format!("openapi parse: {e}"))?;
    let mut rows: Vec<(String, String, String, String, Option<u32>)> = Vec::new();
    let paths = doc
        .get("paths")
        .and_then(|p| p.as_mapping())
        .ok_or("openapi paths missing")?;
    for (path, methods) in paths {
        let Some(methods) = methods.as_mapping() else {
            continue;
        };
        for (method, op) in methods {
            let (Some(path), Some(method), Some(op)) =
                (path.as_str(), method.as_str(), op.as_mapping())
            else {
                continue;
            };
            let method = method.to_uppercase();
            if !["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&method.as_str()) {
                continue;
            }
            let operation = op
                .get(serde_yaml::Value::from("operationId"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let tag = op
                .get(serde_yaml::Value::from("tags"))
                .and_then(|v| v.as_sequence())
                .and_then(|seq| seq.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let module = tag_to_module(&tag).to_string();
            rows.push((path.to_string(), method, operation, module, None));
        }
    }
    rows.sort_by_key(|a| (a.0.clone(), a.1.clone()));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tag→模块映射表全量钉死（上游 TestServiceTagToBusinessModuleExactMapping
    /// 同教义）：这张表驱动 sys_apis.business_module，是套餐模块白名单的判定
    /// 来源——任何条目漂移都会让对应服务的 API 在租户侧被误放行或误拒绝。
    #[test]
    fn tag_to_module_table_pinned_whole() {
        let expected: Vec<(&str, &str)> = vec![
            ("AdminPortalService", "DASHBOARD"),
            ("DashboardService", "DASHBOARD"),
            ("AuthenticationService", "DASHBOARD"),
            ("UserService", "OPM"),
            ("OrgUnitService", "OPM"),
            ("PositionService", "OPM"),
            ("UserProfileService", "OPM"),
            ("RoleService", "OPM"),
            ("MenuService", "PERMISSION"),
            ("ApiService", "PERMISSION"),
            ("PermissionService", "PERMISSION"),
            ("PermissionGroupService", "PERMISSION"),
            ("DictTypeService", "DICT"),
            ("DictEntryService", "DICT"),
            ("LanguageService", "SYSTEM"),
            ("FileService", "FILE"),
            ("FileTransferService", "FILE"),
            ("TaskService", "TASK"),
            ("TaskMonitorService", "TASK"),
            ("LoginPolicyService", "SYSTEM"),
            ("ConfigService", "SYSTEM"),
            ("AccessKeyService", "SYSTEM"),
            ("AiProviderService", "AI"),
            ("AiConversationService", "AI"),
            ("AiMessageService", "AI"),
            ("AiChatService", "AI"),
            ("AiUsageLogService", "AI"),
            ("AiKnowledgeBaseService", "AI"),
            ("AiQueryService", "AI"),
            ("AiAgentService", "AI"),
            ("AiContentService", "AI"),
            ("TenantService", "TENANT"),
            ("PlanService", "TENANT"),
            ("PlanQuotaService", "TENANT"),
            ("PlanModuleService", "TENANT"),
            ("MyTenantUsageService", "TENANT"),
            ("ApiAuditLogService", "LOG"),
            ("LoginAuditLogService", "LOG"),
            ("OperationAuditLogService", "LOG"),
            ("DataAccessAuditLogService", "LOG"),
            ("PermissionAuditLogService", "LOG"),
            ("PolicyEvaluationLogService", "LOG"),
            ("RedisCacheMonitorService", "LOG"),
            ("ServerMonitorService", "SYSTEM"),
            ("MonitorAlertService", "SYSTEM"),
            ("NotificationChannelService", "SYSTEM"),
            ("NotificationService", "SYSTEM"),
            ("NotificationRuleService", "SYSTEM"),
            ("NotificationTemplateService", "SYSTEM"),
            ("OnlineSessionService", "SYSTEM"),
            ("InternalMessageService", "INTERNAL_MESSAGE"),
            ("InternalMessageCategoryService", "INTERNAL_MESSAGE"),
            ("InternalMessageRecipientService", "INTERNAL_MESSAGE"),
            ("NotificationPreferenceService", "INTERNAL_MESSAGE"),
            ("ScriptService", "SYSTEM"),
            ("ScriptLogService", "SYSTEM"),
            ("BiDashboardService", "SYSTEM"),
            ("PrintTemplateService", "PRINT"),
            ("MfaService", "SYSTEM"),
        ];
        for (tag, module) in &expected {
            assert_eq!(tag_to_module(tag), *module, "{tag}");
        }
        // 未登记 tag 落 SYSTEM 兜底（租户白名单最宽域，不误拒）
        assert_eq!(tag_to_module("UnknownService"), "SYSTEM");
    }
}
