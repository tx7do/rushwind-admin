//! InternalMessageCategoryService — tenant-scoped message
//! category CRUD.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::admin::state::{db_err, not_found, operator_of, tenant_of, AppState, StatusError};
use crate::proto::internal_message::service::v1::{
    delete_internal_message_category_request::QueryBy as DeleteQueryBy,
    get_internal_message_category_request::QueryBy as GetQueryBy,
    GetInternalMessageCategoryRequest, InternalMessageCategory,
    ListInternalMessageCategoryResponse,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn category_proto(
    r: crate::admin::data::internal_message_categories::Model,
) -> InternalMessageCategory {
    InternalMessageCategory {
        id: Some(r.id),
        tenant_id: r.tenant_id,
        name: Some(r.name),
        code: Some(r.code),
        icon_url: r.icon_url,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        tenant_name: None,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

pub struct InternalMessageCategoryService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::InternalMessageCategoryServiceHandlers
    for InternalMessageCategoryService
{
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListInternalMessageCategoryResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let repo = crate::admin::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tid, &req).await?;
        Ok(ListInternalMessageCategoryResponse {
            items: rows.into_iter().map(category_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetInternalMessageCategoryRequest,
    ) -> Result<InternalMessageCategory, StatusError> {
        let id = crate::query_by_id!(req.query_by, GetQueryBy);
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::admin::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let row = repo
            .find_scoped(id, scope)
            .await?
            .ok_or_else(|| not_found("message category"))?;
        Ok(category_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::internal_message::service::v1::CreateInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::internal_message_categories::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code.unwrap_or_default()),
            icon_url: Set(data.icon_url),
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
        req: crate::proto::internal_message::service::v1::UpdateInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::admin::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        let row = repo
            .find_scoped(req.id, scope)
            .await?
            .ok_or_else(|| not_found("message category"))?;
        let mut a: crate::admin::data::internal_message_categories::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
            // 存量 if-let-Some 全量提交语义不变）。name 是 NOT NULL 列走
            // req 形态（None 永不写、无可清语义）；掩码选中 + 载荷 None =
            // 显式清可空列；code 是发送/引用锚，更新面不开放（存量如此）。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
            crate::set_masked!(opt mask, a, icon_url, data.icon_url, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(
                *v
            ));
            crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(
                *v
            ));
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::internal_message::service::v1::DeleteInternalMessageCategoryRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(req.query_by, DeleteQueryBy);
        let scope = crate::admin::data::Viewer::from_ctx(&ctx).tenant_scope();
        let repo = crate::admin::data::repos::InternalMessageCategoryRepo::new(&self.state.db);
        repo.delete_scoped(id, scope).await?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::internal_message_categories as categories;
    use crate::proto::gen::services::InternalMessageCategoryServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, tenant: i64, name: &str) -> categories::Model {
        categories::ActiveModel {
            tenant_id: Set(Some(tenant)),
            name: Set(name.into()),
            code: Set("demo".into()),
            icon_url: Set(Some("lucide:inbox".into())),
            is_enabled: Set(Some(true)),
            sort_order: Set(Some(7)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> categories::Model {
        categories::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(
        id: i64,
        data: InternalMessageCategory,
        paths: &[&str],
    ) -> crate::proto::internal_message::service::v1::UpdateInternalMessageCategoryRequest {
        crate::proto::internal_message::service::v1::UpdateInternalMessageCategoryRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            ..Default::default()
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 即便带了 icon_url/sort_order 也不许动——部分提交不再误写未选中列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = InternalMessageCategoryService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "旧分类").await;

        svc.update(
            ctx(1, 9),
            update_req(
                row.id,
                InternalMessageCategory {
                    name: Some("新分类".into()),
                    icon_url: Some("lucide:evil".into()),
                    sort_order: Some(99),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "新分类", "masked name lands");
        assert_eq!(
            fresh.icon_url.as_deref(),
            Some("lucide:inbox"),
            "unmasked icon_url is not writable through the gate"
        );
        assert_eq!(fresh.sort_order, Some(7), "unmasked sort_order untouched");
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = InternalMessageCategoryService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "分类").await;

        // 无掩码：None 不动列。
        svc.update(
            ctx(1, 9),
            crate::proto::internal_message::service::v1::UpdateInternalMessageCategoryRequest {
                id: row.id,
                data: Some(InternalMessageCategory {
                    name: Some("改名".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.icon_url.as_deref(),
            Some("lucide:inbox"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            ctx(1, 9),
            update_req(row.id, InternalMessageCategory::default(), &["icon_url"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.icon_url.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
