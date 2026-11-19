//! DictTypeService — service layer for module

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::data::repos::DictTypeRepo;
use crate::admin::data::Viewer;
use crate::admin::state::{
    db_err, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use crate::proto::dict::service::v1::{
    CreateDictTypeRequest, DeleteDictTypeRequest, DictType, GetDictTypeRequest,
    ListDictTypeResponse, UpdateDictTypeRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn dict_type_proto(r: crate::admin::data::sys_dict_types::Model) -> DictType {
    DictType {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        type_code: Some(r.type_code),
        type_name: Some(r.type_name),
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
        tenant_name: None,
    }
}

pub struct DictTypeService {
    pub state: Arc<AppState>,
}

impl DictTypeService {
    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。type_code 是业务引用锚，更新面不开放。
    fn apply_fields(
        a: &mut crate::data::sys_dict_types::ActiveModel,
        data: &DictType,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(req mask, a, type_name, data.type_name, |v: &String| v.clone());
        crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(*v));
        crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::DictTypeServiceHandlers for DictTypeService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListDictTypeResponse, StatusError> {
        // Listing rides the repo: tenancy predicates live in the data layer.
        let repo = DictTypeRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListDictTypeResponse {
            items: rows.into_iter().map(dict_type_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetDictTypeRequest,
    ) -> Result<DictType, StatusError> {
        let tenant = tenant_of(&ctx);
        let id = match req.query_by {
            Some(crate::proto::dict::service::v1::get_dict_type_request::QueryBy::Id(id)) => {
                id as i64
            }
            Some(crate::proto::dict::service::v1::get_dict_type_request::QueryBy::Code(code)) => {
                crate::admin::data::sys_dict_types::Entity::find()
                    .filter(
                        Condition::all()
                            .add(crate::admin::data::sys_dict_types::Column::TenantId.eq(tenant))
                            .add(crate::admin::data::sys_dict_types::Column::TypeCode.eq(code)),
                    )
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
                    .map(|r| r.id)
                    .ok_or_else(|| not_found("dict type"))?
            }
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        };
        let row = crate::admin::data::sys_dict_types::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("dict type"))?;
        Ok(dict_type_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateDictTypeRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_dict_types::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            type_code: Set(data.type_code.unwrap_or_default()),
            type_name: Set(data.type_name.unwrap_or_default()),
            is_enabled: Set(data.is_enabled.or(Some(true))),
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
        req: UpdateDictTypeRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::sys_dict_types::Entity::find_by_id(req.id as i64)
            .filter(crate::admin::data::sys_dict_types::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("dict type"))?;
        let mut a: crate::admin::data::sys_dict_types::ActiveModel = row.into();
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
        req: DeleteDictTypeRequest,
    ) -> Result<Empty, StatusError> {
        let _ = ctx;
        // Delete cascades the type's entries (batch delete).
        crate::admin::data::sys_dict_entries::Entity::delete_many()
            .filter(crate::admin::data::sys_dict_entries::Column::TypeId.is_in(req.ids.clone()))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_dict_types::Entity::delete_many()
            .filter(crate::admin::data::sys_dict_types::Column::Id.is_in(req.ids))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::{ctx, test_state};
    // QueryBy 的 oneof 子模块（service 文件体里全限定引用的同一形状）。
    use proto::proto::dict::service::v1::get_dict_type_request::QueryBy;
    // handler 方法挂在 trait 上——测试模块里显式引入。
    use proto::gen::services::DictTypeServiceHandlers as _;

    fn data(code: &str, name: &str) -> DictType {
        DictType {
            type_code: Some(code.into()),
            type_name: Some(name.into()),
            ..Default::default()
        }
    }

    /// 租户 CRUD 全链：create 落租户 7 → list 租户隔离 → update 改名
    /// 失效 → delete 级联清空。is_enabled 缺省 true（create 的 or 兜底）。
    #[tokio::test]
    async fn tenant_scoped_crud_chain() {
        let state = test_state().await;
        let svc = DictTypeService {
            state: state.clone(),
        };
        let me = ctx(7, 1);

        svc.create(
            me.clone(),
            CreateDictTypeRequest {
                data: Some(data("app_type", "应用类型")),
            },
        )
        .await
        .unwrap();
        svc.create(
            me.clone(),
            CreateDictTypeRequest {
                data: Some(data("risk_type", "风险类型")),
            },
        )
        .await
        .unwrap();

        // 租户隔离：租户 7 见 2 行，租户 9 见 0 行。
        let mine = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        assert_eq!(mine.total, 2);
        let other = svc.list(ctx(9, 1), PagingRequest::default()).await.unwrap();
        assert_eq!(other.total, 0);

        // 缺省兜底：is_enabled=true、sort_order=0、operator 记账。
        let row = &mine.items[0];
        assert_eq!(row.is_enabled, Some(true));
        assert_eq!(row.sort_order, Some(0));
        assert_eq!(row.created_by, Some(1));
        assert_eq!(row.tenant_id, Some(7));

        // 按 code 寻路走租户谓词。
        let got = svc
            .get(
                me.clone(),
                GetDictTypeRequest {
                    query_by: Some(QueryBy::Code("app_type".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(got.type_name, Some("应用类型".into()));

        // update 改名 + 失效。
        let id = got.id.unwrap();
        svc.update(
            me.clone(),
            UpdateDictTypeRequest {
                id,
                data: Some(DictType {
                    type_name: Some("应用类型改".into()),
                    is_enabled: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let got = svc
            .get(
                me.clone(),
                GetDictTypeRequest {
                    query_by: Some(QueryBy::Id(id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(got.type_name, Some("应用类型改".into()));
        assert_eq!(got.is_enabled, Some(false));
        assert_eq!(got.updated_by, Some(1));

        // delete 后 list 清空。
        svc.delete(me.clone(), DeleteDictTypeRequest { ids: vec![id] })
            .await
            .unwrap();
        let mine = svc.list(me, PagingRequest::default()).await.unwrap();
        assert_eq!(mine.total, 1, "only the untouched row remains");
    }

    /// update 不存在的 id → NOT_FOUND（租户谓词下的字典文案）。
    #[tokio::test]
    async fn update_missing_row_maps_to_not_found() {
        let state = test_state().await;
        let svc = DictTypeService { state };
        let err = svc
            .update(
                ctx(7, 1),
                UpdateDictTypeRequest {
                    id: 4242,
                    data: Some(data("x", "x")),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.reason, "NOT_FOUND");
        assert_eq!(err.message, "dict type not found");
    }

    /// create 缺 data → BAD_REQUEST；无身份 ctx → UNAUTHORIZED。
    #[tokio::test]
    async fn create_guards_request_shape() {
        let state = test_state().await;
        let svc = DictTypeService { state };

        let err = svc
            .create(ctx(7, 1), CreateDictTypeRequest::default())
            .await
            .unwrap_err();
        assert_eq!(err.reason, "BAD_REQUEST");

        let err = svc
            .create(
                rushwind_http_binding::ctx::RequestContext::default(),
                CreateDictTypeRequest {
                    data: Some(data("x", "x")),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.reason, "UNAUTHORIZED");
    }

    /// get 无 query_by → BAD_REQUEST。
    #[tokio::test]
    async fn get_requires_query_by() {
        let state = test_state().await;
        let svc = DictTypeService { state };
        let err = svc
            .get(ctx(7, 1), GetDictTypeRequest::default())
            .await
            .unwrap_err();
        assert_eq!(err.reason, "BAD_REQUEST");
    }

    /// update_mask 白名单门：掩码只选 type_name 时，载荷多带的
    /// is_enabled/sort_order 不许动——按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = DictTypeService {
            state: state.clone(),
        };
        let me = ctx(7, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateDictTypeRequest {
                data: Some(DictType {
                    type_code: Some("app_type".into()),
                    type_name: Some("应用类型".into()),
                    is_enabled: Some(true),
                    sort_order: Some(3),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = crate::data::sys_dict_types::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("created row");

        svc.update(
            me,
            UpdateDictTypeRequest {
                id: row.id as u64,
                data: Some(DictType {
                    type_name: Some("应用类型改".into()),
                    is_enabled: Some(false),
                    sort_order: Some(99),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["type_name".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fresh = crate::data::sys_dict_types::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fresh.type_name, "应用类型改", "masked field lands");
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
        assert_eq!(
            fresh.sort_order,
            Some(3),
            "unmasked sort_order is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = DictTypeService {
            state: state.clone(),
        };
        let me = ctx(7, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateDictTypeRequest {
                data: Some(DictType {
                    type_code: Some("risk_type".into()),
                    type_name: Some("风险类型".into()),
                    is_enabled: Some(false),
                    sort_order: Some(5),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = crate::data::sys_dict_types::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("created row");

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(7, 1),
            UpdateDictTypeRequest {
                id: row.id as u64,
                data: Some(DictType {
                    type_name: Some("风险类型".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = crate::data::sys_dict_types::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fresh.is_enabled,
            Some(false),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            me,
            UpdateDictTypeRequest {
                id: row.id as u64,
                data: Some(DictType::default()),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["is_enabled".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = crate::data::sys_dict_types::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            fresh.is_enabled.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.sort_order,
            Some(5),
            "req column (type_name None) never cleared by the mask"
        );
    }
}
