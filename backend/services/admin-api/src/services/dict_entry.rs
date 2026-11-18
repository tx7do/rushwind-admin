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

impl DictEntryService {
    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。type_id 在更新面不开放（归属关系建后不改）。
    fn apply_fields(
        a: &mut crate::data::sys_dict_entries::ActiveModel,
        data: &DictEntry,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(opt mask, a, entry_value, data.entry_value, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, numeric_value, data.numeric_value, |v: &i32| Some(
            *v
        ));
        crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(*v));
        crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
    }
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
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            Self::apply_fields(&mut a, data, &mask);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::{ctx, test_state};
    // handler 方法挂在 trait 上——测试模块里显式引入。
    use proto::gen::services::DictEntryServiceHandlers as _;

    async fn row_by_value(
        db: &sea_orm::DatabaseConnection,
        value: &str,
    ) -> crate::data::sys_dict_entries::Model {
        crate::data::sys_dict_entries::Entity::find()
            .filter(crate::data::sys_dict_entries::Column::EntryValue.eq(value))
            .one(db)
            .await
            .unwrap()
            .expect("entry row")
    }

    /// update_mask 白名单门：掩码只选 entry_value 时，载荷多带的
    /// numeric_value/sort_order 不许动——按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = DictEntryService {
            state: state.clone(),
        };
        let me = ctx(7, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateDictEntryRequest {
                data: Some(DictEntry {
                    entry_value: Some("MALE".into()),
                    numeric_value: Some(1),
                    is_enabled: Some(true),
                    sort_order: Some(3),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = row_by_value(db, "MALE").await;

        svc.update(
            me,
            UpdateDictEntryRequest {
                id: row.id as u64,
                data: Some(DictEntry {
                    entry_value: Some("FEMALE".into()),
                    numeric_value: Some(999),
                    sort_order: Some(88),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["entry_value".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fresh = row_by_value(db, "FEMALE").await;
        assert_eq!(
            fresh.entry_value.as_deref(),
            Some("FEMALE"),
            "masked field lands"
        );
        assert_eq!(
            fresh.numeric_value,
            Some(1),
            "unmasked numeric_value is not writable through the gate"
        );
        assert_eq!(
            fresh.sort_order,
            Some(3),
            "unmasked sort_order is not writable through the gate"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = DictEntryService {
            state: state.clone(),
        };
        let me = ctx(7, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateDictEntryRequest {
                data: Some(DictEntry {
                    entry_value: Some("PENDING".into()),
                    numeric_value: Some(7),
                    is_enabled: Some(true),
                    sort_order: Some(2),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = row_by_value(db, "PENDING").await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(7, 1),
            UpdateDictEntryRequest {
                id: row.id as u64,
                data: Some(DictEntry {
                    entry_value: Some("PENDING".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = row_by_value(db, "PENDING").await;
        assert_eq!(
            fresh.numeric_value,
            Some(7),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            me,
            UpdateDictEntryRequest {
                id: row.id as u64,
                data: Some(DictEntry::default()),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["numeric_value".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = row_by_value(db, "PENDING").await;
        assert!(
            fresh.numeric_value.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.sort_order,
            Some(2),
            "unmasked sort_order stays put while clearing"
        );
    }
}
