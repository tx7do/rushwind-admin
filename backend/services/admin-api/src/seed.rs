//! Boot seeds:
//! when a table is empty the default data is inserted
//! is inserted under a system viewer: superuser + credentials + role
//! binding, roles (+ role metadata), permission groups, permissions,
//! menus, languages, config keys.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::state::AppState;

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
    seed_admin_user(state).await?;
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
    if menus::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());
    }
    let meta = |title: &str, icon: &str, order: i32| {
        serde_json::json!({
            "title": title, "icon": icon, "order": order,
        })
    };
    type MenuSeedRow = (
        u32,
        u32,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        i32,
    );
    let rows: [MenuSeedRow; 34] = [
        (
            1,
            0,
            "CATALOG",
            "/dashboard",
            "dashboard",
            "dashboard",
            "DASHBOARD",
            "仪表盘",
            1,
        ),
        (
            2,
            1,
            "MENU",
            "/dashboard/console",
            "console",
            "dashboard/console/index",
            "DASHBOARD",
            "控制台",
            1,
        ),
        (
            3,
            0,
            "CATALOG",
            "/system",
            "system",
            "system",
            "SYSTEM",
            "系统管理",
            2,
        ),
        (
            4,
            3,
            "MENU",
            "/system/user",
            "user",
            "system/user/index",
            "SYSTEM",
            "用户管理",
            1,
        ),
        (
            5,
            3,
            "MENU",
            "/system/role",
            "role",
            "system/role/index",
            "SYSTEM",
            "角色管理",
            2,
        ),
        (
            6,
            3,
            "MENU",
            "/system/menu",
            "menu",
            "system/menu/index",
            "SYSTEM",
            "菜单管理",
            3,
        ),
        (
            74,
            0,
            "CATALOG",
            "/notification",
            "notification",
            "notification",
            "SYSTEM",
            "通知管理",
            6,
        ),
        (
            68,
            74,
            "MENU",
            "/notification/channels",
            "notification-channels",
            "notification/channel/index",
            "SYSTEM",
            "通知渠道",
            1,
        ),
        (
            73,
            74,
            "MENU",
            "/notification/rules",
            "notification-rules",
            "notification/rule/index",
            "SYSTEM",
            "通知规则",
            2,
        ),
        (
            72,
            74,
            "MENU",
            "/notification/deliveries",
            "notification-deliveries",
            "notification/delivery/index",
            "SYSTEM",
            "投递台账",
            3,
        ),
        (
            10,
            3,
            "MENU",
            "/system/org",
            "org",
            "system/org/index",
            "SYSTEM",
            "组织管理",
            4,
        ),
        (
            11,
            3,
            "MENU",
            "/system/position",
            "position",
            "system/position/index",
            "SYSTEM",
            "职位管理",
            5,
        ),
        (
            20,
            3,
            "MENU",
            "/system/api",
            "api",
            "system/api/index",
            "PERMISSION",
            "接口管理",
            6,
        ),
        (
            21,
            3,
            "MENU",
            "/system/permission",
            "permission",
            "system/permission/index",
            "PERMISSION",
            "权限管理",
            7,
        ),
        (
            22,
            3,
            "MENU",
            "/system/permission-group",
            "permissionGroup",
            "system/permissionGroup/index",
            "PERMISSION",
            "权限组",
            8,
        ),
        (
            23,
            3,
            "MENU",
            "/system/policy",
            "policy",
            "system/policy/index",
            "PERMISSION",
            "策略日志",
            9,
        ),
        (
            24,
            3,
            "MENU",
            "/system/permission-audit",
            "permissionAudit",
            "system/permissionAudit/index",
            "PERMISSION",
            "权限审计",
            10,
        ),
        (
            30,
            0,
            "CATALOG",
            "/dict",
            "dict",
            "dict",
            "DICT",
            "字典管理",
            3,
        ),
        (
            31,
            30,
            "MENU",
            "/dict/type",
            "dictType",
            "dict/type/index",
            "DICT",
            "字典类型",
            1,
        ),
        (
            32,
            30,
            "MENU",
            "/dict/entry",
            "dictEntry",
            "dict/entry/index",
            "DICT",
            "字典条目",
            2,
        ),
        (
            33,
            30,
            "MENU",
            "/dict/language",
            "language",
            "dict/language/index",
            "DICT",
            "语言管理",
            3,
        ),
        (
            34,
            30,
            "MENU",
            "/dict/i18n",
            "i18n",
            "dict/i18n/index",
            "DICT",
            "国际化",
            4,
        ),
        (
            40,
            0,
            "CATALOG",
            "/tenant",
            "tenant",
            "tenant",
            "TENANT",
            "租户管理",
            4,
        ),
        (
            41,
            40,
            "MENU",
            "/tenant/list",
            "tenantList",
            "tenant/list/index",
            "TENANT",
            "租户列表",
            1,
        ),
        (
            42,
            40,
            "MENU",
            "/tenant/plan",
            "tenantPlan",
            "tenant/plan/index",
            "TENANT",
            "套餐管理",
            2,
        ),
        (50, 0, "CATALOG", "/log", "log", "log", "LOG", "日志管理", 5),
        (
            51,
            50,
            "MENU",
            "/log/login",
            "loginLog",
            "log/login/index",
            "LOG",
            "登录日志",
            1,
        ),
        (
            52,
            50,
            "MENU",
            "/log/api",
            "apiLog",
            "log/api/index",
            "LOG",
            "API日志",
            2,
        ),
        (
            53,
            50,
            "MENU",
            "/log/operation",
            "operationLog",
            "log/operation/index",
            "LOG",
            "操作日志",
            3,
        ),
        (
            54,
            50,
            "MENU",
            "/log/data-access",
            "dataAccessLog",
            "log/dataAccess/index",
            "LOG",
            "数据访问日志",
            4,
        ),
        (
            55,
            50,
            "MENU",
            "/log/script",
            "scriptLog",
            "log/script/index",
            "LOG",
            "脚本日志",
            5,
        ),
        (
            60,
            0,
            "CATALOG",
            "/message",
            "message",
            "message",
            "INTERNAL_MESSAGE",
            "消息管理",
            6,
        ),
        (
            61,
            60,
            "MENU",
            "/message/list",
            "messageList",
            "message/list/index",
            "INTERNAL_MESSAGE",
            "消息列表",
            1,
        ),
        (
            62,
            60,
            "MENU",
            "/message/category",
            "messageCategory",
            "message/category/index",
            "INTERNAL_MESSAGE",
            "消息分类",
            2,
        ),
    ];
    for (id, parent, mtype, path, name, component, module, title, order) in rows {
        insert_seed!(
            &state.db,
            menus::ActiveModel {
                id: Set(id),
                parent_id: Set(if parent == 0 { None } else { Some(parent) }),
                type_column: Set(Some(mtype.into())),
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
