//! InternalMessageCategoryService — tenant-scoped message
//! category CRUD.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::state::{db_err, not_found, operator_of, tenant_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::internal_message::service::v1::{
    delete_internal_message_category_request::QueryBy as DeleteQueryBy,
    get_internal_message_category_request::QueryBy as GetQueryBy,
    GetInternalMessageCategoryRequest, InternalMessageCategory,
    ListInternalMessageCategoryResponse,
};
use proto::proto::pagination::PagingRequest;

fn category_proto(r: crate::data::internal_message_categories::Model) -> InternalMessageCategory {
    InternalMessageCategory {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        name: Some(r.name),
        code: Some(r.code),
        icon_url: r.icon_url,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        tenant_name: None,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct InternalMessageCategoryService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::InternalMessageCategoryServiceHandlers
    for InternalMessageCategoryService
{
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListInternalMessageCategoryResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let repo = crate::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tid, &req).await?;
        Ok(ListInternalMessageCategoryResponse {
            items: rows.into_iter().map(category_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetInternalMessageCategoryRequest,
    ) -> Result<InternalMessageCategory, StatusError> {
        let id = crate::query_by_id!(req.query_by, GetQueryBy);
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let row = repo
            .find_scoped(id as i64, scope)
            .await?
            .ok_or_else(|| not_found("message category"))?;
        Ok(category_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: proto::proto::internal_message::service::v1::CreateInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::internal_message_categories::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code.unwrap_or_default()),
            icon_url: Set(data.icon_url),
            is_enabled: Set(data.is_enabled.or(Some(true))),
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
        req: proto::proto::internal_message::service::v1::UpdateInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let row = repo
            .find_scoped(req.id as i64, scope)
            .await?
            .ok_or_else(|| not_found("message category"))?;
        let mut a: crate::data::internal_message_categories::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.icon_url {
                a.icon_url = Set(Some(v.clone()));
            }
            if let Some(v) = data.is_enabled {
                a.is_enabled = Set(Some(v));
            }
            if let Some(v) = data.sort_order {
                a.sort_order = Set(Some(v));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: proto::proto::internal_message::service::v1::DeleteInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(req.query_by, DeleteQueryBy);
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        repo.delete_scoped(id as i64, scope).await?;
        Ok(Empty {})
    }
}
