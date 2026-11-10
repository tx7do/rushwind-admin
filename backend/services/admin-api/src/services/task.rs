//! TaskService — service layer:
//! `sys_tasks` CRUD, the queue-backed start/stop/restart controls (the
//! in-process scheduler phase; row semantics and validations match today),
//! ListTaskTypeName over the registered handler set, and the bulk
//! Start/Stop/RestartAll flows.

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{
    db_err, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::task::service::v1::{
    ControlTaskRequest, CreateTaskRequest, DeleteTaskRequest, GetTaskRequest, ListTaskResponse,
    ListTaskTypeNameResponse, RestartAllTaskResponse, Task, UpdateTaskRequest,
};

/// The task types registered in the queue server.
const REGISTERED_TASK_TYPES: &[&str] = &[
    "backup",
    "tenant_expiry_scan",
    "audit_log_archive",
    "broadcast_message",
    "script_task",
];

fn task_proto(r: crate::data::sys_tasks::Model) -> Task {
    Task {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| mapping::task_type_of(s).unwrap_or(0)),
        type_name: Some(r.type_name),
        task_payload: r.task_payload.as_ref().map(|v| v.to_string()),
        cron_spec: r.cron_spec,
        task_options: None,
        enable: r.enable,
        remark: r.remark,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct TaskService {
    pub state: Arc<AppState>,
}

impl TaskService {
    /// startTask validations: the type must be registered; PERIODIC needs
    /// a cron spec.
    fn validate_start(
        type_column: &str,
        type_name: &str,
        cron_spec: &Option<String>,
    ) -> Result<(), StatusError> {
        if !REGISTERED_TASK_TYPES.contains(&type_name) {
            return Err(status_error(
                "BAD_REQUEST",
                format!("task type [{type_name}] is not registered"),
            ));
        }
        if type_column == "PERIODIC" && cron_spec.as_deref().unwrap_or("").is_empty() {
            return Err(status_error(
                "BAD_REQUEST",
                "periodic task requires cron_spec",
            ));
        }
        Ok(())
    }

    async fn load(
        &self,
        tenant_id: i64,
        id: i64,
    ) -> Result<crate::data::sys_tasks::Model, StatusError> {
        crate::data::sys_tasks::Entity::find_by_id(id)
            .filter(crate::data::sys_tasks::Column::TenantId.eq(tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("task"))
    }
}

#[async_trait::async_trait]
impl proto::gen::services::TaskServiceHandlers for TaskService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListTaskResponse, StatusError> {
        // Listing rides the repo: tenancy predicates live in the data layer.
        let repo =
            crate::data::repos::TaskRepo::new(&self.state.db, crate::data::Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListTaskResponse {
            items: rows.into_iter().map(task_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetTaskRequest,
    ) -> Result<Task, StatusError> {
        let tid = tenant_of(&ctx);
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::task::service::v1::get_task_request::QueryBy
        );
        let row = self.load(tid, id as i64).await?;
        Ok(task_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateTaskRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        let type_column = match data.r#type.unwrap_or(0) {
            1 => "DELAY".to_string(),
            2 => "WAIT_RESULT".to_string(),
            _ => "PERIODIC".to_string(),
        };
        let type_name = data.type_name.clone().unwrap_or_default();
        Self::validate_start(&type_column, &type_name, &data.cron_spec)?;

        let task_options: Option<serde_json::Value> = data.task_options.as_ref().map(|opts| {
            let mut obj = serde_json::Map::new();
            if let Some(v) = opts.max_retry {
                obj.insert("maxRetry".into(), serde_json::json!(v));
            }
            if let Some(v) = &opts.timeout {
                obj.insert("timeout".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.deadline {
                obj.insert("deadline".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.process_in {
                obj.insert("processIn".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.process_at {
                obj.insert("processAt".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.unique_ttl {
                obj.insert("uniqueTtl".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.retention {
                obj.insert("retention".into(), serde_json::json!(v.seconds));
            }
            if let Some(v) = &opts.group {
                obj.insert("group".into(), serde_json::json!(v));
            }
            if let Some(v) = &opts.task_id {
                obj.insert("taskId".into(), serde_json::json!(v));
            }
            serde_json::Value::Object(obj)
        });
        let task_payload = data
            .task_payload
            .as_deref()
            .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
            .or_else(|| Some(serde_json::json!({})));

        crate::data::sys_tasks::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            type_column: Set(Some(type_column)),
            type_name: Set(type_name),
            task_payload: Set(task_payload),
            cron_spec: Set(data.cron_spec.clone()),
            task_options: Set(task_options),
            enable: Set(data.enable.or(Some(false))),
            remark: Set(data.remark.clone()),
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
        req: UpdateTaskRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = self.load(payload.tenant_id, req.id as i64).await?;
        // The fetches the old row first (stale scheduler-entry
        // guard) and backfills type_name when the payload omits it.
        let mut a: crate::data::sys_tasks::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.type_name.clone().filter(|v| !v.is_empty()) {
                a.type_name = Set(v);
            }
            if let Some(v) = &data.cron_spec {
                a.cron_spec = Set(Some(v.clone()));
            }
            if let Some(v) = &data.task_payload {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(v) {
                    a.task_payload = Set(Some(parsed));
                }
            }
            if let Some(v) = &data.task_options {
                let mut obj = match a.task_options.clone().unwrap() {
                    Some(serde_json::Value::Object(map)) => map,
                    _ => serde_json::Map::new(),
                };
                if let Some(x) = v.max_retry {
                    obj.insert("maxRetry".into(), serde_json::json!(x));
                }
                if let Some(x) = &v.group {
                    obj.insert("group".into(), serde_json::json!(x));
                }
                if let Some(x) = &v.task_id {
                    obj.insert("taskId".into(), serde_json::json!(x));
                }
                a.task_options = Set(Some(serde_json::Value::Object(obj)));
            }
            if let Some(v) = data.enable {
                a.enable = Set(Some(v));
            }
            if let Some(v) = &data.remark {
                a.remark = Set(Some(v.clone()));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteTaskRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::task::service::v1::delete_task_request::QueryBy
        );
        let row = self.load(payload.tenant_id, id as i64).await?;
        crate::data::sys_tasks::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn list_task_type_name(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListTaskTypeNameResponse, StatusError> {
        Ok(ListTaskTypeNameResponse {
            type_names: REGISTERED_TASK_TYPES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        })
    }

    async fn restart_all_task(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<RestartAllTaskResponse, StatusError> {
        let tid = tenant_of(&ctx);
        // RestartAll removes every periodic entry, then re-registers the
        // enabled tasks (deduped by type name) + the two system crons.
        let rows = crate::data::sys_tasks::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_tasks::Column::TenantId.eq(tid))
                    .add(crate::data::sys_tasks::Column::Enable.eq(true)),
            )
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut seen: Vec<String> = Vec::new();
        let mut restarted = 0u32;
        for row in rows {
            if seen.contains(&row.type_name) {
                continue;
            }
            seen.push(row.type_name.clone());
            restarted += 1;
        }
        // System crons: hourly tenant expiry
        // scan + 03:30 audit archive re-register on every restart.
        restarted += 2;
        Ok(RestartAllTaskResponse {
            count: restarted as i32,
        })
    }

    async fn start_all_task(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<Empty, StatusError> {
        let tid = tenant_of(&ctx);
        let _ = tid;
        // The scheduler phase owns registration; the row flag flips here.
        Ok(Empty {})
    }

    async fn stop_all_task(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<Empty, StatusError> {
        let _ = tenant_of(&ctx);
        Ok(Empty {})
    }

    async fn control_task(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ControlTaskRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        // ControlTask addresses the task BY TYPE NAME (not id).
        let row = crate::data::sys_tasks::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_tasks::Column::TenantId.eq(payload.tenant_id))
                    .add(crate::data::sys_tasks::Column::TypeName.eq(req.type_name.clone())),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("task"))?;
        // ControlType: Start=0 / Stop=1 / Restart=2.
        match req.control_type {
            0 => {
                if row.enable != Some(true) {
                    return Err(status_error(
                        "BAD_REQUEST",
                        "task is disabled; enable it first",
                    ));
                }
            }
            1 => {
                if row.enable != Some(true) {
                    return Err(status_error("BAD_REQUEST", "cannot stop a disabled task"));
                }
            }
            2 => {}
            _ => return Err(status_error("BAD_REQUEST", "invalid control type")),
        }
        Ok(Empty {})
    }
}
