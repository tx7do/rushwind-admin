//! LanguageService — service layer for module

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::admin::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use crate::proto::dict::service::v1::{
    BatchCreateLanguagesRequest, CreateLanguageRequest, DeleteLanguageRequest, Language,
    ListLanguageResponse, UpdateLanguageRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn language_proto(r: crate::admin::data::sys_languages::Model) -> Language {
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
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct LanguageService {
    pub state: Arc<AppState>,
}

impl LanguageService {
    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。language_code 是标准代码锚，更新面不开放。
    fn apply_fields(
        a: &mut crate::data::sys_languages::ActiveModel,
        data: &Language,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(req mask, a, language_name, data.language_name, |v: &String| v
            .clone());
        crate::set_masked!(opt mask, a, native_name, data.native_name, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, is_default, data.is_default, |v: &bool| Some(*v));
        crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(*v));
        crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::LanguageServiceHandlers for LanguageService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListLanguageResponse, StatusError> {
        let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListLanguageResponse {
            items: rows.into_iter().map(language_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::dict::service::v1::GetLanguageRequest,
    ) -> Result<Language, StatusError> {
        let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
        let id = match req.query_by {
            Some(crate::proto::dict::service::v1::get_language_request::QueryBy::Id(id)) => id,
            Some(crate::proto::dict::service::v1::get_language_request::QueryBy::Code(code)) => {
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
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::repos::LanguageRepo::new(&self.state.db)
            .insert(crate::admin::data::sys_languages::ActiveModel {
                language_code: Set(data.language_code.unwrap_or_default()),
                language_name: Set(data.language_name.unwrap_or_default()),
                native_name: Set(data.native_name),
                is_default: Set(data.is_default.or(Some(false))),
                is_enabled: Set(data.is_enabled.or(Some(true))),
                sort_order: Set(data.sort_order.or(Some(0))),
                created_by: Set(Some(payload.user_id)),
                created_at: Set(Some(crate::admin::data::now())),
                updated_at: Set(Some(crate::admin::data::now())),
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
        let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
        for data in req.items {
            repo.insert(crate::admin::data::sys_languages::ActiveModel {
                language_code: Set(data.language_code.unwrap_or_default()),
                language_name: Set(data.language_name.unwrap_or_default()),
                native_name: Set(data.native_name),
                is_default: Set(data.is_default.or(Some(false))),
                is_enabled: Set(data.is_enabled.or(Some(true))),
                sort_order: Set(data.sort_order.or(Some(0))),
                created_by: Set(operator_of(&ctx).ok().map(|p| p.user_id)),
                created_at: Set(Some(crate::admin::data::now())),
                updated_at: Set(Some(crate::admin::data::now())),
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
        let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
        let mut a: crate::admin::data::sys_languages::ActiveModel =
            repo.get_by_id(req.id as i64).await?.into();
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
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteLanguageRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::dict::service::v1::delete_language_request::QueryBy
        );
        crate::admin::data::repos::LanguageRepo::new(&self.state.db)
            .delete_by_id(id as i64)
            .await?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::{ctx, test_state};
    use proto::gen::services::LanguageServiceHandlers as _;
    use proto::proto::dict::service::v1::get_language_request::QueryBy;
    use proto::proto::dict::service::v1::GetLanguageRequest;
    use sea_orm::EntityTrait;

    fn data(code: &str, name: &str) -> Language {
        Language {
            language_code: Some(code.into()),
            language_name: Some(name.into()),
            ..Default::default()
        }
    }

    /// 全局面 CRUD：create 不落租户（global 表）、按 code 寻路、
    /// batch_create 批量进、update 改名。
    #[tokio::test]
    async fn global_crud_and_code_lookup() {
        let state = test_state().await;
        let svc = LanguageService {
            state: state.clone(),
        };

        svc.create(
            ctx(7, 1),
            CreateLanguageRequest {
                data: Some(data("zh-CN", "简体中文")),
            },
        )
        .await
        .unwrap();
        svc.batch_create(
            ctx(0, 1),
            BatchCreateLanguagesRequest {
                items: vec![data("en-US", "English"), data("ja-JP", "日本語")],
            },
        )
        .await
        .unwrap();

        // 全局表：任意租户视角都见全部行。
        let listed = svc.list(ctx(7, 1), PagingRequest::default()).await.unwrap();
        assert_eq!(listed.total, 3);
        let again = svc.list(ctx(9, 2), PagingRequest::default()).await.unwrap();
        assert_eq!(again.total, 3);

        let got = svc
            .get(
                ctx(0, 1),
                GetLanguageRequest {
                    query_by: Some(QueryBy::Code("ja-JP".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(got.language_name, Some("日本語".into()));
        let id = got.id.unwrap();

        svc.update(
            ctx(0, 1),
            UpdateLanguageRequest {
                id,
                data: Some(Language {
                    language_name: Some("日本語（改）".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let got = svc
            .get(
                ctx(0, 1),
                GetLanguageRequest {
                    query_by: Some(QueryBy::Id(id)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(got.language_name, Some("日本語（改）".into()));
        assert_eq!(got.updated_by, Some(1));
    }

    /// get 未知 code → NOT_FOUND 文案；无 query_by → BAD_REQUEST。
    #[tokio::test]
    async fn get_error_mappings() {
        let state = test_state().await;
        let svc = LanguageService { state };

        let err = svc
            .get(
                ctx(0, 1),
                GetLanguageRequest {
                    query_by: Some(QueryBy::Code("xx-XX".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.reason, "NOT_FOUND");
        assert_eq!(err.message, "language not found");

        let err = svc
            .get(ctx(0, 1), GetLanguageRequest::default())
            .await
            .unwrap_err();
        assert_eq!(err.reason, "BAD_REQUEST");
        assert_eq!(err.message, "query_by required");
    }

    /// update_mask 白名单门：掩码只选 language_name 时，载荷多带的
    /// native_name/is_enabled 不许动——按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = LanguageService {
            state: state.clone(),
        };
        let db = &state.db;
        svc.create(
            ctx(0, 1),
            CreateLanguageRequest {
                data: Some(Language {
                    language_code: Some("zh-CN".into()),
                    language_name: Some("简体中文".into()),
                    native_name: Some("简体中文".into()),
                    is_enabled: Some(true),
                    sort_order: Some(2),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = crate::data::sys_languages::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("created row");

        svc.update(
            ctx(0, 1),
            UpdateLanguageRequest {
                id: row.id as u64,
                data: Some(Language {
                    language_name: Some("中文（简体）".into()),
                    native_name: Some(" evil ".into()),
                    is_enabled: Some(false),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["language_name".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fresh = crate::data::sys_languages::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fresh.language_name, "中文（简体）", "masked field lands");
        assert_eq!(
            fresh.native_name.as_deref(),
            Some("简体中文"),
            "unmasked native_name is not writable through the gate"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
        assert_eq!(fresh.sort_order, Some(2), "unmasked sort_order stays put");
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = LanguageService {
            state: state.clone(),
        };
        let db = &state.db;
        svc.create(
            ctx(0, 1),
            CreateLanguageRequest {
                data: Some(Language {
                    language_code: Some("en-US".into()),
                    language_name: Some("English".into()),
                    native_name: Some("English".into()),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = crate::data::sys_languages::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("created row");

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateLanguageRequest {
                id: row.id as u64,
                data: Some(Language {
                    language_name: Some("English".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = crate::data::sys_languages::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fresh.native_name.as_deref(),
            Some("English"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            ctx(0, 1),
            UpdateLanguageRequest {
                id: row.id as u64,
                data: Some(Language::default()),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["native_name".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = crate::data::sys_languages::Entity::find_by_id(row.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            fresh.native_name.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.language_name, "English",
            "req column (language_name None) never cleared by the mask"
        );
    }
}
