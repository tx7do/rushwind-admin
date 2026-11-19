//! ConfigService — service layer:
//! key-value platform config CRUD; built-in rows refuse deletion; the
//! value_type INVALID zero-value is skipped on writes.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use crate::proto::config::service::v1::{
    Config, CreateConfigRequest, DeleteConfigRequest, GetConfigRequest, ListConfigResponse,
    UpdateConfigRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn value_type_to_str(v: i32) -> Option<String> {
    // ConfigValueType: INVALID=0 (skipped), STRING=1, BOOL=2, INT=3.
    match v {
        1 => Some("STRING".into()),
        2 => Some("BOOL".into()),
        3 => Some("INT".into()),
        _ => None,
    }
}

/// Unknown rows read as the string variant (1).
fn value_type_to_proto(s: &str) -> i32 {
    mapping::config_value_type_of(s).unwrap_or(1)
}

fn config_proto(r: crate::admin::data::sys_configs::Model) -> Config {
    Config {
        id: Some(r.id as u64),
        name: Some(r.name),
        key: Some(r.key),
        value: r.value,
        value_type: r.value_type.as_deref().map(value_type_to_proto),
        is_built_in: r.is_built_in,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct ConfigService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::ConfigServiceHandlers for ConfigService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListConfigResponse, StatusError> {
        let repo = crate::admin::data::repos::ConfigRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListConfigResponse {
            items: rows.into_iter().map(config_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetConfigRequest,
    ) -> Result<Config, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::config::service::v1::get_config_request::QueryBy
        );
        let row = crate::admin::data::sys_configs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("config"))?;
        Ok(config_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateConfigRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        let value_type = data.value_type.and_then(value_type_to_str);
        crate::admin::data::sys_configs::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            key: Set(data.key.unwrap_or_default()),
            value: Set(data.value),
            value_type: Set(value_type.or(Some("STRING".into()))),
            is_built_in: Set(data.is_built_in.or(Some(false))),
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
        req: UpdateConfigRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        let key = data.key.clone().unwrap_or_default();
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());

        // allow_missing upsert (upsert semantics).
        let existing = if req.allow_missing == Some(true) && !key.is_empty() {
            crate::admin::data::sys_configs::Entity::find()
                .filter(crate::admin::data::sys_configs::Column::Key.eq(key.clone()))
                .one(&self.state.db)
                .await
                .map_err(db_err)?
        } else {
            crate::admin::data::sys_configs::Entity::find_by_id(req.id as i64)
                .one(&self.state.db)
                .await
                .map_err(db_err)?
        };
        let value_type = data.value_type.and_then(value_type_to_str);
        match existing {
            None if req.allow_missing == Some(true) => {
                // upsert-insert 分支按契约忽略 update_mask（proto 注释同款），
                // 全量落列——门只管存量行的载荷写面。
                crate::data::sys_configs::ActiveModel {
                    name: Set(data.name.unwrap_or_else(|| key.clone())),
                    key: Set(key),
                    value: Set(data.value),
                    value_type: Set(value_type.or(Some("STRING".into()))),
                    is_built_in: Set(Some(false)),
                    created_by: Set(Some(payload.user_id)),
                    created_at: Set(Some(crate::admin::data::now())),
                    updated_at: Set(Some(crate::admin::data::now())),
                    ..Default::default()
                }
                .insert(&self.state.db)
                .await
                .map_err(db_err)?;
            }
            Some(row) => {
                let mut a: crate::data::sys_configs::ActiveModel = row.into();
                // 字段写入过 update_mask 白名单门（字段级更新语义，无掩码即
                // 存量全量提交语义）。key 是读取锚、is_built_in 是内置标记，
                // 更新面都不开放；value_type 的 INVALID(0) 映射 None 不写列
                // （存量跳过语义），掩码显式选中时才随 None 清列。
                crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
                crate::set_masked!(opt mask, a, value, data.value, |v: &String| Some(
                    v.clone()
                ));
                crate::set_masked!(opt mask, a, value_type, value_type, |v: &String| Some(
                    v.clone()
                ));
                crate::stamp_update!(a, payload.user_id);
                a.update(&self.state.db).await.map_err(db_err)?;
            }
            None => return Err(not_found("config")),
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteConfigRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::config::service::v1::delete_config_request::QueryBy
        );
        let row = crate::admin::data::sys_configs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?;
        // Built-in rows refuse deletion; missing rows delete idempotently.
        let Some(row) = row else {
            return Ok(Empty {});
        };
        if row.is_built_in == Some(true) {
            return Err(status_error(
                "FORBIDDEN",
                "built-in config cannot be deleted",
            ));
        }
        crate::admin::data::sys_configs::Entity::delete_by_id(row.id)
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
    // handler 方法挂在 trait 上——测试模块里显式引入。
    use crate::proto::gen::services::ConfigServiceHandlers as _;

    fn data(name: &str, key: &str, value: Option<String>) -> Config {
        Config {
            name: Some(name.into()),
            key: Some(key.into()),
            value,
            value_type: Some(1),
            is_built_in: Some(false),
            ..Default::default()
        }
    }

    async fn first_row(db: &sea_orm::DatabaseConnection) -> crate::data::sys_configs::Model {
        crate::data::sys_configs::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("config row")
    }

    /// update_mask 白名单门：掩码只选 value 时，载荷多带的
    /// name/value_type 不许动——按契约送部分字段不再误写。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = ConfigService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateConfigRequest {
                data: Some(data("站点名", "site.name", Some("admin".into()))),
            },
        )
        .await
        .unwrap();
        let row = first_row(db).await;

        svc.update(
            me,
            UpdateConfigRequest {
                id: row.id as u64,
                data: Some(Config {
                    name: Some("越权改名".into()),
                    value: Some("新值".into()),
                    value_type: Some(3),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["value".into()],
                }),
                allow_missing: None,
            },
        )
        .await
        .unwrap();

        let fresh = first_row(db).await;
        assert_eq!(fresh.value.as_deref(), Some("新值"), "masked field lands");
        assert_eq!(
            fresh.name, "站点名",
            "unmasked name is not writable through the gate"
        );
        assert_eq!(
            fresh.value_type.as_deref(),
            Some("STRING"),
            "unmasked value_type is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = ConfigService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateConfigRequest {
                data: Some(data("站点名", "site.name", Some("旧值".into()))),
            },
        )
        .await
        .unwrap();
        let row = first_row(db).await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateConfigRequest {
                id: row.id as u64,
                data: Some(data("站点名", "site.name", None)),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = first_row(db).await;
        assert_eq!(
            fresh.value.as_deref(),
            Some("旧值"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            me,
            UpdateConfigRequest {
                id: row.id as u64,
                data: Some(Config {
                    name: Some("站点名".into()),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["value".into()],
                }),
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = first_row(db).await;
        assert!(
            fresh.value.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.name, "站点名",
            "req column (name None) never cleared by the mask"
        );
    }
}
