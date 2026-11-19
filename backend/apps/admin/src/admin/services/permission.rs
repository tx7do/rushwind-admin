//! PermissionService — //! internal/service/service: permission CRUD plus
//! SyncPermissions, the menu→permission rebuild (compose full menu
//! paths, convert to resource:action codes, CATALOG menus become
//! groups, enabled apis attach by converted path codes), then role
//! policy reload.

use std::collections::BTreeMap;
use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use crate::proto::pagination::PagingRequest;
use crate::proto::permission::service::v1::{
    CreatePermissionRequest, DeletePermissionRequest, GetPermissionRequest, ListPermissionResponse,
    Permission, UpdatePermissionRequest,
};

/// 行集 → proto 集合，三张绑定/名称面各一次 IN 查询（issue #9d：旧实现
/// 每行 3 条顺序 SQL × 不分页全量 ≈ 每刷一次权限页上千条查询，抽屉 10 秒+
/// 卡顿的主因在此）。
async fn permission_protos(
    state: &AppState,
    rows: Vec<crate::data::sys_permissions::Model>,
) -> Vec<Permission> {
    if rows.is_empty() {
        return Vec::new();
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let menu_rows: std::collections::HashMap<i64, Vec<u64>> =
        crate::data::sys_permission_menus::Entity::find()
            .filter(crate::data::sys_permission_menus::Column::PermissionId.is_in(ids.clone()))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .fold(std::collections::HashMap::new(), |mut acc, m| {
                if let Some(mid) = m.menu_id {
                    acc.entry(m.permission_id.unwrap_or(0))
                        .or_default()
                        .push(mid as u64);
                }
                acc
            });
    let api_rows: std::collections::HashMap<i64, Vec<u64>> =
        crate::data::sys_permission_apis::Entity::find()
            .filter(crate::data::sys_permission_apis::Column::PermissionId.is_in(ids.clone()))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .fold(std::collections::HashMap::new(), |mut acc, m| {
                if let Some(aid) = m.api_id {
                    acc.entry(m.permission_id.unwrap_or(0))
                        .or_default()
                        .push(aid as u64);
                }
                acc
            });
    let group_ids: Vec<i64> = rows.iter().filter_map(|r| r.group_id).collect();
    let group_names: std::collections::HashMap<i64, String> = if group_ids.is_empty() {
        Default::default()
    } else {
        crate::data::sys_permission_groups::Entity::find()
            .filter(crate::data::sys_permission_groups::Column::Id.is_in(group_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.id, g.name))
            .collect()
    };
    rows.into_iter()
        .map(|r| {
            let (menu_ids, api_ids) = (
                menu_rows.get(&r.id).cloned().unwrap_or_default(),
                api_rows.get(&r.id).cloned().unwrap_or_default(),
            );
            Permission {
                id: Some(r.id as u64),
                name: Some(r.name),
                code: Some(r.code),
                description: r.description,
                status: r.status.as_deref().map(|s| if s == "OFF" { 0 } else { 1 }),
                group_id: r.group_id.map(|v| v as u64),
                group_name: r.group_id.and_then(|gid| group_names.get(&gid).cloned()),
                menu_ids,
                api_ids,
                created_by: r.created_by.map(|v| v as u64),
                updated_by: r.updated_by.map(|v| v as u64),
                deleted_by: r.deleted_by.map(|v| v as u64),
                created_at: r.created_at.and_then(crate::state::naive_to_ts),
                updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
                deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
            }
        })
        .collect()
}

async fn permission_proto(state: &AppState, r: crate::data::sys_permissions::Model) -> Permission {
    permission_protos(state, vec![r])
        .await
        .pop()
        .expect("single-row batch yields one proto")
}

/// The naive English singularizer the converter relies on
/// (users→user, categories→category).
fn singularize(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if let Some(stem) = word.strip_suffix('s') {
        if !stem.is_empty() {
            return stem.to_string();
        }
    }
    word.to_string()
}

/// The BUTTON menu title→action keyword rule
/// (utils/converter/module:143-240).
fn button_action(title: &str) -> &'static str {
    let t = title.to_lowercase();
    for (keyword, action) in [
        ("新增", "create"),
        ("创建", "create"),
        ("添加", "create"),
        ("add", "create"),
        ("create", "create"),
        ("编辑", "edit"),
        ("修改", "edit"),
        ("edit", "edit"),
        ("update", "edit"),
        ("删除", "delete"),
        ("delete", "delete"),
        ("remove", "delete"),
        ("导入", "import"),
        ("import", "import"),
        ("导出", "export"),
        ("export", "export"),
    ] {
        if t.contains(keyword) {
            return action;
        }
    }
    "act"
}

/// Menu path → permission code (ConvertCode): drop the first segment,
/// skip `:param` segments, join the rest with `:`, append the type's
/// action suffix.
fn menu_code(path: &str, title: &str, menu_type: &str) -> String {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let rest: Vec<String> = segments
        .iter()
        .skip(1)
        .filter(|s| !s.starts_with(':'))
        .map(|s| singularize(s))
        .collect();
    let mut code = rest.join(":");
    if !code.is_empty() {
        code.push(':');
    }
    let suffix = match menu_type {
        "CATALOG" => "dir",
        "MENU" | "EMBEDDED" => "view",
        "LINK" => "jump",
        "BUTTON" => button_action(title),
        _ => "view",
    };
    format!("{code}{suffix}")
}

/// Api path+method → permission code (ConvertCodeByPath): strip the
/// version prefix, drop param segments, singularize the resource; the
/// action derives from the method.
fn api_code(method: &str, path: &str) -> String {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .skip(2) // /admin/v1
        .filter(|s| !s.starts_with('{') && !s.starts_with(':'))
        .collect();
    let resource = segments.first().map(|s| singularize(s)).unwrap_or_default();
    let action = if path.ends_with("/list") && method == "GET" {
        "view"
    } else {
        match method {
            "GET" => "view",
            "POST" => "create",
            "PUT" | "PATCH" => "edit",
            "DELETE" => "delete",
            _ => "view",
        }
    };
    if resource.is_empty() {
        return action.to_string();
    }
    format!("{resource}:{action}")
}

/// 绑定整体替换（同步 MERGE 用，事务内）：派生集里每个权限至多挂一菜单/
/// 一端点（同码族归并后的形态），空派生 = 清空该权限的绑定（上游
/// AssignSyncBindings 替换语义）。
async fn replace_menu_binding(
    txn: &sea_orm::DatabaseTransaction,
    permission_id: i64,
    menu: Option<(u64, String)>,
) -> Result<(), StatusError> {
    crate::data::sys_permission_menus::Entity::delete_many()
        .filter(crate::data::sys_permission_menus::Column::PermissionId.eq(permission_id))
        .exec(txn)
        .await
        .map_err(db_err)?;
    if let Some((menu_id, full_path)) = menu {
        crate::data::sys_permission_menus::ActiveModel {
            permission_id: Set(Some(permission_id)),
            menu_id: Set(Some(menu_id as i64)),
            full_path: Set(Some(full_path)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// 端点绑定替换（同步 MERGE 用，事务内）：api_id 与 (path, method) 快照
/// 同写——快照是裁决键，api_id 仅历史兼容。
async fn replace_api_binding(
    txn: &sea_orm::DatabaseTransaction,
    permission_id: i64,
    api: &Option<(u64, String, String)>,
) -> Result<(), StatusError> {
    crate::data::sys_permission_apis::Entity::delete_many()
        .filter(crate::data::sys_permission_apis::Column::PermissionId.eq(permission_id))
        .exec(txn)
        .await
        .map_err(db_err)?;
    if let Some((api_id, path, method)) = api {
        crate::data::sys_permission_apis::ActiveModel {
            permission_id: Set(Some(permission_id)),
            api_id: Set(Some(*api_id as i64)),
            endpoint_path: Set(Some(path.clone())),
            endpoint_method: Set(Some(method.clone())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// CRUD 路径的端点绑定写入：按 api_id 解析端点身份后连快照一起落
/// （api 行已删的 id 落 NULL 快照——不匹配任何端点，fail-closed）。
async fn bind_api_rows(
    db: &sea_orm::DatabaseConnection,
    permission_id: i64,
    api_ids: &[u64],
) -> Result<(), StatusError> {
    for api_id in api_ids {
        let row = crate::data::sys_apis::Entity::find_by_id(*api_id as i64)
            .one(db)
            .await
            .map_err(db_err)?;
        let (path, method) = row
            .map(|r| {
                (
                    r.path.clone().unwrap_or_default(),
                    r.method.map(|m| m.to_uppercase()).unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        crate::data::sys_permission_apis::ActiveModel {
            permission_id: Set(Some(permission_id)),
            api_id: Set(Some(*api_id as i64)),
            endpoint_path: Set((!path.is_empty()).then_some(path)),
            endpoint_method: Set((!method.is_empty()).then_some(method)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(db)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// 一批菜单 id 的全路径，内存父链一次算完（CRUD 绑定写入用：旧实现每
/// 个 id 逐级 find_by_id 走库，抽屉保存 N 个菜单就是 N 条链路查询）。
async fn menu_full_paths(
    db: &sea_orm::DatabaseConnection,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, Option<String>>, StatusError> {
    use std::collections::{HashMap, HashSet};
    let menus = crate::data::sys_menus::Entity::find()
        .all(db)
        .await
        .map_err(db_err)?;
    let by_id: HashMap<i64, &crate::data::sys_menus::Model> =
        menus.iter().map(|m| (m.id, m)).collect();
    let mut out: HashMap<i64, Option<String>> = HashMap::new();
    for id in ids {
        if out.contains_key(id) {
            continue;
        }
        let mut parts: Vec<String> = Vec::new();
        let mut current = Some(*id);
        let mut walked: HashSet<i64> = HashSet::new();
        for _ in 0..32 {
            let Some(mid) = current else { break };
            let Some(row) = by_id.get(&mid) else {
                out.insert(*id, None);
                current = None;
                break;
            };
            if !walked.insert(mid) {
                break; // 环：fail-closed
            }
            parts.push(row.path.clone().unwrap_or_default());
            current = row.parent_id;
        }
        if current.is_some() {
            out.insert(*id, None); // 环或超深：不做拼接
            continue;
        }
        if !out.contains_key(id) {
            parts.reverse();
            let full = parts.concat();
            out.insert(*id, (!full.is_empty()).then_some(full));
        }
    }
    Ok(out)
}

pub struct PermissionService {
    pub state: Arc<AppState>,
}

impl PermissionService {
    /// SyncPermissions: truncate biz permissions + groups, rebuild from
    /// ON menus (codes per menu type; CATALOG → group) and from enabled
    /// apis (code by path, grouped under UncategorizedPermissionGroup
    async fn sync_from_sources(&self, operator_id: i64) -> Result<(), StatusError> {
        use sea_orm::TransactionTrait as _;
        let txn = self.state.db.begin().await.map_err(db_err)?;
        let result = self.sync_from_sources_in_txn(&txn, operator_id).await;
        match result {
            Ok(()) => {
                txn.commit().await.map_err(db_err)?;
            }
            Err(_) => {
                let _ = txn.rollback().await;
            }
        }
        result
    }

    /// 同步权限点 —— 组与权限点两层全 MERGE 化（仓内不再有删除重建型
    /// 同步）：派生集（菜单/接口 → 权限码；CATALOG 菜单 → 组）按自然键
    /// 对齐——
    ///   * 权限点按 code 对齐（保 id：角色授权/租户模板副本引用零漂移）；
    ///   * 组按 code 自然键 MERGE（保 id：旧实现 delete_many 全删重建，
    ///     每次 id 洗牌 + 种子组被清、sys:* 权限的 group_id 成悬挂引用；
    ///     issue #9a/#9b）。组名取菜单 meta.title（中文随菜单数据走），
    ///     兜底 menu.name；parent_id 从菜单父链的 CATALOG 组派生（层级
    ///     恢复，issue #9 分组列表树形）；
    ///   * 归类（issue #9c）：菜单权限挂最近的 CATALOG 祖先组（旧实现的
    ///     「权限码前缀匹配 URL 路径」恒假 → 全部落 NULL）；API 权限挂其
    ///     码族命中菜单的所属组（旧实现命中反而置 NULL、未命中兜底进
    ///     第一组——用户环境里即 Profile）；两者未命中统一落独立
    ///     「未分组」组（派生集常驻），不再兜底第一组。
    ///
    /// `sys:` 前缀的系统权限（登录链路身份权限）不在派生集、不参与对齐。
    /// 删除段最后跑：权限点级联清角色授权/端点/菜单绑定；组级只收敛
    /// 「g 码（存量迁移回填标记）+ 零引用 + 非派生」的行——种子组被
    /// sys:* 权限引用而幸存，手工组（NULL/空码）永不触碰。
    async fn sync_from_sources_in_txn(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        operator_id: i64,
    ) -> Result<(), StatusError> {
        // —— 菜单派生基面：ON 菜单，id 序（seed/同步都保证父先于子）——
        let menus = crate::data::sys_menus::Entity::find()
            .filter(crate::data::sys_menus::Column::Status.eq("ON"))
            .order_by_asc(crate::data::sys_menus::Column::Id)
            .all(txn)
            .await
            .map_err(db_err)?;

        // parent chain → full path（与授权绑定快照同口径：parent_path + path
        // 直连——**既有派生口径**，seed 的绝对子路径会重复拼接，权限码因此
        // 带冗余段；码是自然键，口径一动 = 全量换码 = 角色授权漂移，故保留）。
        let mut path_by_id: BTreeMap<u64, String> = BTreeMap::new();
        for menu in &menus {
            let parent_path = menu
                .parent_id
                .and_then(|pid| path_by_id.get(&(pid as u64)).cloned())
                .unwrap_or_default();
            let full = format!("{}{}", parent_path, menu.path.clone().unwrap_or_default());
            path_by_id.insert(menu.id as u64, full);
        }
        // 规范化全路径（绝对覆盖 / 相对拼接）——组码没有历史包袱，用干净
        // 口径；嵌套 CATALOG 的组码不再继承直连口径的冗余段。
        let mut norm_path_by_id: BTreeMap<u64, String> = BTreeMap::new();
        for menu in &menus {
            let Some(path) = menu
                .path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            else {
                continue;
            };
            let parent_norm = menu
                .parent_id
                .and_then(|pid| norm_path_by_id.get(&(pid as u64)).cloned());
            let full = match (&parent_norm, path.starts_with('/')) {
                (None, _) | (_, true) => format!("/{}", path.trim_matches('/')),
                (Some(pp), false) => format!(
                    "{}/{}",
                    pp.trim_end_matches('/'),
                    path.trim_start_matches('/')
                ),
            };
            norm_path_by_id.insert(menu.id as u64, full);
        }
        // 菜单路径末段（单数化）索引：API 资源段 → 菜单 → 组 的匹配锚。
        let segment_menu_by_resource: BTreeMap<String, u64> = menus
            .iter()
            .filter_map(|m| {
                let last = m
                    .path
                    .as_deref()?
                    .split('/')
                    .rev()
                    .find(|s| !s.is_empty())?
                    .to_string();
                Some((singularize(&last), m.id as u64))
            })
            .collect();
        let type_by_id: std::collections::HashMap<u64, &str> = menus
            .iter()
            .map(|m| (m.id as u64, m.type_column.as_deref().unwrap_or("MENU")))
            .collect();
        let parent_by_id: std::collections::HashMap<u64, u64> = menus
            .iter()
            .filter_map(|m| m.parent_id.map(|p| (m.id as u64, p as u64)))
            .collect();
        // 菜单名 → 标题（meta.title 中文优先，兜底路由名——issue #9b）。
        let title_of = |menu: &crate::data::sys_menus::Model| -> String {
            menu.meta
                .as_ref()
                .and_then(crate::admin::services::menu::menu_meta_from_json)
                .and_then(|m| m.title)
                .unwrap_or_else(|| menu.name.clone())
        };
        // 最近的 CATALOG 祖先（含环防护 32 层）。
        let nearest_catalog = |start: u64| -> Option<u64> {
            let mut cur = parent_by_id.get(&start).copied();
            for _ in 0..32 {
                let mid = cur?;
                if type_by_id.get(&mid).copied() == Some("CATALOG") {
                    return Some(mid);
                }
                cur = parent_by_id.get(&mid).copied();
            }
            None
        };

        // —— 组派生集：CATALOG 菜单成组（code = 菜单全路径段），未分组常驻 ——
        struct DerivedGroup {
            code: String,
            name: String,
            module: Option<String>,
            parent_code: Option<String>,
        }
        let mut derived_groups: Vec<DerivedGroup> = Vec::new();
        let mut catalog_code_by_menu: std::collections::HashMap<u64, String> =
            std::collections::HashMap::new();
        for menu in &menus {
            if type_by_id.get(&(menu.id as u64)).copied() != Some("CATALOG") {
                continue;
            }
            let full_path = path_by_id
                .get(&(menu.id as u64))
                .cloned()
                .unwrap_or_default();
            if full_path.is_empty() {
                continue;
            }
            let norm_full = norm_path_by_id
                .get(&(menu.id as u64))
                .cloned()
                .unwrap_or_else(|| full_path.clone());
            let code = norm_full.trim_start_matches('/').to_string();
            let parent_code = nearest_catalog(menu.id as u64)
                .and_then(|mid| catalog_code_by_menu.get(&mid).cloned());
            derived_groups.push(DerivedGroup {
                module: norm_full
                    .split('/')
                    .find(|s| !s.is_empty())
                    .map(str::to_string),
                name: title_of(menu),
                code,
                parent_code,
            });
            catalog_code_by_menu.insert(
                menu.id as u64,
                norm_full.trim_start_matches('/').to_string(),
            );
        }
        derived_groups.push(DerivedGroup {
            code: "ungrouped".into(),
            name: "未分组".into(),
            module: None,
            parent_code: None,
        });

        // —— 组 MERGE（自然键 code；命中原位更新保 id，缺失插入）——
        let groups = crate::data::sys_permission_groups::Entity::find()
            .all(txn)
            .await
            .map_err(db_err)?;
        let existing_group_by_code: std::collections::HashMap<String, i64> = groups
            .iter()
            .filter_map(|g| g.code.clone().filter(|c| !c.is_empty()).map(|c| (c, g.id)))
            .collect();
        let mut group_id_by_code: BTreeMap<String, i64> = BTreeMap::new();
        let mut group_path_by_code: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for g in &derived_groups {
            let parent_id = g
                .parent_code
                .as_ref()
                .and_then(|c| group_id_by_code.get(c).copied());
            let parent_path = g
                .parent_code
                .as_ref()
                .and_then(|c| group_path_by_code.get(c).cloned());
            let module = g.module.clone();
            match existing_group_by_code.get(&g.code) {
                Some(&gid) => {
                    let row = crate::data::sys_permission_groups::Entity::find_by_id(gid)
                        .one(txn)
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| not_found("permission group"))?;
                    let path = parent_path
                        .map(|pp| format!("{pp}{gid}/"))
                        .unwrap_or_else(|| format!("/{gid}/"));
                    let mut a: crate::data::sys_permission_groups::ActiveModel = row.into();
                    a.name = Set(g.name.clone());
                    a.module = Set(module);
                    a.path = Set(Some(path.clone()));
                    a.parent_id = Set(parent_id);
                    crate::stamp_update!(a, operator_id);
                    a.update(txn).await.map_err(db_err)?;
                    group_id_by_code.insert(g.code.clone(), gid);
                    group_path_by_code.insert(g.code.clone(), path);
                }
                None => {
                    let row = crate::data::sys_permission_groups::ActiveModel {
                        code: Set(Some(g.code.clone())),
                        name: Set(g.name.clone()),
                        module: Set(module),
                        parent_id: Set(parent_id),
                        status: Set(Some("ON".into())),
                        created_at: Set(Some(crate::data::now())),
                        updated_at: Set(Some(crate::data::now())),
                        ..Default::default()
                    }
                    .insert(txn)
                    .await
                    .map_err(db_err)?;
                    let new_gid = row.id;
                    let path = parent_path
                        .map(|pp| format!("{pp}{new_gid}/"))
                        .unwrap_or_else(|| format!("/{new_gid}/"));
                    let mut a: crate::data::sys_permission_groups::ActiveModel = row.into();
                    a.path = Set(Some(path.clone()));
                    a.update(txn).await.map_err(db_err)?;
                    group_id_by_code.insert(g.code.clone(), new_gid);
                    group_path_by_code.insert(g.code.clone(), path);
                }
            }
        }

        // —— 权限派生集：(code, name, group_code, menu 绑定, api 绑定快照)。
        // 菜单权限 → 最近的 CATALOG 祖先组；API 权限 → 码族命中菜单的所属
        // 组；未命中 → 未分组。
        type DerivedPermission = (
            String,
            String,
            Option<String>,
            Option<(u64, String)>,
            Option<(u64, String, String)>,
        );
        let mut derived: Vec<DerivedPermission> = Vec::new();
        let mut menu_code_by_menu: BTreeMap<u64, String> = BTreeMap::new();
        for menu in &menus {
            let menu_type = menu.type_column.clone().unwrap_or_default();
            let full_path = path_by_id
                .get(&(menu.id as u64))
                .cloned()
                .unwrap_or_default();
            if full_path.is_empty() {
                continue;
            }
            let code = menu_code(&full_path, &menu.name, &menu_type);
            let owner = if menu_type == "CATALOG" {
                Some(menu.id as u64)
            } else {
                nearest_catalog(menu.id as u64)
            };
            let group_code = owner
                .and_then(|mid| catalog_code_by_menu.get(&mid).cloned())
                .or_else(|| Some("ungrouped".into()));
            derived.push((
                code.clone(),
                menu.name.clone(),
                group_code,
                Some((menu.id as u64, full_path.clone())),
                None,
            ));
            menu_code_by_menu.insert(menu.id as u64, code);
        }

        let apis = crate::data::sys_apis::Entity::find()
            .filter(crate::data::sys_apis::Column::Status.eq("ON"))
            .all(txn)
            .await
            .map_err(db_err)?;
        for api in apis {
            let (Some(method), Some(path)) = (api.method.clone(), api.path.clone()) else {
                continue;
            };
            let code = api_code(&method, &path);
            let matched_menu = menu_code_by_menu.iter().find_map(|(menu_id, base)| {
                let base_root = base
                    .rsplit_once(':')
                    .map(|(r, _)| r.to_string())
                    .unwrap_or_else(|| base.clone());
                (code.starts_with(&base_root) || base_root.starts_with(&code)).then_some(*menu_id)
            });
            let group_code = matched_menu
                .and_then(|mid| {
                    let owner = if type_by_id.get(&mid).copied() == Some("CATALOG") {
                        Some(mid)
                    } else {
                        nearest_catalog(mid)
                    };
                    owner.and_then(|o| catalog_code_by_menu.get(&o).cloned())
                })
                // 码族未命中（绝对路径数据下 2 段 API 码对 ≥3 段菜单码族结构性
                // 不可能命中）的兜底归类：API 资源段（单数化）对菜单路径末段
                // （单数化）匹配——/admin/v1/positions → "position" →
                // /system/position → 系统管理组。仍未命中才落「未分组」。
                .or_else(|| {
                    let resource = code.rsplit(':').nth(1).unwrap_or("");
                    (!resource.is_empty())
                        .then(|| segment_menu_by_resource.get(resource).copied())
                        .flatten()
                        .and_then(|mid| {
                            let owner = if type_by_id.get(&mid).copied() == Some("CATALOG") {
                                Some(mid)
                            } else {
                                nearest_catalog(mid)
                            };
                            owner.and_then(|o| catalog_code_by_menu.get(&o).cloned())
                        })
                })
                .or_else(|| Some("ungrouped".into()));
            derived.push((
                code.clone(),
                code.clone(),
                group_code,
                None,
                Some((api.id as u64, path, method.to_uppercase())),
            ));
        }

        // 派生集同码去重（首见胜出，菜单先于接口）：menu_code 丢首段的派生
        // 口径让多个顶层 CATALOG 同落 "dir" 码——不去重会把同码插成两行权限
        // （历史行为缺陷，用户库里的 "dir" 重复行即此来源）。
        {
            let mut seen_codes: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(derived.len());
            derived.retain(|(code, ..)| seen_codes.insert(code.clone()));
        }

        // —— 权限 MERGE 三段 ——
        // 基线：sys: 保留码之外的全部权限（业务面对齐域）。同码多行（历史
        // 重复派生的存量）收敛到最小 id，其余行随删除段级联清理——对齐域里
        // 一个 code 只允许一行。
        let mut existing = crate::data::sys_permissions::Entity::find()
            .filter(crate::data::sys_permissions::Column::Code.ne(""))
            .all(txn)
            .await
            .map_err(db_err)?
            .into_iter()
            .filter(|r| !r.code.starts_with("sys:"))
            .collect::<Vec<_>>();
        existing.sort_by_key(|r| r.id);
        let mut duplicate_ids: Vec<i64> = Vec::new();
        {
            let mut seen: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(existing.len());
            existing.retain(|r| {
                if seen.insert(r.code.clone()) {
                    true
                } else {
                    duplicate_ids.push(r.id);
                    false
                }
            });
        }
        let existing_by_code: BTreeMap<String, crate::data::sys_permissions::Model> = existing
            .iter()
            .cloned()
            .map(|r| (r.code.clone(), r))
            .collect();

        let mut merge_hit: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(derived.len());

        // 更新段：code 已存在 → 原位更新（保 id），绑定按派生集整体替换。
        for (code, name, group_code, menu_id, api_binding) in &derived {
            let Some(exist) = existing_by_code.get(code) else {
                continue;
            };
            let gid = group_code
                .as_ref()
                .and_then(|g| group_id_by_code.get(g).copied());
            let mut a: crate::data::sys_permissions::ActiveModel = exist.clone().into();
            a.name = Set(name.clone());
            a.group_id = Set(gid);
            a.updated_at = Set(Some(crate::data::now()));
            a.update(txn).await.map_err(db_err)?;
            replace_menu_binding(txn, exist.id as u64 as i64, menu_id.clone()).await?;
            replace_api_binding(txn, exist.id as u64 as i64, api_binding).await?;
            merge_hit.insert(code.clone());
        }

        // 插入段：派生集有、库没有。
        for (code, name, group_code, menu_id, api_binding) in &derived {
            if merge_hit.contains(code) || existing_by_code.contains_key(code) {
                continue; // 防御：命中必进更新段，保留护栏
            }
            let gid = group_code
                .as_ref()
                .and_then(|g| group_id_by_code.get(g).copied());
            let perm = crate::data::sys_permissions::ActiveModel {
                name: Set(name.clone()),
                code: Set(code.clone()),
                group_id: Set(gid),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(txn)
            .await
            .map_err(db_err)?;
            replace_menu_binding(txn, perm.id as u64 as i64, menu_id.clone()).await?;
            replace_api_binding(txn, perm.id as u64 as i64, api_binding).await?;
        }

        // 权限删除段：库有、派生集没有 → 删行并级联清角色授权/端点/菜单
        // 绑定（授权链上不允许残留指向已删权限行的绑定）。
        let mut gone_ids: Vec<i64> = existing_by_code
            .iter()
            .filter(|(code, _)| !merge_hit.contains(*code))
            .map(|(_, r)| r.id)
            .collect();
        gone_ids.extend(duplicate_ids);
        if !gone_ids.is_empty() {
            crate::data::sys_role_permissions::Entity::delete_many()
                .filter(
                    crate::data::sys_role_permissions::Column::PermissionId.is_in(gone_ids.clone()),
                )
                .exec(txn)
                .await
                .map_err(db_err)?;
            crate::data::sys_permission_apis::Entity::delete_many()
                .filter(
                    crate::data::sys_permission_apis::Column::PermissionId.is_in(gone_ids.clone()),
                )
                .exec(txn)
                .await
                .map_err(db_err)?;
            crate::data::sys_permission_menus::Entity::delete_many()
                .filter(
                    crate::data::sys_permission_menus::Column::PermissionId.is_in(gone_ids.clone()),
                )
                .exec(txn)
                .await
                .map_err(db_err)?;
            crate::data::sys_permissions::Entity::delete_many()
                .filter(crate::data::sys_permissions::Column::Id.is_in(gone_ids))
                .exec(txn)
                .await
                .map_err(db_err)?;
        }

        // —— 组清理段（存量收敛，只走一次）：g 码（迁移回填标记）+ 零引用
        // + 非派生 = 旧派生组（英文键名时代）与空手工组；种子组被 sys:*
        // 权限引用而幸存；NULL/空码手工组与派生组永不进入候选。权限归并
        // 之后跑：旧派生组的引用已全部重指到语义码组，此刻才真正归零。
        let derived_ids: std::collections::HashSet<i64> =
            group_id_by_code.values().copied().collect();
        let referenced: std::collections::HashSet<i64> =
            crate::data::sys_permissions::Entity::find()
                .all(txn)
                .await
                .map_err(db_err)?
                .into_iter()
                .filter_map(|p| p.group_id)
                .collect();
        let stale_groups: Vec<i64> = groups
            .iter()
            .filter(|g| {
                g.code
                    .as_deref()
                    .map(|c| c.starts_with('g'))
                    .unwrap_or(false)
                    && !derived_ids.contains(&g.id)
                    && !referenced.contains(&g.id)
            })
            .map(|g| g.id)
            .collect();
        if !stale_groups.is_empty() {
            crate::data::sys_permission_groups::Entity::delete_many()
                .filter(crate::data::sys_permission_groups::Column::Id.is_in(stale_groups))
                .exec(txn)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::PermissionServiceHandlers for PermissionService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPermissionResponse, StatusError> {
        let repo = crate::data::repos::PermissionRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        let items = permission_protos(&self.state, rows).await;
        Ok(ListPermissionResponse { items, total })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPermissionRequest,
    ) -> Result<Permission, StatusError> {
        let _ = &ctx;
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::get_permission_request::QueryBy
        );
        let row = crate::data::sys_permissions::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("permission"))?;
        Ok(permission_proto(&self.state, row).await)
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePermissionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        let perm = crate::data::sys_permissions::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
            description: Set(data.description.clone()),
            group_id: Set(data.group_id.map(|v| v as i64)),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        let paths = menu_full_paths(
            &self.state.db,
            &data.menu_ids.iter().map(|v| *v as i64).collect::<Vec<_>>(),
        )
        .await?;
        for menu_id in &data.menu_ids {
            crate::data::sys_permission_menus::ActiveModel {
                permission_id: Set(Some(perm.id)),
                menu_id: Set(Some(*menu_id as i64)),
                full_path: Set(paths.get(&(*menu_id as i64)).cloned().flatten()),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        bind_api_rows(&self.state.db, perm.id as u64 as i64, &data.api_ids).await?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdatePermissionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_permissions::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("permission"))?;
        let mut a: crate::data::sys_permissions::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.code {
                a.code = Set(v.clone());
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.group_id {
                a.group_id = Set(Some(v as i64));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        if let Some(data) = &req.data {
            if !data.menu_ids.is_empty() {
                crate::data::sys_permission_menus::Entity::delete_many()
                    .filter(crate::data::sys_permission_menus::Column::PermissionId.eq(req.id))
                    .exec(&self.state.db)
                    .await
                    .map_err(db_err)?;
                let paths = menu_full_paths(
                    &self.state.db,
                    &data.menu_ids.iter().map(|v| *v as i64).collect::<Vec<_>>(),
                )
                .await?;
                for menu_id in &data.menu_ids {
                    crate::data::sys_permission_menus::ActiveModel {
                        permission_id: Set(Some(req.id as i64)),
                        menu_id: Set(Some(*menu_id as i64)),
                        full_path: Set(paths.get(&(*menu_id as i64)).cloned().flatten()),
                        created_at: Set(Some(crate::data::now())),
                        updated_at: Set(Some(crate::data::now())),
                        ..Default::default()
                    }
                    .insert(&self.state.db)
                    .await
                    .map_err(db_err)?;
                }
            }
            if !data.api_ids.is_empty() {
                crate::data::sys_permission_apis::Entity::delete_many()
                    .filter(crate::data::sys_permission_apis::Column::PermissionId.eq(req.id))
                    .exec(&self.state.db)
                    .await
                    .map_err(db_err)?;
                bind_api_rows(&self.state.db, req.id as i64, &data.api_ids).await?;
            }
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePermissionRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::delete_permission_request::QueryBy
        );
        crate::data::sys_role_permissions::Entity::delete_many()
            .filter(crate::data::sys_role_permissions::Column::PermissionId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_permission_menus::Entity::delete_many()
            .filter(crate::data::sys_permission_menus::Column::PermissionId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_permission_apis::Entity::delete_many()
            .filter(crate::data::sys_permission_apis::Column::PermissionId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_permissions::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn sync_permissions(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        self.sync_from_sources(payload.user_id).await?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_code_derivation_covers_all_menu_types() {
        // 路径段：跳过首段（locale 前缀）与参数段（:id），资源单数化，连接符 :
        assert_eq!(menu_code("/sys/users", "用户", "MENU"), "user:view");
        assert_eq!(menu_code("/sys/users/:id", "用户", "MENU"), "user:view");
        // 朴素去复数会把 "sys" 咬成 "sy"（尾 s 剥除无例外表）——钉死真行为
        assert_eq!(menu_code("/app/sys/users", "用户", "MENU"), "sy:user:view");
        // CATALOG → dir；LINK → jump；EMBEDDED → view
        assert_eq!(menu_code("/app/system", "系统", "CATALOG"), "system:dir");
        assert_eq!(
            menu_code("/app/system/links", "外链", "LINK"),
            "system:link:jump"
        );
        assert_eq!(
            menu_code("/app/system/embeds", "嵌入", "EMBEDDED"),
            "system:embed:view"
        );
        // 复数规则：ies → y（categories → category），尾 s 剥除
        assert_eq!(
            menu_code("/app/system/categories", "分类", "MENU"),
            "system:category:view"
        );
        // 未知类型按 view 兜底
        assert_eq!(menu_code("/app/system/x", "x", "GARBAGE"), "system:x:view");
    }

    #[test]
    fn button_action_keyword_rule() {
        // BUTTON 菜单按标题关键词取动作（中英两路）
        assert_eq!(
            menu_code("/app/system/users", "新增用户", "BUTTON"),
            "system:user:create"
        );
        assert_eq!(
            menu_code("/app/system/users", "编辑用户", "BUTTON"),
            "system:user:edit"
        );
        assert_eq!(
            menu_code("/app/system/users", "删除用户", "BUTTON"),
            "system:user:delete"
        );
        assert_eq!(
            menu_code("/app/system/users", "Add User", "BUTTON"),
            "system:user:create"
        );
        assert_eq!(
            menu_code("/app/system/users", "导出", "BUTTON"),
            "system:user:export"
        );
        // 无关键词命中 → act（fallback；上游 fallback 词即 act）
        assert_eq!(
            menu_code("/app/system/users", "奇怪的按钮", "BUTTON"),
            "system:user:act"
        );
    }

    #[test]
    fn api_code_maps_method_to_action_and_strips_params() {
        // /admin/v1 前缀剥除、参数段剥除、资源单数化
        assert_eq!(api_code("GET", "/admin/v1/users"), "user:view");
        assert_eq!(api_code("POST", "/admin/v1/users"), "user:create");
        assert_eq!(api_code("PUT", "/admin/v1/users/{id}"), "user:edit");
        assert_eq!(api_code("DELETE", "/admin/v1/users/{id}"), "user:delete");
        // /list 后缀的 GET 也是 view（不产生 list:list）
        assert_eq!(api_code("GET", "/admin/v1/users/list"), "user:view");
        // 复数资源单数化
        assert_eq!(api_code("GET", "/admin/v1/categories"), "category:view");
        // 空资源 → 纯动作
        assert_eq!(api_code("GET", "/admin/v1/"), "view");
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::data::{
        sys_apis as apis, sys_menus as menus, sys_permission_groups as groups,
        sys_permissions as perms,
    };
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    use crate::proto::gen::services::PermissionServiceHandlers as _;

    async fn plant_menu(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        path: &str,
        menu_type: &str,
        parent: Option<i64>,
        title: &str,
    ) -> menus::Model {
        menus::ActiveModel {
            name: Set(name.into()),
            path: Set(Some(path.into())),
            type_column: Set(Some(menu_type.into())),
            parent_id: Set(parent),
            status: Set(Some("ON".into())),
            meta: Set(Some(serde_json::json!({ "title": title }))),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_api(db: &sea_orm::DatabaseConnection, method: &str, path: &str) -> apis::Model {
        apis::ActiveModel {
            path: Set(Some(path.into())),
            method: Set(Some(method.into())),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_group(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        code: Option<&str>,
    ) -> groups::Model {
        groups::ActiveModel {
            name: Set(name.into()),
            code: Set(code.map(String::from)),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_perm(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        code: &str,
        group_id: Option<i64>,
    ) -> perms::Model {
        perms::ActiveModel {
            name: Set(name.into()),
            code: Set(code.into()),
            group_id: Set(group_id),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn group_map(
        db: &sea_orm::DatabaseConnection,
    ) -> std::collections::HashMap<String, groups::Model> {
        groups::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|g| g.code.clone().map(|c| (c, g)))
            .collect()
    }

    async fn perm_group(db: &sea_orm::DatabaseConnection, code: &str) -> Option<String> {
        let gid = perms::Entity::find()
            .filter(perms::Column::Code.eq(code))
            .one(db)
            .await
            .unwrap()
            .and_then(|p| p.group_id)?;
        groups::Entity::find_by_id(gid)
            .one(db)
            .await
            .unwrap()
            .map(|g| g.name)
    }

    /// 组 MERGE + 归类修复 + 存量收敛（issue #9a/b/c）：
    ///   * 组按 code 自然键 MERGE——id 跨同步稳定、组名取 meta.title 中文；
    ///   * CATALOG 组成组（层级随父链），菜单权限挂 CATALOG 祖先组，API 权限
    ///     挂码族命中菜单的组，未命中落独立「未分组」；
    ///   * 种子组（g 码 + sys:* 引用）幸存；无引用的 g 码遗留组收敛删除；
    ///     NULL 码手工组永不触碰。
    #[tokio::test]
    async fn sync_merges_groups_and_distributes_permissions() {
        let state = test_state().await;
        let svc = PermissionService {
            state: state.clone(),
        };
        let db = &state.db;
        let me = ctx(0, 1);

        // 菜单树：两个 CATALOG + 三个 MENU。
        let system = plant_menu(db, "system", "/system", "CATALOG", None, "系统管理").await;
        let _user = plant_menu(
            db,
            "user",
            "/system/user",
            "MENU",
            Some(system.id),
            "用户管理",
        )
        .await;
        let _positions = plant_menu(
            db,
            "positions",
            "/system/positions",
            "MENU",
            Some(system.id),
            "岗位",
        )
        .await;
        let dashboard = plant_menu(db, "dashboard", "/dashboard", "CATALOG", None, "仪表盘").await;
        let _console = plant_menu(
            db,
            "console",
            "/dashboard/console",
            "MENU",
            Some(dashboard.id),
            "控制台",
        )
        .await;

        // 存量形态：g 码种子组（被 sys:* 引用）、g 码遗留派生组（无引用）、
        // NULL 码手工组、非派生业务权限（挂遗留组）。
        let seed_group = plant_group(db, "系统管理", Some("g1")).await;
        plant_perm(db, "访问后台", "sys:access_backend", Some(seed_group.id)).await;
        let legacy_group = plant_group(db, "profile", Some("g9")).await;
        plant_perm(db, "遗留权限", "legacy:thing", Some(legacy_group.id)).await;
        plant_group(db, "手工组", None).await;

        // API：一个码族命中菜单（position:create → /system/positions），一个
        // 不命中（access-key:view）。
        plant_api(db, "POST", "/admin/v1/positions").await;
        plant_api(db, "GET", "/admin/v1/access-keys").await;

        svc.sync_permissions(me.clone(), Empty {}).await.unwrap();
        let first = group_map(db).await;
        assert_eq!(
            first.len(),
            4,
            "dashboard+system+ungrouped+手工组: {:?}",
            first.keys()
        );
        assert_eq!(first["dashboard"].name, "仪表盘", "组名取 meta.title");
        assert_eq!(first["system"].name, "系统管理");
        assert_eq!(first["ungrouped"].name, "未分组");
        assert_eq!(first["dashboard"].parent_id, None, "顶层 CATALOG 组无父");

        // 归类：菜单权限挂祖先 CATALOG 组；CATALOG 自身挂本组。权限码沿用
        // menu_code 的既有派生口径（父链直连 + 丢首段——seed 绝对子路径带出
        // 冗余段：/system/user → "system:user:view"，本测试即钉住该真值）；
        // 两个顶层 CATALOG 同落 "dir" 码，派生集同码去重首见胜出（system
        // 先种 id 更小 → 组归「系统管理」）。API 权限按码族命中挂同组，
        // 未命中走资源段匹配（position → /system/positions → 系统管理），
        // 仍未命中落「未分组」。
        assert_eq!(perm_group(db, "dir").await.as_deref(), Some("系统管理"));
        assert_eq!(
            perm_group(db, "dashboard:console:view").await.as_deref(),
            Some("仪表盘")
        );
        assert_eq!(
            perm_group(db, "system:user:view").await.as_deref(),
            Some("系统管理")
        );
        assert_eq!(
            perm_group(db, "position:create").await.as_deref(),
            Some("系统管理")
        );
        assert_eq!(
            perm_group(db, "access-key:view").await.as_deref(),
            Some("未分组")
        );

        // 存量收敛：遗留 g9 组随其权限删除而归零；种子组（sys:* 引用）与
        // 手工组幸存。
        assert!(groups::Entity::find_by_id(legacy_group.id)
            .one(db)
            .await
            .unwrap()
            .is_none());
        assert!(groups::Entity::find_by_id(seed_group.id)
            .one(db)
            .await
            .unwrap()
            .is_some());
        assert!(perms::Entity::find()
            .filter(perms::Column::Code.eq("legacy:thing"))
            .one(db)
            .await
            .unwrap()
            .is_none());
        assert!(perms::Entity::find()
            .filter(perms::Column::Code.eq("sys:access_backend"))
            .one(db)
            .await
            .unwrap()
            .is_some());
        assert!(
            !group_map(db).await.contains_key("手工组"),
            "手工组 NULL 码不入 code 索引（行本身幸存，见下）"
        );
        assert!(groups::Entity::find()
            .filter(groups::Column::Name.eq("手工组"))
            .one(db)
            .await
            .unwrap()
            .is_some());

        // 二次同步：组 id 稳定（MERGE 保 id），行数不再增长。
        svc.sync_permissions(me, Empty {}).await.unwrap();
        let second = group_map(db).await;
        assert_eq!(
            first["dashboard"].id, second["dashboard"].id,
            "组 id 跨同步稳定"
        );
        assert_eq!(first["system"].id, second["system"].id);
        assert_eq!(second.len(), first.len(), "二次同步不新增组");
    }

    /// permission_protos 批量面（issue #9d）：绑定与组名走三次 IN 查询，
    /// 行集形状与逐行版一致。
    #[tokio::test]
    async fn batched_protos_carry_bindings_and_group_names() {
        let state = test_state().await;
        let db = &state.db;
        let g = plant_group(db, "系统管理", Some("sys")).await;
        let perm = plant_perm(db, "用户查看", "user:view", Some(g.id)).await;
        let m = plant_menu(db, "user", "/system/user", "MENU", None, "用户管理").await;
        let a = plant_api(db, "GET", "/admin/v1/users").await;
        crate::data::sys_permission_menus::ActiveModel {
            permission_id: Set(Some(perm.id)),
            menu_id: Set(Some(m.id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        crate::data::sys_permission_apis::ActiveModel {
            permission_id: Set(Some(perm.id)),
            api_id: Set(Some(a.id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let rows = perms::Entity::find().all(db).await.unwrap();
        let protos = permission_protos(&state, rows).await;
        assert_eq!(protos.len(), 1);
        let p = &protos[0];
        assert_eq!(p.menu_ids, vec![m.id as u64]);
        assert_eq!(p.api_ids, vec![a.id as u64]);
        assert_eq!(p.group_name.as_deref(), Some("系统管理"));
    }
}
