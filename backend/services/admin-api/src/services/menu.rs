//! MenuService — service layer:
//! menu CRUD over `sys_menus` plus SyncMenus (upsert the posted list).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    CreateMenuRequest, DeleteMenuRequest, GetMenuRequest, ListMenuResponse, Menu, SyncMenusRequest,
    UpdateMenuRequest,
};

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

fn menu_proto(r: crate::data::sys_menus::Model) -> Menu {
    Menu {
        id: Some(r.id as u64),
        status: r.status.as_deref().map(crate::state::status_to_proto),
        r#type: r.type_column.as_deref().map(type_to_proto),
        path: r.path,
        redirect: r.redirect,
        alias: r.alias,
        name: Some(r.name),
        component: r.component,
        meta: r.meta.as_ref().and_then(menu_meta_from_json),
        module: r.module.as_deref().map(module_to_proto),
        parent_id: r.parent_id.map(|v| v as u64),
        children: Vec::new(),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct MenuService {
    pub state: Arc<AppState>,
}

impl MenuService {
    /// parent 是否落在 node 的子树内（含环防护——脏环数据收敛返回 false，
    /// 交由 collect_subtree_ids 的 visited 语义兜底）。
    async fn is_menu_descendant(&self, node: u64, parent: u64) -> Result<bool, StatusError> {
        let rows = crate::data::sys_menus::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|m| (m.id, m.parent_id))
            .collect::<Vec<_>>();
        Ok(crate::data::collect_subtree_ids(&rows, node as i64).contains(&(parent as i64)))
    }

    fn apply_fields(a: &mut crate::data::sys_menus::ActiveModel, data: &Menu) {
        if let Some(v) = &data.path {
            a.path = Set(Some(v.clone()));
        }
        if let Some(v) = &data.redirect {
            a.redirect = Set(Some(v.clone()));
        }
        if let Some(v) = &data.alias {
            a.alias = Set(Some(v.clone()));
        }
        if let Some(v) = &data.name {
            a.name = Set(v.clone());
        }
        if let Some(v) = &data.component {
            a.component = Set(Some(v.clone()));
        }
        if let Some(v) = data.r#type {
            a.type_column = Set(Some(type_to_str(v)));
        }
        if let Some(v) = data.status {
            a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
        }
        if let Some(v) = data.module {
            a.module = Set(Some(module_to_str(v)));
        }
        if let Some(v) = data.parent_id {
            a.parent_id = Set(if v == 0 { None } else { Some(v as i64) });
        }
        if let Some(meta) = &data.meta {
            a.meta = Set(Some(menu_meta_to_json(meta)));
        }
    }
}

#[async_trait::async_trait]
impl proto::gen::services::MenuServiceHandlers for MenuService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListMenuResponse, StatusError> {
        let repo = crate::data::repos::MenuRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListMenuResponse {
            items: rows.into_iter().map(menu_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetMenuRequest,
    ) -> Result<Menu, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::permission::service::v1::get_menu_request::QueryBy
        );
        let row = crate::data::sys_menus::Entity::find_by_id(id as i64)
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
        let data = crate::state::require_data(req.data)?;
        let mut a = crate::data::sys_menus::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        Self::apply_fields(&mut a, &data);
        if a.status.clone().unwrap().is_none() {
            a.status = Set(Some("ON".into()));
        }
        a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateMenuRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_menus::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("menu"))?;
        let mut a: crate::data::sys_menus::ActiveModel = row.into();
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
            Self::apply_fields(&mut a, data);
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
            proto::proto::permission::service::v1::delete_menu_request::QueryBy
        );
        // 树形删除加固（上游 ce5de829 同语义）：整棵子树收集（visited 防
        // 护，环脏数据收敛不打转）——旧实现只删一层直接子级，孙级成孤儿；
        // 级联（权限绑定随全子树）与删除同事务。
        let rows = crate::data::sys_menus::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|m| (m.id, m.parent_id))
            .collect::<Vec<_>>();
        let ids = crate::data::collect_subtree_ids(&rows, id as i64);
        use sea_orm::TransactionTrait as _;
        let txn = self.state.db.begin().await.map_err(db_err)?;
        crate::data::sys_permission_menus::Entity::delete_many()
            .filter(crate::data::sys_permission_menus::Column::MenuId.is_in(ids.clone()))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::data::sys_menus::Entity::delete_many()
            .filter(crate::data::sys_menus::Column::Id.is_in(ids))
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
        use proto::proto::permission::service::v1::sync_menus_request::Mode;
        // 合同（menu.proto SyncMenusRequest.Mode）：MERGE 按全路径匹配，
        // 已存在则原位更新（保留 ID 与角色授权），缺失才新增；REPLACE 全量
        // 重建。前端固定送 MERGE 且不带 id——按 id upsert 会让每个顶层路由
        // 都走 insert 分支，seed 行旁多出一批「重复又不完全一致」的行
        // （issue #7），children 还被整体丢弃。
        let merge = req.mode == Some(Mode::Merge as i32);

        // 前序拍平（父先于子）：自然键 = 全路径链（前端子路由送相对路径，
        // 库里存绝对路径——join 规则统一成绝对键）。
        let mut flat: Vec<(String, &Menu)> = Vec::new();
        flatten_paths(&req.items, None, &mut flat);

        use sea_orm::TransactionTrait as _;
        let txn = self.state.db.begin().await.map_err(db_err)?;

        let mut key_to_id: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        if merge {
            // MERGE 前置收敛：历史按 id upsert 留下的重复行（同全路径多行）
            // 收敛到最小 id——子级与权限绑定重指到保留行，指向重复行的权限
            // 绑定直接删除（随下次权限同步重建），重复行删除。
            let rows = crate::data::sys_menus::Entity::find()
                .all(&txn)
                .await
                .map_err(db_err)?;
            let mut key_by_id: std::collections::HashMap<i64, String> =
                std::collections::HashMap::new();
            let mut grouped: std::collections::HashMap<&str, Vec<i64>> =
                std::collections::HashMap::new();
            let mut dup_ids: Vec<i64> = Vec::new();
            for row in &rows {
                let Some(path) = row.path.as_deref().map(str::trim).filter(|p| !p.is_empty())
                else {
                    continue;
                };
                let key = match row
                    .parent_id
                    .and_then(|pid| key_by_id.get(&pid))
                    .map(|pk| join_path(pk, path))
                {
                    Some(k) => k,
                    // 孤儿（父缺失）按绝对路径独立成键；同 id 序先父后子的
                    // 插入序（seed/同步都保证）让单趟解析足够。
                    None => normalize_path(path),
                };
                key_by_id.insert(row.id, key);
            }
            for (id, key) in &key_by_id {
                grouped.entry(key.as_str()).or_default().push(*id);
            }
            let mut repoint: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
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
            if !dup_ids.is_empty() {
                // 重复行的子级重指到保留行（重复行只有顶层孤儿形态，循环量
                // 极小；映射重指无法在一条 UPDATE 里表达，逐行即可）。
                for dup in &dup_ids {
                    if let Some(kept) = repoint.get(dup) {
                        crate::data::sys_menus::Entity::update_many()
                            .col_expr(
                                crate::data::sys_menus::Column::ParentId,
                                sea_orm::sea_query::Expr::value(*kept),
                            )
                            .filter(crate::data::sys_menus::Column::ParentId.eq(*dup))
                            .exec(&txn)
                            .await
                            .map_err(db_err)?;
                    }
                }
                crate::data::sys_permission_menus::Entity::delete_many()
                    .filter(
                        crate::data::sys_permission_menus::Column::MenuId.is_in(dup_ids.clone()),
                    )
                    .exec(&txn)
                    .await
                    .map_err(db_err)?;
                crate::data::sys_menus::Entity::delete_many()
                    .filter(crate::data::sys_menus::Column::Id.is_in(dup_ids))
                    .exec(&txn)
                    .await
                    .map_err(db_err)?;
            }
            for (id, key) in key_by_id {
                if !repoint.contains_key(&id) {
                    key_to_id.insert(key, id);
                }
            }
        } else {
            // REPLACE：清空现有菜单后按传入树重建（合同注明 ID 全部变化、
            // 角色-菜单授权失效——绑定随菜单一并清理，不留悬挂引用）。
            crate::data::sys_permission_menus::Entity::delete_many()
                .exec(&txn)
                .await
                .map_err(db_err)?;
            crate::data::sys_menus::Entity::delete_many()
                .exec(&txn)
                .await
                .map_err(db_err)?;
        }

        let mut inserted = 0usize;
        for (key, data) in &flat {
            let parent_id = parent_key(key).and_then(|pk| key_to_id.get(pk).copied());
            match key_to_id.get(key).copied() {
                Some(id) => {
                    // 命中：原位更新保 id（键相等意味着父链一致，不动 parent）。
                    let row = crate::data::sys_menus::Entity::find_by_id(id)
                        .one(&txn)
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| not_found("menu"))?;
                    let mut a: crate::data::sys_menus::ActiveModel = row.into();
                    Self::apply_fields(&mut a, data);
                    crate::stamp_update!(a, payload.user_id);
                    a.update(&txn).await.map_err(db_err)?;
                }
                None => {
                    let mut a = crate::data::sys_menus::ActiveModel {
                        name: Set(data.name.clone().unwrap_or_default()),
                        created_by: Set(Some(payload.user_id)),
                        created_at: Set(Some(crate::data::now())),
                        updated_at: Set(Some(crate::data::now())),
                        ..Default::default()
                    };
                    Self::apply_fields(&mut a, data);
                    // 结构由推送树定义：parent 从键链解析（前序保证父已就位）。
                    a.parent_id = Set(parent_id);
                    if data.status.is_none() {
                        a.status = Set(Some("ON".into()));
                    }
                    let row = a.insert(&txn).await.map_err(db_err)?;
                    key_to_id.insert(key.clone(), row.id);
                    inserted += 1;
                }
            }
        }
        txn.commit().await.map_err(db_err)?;
        // 显式 id 插入后序列可能落后——对齐一次，防首个 API 插入撞 pkey
        if inserted > 0 {
            let _ = crate::seed::realign_sequence(&self.state.db, "sys_menus").await;
        }
        Ok(Empty {})
    }
}

/// 全路径自然键：绝对路径归一（确保前导 `/`），相对路径拼在父键下。
fn normalize_path(path: &str) -> String {
    format!("/{}", path.trim_matches('/'))
}

fn join_path(parent_key: &str, child: &str) -> String {
    if child.starts_with('/') {
        normalize_path(child)
    } else {
        format!(
            "{}/{}",
            parent_key.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

/// `/a/b` → Some(`/a`)；`/a` → None（根）。
fn parent_key(key: &str) -> Option<&str> {
    match key.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => Some(parent),
        _ => None,
    }
}

/// 前序拍平推送树（父先于子）；无 path 的行跳过。
fn flatten_paths<'a>(
    items: &'a [Menu],
    parent_key: Option<&str>,
    out: &mut Vec<(String, &'a Menu)>,
) {
    for item in items {
        let Some(path) = item
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        else {
            continue;
        };
        let key = match parent_key {
            None => normalize_path(path),
            Some(pk) => join_path(pk, path),
        };
        out.push((key.clone(), item));
        if !item.children.is_empty() {
            flatten_paths(&item.children, Some(&key), out);
        }
    }
}

/// entity jsonb (protojson keys) → MenuMeta proto.
pub(crate) fn menu_meta_from_json(
    value: &serde_json::Value,
) -> Option<proto::proto::permission::service::v1::MenuMeta> {
    let obj = value.as_object()?;
    let str_at = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(String::from);
    let bool_at = |k: &str| obj.get(k).and_then(|v| v.as_bool());
    let i32_at = |k: &str| obj.get(k).and_then(|v| v.as_i64()).map(|v| v as i32);
    Some(proto::proto::permission::service::v1::MenuMeta {
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
    meta: &proto::proto::permission::service::v1::MenuMeta,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_menus as menus;
    use crate::testsupport::{ctx, test_state};
    use proto::gen::services::MenuServiceHandlers as _;
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
                meta: Some(proto::proto::permission::service::v1::MenuMeta {
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
                proto::proto::permission::service::v1::sync_menus_request::Mode::Merge as i32,
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
        crate::data::sys_permission_menus::ActiveModel {
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
                    proto::proto::permission::service::v1::sync_menus_request::Mode::Merge as i32,
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
        let bindings = crate::data::sys_permission_menus::Entity::find()
            .filter(crate::data::sys_permission_menus::Column::MenuId.eq(dup.id))
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
        crate::data::sys_permission_menus::ActiveModel {
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
                    proto::proto::permission::service::v1::sync_menus_request::Mode::Replace as i32,
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
            crate::data::sys_permission_menus::Entity::find()
                .filter(crate::data::sys_permission_menus::Column::MenuId.eq(old.id))
                .all(db)
                .await
                .unwrap()
                .is_empty(),
            "stale bindings do not survive a rebuild"
        );
    }
}
