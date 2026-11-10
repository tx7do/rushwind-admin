//! The admin REST route pack — the admin face over the reusable edge
//! ([`super::edge`]): the admin contract's pool, the registered codec
//! subtypes, the mounted service implementations, the docs face, and
//! the admin audit face, all handed to the edge combinator that a
//! business module's route pack rides identically.
//!
//! Per-route layer composition (see the edge module docs): the
//! framework bind layer outermost on EVERY route, and the auth gate
//! (the auth crate) composed INSIDE it on gated routes only — that
//! order is the wire contract: codec/binding failures answer 400
//! ahead of any 401. The CORS policy and the request budget ride the
//! assembler's per-server edge block, through the gorilla-compatible
//! CORS layer (see rushwind_http::cors_compat — wires gorilla/handlers
//! there, whose emission rules tower-http does not reproduce).

use std::sync::Arc;

use crate::services::{
    AccessKeyService, AdminPortalService, AiAgentService, AiChatService, AiContentService,
    AiConversationService, AiKnowledgeBaseService, AiMessageService, AiProviderService,
    AiQueryService, AiUsageLogService, ApiAuditLogService, ApiService, AuthenticationService,
    BiDashboardService, ConfigService, DashboardService, DataAccessAuditLogService,
    DictEntryService, DictTypeService, FileService, FileTransferService,
    InternalMessageCategoryService, InternalMessageRecipientService, InternalMessageService,
    LanguageService, LoginAuditLogService, LoginPolicyService, MenuService, MfaService,
    MonitorAlertService, MyTenantUsageService, NotificationChannelService,
    NotificationDeliveryService, NotificationPreferenceService, NotificationRuleService,
    NotificationTemplateService, OnlineSessionService, OperationAuditLogService, OrgUnitService,
    PermissionAuditLogService, PermissionGroupService, PermissionService, PlanModuleService,
    PlanQuotaService, PlanService, PolicyEvaluationLogService, PositionService,
    PrintTemplateService, RedisCacheMonitorService, RoleService, ScriptLogService, ScriptService,
    ServerMonitorService, TaskMonitorService, TaskService, TenantService, UserProfileService,
    UserService,
};
use crate::state::AppState;
use proto::pool;

/// The deployment's registered codec subtypes — the packages
/// its binary imports (the compatibility spec §2.1 register).
const REGISTERED_SUBTYPES: &[&str] = &["json", "proto", "x-www-form-urlencoded"];

/// The admin route pack: the mounted router (see the module docs)
/// under this pack's settings node (the docs switches). The edge —
/// the CORS policy and the request budget — rides the assembler's
/// per-server edge block, not the pack.
pub fn pack(
    state: Arc<AppState>,
) -> impl Fn(
    serde_json::Value,
    rushwind_bootstrap::RouteInput,
) -> Result<rushwind_bootstrap::RouteSurface, rushwind_bootstrap::BootstrapError>
       + Send
       + Sync
       + 'static {
    super::edge::pack_with(move |settings| {
        let docs = crate::server::docs::wire(settings)?;
        Ok(build_router(Arc::clone(&state), docs))
    })
}

/// Builds the mounted router. `state` carries the verification engine
/// and the server-side session store; the assembly order lives in the
/// edge combinator.
pub fn build_router(state: Arc<AppState>, docs: crate::server::docs::Wire) -> axum::Router {
    super::edge::edge_router(
        &state,
        super::edge::EdgePack {
            pool: pool(),
            subtypes: REGISTERED_SUBTYPES,
            mounts: mount_items(&state),
            docs: Some(docs),
            audit: Some(crate::audit::AuditFace::admin().clone()),
            extras: vec![image_proxy_router(&state)],
        },
    )
}

/// The signed public image proxy: public by design — the HMAC in
/// the query is the credential — so it mounts without the gate,
/// mirroring the reference's raw (Operation-less) route.
fn image_proxy_router(state: &Arc<AppState>) -> axum::Router {
    axum::Router::new()
        .route(
            "/admin/v1/file/image",
            axum::routing::get(crate::services::image_proxy),
        )
        .with_state(Arc::clone(state))
}

/// The uniform service mount: every service follows the same
/// router-threading, service-construction, and wrap handshake. The
/// state is an explicit parameter — a module-level macro cannot
/// capture the function's local by name.
macro_rules! mount_service {
    ($state:expr, $mount:ident, $service:ident) => {{
        let state = std::sync::Arc::clone($state);
        // The wrap parameter is annotated so the closure is
        // higher-ranked over the reference lifetime — an unannotated
        // closure specializes to one lifetime and fails the
        // MountItem's for<'a> bound.
        Box::new(
            move |router_pub: axum::Router,
                  router_gate: axum::Router,
                  wrap: &super::edge::Wrap|
                  -> (axum::Router, axum::Router) {
                proto::gen::mounts::$mount(
                    router_pub,
                    router_gate,
                    std::sync::Arc::new($service {
                        state: std::sync::Arc::clone(&state),
                    }),
                    wrap,
                )
            },
        ) as super::edge::MountItem
    }};
}

/// The full mounted surface. The generator's auth-free table
/// classifies each of its route bindings.
fn mount_items(state: &Arc<AppState>) -> Vec<super::edge::MountItem> {
    vec![
        mount_service!(state, mount_access_key_service, AccessKeyService),
        mount_service!(state, mount_ai_agent_service, AiAgentService),
        mount_service!(state, mount_ai_chat_service, AiChatService),
        mount_service!(state, mount_ai_content_service, AiContentService),
        mount_service!(
            state,
            mount_ai_knowledge_base_service,
            AiKnowledgeBaseService
        ),
        mount_service!(state, mount_ai_query_service, AiQueryService),
        mount_service!(state, mount_ai_conversation_service, AiConversationService),
        mount_service!(state, mount_ai_message_service, AiMessageService),
        mount_service!(state, mount_ai_provider_service, AiProviderService),
        mount_service!(state, mount_ai_usage_log_service, AiUsageLogService),
        mount_service!(state, mount_admin_portal_service, AdminPortalService),
        mount_service!(state, mount_api_audit_log_service, ApiAuditLogService),
        mount_service!(state, mount_api_service, ApiService),
        mount_service!(state, mount_authentication_service, AuthenticationService),
        mount_service!(state, mount_bi_dashboard_service, BiDashboardService),
        mount_service!(state, mount_config_service, ConfigService),
        mount_service!(state, mount_dashboard_service, DashboardService),
        mount_service!(
            state,
            mount_data_access_audit_log_service,
            DataAccessAuditLogService
        ),
        mount_service!(state, mount_dict_entry_service, DictEntryService),
        mount_service!(state, mount_dict_type_service, DictTypeService),
        mount_service!(state, mount_file_service, FileService),
        mount_service!(state, mount_file_transfer_service, FileTransferService),
        mount_service!(
            state,
            mount_internal_message_category_service,
            InternalMessageCategoryService
        ),
        mount_service!(
            state,
            mount_internal_message_recipient_service,
            InternalMessageRecipientService
        ),
        mount_service!(
            state,
            mount_internal_message_service,
            InternalMessageService
        ),
        mount_service!(state, mount_language_service, LanguageService),
        mount_service!(state, mount_login_audit_log_service, LoginAuditLogService),
        mount_service!(state, mount_login_policy_service, LoginPolicyService),
        mount_service!(state, mount_menu_service, MenuService),
        mount_service!(state, mount_mfa_service, MfaService),
        mount_service!(
            state,
            mount_notification_channel_service,
            NotificationChannelService
        ),
        mount_service!(
            state,
            mount_notification_service,
            NotificationDeliveryService
        ),
        mount_service!(
            state,
            mount_notification_preference_service,
            NotificationPreferenceService
        ),
        mount_service!(
            state,
            mount_notification_rule_service,
            NotificationRuleService
        ),
        mount_service!(
            state,
            mount_notification_template_service,
            NotificationTemplateService
        ),
        mount_service!(state, mount_online_session_service, OnlineSessionService),
        mount_service!(
            state,
            mount_operation_audit_log_service,
            OperationAuditLogService
        ),
        mount_service!(state, mount_org_unit_service, OrgUnitService),
        mount_service!(
            state,
            mount_permission_audit_log_service,
            PermissionAuditLogService
        ),
        mount_service!(
            state,
            mount_permission_group_service,
            PermissionGroupService
        ),
        mount_service!(state, mount_permission_service, PermissionService),
        mount_service!(state, mount_plan_module_service, PlanModuleService),
        mount_service!(state, mount_plan_quota_service, PlanQuotaService),
        mount_service!(state, mount_plan_service, PlanService),
        mount_service!(
            state,
            mount_policy_evaluation_log_service,
            PolicyEvaluationLogService
        ),
        mount_service!(state, mount_position_service, PositionService),
        mount_service!(state, mount_print_template_service, PrintTemplateService),
        mount_service!(
            state,
            mount_redis_cache_monitor_service,
            RedisCacheMonitorService
        ),
        mount_service!(state, mount_monitor_alert_service, MonitorAlertService),
        mount_service!(state, mount_my_tenant_usage_service, MyTenantUsageService),
        mount_service!(state, mount_role_service, RoleService),
        mount_service!(state, mount_script_log_service, ScriptLogService),
        mount_service!(state, mount_script_service, ScriptService),
        mount_service!(state, mount_server_monitor_service, ServerMonitorService),
        mount_service!(state, mount_task_monitor_service, TaskMonitorService),
        mount_service!(state, mount_task_service, TaskService),
        mount_service!(state, mount_tenant_service, TenantService),
        mount_service!(state, mount_user_profile_service, UserProfileService),
        mount_service!(state, mount_user_service, UserService),
    ]
}
