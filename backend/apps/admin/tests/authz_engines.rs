//! The authz engine family over a real (sqlite) database: the policy
//! snapshot load, the three engine kinds' verdicts, the per-role trail
//! loop, and the mutation-face reset — all through the same
//! [`admin::authorizer::AccessAuthorizer`] the gate rides, no second
//! truth.

use std::sync::Arc;

use admin::admin::authorizer::{AccessAuthorizer, PolicyAuthorizer};
use admin::config::AuthzKind;
use auth::{AuthorizationEvaluator as _, AuthzEvaluation, AuthzRejection};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait as _, ColumnTrait as _, DatabaseConnection, EntityTrait as _,
    QueryFilter as _, QueryOrder as _,
};

use admin::data::{
    sys_permission_apis, sys_permissions, sys_policy_evaluation_logs, sys_role_permissions,
    sys_roles,
};

/// The shared-cache in-memory sqlite (single logical db across the
/// pool; a plain `sqlite::memory:` gives each connection its own empty
/// database) with the full entity schema.
async fn harness_db() -> DatabaseConnection {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let db = sea_orm::Database::connect(format!(
        "sqlite:file:authz_engines_{pid}_{seq}?mode=memory&cache=shared",
        pid = std::process::id()
    ))
    .await
    .expect("sqlite connect");
    admin::migration::sqlite_integration_schema(&db).await;
    db
}

/// Seeds one role with one permission bound to one endpoint:
/// role(`code`, tenant `tid`) → permission `p1` → GET /admin/v1/users.
/// Returns the permission id.
async fn seed_role_endpoint(db: &DatabaseConnection, code: &str, tid: i64) -> i64 {
    let role = sys_roles::ActiveModel {
        tenant_id: sea_orm::Set(Some(tid)),
        name: sea_orm::Set(format!("role {code}")),
        code: sea_orm::Set(code.to_string()),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert role");

    let perm = sys_permissions::ActiveModel {
        name: sea_orm::Set("users read".into()),
        code: sea_orm::Set(format!("user:view:{code}")),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert permission");

    sys_role_permissions::ActiveModel {
        tenant_id: sea_orm::Set(Some(tid)),
        role_id: sea_orm::Set(Some(role.id)),
        permission_id: sea_orm::Set(Some(perm.id)),
        effect: sea_orm::Set(Some("ALLOW".into())),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert role permission");

    sys_permission_apis::ActiveModel {
        permission_id: sea_orm::Set(Some(perm.id)),
        endpoint_path: sea_orm::Set(Some("/admin/v1/users".into())),
        endpoint_method: sea_orm::Set(Some("GET".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert permission api");

    perm.id
}

/// One authorization call with the given roles.
async fn judge(
    authorizer: &AccessAuthorizer,
    roles: &[String],
    resource: &str,
    action: &str,
) -> Result<(), AuthzRejection> {
    authorizer
        .authorize(AuthzEvaluation {
            user_id: 7,
            tenant_id: 1,
            roles,
            action,
            resource,
            ip: "10.0.0.1",
            trace_id: "trace-1",
        })
        .await
}

fn roles(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|c| c.to_string()).collect()
}

async fn trail(db: &DatabaseConnection) -> Vec<sys_policy_evaluation_logs::Model> {
    sys_policy_evaluation_logs::Entity::find()
        .order_by_asc(sys_policy_evaluation_logs::Column::Id)
        .all(db)
        .await
        .expect("trail rows")
}

/// opa 档：策略装载 → (role, path, method) 双精确相等即放行；未装载
/// （reset 之前）空快照全拒；每次尝试留一行，`result` 与
/// `effect_details` 携带引擎名与判定。
#[tokio::test]
async fn opa_engine_judges_the_loaded_endpoint_snapshot() {
    let db = harness_db().await;
    let perm_id = seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Opa)));

    // Pre-reset: the empty snapshot denies (fail-closed).
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));
    assert!(matches!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET").await,
        Err(AuthzRejection::Unauthorized)
    ));

    // Reset loads the snapshot: the bound pair passes, others deny.
    authz.reset(&db).await.expect("policy load");
    assert!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET")
            .await
            .is_ok()
    );
    assert!(matches!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "POST").await,
        Err(AuthzRejection::Unauthorized)
    ));
    assert!(matches!(
        judge(&authorizer, &roles(&["other"]), "/admin/v1/users", "GET").await,
        Err(AuthzRejection::Unauthorized)
    ));

    // The trail: one row per attempt (1 pre-reset + 3 after), verdicts
    // and the engine name carried, the held permission back-resolved.
    let rows = trail(&db).await;
    assert_eq!(rows.len(), 4, "one evaluation row per subject attempt");
    for row in &rows {
        let details = row.effect_details.as_deref().unwrap_or_default();
        assert!(details.starts_with("engine=opa subject="), "{details}");
        assert_eq!(row.request_path.as_deref(), Some("/admin/v1/users"));
    }
    // The methods mirror the attempts in order: GET (pre-reset),
    // GET (permitted), POST (denied), GET (unknown subject).
    let methods: Vec<_> = rows.iter().map(|r| r.request_method.clone()).collect();
    assert_eq!(
        methods,
        vec![
            Some("GET".into()),
            Some("GET".into()),
            Some("POST".into()),
            Some("GET".into())
        ]
    );
    assert_eq!(rows[1].result, Some(true));
    assert!(rows[1]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .ends_with("; allowed"));
    assert_eq!(rows[2].result, Some(false));
    assert!(rows[2]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .ends_with("; denied"));
    // The permitted attempt resolves the permission it rode.
    assert_eq!(rows[1].permission_id, Some(perm_id));
}

/// casbin 档：策略行按 restfull-with-role 模型裁决——路径 keyMatch2、
/// 方法正则/ANY、域子句。装载行的域是租户 id、请求域恒 `*`：
/// 域子句恒假（参照语义原样），因此 casbin 档在本系统装载面下
/// 全拒——这是上游机制的真实形状，不是本移植的选择。
#[tokio::test]
async fn casbin_engine_follows_the_loaded_model_verbatim() {
    let db = harness_db().await;
    seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Casbin)));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));

    // The snapshot carries the pair, but the domain clause (policy
    // domain "1", request domain "*") never fires — denied, verbatim.
    assert!(matches!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET").await,
        Err(AuthzRejection::Unauthorized)
    ));
    let rows = trail(&db).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .starts_with("engine=casbin subject=ops"));
}

/// noop 档：恒放行（首个角色即命中），轨迹照写——与 casbin/opa 同一
/// 环，唯一差异在判定。
#[tokio::test]
async fn noop_engine_permits_and_still_writes_the_trail() {
    let db = harness_db().await;
    seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Noop)));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));

    assert!(judge(&authorizer, &roles(&["ops"]), "/anything", "DELETE")
        .await
        .is_ok());
    let rows = trail(&db).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].result, Some(true));
    assert!(rows[0]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .starts_with("engine=noop subject=ops"));
}

/// 空角色：missing authz subject，评估不发生、轨迹零行（middleware
/// 的 missing-subject 臂在 IsAuthorized 之前）。
#[tokio::test]
async fn empty_roles_answer_missing_subject_without_a_trail_row() {
    let db = harness_db().await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Noop)));
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));
    assert!(matches!(
        judge(&authorizer, &[], "/admin/v1/users", "GET").await,
        Err(AuthzRejection::MissingSubject)
    ));
    assert!(trail(&db).await.is_empty());
}

/// 多角色：逐角色评估、首个许可即短路——被拒角色各留一行，许可角色
/// 一行，其后的角色不再评估。
#[tokio::test]
async fn multi_role_loop_stops_at_the_first_permit() {
    let db = harness_db().await;
    seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Opa)));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));

    assert!(judge(
        &authorizer,
        &roles(&["dev", "ops", "never-reached"]),
        "/admin/v1/users",
        "GET"
    )
    .await
    .is_ok());
    let rows = trail(&db).await;
    assert_eq!(rows.len(), 2, "dev denied, ops permitted, third untouched");
    assert_eq!(rows[0].result, Some(false));
    assert_eq!(rows[1].result, Some(true));
    assert!(rows[1]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .contains("subject=ops"));
    // The context snapshot carries the attempt's own subject: the
    // denied row names `dev`, the permitted one `ops`.
    let ctx = |row: &sys_policy_evaluation_logs::Model| -> serde_json::Value {
        serde_json::from_str(row.evaluation_context.as_deref().unwrap_or("{}"))
            .expect("context json")
    };
    assert_eq!(ctx(&rows[0])["subject"], serde_json::json!("dev"));
    assert_eq!(ctx(&rows[1])["subject"], serde_json::json!("ops"));
    assert_eq!(ctx(&rows[1])["engine"], serde_json::json!("opa"));
}

/// 引擎缺席（authz 节缺失 / zanzibar）：authorize 恒 Err——闸门以
/// Option::None 摘下该阶段本就不调用；直呼时给出 fail-closed 的
/// deny-all 错误文本。
#[tokio::test]
async fn absent_engine_answers_the_deny_all_error() {
    let db = harness_db().await;
    seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(None));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));
    let rejection = judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET").await;
    assert!(
        matches!(rejection, Err(AuthzRejection::Engine(ref m)) if m.contains("deny-all") || m.contains("unavailable"))
    );
}

/// 变更面 reset：绑定解除后重载，下一判定随快照翻转（授权跟随数据，
/// 而不是进程启动时的一次快照）。
#[tokio::test]
async fn reset_after_mutation_flips_the_verdict() {
    let db = harness_db().await;
    let perm_id = seed_role_endpoint(&db, "ops", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Opa)));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));
    assert!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET")
            .await
            .is_ok()
    );

    // The role's hold is removed, the snapshot reloaded: denied.
    sys_role_permissions::Entity::delete_many()
        .filter(sys_role_permissions::Column::PermissionId.eq(perm_id))
        .exec(&db)
        .await
        .expect("unbind");
    authz.reset(&db).await.expect("policy reload");
    assert!(matches!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET").await,
        Err(AuthzRejection::Unauthorized)
    ));

    // The endpoint binding is re-pointed, the snapshot reloaded: a new
    // pair passes without a restart.
    sys_permission_apis::Entity::update_many()
        .col_expr(
            sys_permission_apis::Column::EndpointPath,
            Expr::value("/admin/v1/roles"),
        )
        .filter(sys_permission_apis::Column::PermissionId.eq(perm_id))
        .exec(&db)
        .await
        .expect("repoint");
    sys_role_permissions::ActiveModel {
        tenant_id: sea_orm::Set(Some(1)),
        role_id: sea_orm::Set(Some(
            sys_roles::Entity::find()
                .filter(sys_roles::Column::Code.eq("ops"))
                .one(&db)
                .await
                .expect("role lookup")
                .expect("role row")
                .id,
        )),
        permission_id: sea_orm::Set(Some(perm_id)),
        effect: sea_orm::Set(Some("ALLOW".into())),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("rebind");
    authz.reset(&db).await.expect("policy reload");
    assert!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/roles", "GET")
            .await
            .is_ok()
    );
    assert!(matches!(
        judge(&authorizer, &roles(&["ops"]), "/admin/v1/users", "GET").await,
        Err(AuthzRejection::Unauthorized)
    ));
}

/// 模板角色不装载（参照 IsTemplateRoleCode）：template: 前缀的角色即
/// 便有绑定也不进策略面。
#[tokio::test]
async fn template_roles_are_skipped_by_the_loader() {
    let db = harness_db().await;
    seed_role_endpoint(&db, "template:tenant:manager", 1).await;
    let authz = Arc::new(PolicyAuthorizer::new(Some(AuthzKind::Opa)));
    authz.reset(&db).await.expect("policy load");
    let authorizer = AccessAuthorizer::over(db.clone(), Arc::clone(&authz));
    assert!(matches!(
        judge(
            &authorizer,
            &roles(&["template:tenant:manager"]),
            "/admin/v1/users",
            "GET"
        )
        .await,
        Err(AuthzRejection::Unauthorized)
    ));
}
