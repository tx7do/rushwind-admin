//! NoticeService — the module's service implementation over the
//! generated `NoticeServiceHandlers` trait: the admin state helpers
//! for identity/errors, the module's own table for module reasons,
//! the exported macros for the shared boilerplate.

use std::sync::Arc;

use crate::admin::data::Viewer;
use crate::admin::state::{db_err, operator_of, tenant_of, AppState, StatusError};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use super::{not_found, status_error};
use crate::modules::template::data::{notices, NoticeRepo};

fn notice_proto(r: notices::Model) -> crate::proto::notice::service::v1::Notice {
    crate::proto::notice::service::v1::Notice {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        title: Some(r.title),
        content: r.content,
        is_published: r.is_published,
        published_at: r.published_at.and_then(crate::admin::state::naive_to_ts),
        sort_order: r.sort_order,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct NoticeService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::template::services::NoticeServiceHandlers for NoticeService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::pagination::PagingRequest,
    ) -> Result<crate::proto::notice::service::v1::ListNoticeResponse, StatusError> {
        // One workspace, one pagination package: the module's
        // PagingRequest IS the admin-contract envelope the shared repo
        // shell's paged list consumes — no transcode, no boundary.
        // Tenancy predicates live in the repo layer.
        let repo = NoticeRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(crate::proto::notice::service::v1::ListNoticeResponse {
            items: rows.into_iter().map(notice_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::notice::service::v1::GetNoticeRequest,
    ) -> Result<crate::proto::notice::service::v1::Notice, StatusError> {
        let repo = NoticeRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .get(req.id as i64)
            .await?
            .ok_or_else(|| not_found("notice"))?;
        Ok(notice_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::notice::service::v1::CreateNoticeRequest,
    ) -> Result<pbjson_types::Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        if data.title.as_ref().map_or(true, |t| t.is_empty()) {
            return Err(status_error("BAD_REQUEST", "title required"));
        }
        let now = crate::admin::data::now();
        notices::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            title: Set(data.title.unwrap_or_default()),
            content: Set(data.content),
            is_published: Set(data.is_published.or(Some(false))),
            published_at: Set(if data.is_published.unwrap_or(false) {
                Some(now)
            } else {
                None
            }),
            sort_order: Set(data.sort_order.or(Some(0))),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(now)),
            updated_at: Set(Some(now)),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(pbjson_types::Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::notice::service::v1::UpdateNoticeRequest,
    ) -> Result<pbjson_types::Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        // Tenant-scoped fetch: the predicate rides the operator's
        // tenant, mirroring the admin tree's update paths.
        let row = notices::Entity::find_by_id(req.id as i64)
            .filter(notices::Column::TenantId.eq(tenant_of(&ctx)))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("notice"))?;
        let was_published = row.is_published.unwrap_or(false);
        let mut a: notices::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.title {
                if v.is_empty() {
                    return Err(status_error("BAD_REQUEST", "title required"));
                }
                a.title = Set(v.clone());
            }
            if let Some(v) = &data.content {
                a.content = Set(Some(v.clone()));
            }
            if let Some(v) = data.is_published {
                a.is_published = Set(Some(v));
                if v && !was_published {
                    a.published_at = Set(Some(crate::admin::data::now()));
                }
            }
            if let Some(v) = data.sort_order {
                a.sort_order = Set(Some(v));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(pbjson_types::Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::notice::service::v1::DeleteNoticeRequest,
    ) -> Result<pbjson_types::Empty, StatusError> {
        // Tenant-scoped fetch, then the hard delete: the viewer
        // predicate rides the repo fetch (the shell deliberately
        // leaves tenant-scoped get/delete hand-written), mirroring the
        // update path above — a cross-tenant id answers 404, not a
        // foreign-row delete.
        let repo = NoticeRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .get(req.id as i64)
            .await?
            .ok_or_else(|| not_found("notice"))?;
        notices::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(pbjson_types::Empty {})
    }
}
