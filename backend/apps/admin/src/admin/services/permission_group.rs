//! PermissionGroupService — //! internal/service/service: permission group CRUD.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::admin::state::{db_err, not_found, operator_of, AppState, StatusError};
use crate::proto::pagination::PagingRequest;
use crate::proto::permission::service::v1::{
    CreatePermissionGroupRequest, DeletePermissionGroupRequest, GetPermissionGroupRequest,
    ListPermissionGroupResponse, PermissionGroup, UpdatePermissionGroupRequest,
};
use pbjson_types::Empty;

fn group_proto(r: crate::admin::data::sys_permission_groups::Model) -> PermissionGroup {
    PermissionGroup {
        id: Some(r.id as u64),
        name: Some(r.name),
        path: r.path,
        module: r.module,
        sort_order: r.sort_order,
        status: r
            .status
            .as_deref()
            .map(crate::admin::state::status_to_proto),
        description: r.description,
        parent_id: r.parent_id.map(|v| v as u64),
        children: Vec::new(),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct PermissionGroupService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::PermissionGroupServiceHandlers for PermissionGroupService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPermissionGroupResponse, StatusError> {
        let repo = crate::admin::data::repos::PermissionGroupRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPermissionGroupResponse {
            items: rows.into_iter().map(group_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPermissionGroupRequest,
    ) -> Result<PermissionGroup, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::get_permission_group_request::QueryBy
        );
        let repo = crate::admin::data::repos::PermissionGroupRepo::new(&self.state.db);
        let row = repo
            .find(id as i64)
            .await?
            .ok_or_else(|| not_found("permission group"))?;
        Ok(group_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePermissionGroupRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_permission_groups::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            module: Set(data.module),
            path: Set(data.path),
            parent_id: Set(data.parent_id.map(|v| v as i64)),
            description: Set(data.description),
            status: Set(Some("ON".into())),
            sort_order: Set(data.sort_order.or(Some(0))),
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
        req: UpdatePermissionGroupRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::admin::data::repos::PermissionGroupRepo::new(&self.state.db);
        let row = repo
            .find(req.id as i64)
            .await?
            .ok_or_else(|| not_found("permission group"))?;
        let mut a: crate::admin::data::sys_permission_groups::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
            // 存量 if-let-Some 全量提交语义不变）。name 是 NOT NULL 列走
            // req 形态（None 永不写、无可清语义）；掩码选中 + 载荷 None =
            // 显式清可空列；无掩码 None 不动列。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
            crate::set_masked!(opt mask, a, module, data.module, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, path, data.path, |v: &String| Some(v.clone()));
            crate::set_masked!(opt mask, a, parent_id, data.parent_id, |v: &u64| Some(
                *v as i64
            ));
            crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(
                *v
            ));
            crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(if *v == 0 {
                "OFF".into()
            } else {
                "ON".into()
            }));
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePermissionGroupRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::permission::service::v1::delete_permission_group_request::QueryBy
        );
        let repo = crate::admin::data::repos::PermissionGroupRepo::new(&self.state.db);
        repo.delete_by_id(id as i64).await?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_permission_groups as groups;
    use crate::proto::gen::services::PermissionGroupServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, name: &str, module: &str) -> groups::Model {
        groups::ActiveModel {
            name: Set(name.into()),
            module: Set(Some(module.into())),
            description: Set(Some("原始描述".into())),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> groups::Model {
        groups::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: u64, data: PermissionGroup, paths: &[&str]) -> UpdatePermissionGroupRequest {
        UpdatePermissionGroupRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 即便带了 module 与 description 也不许动——NOT NULL 列同样受门管辖。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = PermissionGroupService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "系统管理", "PERMISSION").await;

        svc.update(
            me,
            update_req(
                row.id as u64,
                PermissionGroup {
                    name: Some("权限新组".into()),
                    module: Some("OPM".into()),
                    description: Some("不该落的描述".into()),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "权限新组", "masked field lands");
        assert_eq!(
            fresh.module.as_deref(),
            Some("PERMISSION"),
            "unmasked module is not writable"
        );
        assert_eq!(
            fresh.description.as_deref(),
            Some("原始描述"),
            "unmasked description is not writable"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清可空列；无掩码时 None 仍是"不动"
    /// （全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = PermissionGroupService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "系统管理", "PERMISSION").await;

        // 无掩码：None 不动列（只改 name，description 原样）。
        svc.update(
            ctx(0, 1),
            UpdatePermissionGroupRequest {
                id: row.id as u64,
                data: Some(PermissionGroup {
                    name: Some("权限新组".into()),
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
            fresh.description.as_deref(),
            Some("原始描述"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            ctx(0, 1),
            update_req(row.id as u64, PermissionGroup::default(), &["description"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.description.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(fresh.name, "权限新组", "req column untouched by None");
    }
}
