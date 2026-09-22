//! The REST route pack — //! `internal/server/module`: mounts the full
//! route surface behind the real service implementations, splits the
//! auth-free public subtree from the gated one (the auth-free set),
//! composes the per-route layer stack, merges the two with the docs
//! surface and the audit layer, and hands the router to the lifecycle
//! assembler — whose edge block (the CORS policy and the request budget
//! from the server document) wraps the whole thing.
//!
//! Per-route layer composition (the `wrap` closure below): the framework
//! bind layer — body and query binding — outermost on EVERY route, and
//! the auth gate (the auth crate) composed INSIDE it on gated routes
//! only. That order is the wire contract: codec/binding failures answer
//! 400 ahead of any 401.
//!
//! The authorization engine wires here too lands with the
//! storage phase; until then the gate is the protected subtree's only
//! defense.
//!
//! The docs switches (Swagger UI / Redoc / the raw spec mount) ride
//! the pack's settings node; the CORS policy and the request budget
//! ride the assembler's per-server edge block, through the
//! gorilla-compatible CORS layer (see rushwind_http::cors_compat — wires
//! gorilla/handlers there, whose emission rules tower-http does not
//! reproduce).

use std::sync::Arc;

use axum::routing::MethodRouter;

use crate::services::{
    AccessKeyService, AdminPortalService, ApiAuditLogService, ApiService, AuthenticationService,
    ConfigService, DashboardService, DataAccessAuditLogService, DictEntryService, DictTypeService,
    FileService, FileTransferService, InternalMessageCategoryService,
    InternalMessageRecipientService, InternalMessageService, LanguageService, LoginAuditLogService,
    LoginPolicyService, MenuService, MfaService, NotificationChannelService, OnlineSessionService,
    OperationAuditLogService, OrgUnitService, PermissionAuditLogService, PermissionGroupService,
    PermissionService, PlanModuleService, PlanQuotaService, PlanService,
    PolicyEvaluationLogService, PositionService, RedisCacheMonitorService, RoleService,
    ScriptLogService, ScriptService, ServerMonitorService, TaskService, TenantService,
    UserProfileService, UserService,
};
use crate::state::AppState;
use auth::auth_gate;
use proto::pool;
use rushwind_http_binding::bindgate::bind_run;
use rushwind_http_binding::wire::RouteWire;

/// The deployment's registered codec subtypes — the packages
/// its binary imports (the compatibility spec §2.1 register).
const REGISTERED_SUBTYPES: &[&str] = &["json", "proto", "x-www-form-urlencoded"];

/// The adapter wiring the service-side Redis session store into the
/// auth gate's server-side check stage.
struct RedisTokenChecker(crate::token::TokenStore);

#[async_trait::async_trait]
impl auth::AccessTokenChecker for RedisTokenChecker {
    async fn is_valid_access_token(&self, uid: u32, jti: &str, token: &str) -> bool {
        self.0.is_valid_access_token(uid, jti, token).await
    }
    async fn is_blocked_access_token(&self, jti: &str) -> bool {
        self.0.is_blocked_access_token(jti).await
    }
}

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
    move |settings, _input| {
        let docs = crate::server::docs::wire(settings)?;
        Ok(rushwind_bootstrap::RouteSurface::new(build_router(
            Arc::clone(&state),
            docs,
        )))
    }
}

/// Builds the mounted router. `state` carries the verification engine
/// and the server-side session store; the assembly order mirrors
/// module.
pub fn build_router(state: Arc<AppState>, docs: crate::server::docs::Wire) -> axum::Router {
    let descriptor_pool = pool();
    let authenticator = Arc::clone(&state.authenticator);
    let checker = Arc::new(RedisTokenChecker(state.tokens.clone()))
        as Arc<dyn auth::AccessTokenChecker + 'static>;
    let tenant_checker = Arc::new(crate::authorizer::TenantAccessChecker(Arc::clone(&state)))
        as Arc<dyn auth::TenantAccessChecker + 'static>;
    let authorizer = Arc::new(crate::authorizer::AccessAuthorizer::new(Arc::clone(&state)))
        as Arc<dyn auth::AuthorizationEvaluator + 'static>;

    // The per-route layer composition (see the module docs): bind layer
    // outermost always, auth gate inside it on gated routes.
    let wrap = |mr: MethodRouter, wire: &RouteWire, gated: bool| -> MethodRouter {
        let mut out = mr;
        if gated {
            let auth = Arc::clone(&authenticator);
            let checker = Arc::clone(&checker);
            let tenant_checker = Arc::clone(&tenant_checker);
            let authorizer = Arc::clone(&authorizer);
            let gate = axum::middleware::from_fn(move |req, next| {
                let auth = Arc::clone(&auth);
                let checker = Arc::clone(&checker);
                let tenant_checker = Arc::clone(&tenant_checker);
                let authorizer = Arc::clone(&authorizer);
                async move { auth_gate(auth, checker, tenant_checker, authorizer, req, next).await }
            });
            out = out.layer(gate);
        }
        let input_fq = wire.input_fq;
        let body_star = wire.body_star;
        let bind = axum::middleware::from_fn(move |req, next| {
            let (fq, body) = (input_fq, body_star);
            async move { bind_run(descriptor_pool, fq, body, REGISTERED_SUBTYPES, req, next).await }
        });
        out = out.layer(bind);
        out
    };

    // The full mounted surface, placeholder services behind it. Every
    // mount call threads both routers; the generator's auth-free table
    // classifies each of its route bindings.
    let mut router_pub = axum::Router::new();
    let mut router_gate = axum::Router::new();
    /// The uniform service mount: every service follows the same
    /// router-threading, service-construction, and wrap handshake.
    macro_rules! mount_services {
    ($(($mount:ident, $service:ident)),* $(,)?) => {
        $(
            (router_pub, router_gate) = proto::gen::mounts::$mount(
                router_pub,
                router_gate,
                std::sync::Arc::new($service {
                    state: std::sync::Arc::clone(&state),
                }),
                &wrap,
            );
        )*
    };
}

    // The full mounted surface. The generator's auth-free table
    // classifies each of its route bindings.
    mount_services!(
        (mount_access_key_service, AccessKeyService),
        (mount_admin_portal_service, AdminPortalService),
        (mount_api_audit_log_service, ApiAuditLogService),
        (mount_api_service, ApiService),
        (mount_authentication_service, AuthenticationService),
        (mount_config_service, ConfigService),
        (mount_dashboard_service, DashboardService),
        (
            mount_data_access_audit_log_service,
            DataAccessAuditLogService
        ),
        (mount_dict_entry_service, DictEntryService),
        (mount_dict_type_service, DictTypeService),
        (mount_file_service, FileService),
        (mount_file_transfer_service, FileTransferService),
        (
            mount_internal_message_category_service,
            InternalMessageCategoryService
        ),
        (
            mount_internal_message_recipient_service,
            InternalMessageRecipientService
        ),
        (mount_internal_message_service, InternalMessageService),
        (mount_language_service, LanguageService),
        (mount_login_audit_log_service, LoginAuditLogService),
        (mount_login_policy_service, LoginPolicyService),
        (mount_menu_service, MenuService),
        (mount_mfa_service, MfaService),
        (
            mount_notification_channel_service,
            NotificationChannelService
        ),
        (mount_online_session_service, OnlineSessionService),
        (mount_operation_audit_log_service, OperationAuditLogService),
        (mount_org_unit_service, OrgUnitService),
        (
            mount_permission_audit_log_service,
            PermissionAuditLogService
        ),
        (mount_permission_group_service, PermissionGroupService),
        (mount_permission_service, PermissionService),
        (mount_plan_module_service, PlanModuleService),
        (mount_plan_quota_service, PlanQuotaService),
        (mount_plan_service, PlanService),
        (
            mount_policy_evaluation_log_service,
            PolicyEvaluationLogService
        ),
        (mount_position_service, PositionService),
        (mount_redis_cache_monitor_service, RedisCacheMonitorService),
        (mount_role_service, RoleService),
        (mount_script_log_service, ScriptLogService),
        (mount_script_service, ScriptService),
        (mount_server_monitor_service, ServerMonitorService),
        (mount_task_service, TaskService),
        (mount_tenant_service, TenantService),
        (mount_user_profile_service, UserProfileService),
        (mount_user_service, UserService),
    );

    // 上游 notification 契约新增的两个服务(rule / delivery 台账)Rust 侧尚无
    // 真实实现:挂生成器的 null 桩(每个方法应答 Unknown 错误形态),与差分
    // 台架"公开路由 null 桩"的验收语义一致;真实出站/台账移植落地时换成
    // 带 state 的服务结构体并并入上方宏表。
    (router_pub, router_gate) = proto::gen::mounts::mount_notification_rule_service(
        router_pub,
        router_gate,
        proto::gen::nulls::null_notification_rule_service(),
        &wrap,
    );
    (router_pub, router_gate) = proto::gen::mounts::mount_notification_service(
        router_pub,
        router_gate,
        proto::gen::nulls::null_notification_service(),
        &wrap,
    );

    let mut app = router_pub.merge(router_gate);

    // The docs surface (Swagger UI / Redoc / raw spec), switched by
    // server.rest.enable_swagger / enable_redoc.
    app = app.merge(crate::server::docs::router(
        docs.enable_swagger,
        docs.enable_redoc,
    ));

    // The signed public image proxy: public by design — the HMAC in
    // the query is the credential — so it mounts without the gate,
    // mirroring the reference's raw (Operation-less) route.
    let image_proxy = axum::Router::new()
        .route(
            "/admin/v1/file/image",
            axum::routing::get(crate::services::image_proxy),
        )
        .with_state(Arc::clone(&state));
    app = app.merge(image_proxy);

    // The audit-write layer:
    // post-handler persistence into the audit tables, outermost so it
    // sees final status codes.
    app.layer(axum::middleware::from_fn_with_state(
        Arc::clone(&state),
        crate::audit::layer,
    ))
}
