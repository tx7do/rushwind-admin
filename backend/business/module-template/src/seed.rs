//! The module's boot seeds — idempotent by natural key (never by
//! fixed id, so they never collide with the admin seeds' id space or
//! need sequence realignment):
//!
//! * menus — one catalog + one page under module `TEMPLATE`, keyed by
//!   the page row's existence;
//! * permission — `template:manage_notice` bound to the platform-admin
//!   and tenant-manager-template roles (looked up by code, skipped
//!   when absent);
//! * sys_apis — the module's route table upserted by (path, method);
//!   the runtime gate (TenantAccessChecker) rejects unregistered
//!   routes fail-closed, so this sync is what makes the module's
//!   routes reachable for tenant viewers.
//!
//! Order in the assembly binary: AFTER the admin seeds (the roles and
//! the menus table must exist).

use std::sync::Arc;

use admin_api::state::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

/// The plan-module registration is an OPERATOR step, deliberately not
/// seeded: which plans may open `TEMPLATE` is a business decision
/// (admin console → 套餐管理). Until registered, tenant viewers get
/// 403 on the module's routes (platform viewers bypass the plan
/// stage).
pub const BUSINESS_MODULE: &str = "TEMPLATE";

pub async fn run(state: &Arc<AppState>) -> Result<(), String> {
    seed_menu(state).await?;
    seed_permission(state).await?;
    sync_apis(state).await?;
    Ok(())
}

/// One catalog + one page menu; the page's component matches the
/// frontend module convention (`notice/notices/index` — the fork's
/// frontend work registers the view there).
async fn seed_menu(state: &Arc<AppState>) -> Result<(), String> {
    use admin_api::data::sys_menus as menus;
    let seeded = menus::Entity::find()
        .filter(menus::Column::Module.eq(BUSINESS_MODULE))
        .count(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if seeded > 0 {
        return Ok(());
    }
    let meta = |title: &str, order: i32| {
        serde_json::json!({
            "title": title, "icon": "lucide:megaphone", "order": order,
        })
    };
    let catalog = menus::ActiveModel {
        parent_id: Set(None),
        type_column: Set(Some("CATALOG".into())),
        path: Set(Some("/notice".into())),
        name: Set("Notice".into()),
        component: Set(Some("".into())),
        module: Set(Some(BUSINESS_MODULE.into())),
        meta: Set(Some(meta("公告", 200))),
        status: Set(Some("ON".into())),
        created_at: Set(Some(admin_api::data::now())),
        updated_at: Set(Some(admin_api::data::now())),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    menus::ActiveModel {
        parent_id: Set(Some(catalog.id)),
        type_column: Set(Some("EMBEDDED".into())),
        path: Set(Some("/notice/notices".into())),
        name: Set("NoticeList".into()),
        component: Set(Some("notice/notices/index".into())),
        module: Set(Some(BUSINESS_MODULE.into())),
        meta: Set(Some(meta("公告管理", 1))),
        status: Set(Some("ON".into())),
        created_at: Set(Some(admin_api::data::now())),
        updated_at: Set(Some(admin_api::data::now())),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The manage permission, bound to the platform-admin role and the
/// tenant-manager TEMPLATE role (both looked up by code — the roles
/// ride the admin seeds).
async fn seed_permission(state: &Arc<AppState>) -> Result<(), String> {
    use admin_api::data::sys_permissions as perms;
    use admin_api::data::sys_role_permissions as role_perms;
    use admin_api::data::sys_roles as roles;
    const CODE: &str = "template:manage_notice";

    let existing = perms::Entity::find()
        .filter(perms::Column::Code.eq(CODE))
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    let permission_id = match existing {
        Some(p) => p.id,
        None => {
            perms::ActiveModel {
                name: Set("公告管理".into()),
                code: Set(CODE.into()),
                group_id: Set(None),
                status: Set(Some("ON".into())),
                created_at: Set(Some(admin_api::data::now())),
                updated_at: Set(Some(admin_api::data::now())),
                ..Default::default()
            }
            .insert(&state.db)
            .await
            .map_err(|e| e.to_string())?
            .id
        }
    };

    for role_code in ["platform:admin", "template:tenant:manager"] {
        let Some(role) = roles::Entity::find()
            .filter(roles::Column::Code.eq(role_code))
            .one(&state.db)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let bound = role_perms::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(role_perms::Column::RoleId.eq(Some(role.id)))
                    .add(role_perms::Column::PermissionId.eq(Some(permission_id))),
            )
            .count(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        if bound > 0 {
            continue;
        }
        role_perms::ActiveModel {
            tenant_id: Set(Some(0)),
            role_id: Set(Some(role.id)),
            permission_id: Set(Some(permission_id)),
            effect: Set(Some("ALLOW".into())),
            status: Set(Some("ON".into())),
            created_at: Set(Some(admin_api::data::now())),
            updated_at: Set(Some(admin_api::data::now())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The module's route table upserted by (path, method): every
/// non-shadowed generated route lands as a sys_apis row with
/// `business_module = TEMPLATE` — the runtime gate reads that column
/// to classify the route into the plan-module whitelist. Idempotent:
/// existing rows are updated in place (a stale business_module
/// self-heals on the next boot).
async fn sync_apis(state: &Arc<AppState>) -> Result<(), String> {
    use admin_api::data::sys_apis;
    for spec in crate::gen::routes::ROUTES {
        if spec.shadowed {
            continue;
        }
        let existing = sys_apis::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(sys_apis::Column::Path.eq(spec.path))
                    .add(sys_apis::Column::Method.eq(spec.method)),
            )
            .one(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        let payload = sys_apis::ActiveModel {
            operation: Set(Some(spec.operation_id.to_string())),
            module: Set(Some(BUSINESS_MODULE.into())),
            business_module: Set(Some(BUSINESS_MODULE.into())),
            scope: Set(Some("ADMIN".into())),
            status: Set(Some("ON".into())),
            created_at: Set(Some(admin_api::data::now())),
            updated_at: Set(Some(admin_api::data::now())),
            ..Default::default()
        };
        match existing {
            Some(row) => {
                let mut a: sys_apis::ActiveModel = row.into();
                a.operation = payload.operation;
                a.module = payload.module;
                a.business_module = payload.business_module;
                a.scope = payload.scope;
                a.status = payload.status;
                a.updated_at = payload.updated_at;
                a.update(&state.db).await.map_err(|e| e.to_string())?;
            }
            None => {
                payload.insert(&state.db).await.map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
