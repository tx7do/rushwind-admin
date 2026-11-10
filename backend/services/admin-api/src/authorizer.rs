//! The storage-side implementations of the gate's two later stages:
//! the tenant access checks (status / expiry read-only / plan module
//! whitelist, fail-closed) and the authorization evaluation (the
//! per-role engine point under the configured no-op engine, with the
//! evaluation trail into `sys_policy_evaluation_logs` and the
//! best-effort permission/policy back-resolution behind a TTL cache).

use std::sync::Arc;
use std::time::{Duration, Instant};

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _, Set};

use crate::data::{
    sys_apis, sys_permission_apis, sys_plan_modules, sys_plans, sys_policy_evaluation_logs,
    sys_role_permissions, sys_roles, sys_tenants,
};
use crate::state::AppState;

/// The TTL of the route → (permission, policy) back-resolution cache —
/// permission data changes surface within this bound.
const RESOLVE_TTL: Duration = Duration::from_secs(60);

/// The resolve cache's entry ceiling — expired entries drain on every
/// write and an at-ceiling insert recycles the soonest-expiring entry,
/// so runs of distinct subjects cannot grow the map without bound.
const RESOLVE_CACHE_CAP: usize = 1024;

/// The tenant access checks behind the gate's tenant stage. The
/// reference semantics, in order: unknown tenant → `access denied`;
/// status off `ON` → `tenant is not active`; expired under a
/// `READONLY` plan → writes denied (`tenant is read-only due to
/// expiry`); the route unregistered in `sys_apis` → fail-closed
/// `access denied`; unclassified business module or a tenant without a
/// plan or a module outside the plan whitelist → `module not allowed`
/// / `no subscription plan`.
pub struct TenantAccessChecker(pub Arc<AppState>);

#[async_trait::async_trait]
impl auth::TenantAccessChecker for TenantAccessChecker {
    async fn check_tenant_access(
        &self,
        tenant_id: i64,
        path: &str,
        method: &str,
    ) -> Result<(), String> {
        let db = &self.0.db;
        let denied = || "access denied".to_string();

        // 1. The tenant row and its plan's expiry policy.
        let tenant = sys_tenants::Entity::find_by_id(tenant_id)
            .one(db)
            .await
            .map_err(|_| denied())?
            .ok_or_else(denied)?;
        if tenant.status.as_deref() != Some("ON") {
            return Err("tenant is not active".to_string());
        }

        let plan_id = tenant.plan_id.unwrap_or(0);
        let mut expiry_policy = String::new();
        if plan_id > 0 {
            if let Ok(Some(plan)) = sys_plans::Entity::find_by_id(plan_id).one(db).await {
                expiry_policy = plan.expiry_policy.unwrap_or_default();
            }
        }

        // 2. The expiry read-only window: reads stay open, writes deny.
        let expired = tenant
            .expired_at
            .map(|at| at < crate::data::now())
            .unwrap_or(false);
        if expired && expiry_policy == "READONLY" && !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            return Err("tenant is read-only due to expiry".to_string());
        }

        // 3. The route's business module; an unregistered route cannot
        // be classified and fails closed.
        let api = sys_apis::Entity::find()
            .filter(
                Condition::all()
                    .add(sys_apis::Column::Path.eq(path))
                    .add(sys_apis::Column::Method.eq(method.to_uppercase())),
            )
            .one(db)
            .await
            .map_err(|_| denied())?
            .ok_or_else(denied)?;
        let module = api.business_module.clone().unwrap_or_default();
        if module.is_empty() || module == "MODULE_UNSPECIFIED" {
            return Err("module not allowed".to_string());
        }

        // 4. The plan module whitelist.
        if plan_id == 0 {
            return Err("no subscription plan".to_string());
        }
        let allowed = sys_plan_modules::Entity::find()
            .filter(
                Condition::all()
                    .add(sys_plan_modules::Column::PlanId.eq(plan_id))
                    .add(sys_plan_modules::Column::Module.eq(&module)),
            )
            .one(db)
            .await
            .map_err(|_| denied())?
            .is_some();
        if !allowed {
            return Err("module not allowed".to_string());
        }
        Ok(())
    }
}

/// The authorization evaluator: the reference deployment runs the
/// no-op engine (every subject permits), so the first role decides and
/// each request leaves one evaluation row — subject, action, resource,
/// verdict, effect details, and the permission/policy back-resolution
/// behind the TTL cache. An operator with no roles denies.
pub struct AccessAuthorizer {
    state: Arc<AppState>,
    resolve_cache: std::sync::Mutex<std::collections::HashMap<String, (u64, u64, Instant)>>,
}

impl AccessAuthorizer {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            resolve_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The route → (permission, policy) resolution, cached per
    /// subject/path/method. Best-effort: every miss returns zeros and
    /// leaves the trail's references empty.
    async fn resolve_cached(&self, subject: &str, path: &str, method: &str) -> (u64, u64) {
        if subject.is_empty() || path.is_empty() || method.is_empty() {
            return (0, 0);
        }
        let key = format!("{subject}|{path}|{}", method.to_uppercase());
        if let Ok(cache) = self.resolve_cache.lock() {
            if let Some((pid, pol, expires_at)) = cache.get(&key) {
                if Instant::now() < *expires_at {
                    return (*pid, *pol);
                }
            }
        }
        let resolved = self.resolve_permission_policy(subject, path, method).await;
        if let Ok(mut cache) = self.resolve_cache.lock() {
            let now = Instant::now();
            cache.retain(|_, (_, _, expires_at)| *expires_at > now);
            if cache.len() >= RESOLVE_CACHE_CAP {
                let victim = cache
                    .iter()
                    .min_by_key(|(_, (_, _, expires_at))| *expires_at)
                    .map(|(k, _)| k.clone());
                if let Some(victim) = victim {
                    cache.remove(&victim);
                }
            }
            cache.insert(key, (resolved.0, resolved.1, now + RESOLVE_TTL));
        }
        resolved
    }

    /// `sys_apis` → `sys_permission_apis` → the role's hold of the
    /// permission via `sys_role_permissions` → the permission's first
    /// policy by evaluation order (this schema carries no policy table,
    /// so the policy reference stays empty).
    async fn resolve_permission_policy(
        &self,
        role_code: &str,
        path: &str,
        method: &str,
    ) -> (u64, u64) {
        let db = &self.state.db;
        // 绑定按端点身份快照裁决（上游 7eb7f9f0 同语义）：与路由闸的
        // (path, method) 键同形，免疫 sys_apis 行删除重建与 id 洗牌——
        // api_id 列仅历史兼容。
        let Some(link) = sys_permission_apis::Entity::find()
            .filter(sys_permission_apis::Column::EndpointPath.eq(path))
            .filter(sys_permission_apis::Column::EndpointMethod.eq(method.to_uppercase()))
            .one(db)
            .await
            .ok()
            .flatten()
        else {
            return (0, 0);
        };
        let permission_id = link.permission_id.unwrap_or(0);
        if permission_id == 0 {
            return (0, 0);
        }
        let Some(role) = sys_roles::Entity::find()
            .filter(sys_roles::Column::Code.eq(role_code))
            .one(db)
            .await
            .ok()
            .flatten()
        else {
            return (0, 0);
        };
        let held = sys_role_permissions::Entity::find()
            .filter(
                Condition::all()
                    .add(sys_role_permissions::Column::RoleId.eq(role.id))
                    .add(sys_role_permissions::Column::PermissionId.eq(permission_id)),
            )
            .one(db)
            .await
            .ok()
            .flatten()
            .is_some();
        if !held {
            return (0, 0);
        }
        (permission_id as u64, 0)
    }
}

#[async_trait::async_trait]
impl auth::AuthorizationEvaluator for AccessAuthorizer {
    async fn authorize(&self, evaluation: auth::AuthzEvaluation<'_>) -> Result<(), String> {
        let start = Instant::now();
        // The per-role short-circuit: the first subject the engine
        // permits wins. The no-op engine permits every subject, so the
        // first role is the one evaluation — and the one trail row.
        let Some(subject) = evaluation.roles.first() else {
            return Err("missing authz subject".to_string());
        };
        // The no-op engine permits every subject, so the first role is
        // the one evaluation — and the one trail row.
        let allowed = true;

        let (permission_id, policy_id) = self
            .resolve_cached(subject, evaluation.resource, evaluation.action)
            .await;
        let effect_details = format!(
            "engine=noop subject={subject} latency={}ms; allowed",
            start.elapsed().as_millis()
        );
        let evaluation_context = serde_json::json!({
            "engine": "noop",
            "subject": subject,
            "action": evaluation.action,
            "resource": evaluation.resource,
            "userId": evaluation.user_id,
            "tenantId": evaluation.tenant_id,
        })
        .to_string();

        let row = sys_policy_evaluation_logs::ActiveModel {
            tenant_id: Set(Some(evaluation.tenant_id)),
            user_id: Set(Some(evaluation.user_id)),
            permission_id: Set((permission_id != 0).then_some(permission_id as i64)),
            policy_id: Set((policy_id != 0).then_some(policy_id as i64)),
            request_path: Set(Some(evaluation.resource.to_owned())),
            request_method: Set(Some(evaluation.action.to_owned())),
            result: Set(Some(allowed)),
            effect_details: Set(Some(effect_details)),
            ip_address: Set((!evaluation.ip.is_empty()).then_some(evaluation.ip.to_owned())),
            trace_id: Set(
                (!evaluation.trace_id.is_empty()).then_some(evaluation.trace_id.to_owned())
            ),
            evaluation_context: Set(Some(evaluation_context)),
            created_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        // A failed trail write never blocks the request.
        let _ = sys_policy_evaluation_logs::Entity::insert(row)
            .exec(&self.state.db)
            .await;
        Ok(())
    }
}
