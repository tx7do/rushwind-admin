//! ScriptService and ScriptLogService — //! module / service: script CRUD over
//! `sys_scripts` (TestRun reports UNIMPLEMENTED until the Lua/JS engine
//! phase), and the `sys_script_logs` list/count/purge surface.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::script::service::v1::{
    CountScriptLogsResponse, CountScriptsResponse, CreateScriptRequest, DeleteScriptRequest,
    GetScriptRequest, ListScriptLogsResponse, ListScriptsResponse, PurgeScriptLogsRequest,
    PurgeScriptLogsResponse, Script, ScriptLog, TestRunScriptRequest, TestRunScriptResponse,
    UpdateScriptRequest,
};

/// Unknown rows read as LUA.
fn language_to_proto(s: &str) -> i32 {
    mapping::script_language_of(s).unwrap_or(0)
}

fn language_to_str(v: i32) -> String {
    mapping::script_language_str(v).unwrap_or("LUA").into()
}

fn script_proto(r: crate::data::sys_scripts::Model) -> Script {
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
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn script_log_proto(r: crate::data::sys_script_logs::Model) -> ScriptLog {
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
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct ScriptService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::ScriptServiceHandlers for ScriptService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListScriptsResponse, StatusError> {
        let repo = crate::data::repos::ScriptRepo::new(&self.state.db);
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
            count: crate::data::sys_scripts::Entity::find()
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
            proto::proto::script::service::v1::get_script_request::QueryBy
        );
        let row = crate::data::sys_scripts::Entity::find_by_id(id as i64)
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
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_scripts::ActiveModel {
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
        req: UpdateScriptRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_scripts::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("script"))?;
        let mut a: crate::data::sys_scripts::ActiveModel = row.into();
        let mut version_bump = false;
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.source {
                a.source = Set(Some(v.clone()));
                version_bump = true;
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.priority {
                a.priority = Set(Some(v));
            }
            if let Some(v) = data.is_enabled {
                a.is_enabled = Set(Some(v));
            }
            if let Some(v) = &data.hook_point {
                a.hook_point = Set(Some(v.clone()));
            }
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
        crate::data::sys_scripts::Entity::delete_many()
            .filter(crate::data::sys_scripts::Column::Id.is_in(req.ids))
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
    ) -> Result<proto::proto::script::service::v1::ListHookPointsResponse, StatusError> {
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
        Ok(proto::proto::script::service::v1::ListHookPointsResponse {
            items: hook_points
                .into_iter()
                .map(|name| proto::proto::script::service::v1::HookPoint {
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
impl proto::gen::services::ScriptLogServiceHandlers for ScriptLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListScriptLogsResponse, StatusError> {
        // ScriptLogRepo owns the newest-first ordering; paged_list applies
        // the PagingRequest slice.
        let repo = crate::data::repos::ScriptLogRepo::new(&self.state.db);
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
            count: crate::data::sys_script_logs::Entity::find()
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
        let mut query = crate::data::sys_script_logs::Entity::delete_many();
        if let Some(before) = &req.before {
            if let Some(cutoff) = crate::state::ts_to_naive(before) {
                query = query.filter(crate::data::sys_script_logs::Column::CreatedAt.lt(cutoff));
            }
        }
        let result = query.exec(&self.state.db).await.map_err(db_err)?;
        Ok(PurgeScriptLogsResponse {
            deleted: result.rows_affected,
        })
    }
}
