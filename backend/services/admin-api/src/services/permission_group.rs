//! PermissionGroupService — //! internal/service/service: permission group CRUD.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    CreatePermissionGroupRequest, DeletePermissionGroupRequest, GetPermissionGroupRequest,
    ListPermissionGroupResponse, PermissionGroup, UpdatePermissionGroupRequest,
};

fn group_proto(r: crate::data::sys_permission_groups::Model) -> PermissionGroup {
    PermissionGroup {
        id: Some(r.id as u64),
        name: Some(r.name),
        path: r.path,
        module: r.module,
        sort_order: r.sort_order,
        status: r.status.as_deref().map(crate::state::status_to_proto),
        description: r.description,
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

pub struct PermissionGroupService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PermissionGroupServiceHandlers for PermissionGroupService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPermissionGroupResponse, StatusError> {
        let repo = crate::data::repos::PermissionGroupRepo::new(&self.state.db);
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
            proto::proto::permission::service::v1::get_permission_group_request::QueryBy
        );
        let repo = crate::data::repos::PermissionGroupRepo::new(&self.state.db);
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
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_permission_groups::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            module: Set(data.module),
            path: Set(data.path),
            parent_id: Set(data.parent_id.map(|v| v as i64)),
            description: Set(data.description),
            status: Set(Some("ON".into())),
            sort_order: Set(data.sort_order.or(Some(0))),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
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
        let repo = crate::data::repos::PermissionGroupRepo::new(&self.state.db);
        let row = repo
            .find(req.id as i64)
            .await?
            .ok_or_else(|| not_found("permission group"))?;
        let mut a: crate::data::sys_permission_groups::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.module {
                a.module = Set(Some(v.clone()));
            }
            if let Some(v) = &data.path {
                a.path = Set(Some(v.clone()));
            }
            if let Some(v) = data.parent_id {
                a.parent_id = Set(Some(v as i64));
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.sort_order {
                a.sort_order = Set(Some(v));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
            }
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
            proto::proto::permission::service::v1::delete_permission_group_request::QueryBy
        );
        let repo = crate::data::repos::PermissionGroupRepo::new(&self.state.db);
        repo.delete_by_id(id as i64).await?;
        Ok(Empty {})
    }
}
