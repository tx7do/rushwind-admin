//! DictEntryService — service layer for module

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::state::{db_err, not_found, operator_of, tenant_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::dict::service::v1::{
    CreateDictEntryRequest, DeleteDictEntryRequest, DictEntry, ListDictEntryByTypeCodeRequest,
    ListDictEntryByTypeCodeResponse, ListDictEntryResponse, UpdateDictEntryRequest,
};
use proto::proto::pagination::PagingRequest;

fn dict_entry_proto(r: crate::data::sys_dict_entries::Model) -> DictEntry {
    DictEntry {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        type_id: r.type_id.map(|v| v as u64),
        entry_value: r.entry_value,
        numeric_value: r.numeric_value,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
        i18n: std::collections::HashMap::new(),
        current_i18n: None,
        tenant_name: None,
    }
}

pub struct DictEntryService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::DictEntryServiceHandlers for DictEntryService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListDictEntryResponse, StatusError> {
        let _tid = tenant_of(&ctx);
        let repo = crate::data::repos::DictEntryRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListDictEntryResponse {
            items: rows.into_iter().map(dict_entry_proto).collect(),
            total,
        })
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateDictEntryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_dict_entries::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            type_id: Set(data.type_id.map(|v| v as i64)),
            entry_value: Set(data.entry_value),
            numeric_value: Set(data.numeric_value),
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
        req: UpdateDictEntryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_dict_entries::Entity::find_by_id(req.id as i64)
            .filter(crate::data::sys_dict_entries::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("dict entry"))?;
        let mut a: crate::data::sys_dict_entries::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.entry_value.clone() {
                a.entry_value = Set(Some(v));
            }
            if let Some(v) = data.numeric_value {
                a.numeric_value = Set(Some(v));
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
        req: DeleteDictEntryRequest,
    ) -> Result<Empty, StatusError> {
        let _ = ctx;
        // BatchDelete semantics: entries + their i18n rows.
        crate::data::sys_dict_entry_i18n::Entity::delete_many()
            .filter(crate::data::sys_dict_entry_i18n::Column::EntryId.is_in(req.ids.clone()))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_dict_entries::Entity::delete_many()
            .filter(crate::data::sys_dict_entries::Column::Id.is_in(req.ids))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn list_by_type_code(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ListDictEntryByTypeCodeRequest,
    ) -> Result<ListDictEntryByTypeCodeResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let type_row = crate::data::sys_dict_types::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_dict_types::Column::TenantId.eq(tid))
                    .add(crate::data::sys_dict_types::Column::TypeCode.eq(req.type_code.clone())),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)?;
        let Some(type_row) = type_row else {
            return Ok(ListDictEntryByTypeCodeResponse { items: Vec::new() });
        };
        let mut query = crate::data::sys_dict_entries::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_dict_entries::Column::TenantId.eq(tid))
                    .add(crate::data::sys_dict_entries::Column::TypeId.eq(type_row.id))
                    .add(crate::data::sys_dict_entries::Column::IsEnabled.eq(true)),
            )
            .order_by_asc(crate::data::sys_dict_entries::Column::SortOrder);
        let _ = &mut query;
        let rows = query.all(&self.state.db).await.map_err(db_err)?;
        Ok(ListDictEntryByTypeCodeResponse {
            items: rows.into_iter().map(dict_entry_proto).collect(),
        })
    }
}
