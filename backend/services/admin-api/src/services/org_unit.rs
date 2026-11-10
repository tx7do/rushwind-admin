//! OrgUnitService — //! internal/service/service: org-unit CRUD with the
//! materialized `path` maintenance (create appends the path segment;
//! reparenting rewrites the subtree's paths; deleting cascades to
//! descendants) and tree-assembly semantics (flat list ordered by
//! sort_order).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::mapping;
use crate::state::{
    db_err, internal_error, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::{
    CreateOrgUnitRequest, DeleteOrgUnitRequest, GetOrgUnitRequest, ListOrgUnitResponse, OrgUnit,
    UpdateOrgUnitRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as DEPARTMENT.
fn org_type_to_proto(s: &str) -> i32 {
    mapping::org_unit_type_of(s).unwrap_or(0)
}

fn org_type_to_str(v: i32) -> String {
    mapping::org_unit_type_str(v).unwrap_or("DEPARTMENT").into()
}

fn org_proto(r: crate::data::sys_org_units::Model) -> OrgUnit {
    OrgUnit {
        id: Some(r.id as u64),
        parent_id: r.parent_id.map(|v| v as u64),
        name: Some(r.name),
        code: r.code,
        leader_id: r.leader_id.map(|v| v as u64),
        leader_name: None,
        r#type: Some(org_type_to_proto(
            r.type_column.as_deref().unwrap_or("DEPARTMENT"),
        )),
        path: r.path,
        status: r.status.as_deref().map(crate::state::status_to_proto),
        sort_order: r.sort_order,
        business_scopes: r
            .business_scopes
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        external_id: r.external_id,
        is_legal_entity: r.is_legal_entity,
        registration_number: r.registration_number,
        tax_id: r.tax_id,
        address: r.address,
        phone: r.phone,
        email: r.email,
        timezone: r.timezone,
        country: r.country,
        remark: r.remark,
        description: r.description,
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        contact_user_id: r.contact_user_id.map(|v| v as u64),
        contact_user_name: None,
        attributes: std::collections::HashMap::new(),
        permission_tags: r
            .permission_tags
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        children: Vec::new(),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
        latitude: r.latitude,
        longitude: r.longitude,
        start_at: r.start_at.and_then(crate::state::naive_to_ts),
        end_at: r.end_at.and_then(crate::state::naive_to_ts),
        legal_entity_org_id: r.legal_entity_org_id.map(|v| v as u64),
    }
}

pub struct OrgUnitService {
    pub state: Arc<AppState>,
}

impl OrgUnitService {
    /// setTreePath: the parent's path (a missing parent for a non-root
    /// node fails; a root takes the fresh root prefix).
    async fn build_path(&self, parent_id: Option<i64>, own_id: i64) -> Result<String, StatusError> {
        let repo = crate::data::repos::OrgUnitRepo::new(&self.state.db);
        let parent_path = match parent_id.filter(|v| *v != 0) {
            Some(pid) => {
                let parent = repo
                    .find(pid)
                    .await
                    .map_err(|_| internal_error("query parent org unit failed"))?
                    .ok_or_else(|| internal_error("query parent org unit failed"))?;
                parent.path.unwrap_or_default()
            }
            None => String::new(),
        };
        Ok(compute_tree_path(&parent_path, own_id))
    }
}

/// The materialized-path join: roots take "/id/", children append to
/// the parent's path (trailing slash normalized).
fn compute_tree_path(parent_path: &str, node_id: i64) -> String {
    if parent_path.is_empty() {
        return format!("/{node_id}/");
    }
    let mut parent = parent_path.to_string();
    if !parent.ends_with('/') {
        parent.push('/');
    }
    format!("{parent}{node_id}/")
}

#[async_trait::async_trait]
impl proto::gen::services::OrgUnitServiceHandlers for OrgUnitService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListOrgUnitResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let repo = crate::data::repos::OrgUnitRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tid, &req).await?;
        Ok(ListOrgUnitResponse {
            items: rows.into_iter().map(org_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetOrgUnitRequest,
    ) -> Result<OrgUnit, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::get_org_unit_request::QueryBy
        );
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::data::repos::OrgUnitRepo::new(&self.state.db);
        let row = repo
            .find_scoped(id as i64, scope)
            .await?
            .ok_or_else(|| not_found("org unit"))?;
        Ok(org_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateOrgUnitRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        let inserted = crate::data::sys_org_units::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            parent_id: Set(data.parent_id.map(|v| v as i64)),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code),
            leader_id: Set(data.leader_id.map(|v| v as i64)),
            type_column: Set(Some(org_type_to_str(data.r#type.unwrap_or(0)))),
            business_scopes: Set((!data.business_scopes.is_empty()).then(|| {
                serde_json::Value::Array(
                    data.business_scopes
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                )
            })),
            status: Set(Some("ON".into())),
            sort_order: Set(data.sort_order.or(Some(0))),
            remark: Set(data.remark),
            description: Set(data.description),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        // Materialized path maintenance (setTreePath).
        let path = self.build_path(inserted.parent_id, inserted.id).await?;
        let mut a: crate::data::sys_org_units::ActiveModel = inserted.into();
        a.path = Set(Some(path));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateOrgUnitRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::data::repos::OrgUnitRepo::new(&self.state.db);
        let row = repo
            .find_scoped(req.id as i64, scope)
            .await?
            .ok_or_else(|| not_found("org unit"))?;
        let mut a: crate::data::sys_org_units::ActiveModel = row.into();
        let mut parent_present = false;
        if let Some(data) = &req.data {
            // 环检测前置（树形加固）：父级改到自身或自己的后代即成环，
            // 物化路径失真、子树删除打转——写前拒绝。
            if let Some(parent) = data.parent_id.filter(|v| *v != 0 && *v != req.id) {
                if repo.descendant_ids(req.id).await?.contains(&parent) {
                    return Err(status_error(
                        "BAD_REQUEST",
                        "org unit parent cannot be moved under its own subtree",
                    ));
                }
            }
            parent_present = data.parent_id.is_some();
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.code {
                a.code = Set(Some(v.clone()));
            }
            if let Some(v) = &data.remark {
                a.remark = Set(Some(v.clone()));
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.r#type {
                a.type_column = Set(Some(org_type_to_str(v)));
            }
            if let Some(v) = data.sort_order {
                a.sort_order = Set(Some(v));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
            }
            if let Some(v) = data.parent_id {
                a.parent_id = Set(if v == 0 { None } else { Some(v as i64) });
            }
            if let Some(v) = &data.path {
                a.path = Set(Some(v.clone()));
            }
            if let Some(v) = data.leader_id {
                a.leader_id = Set(Some(v as i64));
            }
            if let Some(v) = &data.external_id {
                a.external_id = Set(Some(v.clone()));
            }
            if let Some(v) = data.is_legal_entity {
                a.is_legal_entity = Set(Some(v));
            }
            if let Some(v) = &data.registration_number {
                a.registration_number = Set(Some(v.clone()));
            }
            if let Some(v) = &data.tax_id {
                a.tax_id = Set(Some(v.clone()));
            }
            if let Some(v) = data.legal_entity_org_id {
                a.legal_entity_org_id = Set(Some(v as i64));
            }
            if let Some(v) = &data.address {
                a.address = Set(Some(v.clone()));
            }
            if let Some(v) = &data.phone {
                a.phone = Set(Some(v.clone()));
            }
            if let Some(v) = &data.email {
                a.email = Set(Some(v.clone()));
            }
            if let Some(v) = &data.timezone {
                a.timezone = Set(Some(v.clone()));
            }
            if let Some(v) = &data.country {
                a.country = Set(Some(v.clone()));
            }
            if let Some(v) = data.latitude {
                a.latitude = Set(Some(v));
            }
            if let Some(v) = data.longitude {
                a.longitude = Set(Some(v));
            }
            if let Some(v) = &data.start_at {
                a.start_at = Set(crate::state::ts_to_naive(v));
            }
            if let Some(v) = &data.end_at {
                a.end_at = Set(crate::state::ts_to_naive(v));
            }
            if let Some(v) = data.contact_user_id {
                a.contact_user_id = Set(Some(v as i64));
            }
            // Absent scope/tag lists clear the columns (the reference's
            // nil-branch; present lists never ride an update).
            if data.business_scopes.is_empty() {
                a.business_scopes = Set(None);
            }
            if data.permission_tags.is_empty() {
                a.permission_tags = Set(None);
            }
        }
        crate::stamp_update!(a, payload.user_id);
        let updated = a.update(&self.state.db).await.map_err(db_err)?;

        // relocateSubtree: BFS over the parent links — recompute this
        // node's and every descendant's path (dirty ones self-heal); a
        // move under the node's own descendant rejects, and the path
        // writes ride the viewer scope (cross-tenant rows keep theirs).
        if parent_present {
            let parent_path = match updated.parent_id.filter(|v| *v != 0) {
                Some(pid) => {
                    let parent = repo
                        .find(pid)
                        .await
                        .map_err(|_| internal_error("query parent org unit failed"))?
                        .ok_or_else(|| internal_error("query parent org unit failed"))?;
                    parent.path.unwrap_or_default()
                }
                None => String::new(),
            };
            if parent_path.contains(&format!("/{}/", updated.id)) {
                return Err(status_error(
                    "BAD_REQUEST",
                    "cannot move org unit under its own descendant",
                ));
            }
            let mut queue: Vec<(u64, String)> = vec![(updated.id as u64, parent_path)];
            let mut head = 0;
            while head < queue.len() {
                let (id, parent_path) = queue[head].clone();
                head += 1;
                let new_path = compute_tree_path(&parent_path, id as i64);
                let current = repo
                    .find(id as i64)
                    .await
                    .map_err(|_| internal_error("query org unit failed"))?
                    .and_then(|m| m.path);
                if current.as_deref() != Some(new_path.as_str()) {
                    if let Some(row) = repo
                        .find_scoped(id as i64, scope)
                        .await
                        .map_err(|_| internal_error("query org unit failed"))?
                    {
                        let mut a2: crate::data::sys_org_units::ActiveModel = row.into();
                        a2.path = Set(Some(new_path.clone()));
                        a2.update(&self.state.db)
                            .await
                            .map_err(|_| internal_error("update org unit path failed"))?;
                    }
                }
                for child in repo.children_ids(id as i64).await? {
                    queue.push((child, new_path.clone()));
                }
            }
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteOrgUnitRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_org_unit_request::QueryBy
        );
        let repo = crate::data::repos::OrgUnitRepo::new(&self.state.db);
        // The subtree via the recursive parent-chain walk (root
        // included), then the occupation guard: positions still
        // anchored inside the subtree block the delete.
        let ids = repo.descendant_ids(id).await?;
        let positions = repo.count_positions_in(&ids).await?;
        if positions > 0 {
            return Err(status_error(
                "BAD_REQUEST",
                format!(
                    "exist {positions} positions under the org unit subtree, delete or move them first"
                ),
            ));
        }
        // The delete itself: one transaction, the viewer scope riding
        // the statement (cross-tenant rows inside a mixed subtree
        // survive).
        let txn = {
            use sea_orm::TransactionTrait as _;
            self.state.db.begin()
        }
        .await
        .map_err(|_| internal_error("start transaction failed"))?;
        match repo.delete_ids_scoped(&txn, &ids, scope).await {
            Ok(()) => {
                txn.commit()
                    .await
                    .map_err(|_| internal_error("transaction commit failed"))?;
            }
            Err(e) => {
                let _ = txn.rollback().await;
                return Err(e);
            }
        }
        Ok(Empty {})
    }
}
