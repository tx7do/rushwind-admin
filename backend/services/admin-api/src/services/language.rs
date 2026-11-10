//! LanguageService — service layer for module

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::dict::service::v1::{
    BatchCreateLanguagesRequest, CreateLanguageRequest, DeleteLanguageRequest, Language,
    ListLanguageResponse, UpdateLanguageRequest,
};
use proto::proto::pagination::PagingRequest;

fn language_proto(r: crate::data::sys_languages::Model) -> Language {
    Language {
        id: Some(r.id as u64),
        language_code: Some(r.language_code),
        language_name: Some(r.language_name),
        native_name: r.native_name,
        is_default: r.is_default,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct LanguageService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::LanguageServiceHandlers for LanguageService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListLanguageResponse, StatusError> {
        let repo = crate::data::repos::LanguageRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListLanguageResponse {
            items: rows.into_iter().map(language_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: proto::proto::dict::service::v1::GetLanguageRequest,
    ) -> Result<Language, StatusError> {
        let repo = crate::data::repos::LanguageRepo::new(&self.state.db);
        let id = match req.query_by {
            Some(proto::proto::dict::service::v1::get_language_request::QueryBy::Id(id)) => id,
            Some(proto::proto::dict::service::v1::get_language_request::QueryBy::Code(code)) => {
                repo.find_id_by_code(&code)
                    .await?
                    .ok_or_else(|| not_found("language"))?
            }
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        };
        Ok(language_proto(repo.get_by_id(id as i64).await?))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateLanguageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::repos::LanguageRepo::new(&self.state.db)
            .insert(crate::data::sys_languages::ActiveModel {
                language_code: Set(data.language_code.unwrap_or_default()),
                language_name: Set(data.language_name.unwrap_or_default()),
                native_name: Set(data.native_name),
                is_default: Set(data.is_default.or(Some(false))),
                is_enabled: Set(data.is_enabled.or(Some(true))),
                sort_order: Set(data.sort_order.or(Some(0))),
                created_by: Set(Some(payload.user_id)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await?;
        Ok(Empty {})
    }

    async fn batch_create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: BatchCreateLanguagesRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::LanguageRepo::new(&self.state.db);
        for data in req.items {
            repo.insert(crate::data::sys_languages::ActiveModel {
                language_code: Set(data.language_code.unwrap_or_default()),
                language_name: Set(data.language_name.unwrap_or_default()),
                native_name: Set(data.native_name),
                is_default: Set(data.is_default.or(Some(false))),
                is_enabled: Set(data.is_enabled.or(Some(true))),
                sort_order: Set(data.sort_order.or(Some(0))),
                created_by: Set(operator_of(&ctx).ok().map(|p| p.user_id)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await?;
        }
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateLanguageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::LanguageRepo::new(&self.state.db);
        let mut a: crate::data::sys_languages::ActiveModel =
            repo.get_by_id(req.id as i64).await?.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.language_name.clone() {
                a.language_name = Set(v);
            }
            if let Some(v) = data.native_name.clone() {
                a.native_name = Set(Some(v));
            }
            if let Some(v) = data.is_default {
                a.is_default = Set(Some(v));
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
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteLanguageRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::dict::service::v1::delete_language_request::QueryBy
        );
        crate::data::repos::LanguageRepo::new(&self.state.db)
            .delete_by_id(id as i64)
            .await?;
        Ok(Empty {})
    }
}
