//! MenuService — service layer:
//! menu CRUD over `sys_menus` plus SyncMenus — the posted route tree merges
//! in REPLACE mode (wipe + rebuild) or MERGE mode (full-path identity
//! merge that keeps ids and role bindings), both inside one transaction.

use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use crate::proto::pagination::PagingRequest;
use crate::proto::permission::service::v1::{
    CreateMenuRequest, DeleteMenuRequest, GetMenuRequest, ListMenuResponse, Menu, SyncMenusRequest,
    UpdateMenuRequest,
};
use pbjson_types::Empty;

/// Unknown rows read as MENU, the wire enum's default variant.
fn type_to_proto(s: &str) -> i32 {
    mapping::menu_type_of(s).unwrap_or(1)
}

fn type_to_str(v: i32) -> String {
    mapping::menu_type_str(v).unwrap_or("MENU").into()
}

/// Unknown rows read as the zero module.
fn module_to_proto(s: &str) -> i32 {
    mapping::menu_module_of(s).unwrap_or(0)
}

/// Unknown rows read as SYSTEM.
fn module_to_str(v: i32) -> String {
    mapping::menu_module_str(v).unwrap_or("SYSTEM").into()
}

/// 组件路径 → 业务模块派生（基座 ComponentToModule 同口径）：连字符归一为
/// 下划线（react 端页面目录用连字符，种子与另两端用下划线），前缀表匹配；
/// `BasicLayout`/空串/未登记前缀落 None——module 列保持 NULL（套餐白名单
/// 对 NULL 放行，不会误过滤）。
fn module_for_component(component: &str) -> Option<String> {
    let c = component.replace('-', "_");
    if c.is_empty() || c == "BasicLayout" {
        return None;
    }
    // PRINT 前缀必须排在 SYSTEM 之前（更长前缀优先，基座同序）。
    const PREFIXES: &[(&str, i32)] = &[
        ("dashboard/", 1),
        ("app/opm/", 2),
        ("app/system/print_template/", 12),
        ("app/system/", 3),
        ("app/dict/", 4),
        ("app/tenant/", 5),
        ("app/permission/", 6),
        ("app/log/", 7),
        ("app/internal_message/", 8),
        ("app/file/", 9),
        ("app/task/", 10),
        ("app/ai/", 11),
    ];
    for (prefix, module) in PREFIXES {
        if c.starts_with(prefix) {
            return Some(module_to_str(*module));
        }
    }
    None
}

fn menu_proto(r: crate::admin::data::sys_menus::Model) -> Menu {
    Menu {
        id: Some(r.id),
        status: r
            .status
            .as_deref()
            .map(crate::admin::state::status_to_proto),
        r#type: r.type_column.as_deref().map(type_to_proto),
        path: r.path,
        redirect: r.redirect,
        alias: r.alias,
        name: Some(r.name),
        component: r.component,
        meta: r.meta.as_ref().and_then(menu_meta_from_json),
        module: r.module.as_deref().map(module_to_proto),
        parent_id: r.parent_id,
        children: Vec::new(),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

pub struct MenuService {
    pub state: Arc<AppState>,
}

impl MenuService {
    /// parent 是否落在 node 的子树内（含环防护——脏环数据收敛返回 false，
    /// 交由 collect_subtree_ids 的 visited 语义兜底）。
    async fn is_menu_descendant(&self, node: i64, parent: i64) -> Result<bool, StatusError> {
        let rows = crate::admin::data::sys_menus::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|m| (m.id, m.parent_id))
            .collect::<Vec<_>>();
        Ok(crate::admin::data::collect_subtree_ids(&rows, node).contains(&{ parent }))
    }

    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。meta 整列写是"顶层 meta 掩码/无掩码"形态；
    /// 点路径合并（update 里）以基线 jsonb 在其后覆写。
    fn apply_fields(
        a: &mut crate::data::sys_menus::ActiveModel,
        data: &Menu,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(opt mask, a, path, data.path, |v: &String| Some(v.clone()));
        crate::set_masked!(opt mask, a, redirect, data.redirect, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, alias, data.alias, |v: &String| Some(v.clone()));
        crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
        crate::set_masked!(opt mask, a, component, data.component, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt_named mask, a, type_column, "type", data.r#type, |v: &i32| Some(
            type_to_str(*v)
        ));
        crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(if *v == 0 {
            "OFF".into()
        } else {
            "ON".into()
        }));
        crate::set_masked!(opt mask, a, module, data.module, |v: &i32| Some(
            module_to_str(*v)
        ));
        crate::set_masked!(opt mask, a, parent_id, data.parent_id, |v: &i64| if *v == 0 {
            None
        } else {
            Some(*v)
        });
        // meta 不走 opt 清列形态：掩码只选中点路径子段（meta.title）时
        // touches("meta") 为真但 selects_whole 为假——载荷缺 meta 是
        // "未携带"（列原样，合并域接管），只有整域选中/无掩码的 None
        // 才显式清列。
        if mask.touches("meta") {
            match &data.meta {
                Some(m) => a.meta = Set(Some(menu_meta_to_json(m))),
                None => {
                    if mask.selects_whole("meta") {
                        a.meta = Set(None);
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::MenuServiceHandlers for MenuService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListMenuResponse, StatusError> {
        let repo = crate::admin::data::repos::MenuRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListMenuResponse {
            items: rows.into_iter().map(menu_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetMenuRequest,
    ) -> Result<Menu, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::get_menu_request::QueryBy
        );
        let row = crate::admin::data::sys_menus::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("menu"))?;
        Ok(menu_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateMenuRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        let mut a = crate::admin::data::sys_menus::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        Self::apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked());
        crate::ensure_default!(a, status, "ON".to_string());
        a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateMenuRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::sys_menus::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("menu"))?;
        // 存量 meta 先取（row 马上被 move 进 ActiveModel）：meta.* 点路径
        // 合并以它为基线。
        let existing_meta = row.meta.clone();
        let mut a: crate::admin::data::sys_menus::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // 环检测前置（树形加固）：父级改到自身或自己的后代即成环，
            // 物化路径/子树级联全部失真——写前拒绝。
            if let Some(parent) = data.parent_id.filter(|v| *v != 0 && *v != req.id) {
                if self.is_menu_descendant(req.id, parent).await? {
                    return Err(status_error(
                        "BAD_REQUEST",
                        "menu parent cannot be moved under its own subtree",
                    ));
                }
            }
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            Self::apply_fields(&mut a, data, &mask);
            // 上游 dcaa4fa9d 同语义：update_mask 的 meta.* 点路径不再整列
            // 替换——选中子字段合并进存量 jsonb（未选中键保留、选中但为空
            // 的键显式写 null 清残值），NULL 列由 {} 兜底。顶层 meta（或无
            // 掩码）保持整列替换语义（apply_fields 已写）；两种形态同现时
            // 以点路径合并后写为准。掩码里 data.meta 缺席 → Go 侧同款早退，
            // meta 原样不动。
            let meta_leaves: Vec<&str> = req
                .update_mask
                .as_ref()
                .map(|m| {
                    m.paths
                        .iter()
                        .filter_map(|p| p.strip_prefix("meta.").filter(|l| !l.is_empty()))
                        .collect()
                })
                .unwrap_or_default();
            if !meta_leaves.is_empty() {
                if let Some(meta) = &data.meta {
                    let base = existing_meta
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({}));
                    a.meta = Set(Some(merge_meta_fields(base, meta, &meta_leaves)));
                }
            }
            crate::stamp_update!(a, payload.user_id);
            a.update(&self.state.db).await.map_err(db_err)?;
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteMenuRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::delete_menu_request::QueryBy
        );
        // 树形删除加固（上游 ce5de829 同语义）：整棵子树收集（visited 防
        // 护，环脏数据收敛不打转）——旧实现只删一层直接子级，孙级成孤儿；
        // 级联（权限绑定随全子树）与删除同事务。
        let rows = crate::admin::data::sys_menus::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|m| (m.id, m.parent_id))
            .collect::<Vec<_>>();
        let ids = crate::admin::data::collect_subtree_ids(&rows, id);
        use sea_orm::TransactionTrait as _;
        let txn = self.state.db.begin().await.map_err(db_err)?;
        crate::admin::data::sys_permission_menus::Entity::delete_many()
            .filter(crate::admin::data::sys_permission_menus::Column::MenuId.is_in(ids.clone()))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_menus::Entity::delete_many()
            .filter(crate::admin::data::sys_menus::Column::Id.is_in(ids))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        txn.commit().await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn sync_menus(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: SyncMenusRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        // 两模式契约（基座同语义）：REPLACE（缺省）= 清空重建（菜单 ID 全变，
        // 角色授权依赖 full_path 快照存活）；MERGE = 按全路径增量合并（保 ID、
        // 不动 status）。两模式均在单个事务内执行。
        let txn = self.state.db.begin().await.map_err(db_err)?;
        if req.mode == Some(1) {
            collapse_duplicate_menu_paths(&txn).await?;
            let existing = build_full_path_index(&txn).await?;
            sync_merge_tree(&txn, &req.items, None, "", &existing, payload.user_id).await?;
        } else {
            // REPLACE 全量重建：角色-菜单授权随菜单一并清空（绑定与菜单
            // 同生命周期，不留悬挂引用——基座 replaceMenuTree 同语义）。
            crate::admin::data::sys_permission_menus::Entity::delete_many()
                .exec(&txn)
                .await
                .map_err(db_err)?;
            crate::admin::data::sys_menus::Entity::delete_many()
                .exec(&txn)
                .await
                .map_err(db_err)?;
            sync_insert_tree(&txn, &req.items, None, payload.user_id).await?;
        }
        txn.commit().await.map_err(db_err)?;
        // 显式 id 插入后序列可能落后——对齐一次，防首个 API 插入撞 pkey
        let _ = crate::admin::seed::realign_sequence(&self.state.db, "sys_menus").await;
        Ok(Empty {})
    }
}

/// 菜单全路径身份键：子节点相对路径拼在父全路径后，绝对路径子节点按绝对
/// 处理，空路径继承父全路径（基座 joinMenuFullPath 同口径）。
fn join_menu_full_path(parent_full_path: &str, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return parent_full_path.to_string();
    }
    if path.starts_with('/') {
        return path.to_string();
    }
    if parent_full_path.is_empty() || parent_full_path == "/" {
        return format!("/{path}");
    }
    format!("{}/{}", parent_full_path.trim_end_matches('/'), path)
}

/// 全路径 → 现有菜单行。加载全部菜单按 parent 链树遍历（visited 防环——
/// 脏环数据下收敛，不打转挂死），全路径按 [`join_menu_full_path`] 派生。
async fn build_full_path_index<C>(
    db: &C,
) -> Result<HashMap<String, crate::admin::data::sys_menus::Model>, StatusError>
where
    C: sea_orm::ConnectionTrait,
{
    use crate::admin::data::sys_menus as menus;
    let rows = menus::Entity::find().all(db).await.map_err(db_err)?;
    let mut children_of: HashMap<i64, Vec<menus::Model>> = HashMap::new();
    let mut roots: Vec<menus::Model> = Vec::new();
    for row in rows {
        match row.parent_id {
            Some(pid) if pid > 0 => children_of.entry(pid).or_default().push(row),
            _ => roots.push(row),
        }
    }
    let mut by_path = HashMap::new();
    // (node, full_path) 栈式 DFS；visited 防环。
    let mut visited: Vec<i64> = Vec::new();
    let mut stack: Vec<(menus::Model, String)> = roots
        .into_iter()
        .map(|r| {
            let fp = join_menu_full_path("", r.path.as_deref().unwrap_or(""));
            (r, fp)
        })
        .collect();
    while let Some((row, full_path)) = stack.pop() {
        if visited.contains(&row.id) {
            continue;
        }
        visited.push(row.id);
        by_path.insert(full_path.clone(), row.clone());
        for child in children_of.remove(&row.id).unwrap_or_default() {
            let fp = join_menu_full_path(&full_path, child.path.as_deref().unwrap_or(""));
            stack.push((child, fp));
        }
    }
    Ok(by_path)
}

/// MERGE 前置收敛：历史按 id upsert 留下的重复行（同全路径同链）收敛到
/// 最小 id——子级重指到保留行，指向重复行的权限绑定直接删除（随下次
/// 权限同步重建），重复行删除。孤儿（父缺失）按自身绝对路径独立成键；
/// 空路径行不参与成键（它继承父键，撞父行会被误删）。
async fn collapse_duplicate_menu_paths<C>(db: &C) -> Result<(), StatusError>
where
    C: sea_orm::ConnectionTrait,
{
    use crate::admin::data::sys_menus as menus;
    use crate::admin::data::sys_permission_menus as permission_menus;
    let rows = menus::Entity::find().all(db).await.map_err(db_err)?;
    // 单趟按 id 序解析全路径键（seed/同步都保证先父后子的插入序，单趟够）。
    let mut key_by_id: HashMap<i64, String> = HashMap::new();
    for row in &rows {
        let Some(path) = row.path.as_deref().map(str::trim).filter(|p| !p.is_empty()) else {
            continue;
        };
        let key = match row.parent_id.and_then(|pid| key_by_id.get(&pid)) {
            Some(pk) => join_menu_full_path(pk, path),
            None => join_menu_full_path("", path),
        };
        key_by_id.insert(row.id, key);
    }
    let mut grouped: HashMap<&str, Vec<i64>> = HashMap::new();
    for (id, key) in &key_by_id {
        grouped.entry(key.as_str()).or_default().push(*id);
    }
    let mut repoint: HashMap<i64, i64> = HashMap::new();
    let mut dup_ids: Vec<i64> = Vec::new();
    for (_key, mut ids) in grouped {
        if ids.len() < 2 {
            continue;
        }
        ids.sort_unstable();
        let (kept, dups) = ids.split_first().expect("len >= 2");
        for dup in dups {
            repoint.insert(*dup, *kept);
            dup_ids.push(*dup);
        }
    }
    if dup_ids.is_empty() {
        return Ok(());
    }
    // 重复行的子级重指到保留行（映射重指无法在一条 UPDATE 里表达，逐行即可）。
    for dup in &dup_ids {
        if let Some(kept) = repoint.get(dup) {
            menus::Entity::update_many()
                .col_expr(
                    menus::Column::ParentId,
                    sea_orm::sea_query::Expr::value(*kept),
                )
                .filter(menus::Column::ParentId.eq(*dup))
                .exec(db)
                .await
                .map_err(db_err)?;
        }
    }
    permission_menus::Entity::delete_many()
        .filter(permission_menus::Column::MenuId.is_in(dup_ids.clone()))
        .exec(db)
        .await
        .map_err(db_err)?;
    menus::Entity::delete_many()
        .filter(menus::Column::Id.is_in(dup_ids))
        .exec(db)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// REPLACE 模式的树插入：先插父节点拿 ID，再递归子节点。
async fn sync_insert_tree<C>(
    db: &C,
    items: &[Menu],
    parent_id: Option<i64>,
    operator_id: i64,
) -> Result<(), StatusError>
where
    C: sea_orm::ConnectionTrait,
{
    for data in items {
        let mut a = crate::admin::data::sys_menus::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            parent_id: Set(parent_id),
            module: Set(module_for_component(
                data.component.as_deref().unwrap_or(""),
            )),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        MenuService::apply_fields(&mut a, data, &crate::fieldmask::FieldMask::unmasked());
        a.parent_id = Set(parent_id); // apply_fields 只在 Some 时写；树形 parent 以递归链为准
        a.module = Set(module_for_component(
            data.component.as_deref().unwrap_or(""),
        ));
        if data.status.is_none() {
            a.status = Set(Some("ON".into()));
        }
        let inserted = a.insert(db).await.map_err(db_err)?;
        if !data.children.is_empty() {
            Box::pin(sync_insert_tree(
                db,
                &data.children,
                Some(inserted.id),
                operator_id,
            ))
            .await?;
        }
    }
    Ok(())
}

/// MERGE 模式的增量合并：按全路径匹配——已存在则原位更新（保 ID；
/// 无掩码全量提交语义，字段缺省不覆盖），缺失才新增。
#[allow(clippy::too_many_arguments)]
async fn sync_merge_tree<C>(
    db: &C,
    items: &[Menu],
    parent_id: Option<i64>,
    parent_full_path: &str,
    existing: &HashMap<String, crate::admin::data::sys_menus::Model>,
    operator_id: i64,
) -> Result<(), StatusError>
where
    C: sea_orm::ConnectionTrait,
{
    for data in items {
        let full_path = join_menu_full_path(parent_full_path, data.path.as_deref().unwrap_or(""));
        if full_path.is_empty() {
            continue;
        }
        let row_id = match existing.get(&full_path) {
            Some(row) => {
                // 命中：原位更新保 id（无掩码 apply_fields 全写——payload 缺
                // 省的字段不动列）；结构（parent/module）以推送树为准覆写。
                let mut a: crate::admin::data::sys_menus::ActiveModel = row.clone().into();
                MenuService::apply_fields(&mut a, data, &crate::fieldmask::FieldMask::unmasked());
                a.parent_id = Set(parent_id);
                a.module = Set(module_for_component(
                    data.component.as_deref().unwrap_or(""),
                ));
                crate::stamp_update!(a, operator_id);
                a.update(db).await.map_err(db_err)?;
                row.id
            }
            None => {
                let mut a = crate::admin::data::sys_menus::ActiveModel {
                    name: Set(data.name.clone().unwrap_or_default()),
                    parent_id: Set(parent_id),
                    module: Set(module_for_component(
                        data.component.as_deref().unwrap_or(""),
                    )),
                    created_by: Set(Some(operator_id)),
                    created_at: Set(Some(crate::admin::data::now())),
                    updated_at: Set(Some(crate::admin::data::now())),
                    ..Default::default()
                };
                MenuService::apply_fields(&mut a, data, &crate::fieldmask::FieldMask::unmasked());
                a.parent_id = Set(parent_id);
                a.module = Set(module_for_component(
                    data.component.as_deref().unwrap_or(""),
                ));
                if data.status.is_none() {
                    a.status = Set(Some("ON".into()));
                }
                let inserted = a.insert(db).await.map_err(db_err)?;
                inserted.id
            }
        };
        if !data.children.is_empty() {
            Box::pin(sync_merge_tree(
                db,
                &data.children,
                Some(row_id),
                &full_path,
                existing,
                operator_id,
            ))
            .await?;
        }
    }
    Ok(())
}

/// entity jsonb (protojson keys) → MenuMeta proto.
pub(crate) fn menu_meta_from_json(
    value: &serde_json::Value,
) -> Option<crate::proto::permission::service::v1::MenuMeta> {
    let obj = value.as_object()?;
    let str_at = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(String::from);
    let bool_at = |k: &str| obj.get(k).and_then(|v| v.as_bool());
    let i32_at = |k: &str| obj.get(k).and_then(|v| v.as_i64()).map(|v| v as i32);
    Some(crate::proto::permission::service::v1::MenuMeta {
        active_icon: str_at("activeIcon"),
        active_path: str_at("activePath"),
        affix_tab: bool_at("affixTab"),
        affix_tab_order: i32_at("affixTabOrder"),
        authority: obj
            .get("authority")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        badge: str_at("badge"),
        badge_type: str_at("badgeType"),
        badge_variants: str_at("badgeVariants"),
        hide_children_in_menu: bool_at("hideChildrenInMenu"),
        hide_in_breadcrumb: bool_at("hideInBreadcrumb"),
        hide_in_menu: bool_at("hideInMenu"),
        hide_in_tab: bool_at("hideInTab"),
        icon: str_at("icon"),
        iframe_src: str_at("iframeSrc"),
        ignore_access: bool_at("ignoreAccess"),
        keep_alive: bool_at("keepAlive"),
        link: str_at("link"),
        loaded: bool_at("loaded"),
        max_num_of_open_tab: i32_at("maxNumOfOpenTab"),
        menu_visible_with_forbidden: bool_at("menuVisibleWithForbidden"),
        open_in_new_window: bool_at("openInNewWindow"),
        order: i32_at("order"),
        title: str_at("title"),
    })
}

/// MenuMeta proto → the jsonb shape (protojson camelCase keys).
pub(crate) fn menu_meta_to_json(
    meta: &crate::proto::permission::service::v1::MenuMeta,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let put_str =
        |obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, v: &Option<String>| {
            if let Some(v) = v {
                obj.insert(key.to_string(), serde_json::Value::String(v.clone()));
            }
        };
    let put_bool =
        |obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, v: Option<bool>| {
            if let Some(v) = v {
                obj.insert(key.to_string(), serde_json::Value::Bool(v));
            }
        };
    let put_i32 =
        |obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, v: Option<i32>| {
            if let Some(v) = v {
                obj.insert(key.to_string(), serde_json::json!(v));
            }
        };
    put_str(&mut obj, "activeIcon", &meta.active_icon);
    put_str(&mut obj, "activePath", &meta.active_path);
    put_bool(&mut obj, "affixTab", meta.affix_tab);
    put_i32(&mut obj, "affixTabOrder", meta.affix_tab_order);
    if !meta.authority.is_empty() {
        obj.insert("authority".into(), serde_json::json!(meta.authority));
    }
    put_str(&mut obj, "badge", &meta.badge);
    put_str(&mut obj, "badgeType", &meta.badge_type);
    put_str(&mut obj, "badgeVariants", &meta.badge_variants);
    put_bool(&mut obj, "hideChildrenInMenu", meta.hide_children_in_menu);
    put_bool(&mut obj, "hideInBreadcrumb", meta.hide_in_breadcrumb);
    put_bool(&mut obj, "hideInMenu", meta.hide_in_menu);
    put_bool(&mut obj, "hideInTab", meta.hide_in_tab);
    put_str(&mut obj, "icon", &meta.icon);
    put_str(&mut obj, "iframeSrc", &meta.iframe_src);
    put_bool(&mut obj, "ignoreAccess", meta.ignore_access);
    put_bool(&mut obj, "keepAlive", meta.keep_alive);
    put_str(&mut obj, "link", &meta.link);
    put_bool(&mut obj, "loaded", meta.loaded);
    put_i32(&mut obj, "maxNumOfOpenTab", meta.max_num_of_open_tab);
    put_bool(
        &mut obj,
        "menuVisibleWithForbidden",
        meta.menu_visible_with_forbidden,
    );
    put_bool(&mut obj, "openInNewWindow", meta.open_in_new_window);
    put_i32(&mut obj, "order", meta.order);
    put_str(&mut obj, "title", &meta.title);
    serde_json::Value::Object(obj)
}

/// update_mask 掩码叶子（snake/camel 都认）→ protojson camelCase 键。
/// protojson 的 FieldMask 路径是 snake 原名，前端 makeUpdateMask 却可能送
/// camel 表单键——归一（去下划线小写）后对表，两边都收敛到同一存储键。
fn meta_field_json_key(field: &str) -> Option<&'static str> {
    const KEYS: &[&str] = &[
        "activeIcon",
        "activePath",
        "affixTab",
        "affixTabOrder",
        "authority",
        "badge",
        "badgeType",
        "badgeVariants",
        "hideChildrenInMenu",
        "hideInBreadcrumb",
        "hideInMenu",
        "hideInTab",
        "icon",
        "iframeSrc",
        "ignoreAccess",
        "keepAlive",
        "link",
        "loaded",
        "maxNumOfOpenTab",
        "menuVisibleWithForbidden",
        "openInNewWindow",
        "order",
        "title",
    ];
    let want = field.trim().replace('_', "").to_ascii_lowercase();
    KEYS.iter()
        .copied()
        .find(|k| k.to_ascii_lowercase() == want)
}

/// update_mask 点路径选中的 meta 子字段合并进存量 jsonb（上游 dcaa4fa9d
/// 同语义）：选中且有值 → 写值；选中但为空 → 显式写 json null（清空
/// 图标/标题不留残值）；未选中的键 → 存量原样保留。base 为 NULL 列时从
/// `{}` 起步——应用层合并天然免疫 SQL `NULL || x` 静默丢失，等价 Go 侧
/// COALESCE(meta,'{}')。
fn merge_meta_fields(
    base: serde_json::Value,
    meta: &crate::proto::permission::service::v1::MenuMeta,
    leaves: &[&str],
) -> serde_json::Value {
    let mut obj = match base {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    let converted = menu_meta_to_json(meta);
    for leaf in leaves {
        let Some(key) = meta_field_json_key(leaf) else {
            continue;
        };
        let value = converted
            .get(key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        obj.insert(key.to_string(), value);
    }
    serde_json::Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::data::sys_menus as menus;
    use crate::proto::gen::services::MenuServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::{EntityTrait, PaginatorTrait};

    fn menu(name: &str, path: &str, catalog: bool, children: Vec<Menu>) -> Menu {
        Menu {
            name: Some(name.into()),
            path: Some(path.into()),
            r#type: Some(mapping::menu_type_of(if catalog { "CATALOG" } else { "MENU" }).unwrap()),
            children,
            ..Default::default()
        }
    }

    async fn row_by_path(db: &sea_orm::DatabaseConnection, path: &str) -> Option<menus::Model> {
        menus::Entity::find()
            .filter(menus::Column::Path.eq(path))
            .one(db)
            .await
            .unwrap()
    }

    async fn total(db: &sea_orm::DatabaseConnection) -> u64 {
        menus::Entity::find().count(db).await.unwrap()
    }

    async fn plant(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        path: &str,
        parent: Option<i64>,
    ) -> menus::Model {
        menus::ActiveModel {
            name: Set(name.into()),
            path: Set(Some(path.into())),
            type_column: Set(Some(if path.split('/').count() > 2 {
                "MENU".into()
            } else {
                "CATALOG".into()
            })),
            parent_id: Set(parent),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn set_meta(db: &sea_orm::DatabaseConnection, id: i64, meta: serde_json::Value) {
        let row = menus::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let mut a: menus::ActiveModel = row.into();
        a.meta = Set(Some(meta));
        a.update(db).await.unwrap();
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> menus::Model {
        menus::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: i64, data: Menu, paths: &[&str]) -> UpdateMenuRequest {
        UpdateMenuRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// meta.* 点路径 = 子字段合并语义（上游 dcaa4fa9d）：只动掩码选中的
    /// 键，title/keepAlive 等未选中键不被整列替换抹掉。
    #[tokio::test]
    async fn dotted_meta_mask_merges_instead_of_replacing() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        set_meta(
            db,
            row.id,
            serde_json::json!({"title": "仪表盘", "icon": "lucide:home", "keepAlive": true}),
        )
        .await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    meta: Some(crate::proto::permission::service::v1::MenuMeta {
                        icon: Some("lucide:layout-dashboard".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &["meta.icon"],
            ),
        )
        .await
        .unwrap();

        let meta = row_by_id(db, row.id).await.meta.expect("meta present");
        assert_eq!(
            meta.get("icon").and_then(|v| v.as_str()),
            Some("lucide:layout-dashboard"),
            "masked leaf updated"
        );
        assert_eq!(
            meta.get("title").and_then(|v| v.as_str()),
            Some("仪表盘"),
            "unmasked title survives the merge"
        );
        assert_eq!(
            meta.get("keepAlive").and_then(|v| v.as_bool()),
            Some(true),
            "unmasked keepAlive survives the merge"
        );
    }

    /// 掩码选中但值为空 → 显式写 json null（上游 NilValuePaths 语义）：
    /// 清空图标后旧值不残留，读回 proto 时收敛为 None。
    #[tokio::test]
    async fn dotted_meta_mask_writes_null_for_masked_empty_fields() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        set_meta(
            db,
            row.id,
            serde_json::json!({"title": "仪表盘", "icon": "lucide:home"}),
        )
        .await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    meta: Some(crate::proto::permission::service::v1::MenuMeta::default()),
                    ..Default::default()
                },
                &["meta.icon"],
            ),
        )
        .await
        .unwrap();

        let meta = row_by_id(db, row.id).await.meta.expect("meta present");
        assert_eq!(
            meta.get("icon"),
            Some(&serde_json::Value::Null),
            "masked-but-empty leaf written as explicit null"
        );
        assert_eq!(
            meta.get("title").and_then(|v| v.as_str()),
            Some("仪表盘"),
            "unmasked title still survives"
        );
    }

    /// meta 列为 NULL（API 直建/老数据）时点路径合并照常落键——基线从
    /// `{}` 起步，等价上游 COALESCE(meta,'{}')，不再静默丢失。
    #[tokio::test]
    async fn merge_tolerates_null_meta_column() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        // plant 不带 meta → 列为 NULL
        let row = plant(db, "legacy", "/legacy", None).await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    meta: Some(crate::proto::permission::service::v1::MenuMeta {
                        title: Some("新标题".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &["meta.title"],
            ),
        )
        .await
        .unwrap();

        let meta = row_by_id(db, row.id).await.meta.expect("meta present");
        assert_eq!(
            meta,
            serde_json::json!({"title": "新标题"}),
            "NULL column coalesced to an empty object before the merge"
        );
    }

    /// 顶层 meta 掩码（或无掩码）保持整列替换语义——旧行为不回退。
    #[tokio::test]
    async fn top_level_meta_mask_keeps_whole_replace() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        set_meta(
            db,
            row.id,
            serde_json::json!({"title": "旧", "icon": "old:icon"}),
        )
        .await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    meta: Some(crate::proto::permission::service::v1::MenuMeta {
                        title: Some("新".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &["meta"],
            ),
        )
        .await
        .unwrap();

        let meta = row_by_id(db, row.id).await.meta.expect("meta present");
        assert_eq!(
            meta,
            serde_json::json!({"title": "新"}),
            "whole replace: unmentioned icon gone"
        );
    }

    /// 掩码混排标量与 meta 点路径：标量照常落列，meta 子字段照常合并。
    #[tokio::test]
    async fn mixed_scalar_and_dotted_meta_mask_lands_both() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        set_meta(db, row.id, serde_json::json!({"icon": "lucide:home"})).await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    name: Some("工作台".into()),
                    meta: Some(crate::proto::permission::service::v1::MenuMeta {
                        title: Some("工作台标题".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &["name", "meta.title"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "工作台", "scalar field lands");
        let meta = fresh.meta.expect("meta present");
        assert_eq!(
            meta.get("title").and_then(|v| v.as_str()),
            Some("工作台标题"),
            "dotted meta leaf lands"
        );
        assert_eq!(
            meta.get("icon").and_then(|v| v.as_str()),
            Some("lucide:home"),
            "other meta keys preserved"
        );
    }

    /// 掩码带 meta 点路径但 data.meta 缺席 → Go 侧 updateMetaField 同款
    /// 早退：meta 原样不动，不炸不误清。
    #[tokio::test]
    async fn dotted_mask_without_meta_payload_is_noop() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        set_meta(db, row.id, serde_json::json!({"title": "保留"})).await;

        svc.update(me, update_req(row.id, Menu::default(), &["meta.title"]))
            .await
            .unwrap();

        let meta = row_by_id(db, row.id).await.meta.expect("meta present");
        assert_eq!(
            meta.get("title").and_then(|v| v.as_str()),
            Some("保留"),
            "meta untouched when the payload omits it"
        );
    }

    /// MERGE 按全路径匹配（issue #7）：不带 id 的推送树命中库存行原位
    /// 更新保 id，不再旁生重复行；子级递归落位；二次同步幂等。
    #[tokio::test]
    async fn merge_matches_by_full_path_and_stays_idempotent() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let seed_row = plant(db, "dashboard", "/dashboard", None).await;

        let request = SyncMenusRequest {
            items: vec![Menu {
                meta: Some(crate::proto::permission::service::v1::MenuMeta {
                    title: Some("仪表盘".into()),
                    ..Default::default()
                }),
                ..menu(
                    "Dashboard",
                    "/dashboard",
                    true,
                    vec![menu("Console", "/dashboard/console", false, vec![])],
                )
            }],
            mode: Some(
                crate::proto::permission::service::v1::sync_menus_request::Mode::Merge as i32,
            ),
        };
        svc.sync_menus(me.clone(), request.clone()).await.unwrap();

        assert_eq!(
            total(db).await,
            2,
            "matched row updated in place, child inserted"
        );
        let kept = row_by_path(db, "/dashboard").await.expect("row survives");
        assert_eq!(kept.id, seed_row.id, "the seed row keeps its id");
        assert_eq!(
            kept.name, "Dashboard",
            "in-place update applies the pushed name"
        );
        assert_eq!(
            kept.meta
                .as_ref()
                .and_then(|m| m.get("title"))
                .and_then(|t| t.as_str()),
            Some("仪表盘"),
            "meta refreshed from the pushed tree"
        );
        let child = row_by_path(db, "/dashboard/console")
            .await
            .expect("child inserted");
        assert_eq!(
            child.parent_id,
            Some(seed_row.id),
            "child rides the matched parent"
        );

        // 二次同步：命中既有行 + 子级已存在——零新增。
        svc.sync_menus(me, request).await.unwrap();
        assert_eq!(total(db).await, 2, "second sync inserts nothing");
    }

    /// 存量重复行收敛：历史同步留下的同路径多行收敛到最小 id，子级与
    /// 权限绑定重指，重复行删除（不再与 seed 行成对出现）。
    #[tokio::test]
    async fn merge_converges_preexisting_duplicates() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let seed_row = plant(db, "dashboard", "/dashboard", None).await;
        let dup = plant(db, "Dashboard", "/dashboard", None).await;
        let orphan_child = plant(db, "console", "console", Some(dup.id)).await;
        crate::admin::data::sys_permission_menus::ActiveModel {
            menu_id: Set(Some(dup.id)),
            permission_id: Set(Some(9)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        svc.sync_menus(
            me,
            SyncMenusRequest {
                items: vec![menu("Dashboard", "/dashboard", true, vec![])],
                mode: Some(
                    crate::proto::permission::service::v1::sync_menus_request::Mode::Merge as i32,
                ),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            total(db).await,
            2,
            "dup row removed, child repointed survives"
        );
        assert!(row_by_path(db, "/dashboard").await.unwrap().id == seed_row.id);
        let child = menus::Entity::find_by_id(orphan_child.id)
            .one(db)
            .await
            .unwrap()
            .expect("child survives");
        assert_eq!(
            child.parent_id,
            Some(seed_row.id),
            "child repointed to the kept row"
        );
        let bindings = crate::admin::data::sys_permission_menus::Entity::find()
            .filter(crate::admin::data::sys_permission_menus::Column::MenuId.eq(dup.id))
            .all(db)
            .await
            .unwrap();
        assert!(
            bindings.is_empty(),
            "dup-row bindings dropped (rebuilt by permission sync)"
        );
    }

    /// REPLACE 全量重建：菜单与绑定清空、按推送树重插、父链由键解析。
    #[tokio::test]
    async fn replace_rebuilds_the_tree() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let old = plant(db, "legacy", "/legacy", None).await;
        crate::admin::data::sys_permission_menus::ActiveModel {
            menu_id: Set(Some(old.id)),
            permission_id: Set(Some(3)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        svc.sync_menus(
            me,
            SyncMenusRequest {
                items: vec![menu(
                    "Dashboard",
                    "/dashboard",
                    true,
                    vec![menu("Console", "/dashboard/console", false, vec![])],
                )],
                mode: Some(
                    crate::proto::permission::service::v1::sync_menus_request::Mode::Replace as i32,
                ),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            total(db).await,
            2,
            "legacy rows and bindings wiped, tree rebuilt"
        );
        let root = row_by_path(db, "/dashboard").await.expect("root rebuilt");
        let child = row_by_path(db, "/dashboard/console")
            .await
            .expect("child rebuilt");
        assert_eq!(child.parent_id, Some(root.id));
        assert!(
            crate::admin::data::sys_permission_menus::Entity::find()
                .filter(crate::admin::data::sys_permission_menus::Column::MenuId.eq(old.id))
                .all(db)
                .await
                .unwrap()
                .is_empty(),
            "stale bindings do not survive a rebuild"
        );
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 即便带了 path 也不许动——开放 API 按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;

        svc.update(
            me,
            update_req(
                row.id,
                Menu {
                    name: Some("工作台".into()),
                    path: Some("/evil".into()),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "工作台", "masked field lands");
        assert_eq!(
            fresh.path.as_deref(),
            Some("/dashboard"),
            "unmasked path is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = MenuService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "dashboard", "/dashboard", None).await;
        menus::ActiveModel {
            id: Set(row.id),
            alias: Set(Some("/dash".into())),
            ..Default::default()
        }
        .update(db)
        .await
        .unwrap();

        // 无掩码：None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateMenuRequest {
                id: row.id,
                data: Some(Menu {
                    name: Some("dashboard".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.alias.as_deref(),
            Some("/dash"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(me, update_req(row.id, Menu::default(), &["alias"]))
            .await
            .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.alias.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
