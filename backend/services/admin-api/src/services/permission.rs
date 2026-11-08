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
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    CreatePermissionRequest, DeletePermissionRequest, GetPermissionRequest, ListPermissionResponse,
    Permission, UpdatePermissionRequest,
};

async fn permission_proto(state: &AppState, r: crate::data::sys_permissions::Model) -> Permission {
    let menu_ids: Vec<u32> = crate::data::sys_permission_menus::Entity::find()
        .filter(crate::data::sys_permission_menus::Column::PermissionId.eq(r.id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.menu_id)
        .collect();
    let api_ids: Vec<u32> = crate::data::sys_permission_apis::Entity::find()
        .filter(crate::data::sys_permission_apis::Column::PermissionId.eq(r.id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.api_id)
        .collect();
    let group_name = match r.group_id {
        Some(gid) => crate::data::sys_permission_groups::Entity::find_by_id(gid)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|g| g.name),
        None => None,
    };
    Permission {
        id: Some(r.id),
        name: Some(r.name),
        code: Some(r.code),
        description: r.description,
        status: r.status.as_deref().map(|s| if s == "OFF" { 0 } else { 1 }),
        group_id: r.group_id,
        group_name,
        menu_ids,
        api_ids,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
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
    permission_id: u32,
    menu: Option<(u32, String)>,
) -> Result<(), StatusError> {
    crate::data::sys_permission_menus::Entity::delete_many()
        .filter(crate::data::sys_permission_menus::Column::PermissionId.eq(permission_id))
        .exec(txn)
        .await
        .map_err(db_err)?;
    if let Some((menu_id, full_path)) = menu {
        crate::data::sys_permission_menus::ActiveModel {
            permission_id: Set(Some(permission_id)),
            menu_id: Set(Some(menu_id)),
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
    permission_id: u32,
    api: &Option<(u32, String, String)>,
) -> Result<(), StatusError> {
    crate::data::sys_permission_apis::Entity::delete_many()
        .filter(crate::data::sys_permission_apis::Column::PermissionId.eq(permission_id))
        .exec(txn)
        .await
        .map_err(db_err)?;
    if let Some((api_id, path, method)) = api {
        crate::data::sys_permission_apis::ActiveModel {
            permission_id: Set(Some(permission_id)),
            api_id: Set(Some(*api_id)),
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
    permission_id: u32,
    api_ids: &[u32],
) -> Result<(), StatusError> {
    for api_id in api_ids {
        let row = crate::data::sys_apis::Entity::find_by_id(*api_id)
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
            api_id: Set(Some(*api_id)),
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

/// 菜单全路径（父链拼接，与同步派生同口径：parent_path + path 直连）。
/// 环路由 32 层上限兜底（与角色继承环走查同参）。
async fn menu_full_path(
    db: &sea_orm::DatabaseConnection,
    menu_id: u32,
) -> Result<Option<String>, StatusError> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = Some(menu_id);
    for _ in 0..32 {
        let Some(mid) = current else { break };
        let Some(row) = crate::data::sys_menus::Entity::find_by_id(mid)
            .one(db)
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };
        parts.push(row.path.clone().unwrap_or_default());
        current = row.parent_id;
    }
    if current.is_some() {
        return Ok(None); // 环：不做拼接（fail-closed）
    }
    parts.reverse();
    let full = parts.concat();
    Ok((!full.is_empty()).then_some(full))
}

pub struct PermissionService {
    pub state: Arc<AppState>,
}

impl PermissionService {
    /// SyncPermissions: truncate biz permissions + groups, rebuild from
    /// ON menus (codes per menu type; CATALOG → group) and from enabled
    /// apis (code by path, grouped under UncategorizedPermissionGroup
    async fn sync_from_sources(&self, operator_id: u32) -> Result<(), StatusError> {
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

    /// 同步权限点 —— MERGE 化（上游 f0d1029c 同教义，仓内不再有删除重建型
    /// 同步）：派生集（菜单/接口 → 权限码）按自然键 code 三段增量对齐——
    /// 命中→原位更新（**保 id**：sys_role_permissions 角色授权、租户模板副本
    /// 引用全部因此免于漂移）；缺失→插入；库里有、派生集没有的删除并级联
    /// 清理其角色授权/端点/菜单绑定（删除段放最后：中途失败残留的是"本该删
    /// 的旧码"，重跑同步即自愈，好过删多了）。`sys:` 前缀的系统权限是登录
    /// 链路的身份权限（sys:access_backend 等），不在派生集也不参与对齐——
    /// 旧实现的 Truncate 全表会把它们吃掉（同步一次全员掉线），本重写顺带
    /// 根治。分组行无授权绑定引用，保持重建语义。管理端手工停用的权限
    /// status 不被同步触碰（与上游菜单同步 MERGE 同口径）。
    async fn sync_from_sources_in_txn(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        operator_id: u32,
    ) -> Result<(), StatusError> {
        crate::data::sys_permission_groups::Entity::delete_many()
            .exec(txn)
            .await
            .map_err(db_err)?;

        // —— 派生集构造：菜单（ON，路径序）与接口（ON）——
        let menus = crate::data::sys_menus::Entity::find()
            .filter(crate::data::sys_menus::Column::Status.eq("ON"))
            .order_by_asc(crate::data::sys_menus::Column::Id)
            .all(txn)
            .await
            .map_err(db_err)?;

        // parent chain → full path.
        let mut path_by_id: BTreeMap<u32, String> = BTreeMap::new();
        for menu in &menus {
            let parent_path = menu
                .parent_id
                .and_then(|pid| path_by_id.get(&pid).cloned())
                .unwrap_or_default();
            let full = format!("{}{}", parent_path, menu.path.clone().unwrap_or_default());
            path_by_id.insert(menu.id, full);
        }

        // 分组重建（CATALOG 菜单成组；键 = 组码，插入序即路径序）。
        let mut group_ids: BTreeMap<String, u32> = BTreeMap::new();
        let mut group_codes_in_order: Vec<String> = Vec::new();
        for menu in &menus {
            let menu_type = menu.type_column.clone().unwrap_or_default();
            let full_path = path_by_id.get(&menu.id).cloned().unwrap_or_default();
            if full_path.is_empty() || menu_type != "CATALOG" {
                continue;
            }
            let code = menu_code(&full_path, &menu.name, &menu_type);
            let module = full_path
                .split('/')
                .filter(|s| !s.is_empty())
                .nth(1)
                .unwrap_or("sys")
                .to_string();
            let group = crate::data::sys_permission_groups::ActiveModel {
                name: Set(menu.name.clone()),
                module: Set(Some(module)),
                path: Set(Some(format!("/{}/", menu.id))),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(txn)
            .await
            .map_err(db_err)?;
            group_ids.insert(code.clone(), group.id);
            group_codes_in_order.push(code);
        }

        // 派生权限行：(code, name, group_code, menu 绑定, api 绑定快照)。
        // api 权限按前缀挂到菜单权限（同码族），组归属 None；未匹配落
        // 未分组组（重建序第一组，与旧实现同口径）。
        type DerivedPermission = (
            String,
            String,
            Option<String>,
            Option<(u32, String)>,
            Option<(u32, String, String)>,
        );
        let mut derived: Vec<DerivedPermission> = Vec::new();
        let mut menu_code_by_menu: BTreeMap<u32, String> = BTreeMap::new();
        for menu in &menus {
            let menu_type = menu.type_column.clone().unwrap_or_default();
            let full_path = path_by_id.get(&menu.id).cloned().unwrap_or_default();
            if full_path.is_empty() {
                continue;
            }
            let code = menu_code(&full_path, &menu.name, &menu_type);
            let group_code = group_codes_in_order
                .iter()
                .find(|g| full_path.starts_with(g.as_str()))
                .cloned();
            derived.push((
                code.clone(),
                menu.name.clone(),
                group_code,
                Some((menu.id, full_path.clone())),
                None,
            ));
            menu_code_by_menu.insert(menu.id, code);
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
            let group_code = if matched_menu.is_some() {
                None
            } else {
                group_codes_in_order.first().cloned()
            };
            derived.push((
                code.clone(),
                code.clone(),
                group_code,
                None,
                Some((api.id, path, method.to_uppercase())),
            ));
        }

        // —— MERGE 三段 ——
        // 基线：sys: 保留码之外的全部权限（业务面对齐域）。
        let existing = crate::data::sys_permissions::Entity::find()
            .filter(crate::data::sys_permissions::Column::Code.ne(""))
            .all(txn)
            .await
            .map_err(db_err)?
            .into_iter()
            .filter(|r| !r.code.starts_with("sys:"))
            .collect::<Vec<_>>();
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
            let gid = group_code.as_ref().and_then(|g| group_ids.get(g).copied());
            let mut a: crate::data::sys_permissions::ActiveModel = exist.clone().into();
            a.name = Set(name.clone());
            a.group_id = Set(gid);
            a.updated_at = Set(Some(crate::data::now()));
            a.update(txn).await.map_err(db_err)?;
            replace_menu_binding(txn, exist.id, menu_id.clone()).await?;
            replace_api_binding(txn, exist.id, api_binding).await?;
            merge_hit.insert(code.clone());
        }

        // 插入段：派生集有、库没有。
        for (code, name, group_code, menu_id, api_binding) in &derived {
            if merge_hit.contains(code) || existing_by_code.contains_key(code) {
                continue; // 防御：命中必进更新段，保留护栏
            }
            let gid = group_code.as_ref().and_then(|g| group_ids.get(g).copied());
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
            replace_menu_binding(txn, perm.id, menu_id.clone()).await?;
            replace_api_binding(txn, perm.id, api_binding).await?;
        }

        // 删除段：库有、派生集没有 → 删行并级联清角色授权/端点/菜单绑定
        //（授权链上不允许残留指向已删权限行的绑定）。
        let gone_ids: Vec<u32> = existing_by_code
            .iter()
            .filter(|(code, _)| !merge_hit.contains(*code))
            .map(|(_, r)| r.id)
            .collect();
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
        let _ = operator_id;
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
        let mut items = Vec::with_capacity(rows.len());
        for r in rows {
            items.push(permission_proto(&self.state, r).await);
        }
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
            proto::proto::permission::service::v1::get_permission_request::QueryBy
        );
        let row = crate::data::sys_permissions::Entity::find_by_id(id)
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
            group_id: Set(data.group_id),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        for menu_id in &data.menu_ids {
            let full_path = menu_full_path(&self.state.db, *menu_id).await?;
            crate::data::sys_permission_menus::ActiveModel {
                permission_id: Set(Some(perm.id)),
                menu_id: Set(Some(*menu_id)),
                full_path: Set(full_path),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        bind_api_rows(&self.state.db, perm.id, &data.api_ids).await?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdatePermissionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_permissions::Entity::find_by_id(req.id)
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
                a.group_id = Set(Some(v));
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
                for menu_id in &data.menu_ids {
                    let full_path = menu_full_path(&self.state.db, *menu_id).await?;
                    crate::data::sys_permission_menus::ActiveModel {
                        permission_id: Set(Some(req.id)),
                        menu_id: Set(Some(*menu_id)),
                        full_path: Set(full_path),
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
                bind_api_rows(&self.state.db, req.id, &data.api_ids).await?;
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
            proto::proto::permission::service::v1::delete_permission_request::QueryBy
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
        crate::data::sys_permissions::Entity::delete_by_id(id)
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
