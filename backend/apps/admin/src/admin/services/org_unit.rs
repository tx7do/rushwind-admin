//! OrgUnitService — //! internal/service/service: org-unit CRUD with the
//! materialized `path` maintenance (create appends the path segment;
//! reparenting rewrites the subtree's paths; deleting cascades to
//! descendants) and tree-assembly semantics (flat list ordered by
//! sort_order).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::admin::mapping;
use crate::admin::state::{
    db_err, internal_error, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use crate::proto::identity::service::v1::{
    CreateOrgUnitRequest, DeleteOrgUnitRequest, GetOrgUnitRequest, ListOrgUnitResponse, OrgUnit,
    UpdateOrgUnitRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

/// Unknown rows read as DEPARTMENT.
fn org_type_to_proto(s: &str) -> i32 {
    mapping::org_unit_type_of(s).unwrap_or(0)
}

fn org_type_to_str(v: i32) -> String {
    mapping::org_unit_type_str(v).unwrap_or("DEPARTMENT").into()
}

fn org_proto(r: crate::admin::data::sys_org_units::Model) -> OrgUnit {
    OrgUnit {
        id: Some(r.id),
        parent_id: r.parent_id,
        name: Some(r.name),
        code: r.code,
        leader_id: r.leader_id,
        leader_name: None,
        r#type: Some(org_type_to_proto(
            r.type_column.as_deref().unwrap_or("DEPARTMENT"),
        )),
        path: r.path,
        status: r
            .status
            .as_deref()
            .map(crate::admin::state::status_to_proto),
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
        tenant_id: r.tenant_id,
        tenant_name: None,
        contact_user_id: r.contact_user_id,
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
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
        latitude: r.latitude,
        longitude: r.longitude,
        start_at: r.start_at.and_then(crate::admin::state::naive_to_ts),
        end_at: r.end_at.and_then(crate::admin::state::naive_to_ts),
        legal_entity_org_id: r.legal_entity_org_id,
    }
}

pub struct OrgUnitService {
    pub state: Arc<AppState>,
}

impl OrgUnitService {
    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。business_scopes/permission_tags 保留引用同款
    /// nil-branch（空列表清列、非空列表不随 update 落库），只把清列通道
    /// 关进掩码门。
    fn apply_fields(
        a: &mut crate::data::sys_org_units::ActiveModel,
        data: &OrgUnit,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
        crate::set_masked!(opt mask, a, code, data.code, |v: &String| Some(v.clone()));
        crate::set_masked!(opt mask, a, leader_id, data.leader_id, |v: &i64| Some(
            *v
        ));
        crate::set_masked!(opt_named mask, a, type_column, "type", data.r#type, |v: &i32| Some(
            org_type_to_str(*v)
        ));
        crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
        crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(if *v == 0 {
            "OFF".into()
        } else {
            "ON".into()
        }));
        crate::set_masked!(opt mask, a, parent_id, data.parent_id, |v: &i64| if *v == 0 {
            None
        } else {
            Some(*v)
        });
        crate::set_masked!(opt mask, a, path, data.path, |v: &String| Some(v.clone()));
        crate::set_masked!(opt mask, a, external_id, data.external_id, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, is_legal_entity, data.is_legal_entity, |v: &bool| Some(
            *v
        ));
        crate::set_masked!(opt mask, a, registration_number, data.registration_number, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, tax_id, data.tax_id, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, legal_entity_org_id, data.legal_entity_org_id, |v: &i64| Some(
            *v
        ));
        crate::set_masked!(opt mask, a, address, data.address, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, phone, data.phone, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, email, data.email, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, timezone, data.timezone, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, country, data.country, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, latitude, data.latitude, |v: &f64| Some(*v));
        crate::set_masked!(opt mask, a, longitude, data.longitude, |v: &f64| Some(*v));
        crate::set_masked!(opt mask, a, start_at, data.start_at, |v: &pbjson_types::Timestamp| {
            crate::state::ts_to_naive(v)
        });
        crate::set_masked!(opt mask, a, end_at, data.end_at, |v: &pbjson_types::Timestamp| {
            crate::state::ts_to_naive(v)
        });
        crate::set_masked!(opt mask, a, contact_user_id, data.contact_user_id, |v: &i64| Some(
            *v
        ));
        // Absent scope/tag lists clear the columns (the reference's
        // nil-branch; present lists never ride an update)——掩码未选中时
        // 清列通道同样关闭。
        if data.business_scopes.is_empty() && mask.touches("business_scopes") {
            a.business_scopes = Set(None);
        }
        if data.permission_tags.is_empty() && mask.touches("permission_tags") {
            a.permission_tags = Set(None);
        }
    }

    /// setTreePath: the parent's path (a missing parent for a non-root
    /// node fails; a root takes the fresh root prefix).
    async fn build_path(&self, parent_id: Option<i64>, own_id: i64) -> Result<String, StatusError> {
        let repo = crate::admin::data::repos::OrgUnitRepo::new(&self.state.db);
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
impl crate::proto::gen::services::OrgUnitServiceHandlers for OrgUnitService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListOrgUnitResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let repo = crate::admin::data::repos::OrgUnitRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tid, &req).await?;
        Ok(ListOrgUnitResponse {
            items: rows.into_iter().map(org_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetOrgUnitRequest,
    ) -> Result<OrgUnit, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::get_org_unit_request::QueryBy
        );
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::admin::data::repos::OrgUnitRepo::new(&self.state.db);
        let row = repo
            .find_scoped(id, scope)
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
        let data = crate::admin::state::require_data(req.data)?;
        let inserted = crate::admin::data::sys_org_units::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            parent_id: Set(data.parent_id),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code),
            leader_id: Set(data.leader_id),
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
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        // Materialized path maintenance (setTreePath).
        let path = self.build_path(inserted.parent_id, inserted.id).await?;
        let mut a: crate::admin::data::sys_org_units::ActiveModel = inserted.into();
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
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::admin::data::repos::OrgUnitRepo::new(&self.state.db);
        let row = repo
            .find_scoped(req.id, scope)
            .await?
            .ok_or_else(|| not_found("org unit"))?;
        let mut a: crate::admin::data::sys_org_units::ActiveModel = row.into();
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
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。父级未被掩码选中时不动
            // parent，子树 relocate 也不跑（掩码外字段不产生副作用）。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            parent_present = data.parent_id.is_some() && mask.touches("parent_id");
            Self::apply_fields(&mut a, data, &mask);
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
            let mut queue: Vec<(i64, String)> = vec![(updated.id, parent_path)];
            let mut head = 0;
            while head < queue.len() {
                let (id, parent_path) = queue[head].clone();
                head += 1;
                let new_path = compute_tree_path(&parent_path, id);
                let current = repo
                    .find(id)
                    .await
                    .map_err(|_| internal_error("query org unit failed"))?
                    .and_then(|m| m.path);
                if current.as_deref() != Some(new_path.as_str()) {
                    if let Some(row) = repo
                        .find_scoped(id, scope)
                        .await
                        .map_err(|_| internal_error("query org unit failed"))?
                    {
                        let mut a2: crate::admin::data::sys_org_units::ActiveModel = row.into();
                        a2.path = Set(Some(new_path.clone()));
                        a2.update(&self.state.db)
                            .await
                            .map_err(|_| internal_error("update org unit path failed"))?;
                    }
                }
                for child in repo.children_ids(id).await? {
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
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::delete_org_unit_request::QueryBy
        );
        let repo = crate::admin::data::repos::OrgUnitRepo::new(&self.state.db);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_org_units as orgs;
    use crate::proto::gen::services::OrgUnitServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        code: &str,
        remark: &str,
    ) -> orgs::Model {
        orgs::ActiveModel {
            name: Set(name.into()),
            code: Set(Some(code.into())),
            remark: Set(Some(remark.into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> orgs::Model {
        orgs::Entity::find_by_id(id).one(db).await.unwrap().unwrap()
    }

    fn update_req(id: i64, data: OrgUnit, paths: &[&str]) -> UpdateOrgUnitRequest {
        UpdateOrgUnitRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门：掩码只选 name 时，载荷里多带的 code/remark
    /// 不许落列——开放 API 按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = OrgUnitService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "平台一组", "D01", "旧备注").await;

        svc.update(
            me,
            update_req(
                row.id,
                OrgUnit {
                    name: Some("研发一组".into()),
                    code: Some("EVIL".into()),
                    remark: Some("恶意备注".into()),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "研发一组", "masked field lands");
        assert_eq!(
            fresh.code.as_deref(),
            Some("D01"),
            "unmasked code is not writable through the gate"
        );
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧备注"),
            "unmasked remark is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（可空列清值通道）；NOT NULL 列
    /// （name）同样选中时跳过不清；无掩码 None 仍是"不动"。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = OrgUnitService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "平台一组", "D02", "待清备注").await;

        // 无掩码：None 不动列（全量提交语义不回退）。
        svc.update(
            ctx(0, 1),
            UpdateOrgUnitRequest {
                id: row.id,
                data: Some(OrgUnit {
                    name: Some("改名".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "改名");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("待清备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 name + remark、载荷全 None：可空 remark 清列，NOT NULL
        // name 原样保留。
        svc.update(
            ctx(0, 1),
            update_req(row.id, OrgUnit::default(), &["name", "remark"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(fresh.name, "改名", "NOT NULL column never clears");
    }
}
