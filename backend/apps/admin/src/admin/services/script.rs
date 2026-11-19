//! ScriptService and ScriptLogService — //! module / service: script CRUD over
//! `sys_scripts` (TestRun reports UNIMPLEMENTED until the Lua/JS engine
//! phase), and the `sys_script_logs` list/count/purge surface.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use crate::proto::pagination::PagingRequest;
use crate::proto::script::service::v1::{
    CountScriptLogsResponse, CountScriptsResponse, CreateScriptRequest, DeleteScriptRequest,
    GetScriptRequest, ListScriptLogsResponse, ListScriptsResponse, PurgeScriptLogsRequest,
    PurgeScriptLogsResponse, Script, ScriptLog, TestRunScriptRequest, TestRunScriptResponse,
    UpdateScriptRequest,
};
use pbjson_types::Empty;

/// Unknown rows read as LUA.
fn language_to_proto(s: &str) -> i32 {
    mapping::script_language_of(s).unwrap_or(0)
}

fn language_to_str(v: i32) -> String {
    mapping::script_language_str(v).unwrap_or("LUA").into()
}

fn script_proto(r: crate::admin::data::sys_scripts::Model) -> Script {
    Script {
        id: Some(r.id as u64),
        name: Some(r.name),
        language: r.language.as_deref().map(language_to_proto),
        hook_point: r.hook_point,
        source: r.source,
        priority: r.priority,
        description: r.description,
        critical: r.critical,
        version: r.version,
        is_enabled: r.is_enabled,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

fn script_log_proto(r: crate::admin::data::sys_script_logs::Model) -> ScriptLog {
    ScriptLog {
        id: Some(r.id as u64),
        script_id: r.script_id.map(|v| v as u64),
        script_name: r.script_name,
        language: r.language,
        trigger_type: r.trigger_type,
        hook_point: r.hook_point,
        version: r.version,
        success: r.success,
        duration_ms: r.duration_ms,
        error: r.error,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct ScriptService {
    pub state: Arc<AppState>,
}

impl ScriptService {
    /// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
    /// 存量全量提交语义）。language 在更新面不开放（建后不改）。
    fn apply_fields(
        a: &mut crate::data::sys_scripts::ActiveModel,
        data: &Script,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
        crate::set_masked!(opt mask, a, source, data.source, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, priority, data.priority, |v: &i32| Some(*v));
        crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(
            *v
        ));
        crate::set_masked!(opt mask, a, hook_point, data.hook_point, |v: &String| Some(
            v.clone()
        ));
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::ScriptServiceHandlers for ScriptService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListScriptsResponse, StatusError> {
        let repo = crate::admin::data::repos::ScriptRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListScriptsResponse {
            items: rows.into_iter().map(script_proto).collect(),
            total,
        })
    }

    async fn count(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: PagingRequest,
    ) -> Result<CountScriptsResponse, StatusError> {
        Ok(CountScriptsResponse {
            count: crate::admin::data::sys_scripts::Entity::find()
                .count(&self.state.db)
                .await
                .unwrap_or(0),
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetScriptRequest,
    ) -> Result<Script, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::script::service::v1::get_script_request::QueryBy
        );
        let row = crate::admin::data::sys_scripts::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("script"))?;
        Ok(script_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateScriptRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_scripts::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            language: Set(Some(language_to_str(data.language.unwrap_or(0)))),
            hook_point: Set(data.hook_point),
            source: Set(data.source),
            priority: Set(data.priority.or(Some(0))),
            description: Set(data.description),
            critical: Set(data.critical.or(Some(false))),
            version: Set(Some(1)),
            is_enabled: Set(data.is_enabled.or(Some(true))),
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
        req: UpdateScriptRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::sys_scripts::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("script"))?;
        let mut a: crate::admin::data::sys_scripts::ActiveModel = row.into();
        let mut version_bump = false;
        if let Some(data) = &req.data {
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            Self::apply_fields(&mut a, data, &mask);
            // 版本随源码变更递增：只在掩码选中 source 且载荷有值时触发
            // （掩码外携带或载荷缺席都不 bump——门只放行选中的写面）。
            version_bump = mask.touches("source") && data.source.is_some();
        }
        crate::stamp_update!(a, payload.user_id);
        if version_bump {
            a.version = Set(Some(a.version.clone().unwrap().unwrap_or(1) + 1));
        }
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteScriptRequest,
    ) -> Result<Empty, StatusError> {
        crate::admin::data::sys_scripts::Entity::delete_many()
            .filter(crate::admin::data::sys_scripts::Column::Id.is_in(req.ids))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn test_run(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: TestRunScriptRequest,
    ) -> Result<TestRunScriptResponse, StatusError> {
        // The script engine (Lua/JS) lands with the script-runtime phase;

        Err(status_error("UNIMPLEMENTED", "script runtime not wired"))
    }

    async fn list_hook_points(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<crate::proto::script::service::v1::ListHookPointsResponse, StatusError> {
        // The entity-hook registry (module).
        let hook_points = vec![
            "user.before_create",
            "user.after_create",
            "user.before_update",
            "user.after_update",
            "tenant.before_create",
            "tenant.after_create",
            "role.before_update",
            "role.after_update",
            "internal_message.after_create",
            "notification_channel.after_update",
        ];
        Ok(crate::proto::script::service::v1::ListHookPointsResponse {
            items: hook_points
                .into_iter()
                .map(|name| crate::proto::script::service::v1::HookPoint {
                    name: name.to_string(),
                    description: String::new(),
                    script_count: 0,
                })
                .collect(),
            languages: Vec::new(),
        })
    }
}

pub struct ScriptLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::ScriptLogServiceHandlers for ScriptLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListScriptLogsResponse, StatusError> {
        // ScriptLogRepo owns the newest-first ordering; paged_list applies
        // the PagingRequest slice.
        let repo = crate::admin::data::repos::ScriptLogRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListScriptLogsResponse {
            items: rows.into_iter().map(script_log_proto).collect(),
            total,
        })
    }

    async fn count(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: PagingRequest,
    ) -> Result<CountScriptLogsResponse, StatusError> {
        Ok(CountScriptLogsResponse {
            count: crate::admin::data::sys_script_logs::Entity::find()
                .count(&self.state.db)
                .await
                .unwrap_or(0),
        })
    }

    async fn purge(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PurgeScriptLogsRequest,
    ) -> Result<PurgeScriptLogsResponse, StatusError> {
        let mut query = crate::admin::data::sys_script_logs::Entity::delete_many();
        if let Some(before) = &req.before {
            if let Some(cutoff) = crate::admin::state::ts_to_naive(before) {
                query =
                    query.filter(crate::admin::data::sys_script_logs::Column::CreatedAt.lt(cutoff));
            }
        }
        let result = query.exec(&self.state.db).await.map_err(db_err)?;
        Ok(PurgeScriptLogsResponse {
            deleted: result.rows_affected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::{ctx, test_state};
    // handler 方法挂在 trait 上——测试模块里显式引入。
    use proto::gen::services::ScriptServiceHandlers as _;

    async fn first_row(db: &sea_orm::DatabaseConnection) -> crate::data::sys_scripts::Model {
        crate::data::sys_scripts::Entity::find()
            .one(db)
            .await
            .unwrap()
            .expect("script row")
    }

    /// update_mask 白名单门：掩码只选 name 时，载荷多带的 source/priority
    /// 不许动，版本也不随被挡下的 source 递增。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = ScriptService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateScriptRequest {
                data: Some(Script {
                    name: Some("审计钩子".into()),
                    source: Some("return 1".into()),
                    priority: Some(0),
                    is_enabled: Some(true),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = first_row(db).await;
        assert_eq!(row.version, Some(1), "create starts at version 1");

        svc.update(
            me,
            UpdateScriptRequest {
                id: row.id as u64,
                data: Some(Script {
                    name: Some("审计钩子改".into()),
                    source: Some("return 2".into()),
                    priority: Some(99),
                    is_enabled: Some(false),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["name".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fresh = first_row(db).await;
        assert_eq!(fresh.name, "审计钩子改", "masked field lands");
        assert_eq!(
            fresh.source.as_deref(),
            Some("return 1"),
            "unmasked source is not writable through the gate"
        );
        assert_eq!(
            fresh.priority,
            Some(0),
            "unmasked priority is not writable through the gate"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
        assert_eq!(
            fresh.version,
            Some(1),
            "version does not bump when source is masked out"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = ScriptService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        svc.create(
            me.clone(),
            CreateScriptRequest {
                data: Some(Script {
                    name: Some("清理钩子".into()),
                    source: Some("return 0".into()),
                    description: Some("说明".into()),
                    hook_point: Some("user.before_create".into()),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
        let row = first_row(db).await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateScriptRequest {
                id: row.id as u64,
                data: Some(Script {
                    name: Some("清理钩子".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = first_row(db).await;
        assert_eq!(
            fresh.description.as_deref(),
            Some("说明"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列（source 清列不触发版本递增）。
        svc.update(
            me,
            UpdateScriptRequest {
                id: row.id as u64,
                data: Some(Script::default()),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["description".into(), "source".into()],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = first_row(db).await;
        assert!(
            fresh.description.is_none(),
            "masked None explicitly clears description"
        );
        assert!(
            fresh.source.is_none(),
            "masked None explicitly clears source"
        );
        assert_eq!(
            fresh.hook_point.as_deref(),
            Some("user.before_create"),
            "unmasked hook_point stays put while clearing"
        );
        assert_eq!(
            fresh.version,
            Some(1),
            "clearing source does not bump the version"
        );
    }
}
