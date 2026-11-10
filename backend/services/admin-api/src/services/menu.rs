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
        // Upsert mode: rows with ids update; rows without ids insert.
        for data in &req.items {
            match data.id {
                Some(id) if id > 0 => {
                    if let Some(row) = crate::data::sys_menus::Entity::find_by_id(id as i64)
                        .one(&self.state.db)
                        .await
                        .map_err(db_err)?
                    {
                        let mut a: crate::data::sys_menus::ActiveModel = row.into();
                        Self::apply_fields(&mut a, data);
                        crate::stamp_update!(a, payload.user_id);
                        a.update(&self.state.db).await.map_err(db_err)?;
                    }
                }
                _ => {
                    let mut a = crate::data::sys_menus::ActiveModel {
                        name: Set(data.name.clone().unwrap_or_default()),
                        created_by: Set(Some(payload.user_id)),
                        created_at: Set(Some(crate::data::now())),
                        updated_at: Set(Some(crate::data::now())),
                        ..Default::default()
                    };
                    Self::apply_fields(&mut a, data);
                    if data.status.is_none() {
                        a.status = Set(Some("ON".into()));
                    }
                    a.insert(&self.state.db).await.map_err(db_err)?;
                }
            }
        }
        // 显式 id 插入后序列可能落后——对齐一次，防首个 API 插入撞 pkey
        let _ = crate::seed::realign_sequence(&self.state.db, "sys_menus").await;
        Ok(Empty {})
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
