//! Boot seeds:
//! when a table is empty the default data is inserted
//! is inserted under a system viewer: superuser + credentials + role
//! binding, roles (+ role metadata), permission groups, permissions,
//! menus, languages, config keys.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::state::AppState;

mod menu_data;

const DEFAULT_USER_PASSWORD: &str = "Abcd@1234";

/// One seed insert — the uniform stringly error path every seed row
/// builder shares. Value-consuming inserts keep the plain chain.
macro_rules! insert_seed {
    ($db:expr, $row:expr) => {
        $row.insert($db).await.map_err(|e| e.to_string())?;
    };
}

pub async fn run(state: &Arc<AppState>) -> Result<(), String> {
    seed_languages(state).await?;
    seed_configs(state).await?;
    seed_roles(state).await?;
    seed_permission_groups(state).await?;
    seed_permissions(state).await?;
    seed_menus(state).await?;
    seed_notification_rules(state).await?;
    seed_bi_dashboards(state).await?;
    seed_admin_user(state).await?;
    sync_apis(state).await?;
    Ok(())
}

/// 启动期接口同步（上游 api_service init 同语义）：把打进二进制的 OpenAPI
/// 端点清单按 (path, method) MERGE 进 sys_apis——authorizer 的路由闸对未
/// 登记端点 fail-closed，部署新契约后若不同步，租户侧全端点 403。失败
/// 不阻断启动（fail-open 留痕，重跑 SyncApis RPC 即自愈）。
async fn sync_apis(state: &Arc<AppState>) -> Result<(), String> {
    use sea_orm::TransactionTrait as _;
    let rows = crate::services::api::derived_api_rows()?;
    let svc = crate::services::ApiService {
        state: Arc::clone(state),
    };
    let txn = state.db.begin().await.map_err(|e| e.to_string())?;
    let result = svc.sync_merge_in_txn(&txn, rows, 0).await;
    match result {
        Ok(()) => txn.commit().await.map_err(|e| e.to_string())?,
        Err(_) => {
            let _ = txn.rollback().await;
        }
    }
    Ok(())
}

/// 内置 BI 模板（基线仪表盘版式）：按编码缺一补一、不覆盖既有行
/// （与上游 SeedDefaults 同语义）。平台租户（0）下挂，复制派生成租户自有资产。
async fn seed_bi_dashboards(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::bi_dashboards as dash;
    const CODE: &str = "builtin.bi.basic";
    if dash::Entity::find()
        .filter(dash::Column::Code.eq(CODE))
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    insert_seed!(
        &state.db,
        dash::ActiveModel {
            tenant_id: Set(Some(0)),
            name: Set("基础仪表盘模板".into()),
            code: Set(CODE.into()),
            kind: Set(Some("TEMPLATE".into())),
            remark: Set(Some("内置基线：一张柱状图，复制后换成自己的数据集即可".into())),
            payload: Set(Some(
                r#"{"schemaVersion":2,"name":"builtin.bi.basic","grid":{"columns":12,"rowHeight":80,"gap":8},"datasets":[{"id":"ds_sales","name":"月度销售额","type":"static","rows":[{"month":"1月","sales":120},{"month":"2月","sales":180},{"month":"3月","sales":150}]}],"charts":[{"id":"c_bar","title":"月度销售额","datasetId":"ds_sales","type":"bar","x":0,"y":0,"w":6,"h":4,"encoding":{"dimension":"month","measure":"sales"},"aggregation":"sum"}]}"#.into()
            )),
            chart_count: Set(Some(1)),
            is_built_in: Set(Some(true)),
            is_enabled: Set(Some(true)),
            sort_order: Set(Some(1)),
            version: Set(Some(1)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
    );
    Ok(())
}

async fn seed_languages(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_languages as langs;
    if langs::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let rows = [
        ("zh-CN", "简体中文", "简体中文", true, 0u32),
        ("zh-TW", "繁體中文", "繁體中文", false, 0),
        ("en-US", "English", "English", false, 1),
        ("ja-JP", "日本語", "日本語", false, 100),
        ("ko-KR", "한국어", "한국어", false, 100),
        ("es-ES", "Español", "Español", false, 100),
        ("fr-FR", "Français", "Français", false, 100),
    ];
    for (code, name, native, is_default, sort) in rows {
        insert_seed!(
            &state.db,
            langs::ActiveModel {
                language_code: Set(code.into()),
                language_name: Set(name.into()),
                native_name: Set(Some(native.into())),
                is_default: Set(Some(is_default)),
                is_enabled: Set(Some(true)),
                sort_order: Set(Some(sort)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}

async fn seed_configs(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_configs as configs;
    // Per-key missing-only inserts (SeedDefaults semantics).
    let seeds = [
        ("密码最小长度", "sys.password.minLen", "8", "INT"),
        (
            "密码最大有效期（天）",
            "sys.password.maxAgeDays",
            "90",
            "INT",
        ),
        ("密码历史记录数", "sys.password.historyCount", "3", "INT"),
    ];
    for (name, key, value, value_type) in seeds {
        let exists = configs::Entity::find()
            .filter(configs::Column::Key.eq(key))
            .count(&state.db)
            .await
            .unwrap_or(1)
            > 0;
        if exists {
            continue;
        }
        insert_seed!(
            &state.db,
            configs::ActiveModel {
                name: Set(name.into()),
                key: Set(key.into()),
                value: Set(Some(value.into())),
                value_type: Set(Some(value_type.into())),
                is_built_in: Set(Some(true)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}

async fn seed_roles(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_role_metadata as role_meta;
    use crate::data::sys_roles as roles;
    if roles::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    insert_seed!(
        &state.db,
        roles::ActiveModel {
            id: Set(1),
            tenant_id: Set(Some(0)),
            name: Set("平台管理员".into()),
            code: Set("platform:admin".into()),
            is_protected: Set(Some(true)),
            type_column: Set(Some("SYSTEM".into())),
            data_scope: Set(Some("ALL".into())),
            status: Set(Some("ON".into())),
            sort_order: Set(Some(1)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
    );
    insert_seed!(
        &state.db,
        roles::ActiveModel {
            id: Set(2),
            tenant_id: Set(Some(0)),
            name: Set("租户管理员模板".into()),
            code: Set("template:tenant:manager".into()),
            is_protected: Set(Some(true)),
            type_column: Set(Some("TEMPLATE".into())),
            data_scope: Set(Some("ALL".into())),
            status: Set(Some("ON".into())),
            sort_order: Set(Some(2)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
    );
    for (role_id, is_template, template_for, scope) in [
        (1u32, false, None, "PLATFORM"),
        (2u32, true, Some("tenant:manager"), "TENANT"),
    ] {
        insert_seed!(
            &state.db,
            role_meta::ActiveModel {
                tenant_id: Set(Some(0)),
                role_id: Set(Some(role_id)),
                is_template: Set(Some(is_template)),
                template_for: Set(template_for.map(String::from)),
                template_version: Set(Some(1)),
                sync_policy: Set(Some("AUTO".into())),
                scope: Set(Some(scope.into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}

async fn seed_permission_groups(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_permission_groups as groups;
    if groups::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let rows = [
        (1u32, "系统管理", "/", Some(0u32)),
        (2, "系统权限", "/1/2/", Some(1)),
        (3, "租户管理", "/1/3/", Some(1)),
        (4, "审计管理", "/1/4/", Some(1)),
        (5, "安全管理", "/1/5/", Some(1)),
    ];
    for (id, name, path, parent) in rows {
        insert_seed!(
            &state.db,
            groups::ActiveModel {
                id: Set(id),
                name: Set(name.into()),
                module: Set(Some("sys".into())),
                path: Set(Some(path.into())),
                parent_id: Set(parent),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}

async fn seed_permissions(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_permissions as perms;
    use crate::data::sys_role_permissions as role_perms;
    if perms::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let rows = [
        (1u32, "访问后台", "sys:access_backend", 1u32),
        (2, "平台管理员", "sys:platform_admin", 2),
        (3, "租户管理员", "sys:tenant_manager", 2),
        (4, "租户管理", "sys:manage_tenants", 3),
        (5, "审计日志", "sys:audit_logs", 4),
    ];
    for (id, name, code, group_id) in rows {
        insert_seed!(
            &state.db,
            perms::ActiveModel {
                id: Set(id),
                name: Set(name.into()),
                code: Set(code.into()),
                group_id: Set(Some(group_id)),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    // Role 1 (platform:admin) → permissions {1,2,4}; role 2 template → {1,3}.
    for (role_id, perm_ids) in [(1u32, vec![1u32, 2, 4]), (2, vec![1, 3])] {
        for perm in perm_ids {
            insert_seed!(
                &state.db,
                role_perms::ActiveModel {
                    tenant_id: Set(Some(0)),
                    role_id: Set(Some(role_id)),
                    permission_id: Set(Some(perm)),
                    effect: Set(Some("ALLOW".into())),
                    status: Set(Some("ON".into())),
                    created_at: Set(Some(crate::data::now())),
                    updated_at: Set(Some(crate::data::now())),
                    ..Default::default()
                }
            );
        }
    }
    Ok(())
}

/// The 30 fixed-ID default menus: id, parent, type,
/// path, name, component, module.
async fn seed_menus(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_menus as menus;
    use menu_data::MenuSeedRow;
    if menus::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let meta = |title: &str, icon: &str, order: i32| {
        serde_json::json!({
            "title": title, "icon": icon, "order": order,
        })
    };
    // Named fields over the old 9-tuple: 34 positional entries were
    // one transposition away from a silent mis-seed.
    for MenuSeedRow {
        id,
        parent,
        menu_type,
        path,
        name,
        component,
        module,
        title,
        order,
    } in menu_data::ROWS
    {
        insert_seed!(
            &state.db,
            menus::ActiveModel {
                id: Set(id),
                parent_id: Set(if parent == 0 { None } else { Some(parent) }),
                type_column: Set(Some(menu_type.into())),
                path: Set(Some(path.into())),
                name: Set(name.into()),
                component: Set(Some(component.into())),
                module: Set(Some(module.into())),
                meta: Set(Some(meta(title, "lucide:circle", order))),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}

/// 上游 eventChannels 静态表的落地播种(四行一一对应,见
/// docs/notification_domain_design.md §3.5):事件 → 渠道,均同步投递。
async fn seed_notification_rules(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_notification_rules as rules;
    if rules::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let rows = [
        ("PASSWORD_RESET_CODE", "EMAIL"),
        ("CONTACT_BIND_CODE", "EMAIL"),
        ("CHANNEL_TEST_EMAIL", "EMAIL"),
        ("INTERNAL_MESSAGE", "INTERNAL"),
    ];
    for (event_type, channel) in rows {
        insert_seed!(
            &state.db,
            rules::ActiveModel {
                event_type: Set(Some(event_type.into())),
                channel: Set(Some(channel.into())),
                is_async: Set(Some(false)),
                is_enabled: Set(Some(true)),
                remark: Set(Some("初始路由".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
        );
    }
    Ok(())
}
async fn seed_admin_user(state: &Arc<AppState>) -> Result<(), String> {
    use crate::data::sys_user_credentials as credentials;
    use crate::data::sys_user_roles as user_roles;
    use crate::data::sys_users as users;
    if users::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let user = users::ActiveModel {
        tenant_id: Set(Some(0)),
        username: Set("admin".into()),
        realname: Set(Some("喵个咪".into())),
        nickname: Set(Some("鹳狸猿".into())),
        region: Set(Some("中国".into())),
        email: Set(Some("admin@gmail.com".into())),
        gender: Set(Some("SECRET".into())),
        status: Set(Some("NORMAL".into())),
        created_at: Set(Some(crate::data::now())),
        updated_at: Set(Some(crate::data::now())),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    insert_seed!(
        &state.db,
        credentials::ActiveModel {
            tenant_id: Set(Some(0)),
            user_id: Set(Some(user.id)),
            identity_type: Set(Some("USERNAME".into())),
            identifier: Set("admin".into()),
            credential_type: Set(Some("PASSWORD_HASH".into())),
            credential: Set(crate::crypto::hash_password(DEFAULT_USER_PASSWORD)?),
            is_primary: Set(Some(true)),
            status: Set(Some("ENABLED".into())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
    );
    insert_seed!(
        &state.db,
        user_roles::ActiveModel {
            tenant_id: Set(Some(0)),
            user_id: Set(Some(user.id)),
            role_id: Set(Some(1)),
            is_primary: Set(Some(true)),
            status: Set(Some("ACTIVE".into())),
            assigned_at: Set(Some(crate::data::now())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
    );
    Ok(())
}
