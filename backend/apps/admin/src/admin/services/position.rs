//! PositionService — //! internal/service/service: position CRUD over
//! `sys_positions`.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, AppState, StatusError};
use crate::proto::identity::service::v1::{
    CreatePositionRequest, DeletePositionRequest, GetPositionRequest, ListPositionResponse,
    Position, UpdatePositionRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn position_proto(r: crate::admin::data::sys_positions::Model) -> Position {
    Position {
        id: Some(r.id),
        name: Some(r.name),
        code: r.code,
        headcount: r.headcount,
        sort_order: r.sort_order,
        status: r
            .status
            .as_deref()
            .map(crate::admin::state::status_to_proto),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| mapping::position_type_of(s).unwrap_or(0)),
        remark: r.remark,
        description: r.description,
        job_family: r.job_family,
        job_grade: r.job_grade,
        level: r.level,
        is_key_position: r.is_key_position,
        tenant_id: r.tenant_id,
        tenant_name: None,
        org_unit_id: r.org_unit_id,
        org_unit_name: None,
        reports_to_position_id: r.reports_to_position_id,
        reports_to_position_name: None,
        start_at: r.start_at.and_then(crate::admin::state::naive_to_ts),
        end_at: r.end_at.and_then(crate::admin::state::naive_to_ts),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

/// 组织名回填（参照 enrichRelations 的 OrgUnitName 面；best-effort）。
async fn bind_org_unit_names(state: &AppState, items: &mut [Position]) {
    let ids: std::collections::BTreeSet<i64> = items.iter().filter_map(|p| p.org_unit_id).collect();
    if ids.is_empty() {
        return;
    }
    let orgs: std::collections::HashMap<i64, String> =
        crate::admin::data::sys_org_units::Entity::find()
            .filter(crate::admin::data::sys_org_units::Column::Id.is_in(ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|o| (o.id, o.name))
            .collect();
    for p in items {
        if let Some(oid) = p.org_unit_id {
            p.org_unit_name = orgs.get(&oid).cloned();
        }
    }
}

pub struct PositionService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::PositionServiceHandlers for PositionService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPositionResponse, StatusError> {
        let repo = crate::admin::data::repos::PositionRepo::new(
            &self.state.db,
            crate::admin::data::Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        let mut items: Vec<Position> = rows.into_iter().map(position_proto).collect();
        bind_org_unit_names(&self.state, &mut items).await;
        Ok(ListPositionResponse {
            items,
            total: total as i64,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPositionRequest,
    ) -> Result<Position, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::get_position_request::QueryBy
        );
        let row = crate::admin::data::sys_positions::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("position"))?;
        let mut position = position_proto(row);
        bind_org_unit_names(&self.state, std::slice::from_mut(&mut position)).await;
        Ok(position)
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePositionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_positions::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code),
            org_unit_id: Set(data.org_unit_id),
            reports_to_position_id: Set(data.reports_to_position_id),
            description: Set(data.description),
            job_family: Set(data.job_family),
            job_grade: Set(data.job_grade),
            level: Set(data.level),
            headcount: Set(data.headcount.or(Some(0))),
            is_key_position: Set(data.is_key_position.or(Some(false))),
            type_column: Set(Some(
                mapping::position_type_str(data.r#type.unwrap_or(0))
                    .unwrap_or("REGULAR")
                    .to_string(),
            )),
            status: Set(Some("ON".into())),
            sort_order: Set(data.sort_order.or(Some(0))),
            remark: Set(data.remark),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdatePositionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::sys_positions::Entity::find_by_id(req.id)
            .filter(crate::admin::data::sys_positions::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("position"))?;
        let mut a: crate::admin::data::sys_positions::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
            crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, headcount, data.headcount, |v: &u32| Some(*v));
            crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
            crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(if *v == 0 {
                "OFF".into()
            } else {
                "ON".into()
            }));
            crate::set_masked!(opt mask, a, org_unit_id, data.org_unit_id, |v: &i64| Some(
                *v
            ));
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePositionRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::delete_position_request::QueryBy
        );
        crate::admin::data::sys_positions::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_positions as positions;
    use crate::proto::gen::services::PositionServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, name: &str, remark: &str) -> positions::Model {
        positions::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set(name.into()),
            remark: Set(Some(remark.into())),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> positions::Model {
        positions::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: i64, data: Position, paths: &[&str]) -> UpdatePositionRequest {
        UpdatePositionRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门：掩码只选 name 时，载荷里多带的 description
    /// 与 org_unit_id 不许落列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = PositionService {
            state: state.clone(),
        };
        // 同租户视角（payload.tenant_id = 1 的行才可见）。
        let me = ctx(1, 9);
        let db = &state.db;
        let row = plant(db, "招聘专员", "旧说明").await;

        svc.update(
            me,
            update_req(
                row.id,
                Position {
                    name: Some("HR 专员".into()),
                    description: Some("越权说明".into()),
                    org_unit_id: Some(42),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "HR 专员", "masked field lands");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧说明"),
            "unlisted columns keep their values"
        );
        assert_eq!(
            fresh.description, None,
            "unmasked description is not writable through the gate"
        );
        assert_eq!(
            fresh.org_unit_id, None,
            "unmasked org_unit_id is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（可空列清值通道）；NOT NULL 列
    /// （name）同样选中时跳过不清；无掩码 None 仍是"不动"。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = PositionService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "客服", "待清说明").await;

        // 无掩码：None 不动列（全量提交语义不回退）。
        svc.update(
            ctx(1, 9),
            UpdatePositionRequest {
                id: row.id,
                data: Some(Position {
                    name: Some("客服一号".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "客服一号");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("待清说明"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 name + description、载荷全 None：可空 description 清列，
        // NOT NULL name 原样保留。
        svc.update(
            ctx(1, 9),
            update_req(row.id, Position::default(), &["name", "description"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.description.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(fresh.name, "客服一号", "NOT NULL column never clears");
    }

    /// 组织名回填（参照 enrichRelations 的 OrgUnitName 面）——此前恒 None。
    #[tokio::test]
    async fn get_backfills_org_unit_name() {
        let state = test_state().await;
        let svc = PositionService {
            state: state.clone(),
        };
        let db = &state.db;
        let org = crate::admin::data::sys_org_units::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set("岗位归属部".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let row = positions::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set("有归属岗".into()),
            org_unit_id: Set(Some(org.id)),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let p = svc
            .get(
                ctx(1, 9),
                GetPositionRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_position_request::QueryBy::Id(
                            row.id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(p.org_unit_name.as_deref(), Some("岗位归属部"));
    }
}
