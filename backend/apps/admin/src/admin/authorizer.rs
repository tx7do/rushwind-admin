//! The storage-side implementations of the gate's two later stages:
//! the tenant access checks (status / expiry read-only / plan module
//! whitelist, fail-closed) and the authorization evaluation — the
//! engine chosen by auth.yaml `authz.type` (`noop` permits every
//! subject, `casbin`/`opa` judge the loaded role→endpoint policy
//! snapshot), one trail row per subject attempt into
//! `sys_policy_evaluation_logs`, and the best-effort permission/policy
//! back-resolution behind a TTL cache.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait as _, DatabaseConnection, EntityTrait as _, QueryFilter as _, Set};

use crate::admin::config::AuthzKind;
use crate::admin::data::{
    sys_apis, sys_permission_apis, sys_plan_modules, sys_plans, sys_policy_evaluation_logs,
    sys_role_permissions, sys_roles, sys_tenants,
};
use crate::admin::state::AppState;

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
/// `access denied`; unclassified business module → `module not
/// allowed`; a tenant without a plan → `no subscription plan`; a plan
/// with no whitelisted module → `tenant plan has no whitelisted
/// modules`; a module outside the whitelist → `module not allowed`.
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
            .map(|at| at < crate::admin::data::now())
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

        // 4. The plan module whitelist. A plan with NO whitelisted module
        // answers its own message — "module not allowed" would send the
        // tenant admin chasing a module name that no configuration can
        // ever satisfy (the plan simply has nothing enabled).
        if plan_id == 0 {
            return Err("no subscription plan".to_string());
        }
        let whitelisted = sys_plan_modules::Entity::find()
            .filter(sys_plan_modules::Column::PlanId.eq(plan_id))
            .one(db)
            .await
            .map_err(|_| denied())?;
        if whitelisted.is_none() {
            return Err("tenant plan has no whitelisted modules".to_string());
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

// ─────────────────────────── the authz engines ───────────────────────────

/// The template-role code prefix the policy loader skips
/// (`constants.IsTemplateRoleCode`).
const TEMPLATE_ROLE_PREFIX: &str = "template:";

/// The deny-all fallback engine (`deny_all_engine.go`): every
/// evaluation errors loudly, fail-closed. The reference produces it
/// only when a custom OPA model fails to parse; this port's opa
/// evaluator is native (the shipped rbac.rego's fixed semantics — no
/// custom-model surface), so the branch is structurally unreachable
/// from config. The engine still rides, pinned by the suite, for the
/// doctrine: an engine that cannot prove its policy model denies
/// everything rather than guess.
const DENY_ALL_NAME: &str = "deny-all";
const DENY_ALL_MESSAGE: &str =
    "authorization engine unavailable: custom policy model failed to load";

/// One casbin policy line `p, sub, obj, act, dom` — the
/// restfull-with-role model's policy definition, fed from the
/// role→endpoint snapshot.
#[derive(Clone, Debug)]
struct CasbinLine {
    sub: String,
    obj: String,
    act: String,
    dom: String,
}

/// The in-memory policy set the casbin/opa engines judge: the
/// role→permission→endpoint snapshot, reloaded on every reset. The
/// default (pre-reset) face is empty, which denies under both engines —
/// the reference's engines deny the same way before their first
/// SetPolicies.
#[derive(Debug, Default)]
struct AuthzPolicies {
    casbin: Vec<CasbinLine>,
    /// rbac.rego's `data.policies`: role code → [(pattern, method)].
    opa: HashMap<String, Vec<(String, String)>>,
}

/// The configured engine plus its latest policy snapshot — the
/// go-utils `Authorizer` face: [`PolicyAuthorizer::evaluate`] is the
/// per-subject verdict the gate's role loop consumes,
/// [`PolicyAuthorizer::reset`] the mutation-face reload.
pub struct PolicyAuthorizer {
    /// `None` = the stage is absent (the section missing, or
    /// `zanzibar`): the gate drops the evaluator entirely and this
    /// struct never evaluates.
    kind: Option<AuthzKind>,
    policies: RwLock<Arc<AuthzPolicies>>,
}

impl PolicyAuthorizer {
    pub fn new(kind: Option<AuthzKind>) -> Self {
        Self {
            kind,
            policies: RwLock::new(Arc::new(AuthzPolicies::default())),
        }
    }

    /// The engine name the audit trail's `engine=` face carries. An
    /// absent stage never evaluates, so the `deny-all` answer here is
    /// decorative.
    pub fn engine_name(&self) -> &'static str {
        match self.kind {
            Some(kind) => kind.engine_name(),
            None => DENY_ALL_NAME,
        }
    }

    /// IsAuthorized, one subject per call (the gate loops the roles).
    fn evaluate(&self, subject: &str, action: &str, resource: &str) -> Result<bool, String> {
        let policies = Arc::clone(&self.policies.read().expect("policy lock"));
        match self.kind {
            Some(AuthzKind::Noop) => Ok(true),
            Some(AuthzKind::Casbin) => Ok(casbin_enforce(&policies, subject, resource, action)),
            Some(AuthzKind::Opa) => Ok(opa_enforce(&policies, subject, resource, action)),
            None => Err(DENY_ALL_MESSAGE.to_string()),
        }
    }

    /// ResetPolicies: reload the role→endpoint snapshot. The reference
    /// runs the provider load for EVERY engine (the noop arm returns
    /// after it), so the query cost rides every deployment equally and
    /// a later knob flip meets fresh policies. The two real engines log
    /// the reloaded rule count — the noop arm is silent in the
    /// reference and stays silent here.
    pub async fn reset(&self, db: &DatabaseConnection) -> Result<(), String> {
        let policies = load_policies(db).await?;
        if !matches!(self.kind, Some(AuthzKind::Noop) | None) {
            tracing::info!(
                target: "rushwind.admin.authz",
                engine = self.engine_name(),
                rules = policies.casbin.len(),
                subjects = policies.opa.len(),
                "policy snapshot reloaded"
            );
        }
        *self.policies.write().expect("policy lock") = Arc::new(policies);
        Ok(())
    }
}

/// The mutation-face reset — log-only on failure (every reference call
/// site logs and continues; the tenant cleanup silences entirely).
pub async fn reset_policies(state: &Arc<AppState>) {
    if let Err(error) = state.authz.reset(&state.db).await {
        tracing::warn!(target: "rushwind.admin.error", error = %error, "reset policies error");
    }
}

/// The policy snapshot load (`AuthorizerProvider.ProvidePolicies`): all
/// roles (system view, no paging) with empty codes and template roles
/// skipped; per role the permission→endpoint `(path, method)` binding
/// snapshot — the same endpoint identity the tenant gate and the audit
/// back-resolution key on, immune to `sys_apis` deletes and rebuilds.
/// The casbin domain carries the role's tenant id (the provider's
/// `strconv.FormatInt(role.GetTenantId(), 10)`, nil answering `0`);
/// the opa map is domain-free — the shipped rbac.rego knows none.
async fn load_policies(db: &DatabaseConnection) -> Result<AuthzPolicies, String> {
    let roles = sys_roles::Entity::find()
        .all(db)
        .await
        .map_err(|e| format!("load roles: {e}"))?;
    let mut policies = AuthzPolicies::default();
    for role in roles {
        if role.code.is_empty() || role.code.starts_with(TEMPLATE_ROLE_PREFIX) {
            continue;
        }
        let perm_ids: Vec<i64> = sys_role_permissions::Entity::find()
            .filter(sys_role_permissions::Column::RoleId.eq(role.id))
            .all(db)
            .await
            .map_err(|e| format!("load role permissions: {e}"))?
            .into_iter()
            .filter_map(|row| row.permission_id)
            .collect();
        if perm_ids.is_empty() {
            continue;
        }
        // ListEndpoints' predicates: both snapshot columns present and
        // non-empty.
        let endpoints: Vec<(String, String)> = sys_permission_apis::Entity::find()
            .filter(sys_permission_apis::Column::PermissionId.is_in(perm_ids))
            .all(db)
            .await
            .map_err(|e| format!("load permission endpoints: {e}"))?
            .into_iter()
            .filter_map(|row| {
                let path = row.endpoint_path?;
                let method = row.endpoint_method?;
                (!path.is_empty() && !method.is_empty()).then_some((path, method))
            })
            .collect();
        if endpoints.is_empty() {
            continue;
        }
        let dom = role.tenant_id.unwrap_or(0).to_string();
        for (obj, act) in &endpoints {
            policies.casbin.push(CasbinLine {
                sub: role.code.clone(),
                obj: obj.clone(),
                act: act.clone(),
                dom: dom.clone(),
            });
        }
        policies.opa.insert(role.code.clone(), endpoints);
    }
    Ok(policies)
}

// casbin's builtin matchers, ported verbatim
// (`util/builtin_operators.go`, v2.135.0).

/// keyMatch: the FIRST `*` splits — byte-wise prefix compare up to it
/// (`*` is ASCII, so the split index is a byte offset; the byte compare
/// keeps multi-byte keys off the slicing panics Go never has).
fn key_match(key1: &str, key2: &str) -> bool {
    match key2.find('*') {
        None => key1 == key2,
        Some(i) => {
            if key1.len() > i {
                key1.as_bytes()[..i] == key2.as_bytes()[..i]
            } else {
                key1.as_bytes() == &key2.as_bytes()[..i]
            }
        }
    }
}

/// keyMatch2: `/*` → `/.*`, then `:param` → `[^/]+`, and the final
/// match is ANCHORED (the v2.135 form: `^pattern$`).
fn key_match2(key1: &str, key2: &str) -> bool {
    let step1 = key2.replace("/*", "/.*");
    let step2 = regex::Regex::new(r":[^/]+")
        .expect("builtin pattern")
        .replace_all(&step1, "[^/]+");
    regex_match(key1, &format!("^{step2}$"))
}

/// regexMatch: unanchored, the pattern is the POLICY side. Go panics on
/// an uncompilable pattern; the port answers no-match — a pattern that
/// cannot compile can never have matched, and the authz path must not
/// panic (one bad policy row must not become a per-request crash).
fn regex_match(target: &str, pattern: &str) -> bool {
    regex::Regex::new(&go_pattern_to_rust(pattern)).is_ok_and(|re| re.is_match(target))
}

/// Go's RE2 treats a `{…}` that is NOT a valid repetition as LITERAL
/// text (`regexp.Compile("a{id}")` succeeds and matches `a{id}`); the
/// Rust regex crate rejects it as a broken quantifier. That difference
/// decides every `{id}` route template — the reference's paths are all
/// `{id}`-style, so the policy patterns carry them verbatim. The
/// translation escapes such braces (outside character classes and
/// escapes, where both engines already treat them as literal) and
/// leaves valid repetitions (`{2}`, `{2,}`, `{2,4}`) untouched.
fn go_pattern_to_rust(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                out.push('\\');
                if i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                i += 1;
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
                i += 1;
            }
            '{' if !in_class => {
                // A valid repetition: `{n}`, `{n,}`, `{n,m}`.
                let mut j = i + 1;
                let digits_start = j;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let mut valid = j > digits_start;
                if valid && j < chars.len() && chars[j] == ',' {
                    j += 1;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                valid = valid && j < chars.len() && chars[j] == '}';
                if valid {
                    for ch in &chars[i..=j] {
                        out.push(*ch);
                    }
                    i = j + 1;
                } else {
                    out.push('\\');
                    out.push('{');
                    i += 1;
                }
            }
            '}' if !in_class => {
                out.push('\\');
                out.push('}');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// The casbin restfull-with-role matcher (`assets/restfull_with_role.conf`):
/// `g(r.sub, p.sub, r.dom)` reduces to equality here — no `g` role
/// lines are ever loaded — then keyMatch2 on the path,
/// regexMatch-or-`ANY` on the method, and keyMatch-or-`*` on the
/// domain. NOTE the domain clause's real shape in this system: the
/// request domain is always the wildcard `*` (claims.Project is never
/// populated) while policy domains are tenant ids, so `keyMatch("*",
/// "0")` is false and the clause never fires — the reference's casbin
/// stage judges paths and methods only for policies that carry `*` as
/// their domain, and denies the rest. Ported verbatim, not "fixed".
fn casbin_enforce(policies: &AuthzPolicies, subject: &str, resource: &str, action: &str) -> bool {
    policies.casbin.iter().any(|line| {
        line.sub == subject
            && key_match2(resource, &line.obj)
            && (regex_match(action, &line.act) || line.act == "ANY")
            && (key_match("*", &line.dom) || line.dom == "*")
    })
}

/// The shipped rbac.rego's `authorized_pair` reduced to its exact
/// semantics: some grant of the subject equals the request pair on BOTH
/// pattern and method — plain string equality, no wildcard, no domain.
fn opa_enforce(policies: &AuthzPolicies, subject: &str, resource: &str, action: &str) -> bool {
    policies.opa.get(subject).is_some_and(|grants| {
        grants
            .iter()
            .any(|(pattern, method)| pattern == resource && method == action)
    })
}

/// The authorization evaluator: the configured engine judges each of
/// the token's roles in order — the first permitted subject wins, and
/// every attempt leaves one evaluation row (subject, action, resource,
/// verdict, effect details, and the permission/policy back-resolution
/// behind the TTL cache). An operator with no roles denies before any
/// evaluation, an all-denied loop answers `unauthorized access`, and an
/// engine fault answers as the raw error the reference's middleware
/// returns (the unknown-error 500 face) without trying further roles.
pub struct AccessAuthorizer {
    db: DatabaseConnection,
    authz: Arc<PolicyAuthorizer>,
    resolve_cache: std::sync::Mutex<std::collections::HashMap<String, (i64, i64, Instant)>>,
}

impl AccessAuthorizer {
    pub fn new(state: &Arc<AppState>) -> Self {
        Self::over(state.db.clone(), Arc::clone(&state.authz))
    }

    /// The engine face over an explicit db + policy authorizer — the
    /// test entry (the engine suite rides an in-memory sqlite, not a
    /// whole `AppState`).
    pub fn over(db: DatabaseConnection, authz: Arc<PolicyAuthorizer>) -> Self {
        Self {
            db,
            authz,
            resolve_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The route → (permission, policy) resolution, cached per
    /// subject/path/method. Best-effort: every miss returns zeros and
    /// leaves the trail's references empty.
    async fn resolve_cached(&self, subject: &str, path: &str, method: &str) -> (i64, i64) {
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
    ) -> (i64, i64) {
        let db = &self.db;
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
        (permission_id, 0)
    }
}

#[async_trait::async_trait]
impl auth::AuthorizationEvaluator for AccessAuthorizer {
    async fn authorize(
        &self,
        evaluation: auth::AuthzEvaluation<'_>,
    ) -> Result<(), auth::AuthzRejection> {
        // The middleware's missing-subject arm: answered before any
        // evaluation — no IsAuthorized runs, so no trail row either.
        if evaluation.roles.is_empty() {
            return Err(auth::AuthzRejection::MissingSubject);
        }
        // The middleware's Subjects arm: one IsAuthorized per role in
        // order, the first permitting role wins — and one trail row PER
        // ATTEMPT (the eval-logging engine wraps each call), so a
        // multi-role user denied on the first roles lands a row per
        // denial before the permitting one.
        for subject in evaluation.roles {
            let start = Instant::now();
            let engine_name = self.authz.engine_name();
            let verdict = self
                .authz
                .evaluate(subject, evaluation.action, evaluation.resource);
            let effect_details = match &verdict {
                Ok(true) => format!(
                    "engine={engine_name} subject={subject} latency={}ms; allowed",
                    start.elapsed().as_millis()
                ),
                Ok(false) => format!(
                    "engine={engine_name} subject={subject} latency={}ms; denied",
                    start.elapsed().as_millis()
                ),
                Err(error) => format!(
                    "engine={engine_name} subject={subject} latency={}ms; error: {error}",
                    start.elapsed().as_millis()
                ),
            };
            let (permission_id, policy_id) = self
                .resolve_cached(subject, evaluation.resource, evaluation.action)
                .await;
            let allowed = matches!(verdict, Ok(true));
            let row = sys_policy_evaluation_logs::ActiveModel {
                tenant_id: Set(Some(evaluation.tenant_id)),
                user_id: Set(Some(evaluation.user_id)),
                permission_id: Set((permission_id != 0).then_some(permission_id)),
                policy_id: Set((policy_id != 0).then_some(policy_id)),
                request_path: Set(Some(evaluation.resource.to_owned())),
                request_method: Set(Some(evaluation.action.to_owned())),
                result: Set(Some(allowed)),
                effect_details: Set(Some(effect_details)),
                ip_address: Set((!evaluation.ip.is_empty()).then_some(evaluation.ip.to_owned())),
                trace_id: Set(
                    (!evaluation.trace_id.is_empty()).then_some(evaluation.trace_id.to_owned())
                ),
                evaluation_context: Set(Some(evaluation_context(
                    &evaluation,
                    subject,
                    engine_name,
                ))),
                created_at: Set(Some(crate::admin::data::now())),
                ..Default::default()
            };
            // A failed trail write never blocks the request.
            let _ = sys_policy_evaluation_logs::Entity::insert(row)
                .exec(&self.db)
                .await;
            match verdict {
                Ok(true) => return Ok(()),
                Ok(false) => continue,
                // The engine fault: the row above carries the reason;
                // the middleware returns the raw error (the unknown
                // 500 face) without trying the next role.
                Err(error) => return Err(auth::AuthzRejection::Engine(error)),
            }
        }
        Err(auth::AuthzRejection::Unauthorized)
    }
}

/// The evaluation context snapshot (`evaluationContextJSON`): engine,
/// subject, action, resource, plus the operator identity. The
/// reference adds `project` only when set — never in this system, the
/// claims never populate it.
fn evaluation_context(
    evaluation: &auth::AuthzEvaluation<'_>,
    subject: &str,
    engine_name: &'static str,
) -> String {
    serde_json::json!({
        "engine": engine_name,
        "subject": subject,
        "action": evaluation.action,
        "resource": evaluation.resource,
        "userId": evaluation.user_id,
        "tenantId": evaluation.tenant_id,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// casbin 内建匹配器逐条钉扎（v2.135.0 语义，含路径模板形态）。
    #[test]
    fn casbin_matchers_follow_the_builtin_semantics() {
        // keyMatch: equality without `*`; the first `*` splits into a
        // byte-prefix compare.
        assert!(key_match("/foo/bar", "/foo/bar"));
        assert!(key_match("/foo/bar", "/foo/*"));
        assert!(!key_match("/foo/bar", "/foo/bar/*"));
        assert!(key_match("*", "*"));
        assert!(!key_match("*", "0"));
        // keyMatch2: anchored; `:param` is the segment wildcard, `/*`
        // becomes the suffix glob. `{id}`-style OpenAPI templates have
        // no `:` and match as literal regex text.
        assert!(key_match2("/api/users/123", "/api/users/:id"));
        assert!(!key_match2("/api/users/123/posts", "/api/users/:id"));
        assert!(key_match2("/foo/bar/baz", "/foo/*"));
        assert!(!key_match2("/foo/bar/baz", "/foo/bar"));
        assert!(key_match2("/admin/v1/users/{id}", "/admin/v1/users/{id}"));
        assert!(!key_match2("/admin/v1/users/7", "/admin/v1/users/{id}"));
        // regexMatch: unanchored, policy side is the pattern; a broken
        // pattern answers no-match (the Go builtin panics — the port
        // must not, one bad policy row is not a per-request crash).
        assert!(regex_match("GET", "GET"));
        assert!(regex_match("GET", "GET|POST"));
        assert!(!regex_match("POST", "PUT"));
        assert!(!regex_match("GET", "("));
    }

    /// Go RE2 的括号语义（实测钉扎）：无效重复 `{…}` 一律当字面量，
    /// 有效重复（`{n}`/`{n,}`/`{n,m}`）照旧生效，字符类内括号不转义。
    /// 路由模板 `{id}` 全靠这条——参照的策略模式里它必须是字面量。
    #[test]
    fn go_brace_semantics_keep_invalid_repetitions_literal() {
        // The ten probed cases against Go's regexp, verbatim.
        assert!(regex_match("a{1", "a{1"));
        assert!(regex_match("a{1,", "a{1,"));
        assert!(regex_match("a{1,b}", "a{1,b}"));
        assert!(regex_match("a{1,2", "a{1,2"));
        assert!(regex_match("a{}", "a{}"));
        assert!(regex_match("a{,2}", "a{,2"));
        assert!(regex_match("a{x}", "a{x}"));
        assert!(regex_match("a}", "a}"));
        assert!(regex_match("{", "[{]"));
        // Valid repetitions keep their meaning.
        assert!(regex_match("aa", "a{2}"));
        assert!(regex_match("aaa", "a{2,}"));
        assert!(regex_match("a", "a{1,2}"));
        assert!(!regex_match("a", "a{2}"));
        // The anchored template form the casbin clause rides.
        assert!(regex_match(
            "/admin/v1/users/{id}",
            "^/admin/v1/users/{id}$"
        ));
        // Escapes pass through untouched.
        assert!(regex_match("{", "\\{"));
    }

    /// casbin 引擎档：sub 相等 + keyMatch2 + 方法正则/ANY + 域子句。
    /// 域子句的现实形状一并钉扎：请求域恒 `*`、策略域是租户 id，
    /// `keyMatch("*", "0")` 恒假——只有域为 `*` 的策略行能命中。
    #[test]
    fn casbin_enforce_judges_subject_path_method_and_domain() {
        let policies = AuthzPolicies {
            casbin: vec![
                // The loaded shape: domain = the role's tenant id.
                CasbinLine {
                    sub: "platform:admin".into(),
                    obj: "/admin/v1/users".into(),
                    act: "GET".into(),
                    dom: "0".into(),
                },
                CasbinLine {
                    sub: "tenant:manager".into(),
                    obj: "/admin/v1/users/{id}".into(),
                    act: "ANY".into(),
                    dom: "1".into(),
                },
                // A `*`-domain line: the machinery itself is sound.
                CasbinLine {
                    sub: "auditor".into(),
                    obj: "/admin/v1/audit-logs/*".into(),
                    act: "(GET)".into(),
                    dom: "*".into(),
                },
            ],
            opa: HashMap::new(),
        };
        // Subject hit + path hit + method hit — but the numeric domain
        // never matches the wildcard request domain: denied.
        assert!(!casbin_enforce(
            &policies,
            "platform:admin",
            "/admin/v1/users",
            "GET"
        ));
        assert!(!casbin_enforce(
            &policies,
            "tenant:manager",
            "/admin/v1/users/{id}",
            "DELETE"
        ));
        // The `*`-domain line matches (path suffix glob + method regex).
        assert!(casbin_enforce(
            &policies,
            "auditor",
            "/admin/v1/audit-logs/login",
            "GET"
        ));
        // Wrong subject: denied regardless.
        assert!(!casbin_enforce(
            &policies,
            "nobody",
            "/admin/v1/users",
            "GET"
        ));
        // The empty snapshot (pre-reset) denies everything.
        assert!(!casbin_enforce(
            &AuthzPolicies::default(),
            "platform:admin",
            "/x",
            "GET"
        ));
    }

    /// opa 引擎档：内装 rbac.rego 的 `authorized_pair` 归约——主体名下
    /// 存在 (pattern, method) 双精确相等的授予即允许；无域、无通配。
    #[test]
    fn opa_enforce_requires_exact_pair_equality() {
        let mut policies = AuthzPolicies::default();
        policies.opa.insert(
            "platform:admin".to_string(),
            vec![
                ("/admin/v1/users".to_string(), "GET".to_string()),
                ("/admin/v1/users/{id}".to_string(), "DELETE".to_string()),
            ],
        );
        assert!(opa_enforce(
            &policies,
            "platform:admin",
            "/admin/v1/users",
            "GET"
        ));
        assert!(opa_enforce(
            &policies,
            "platform:admin",
            "/admin/v1/users/{id}",
            "DELETE"
        ));
        assert!(!opa_enforce(
            &policies,
            "platform:admin",
            "/admin/v1/users",
            "POST"
        ));
        assert!(!opa_enforce(
            &policies,
            "tenant:manager",
            "/admin/v1/users",
            "GET"
        ));
        // The empty snapshot (pre-reset) denies everything.
        assert!(!opa_enforce(
            &AuthzPolicies::default(),
            "platform:admin",
            "/x",
            "GET"
        ));
    }

    /// deny-all 兜底引擎：恒 Err（fail-closed，错误随每次拒绝可观测）。
    #[test]
    fn deny_all_answers_the_fail_closed_error() {
        assert_eq!(
            PolicyAuthorizer::new(None).evaluate("platform:admin", "GET", "/x"),
            Err(DENY_ALL_MESSAGE.to_string())
        );
        assert_eq!(PolicyAuthorizer::new(None).engine_name(), DENY_ALL_NAME);
    }

    /// noop 引擎档：恒允许（ trails 照写——闸门环的唯一行为差异面）。
    #[test]
    fn noop_permits_every_subject() {
        assert_eq!(
            PolicyAuthorizer::new(Some(AuthzKind::Noop)).evaluate("anyone", "GET", "/x"),
            Ok(true)
        );
    }
}
