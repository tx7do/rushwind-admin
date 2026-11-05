//! TaskMonitorService — 系统级常驻任务巡检（只读队列投影，无副作用）。
//!
//! 上游读 asynq SchedulerEntries + Inspector；本仓对应面是
//! `rushwind_apalis_jobs` 表（rushwind-apalis-postgres 存储态）与
//! apalis::cron_jobs 的调度注册表。状态投影：running→active、
//! pending(attempts=0)→pending、pending(attempts>0)→retry（重试回队列）、
//! killed→archived（死信）。最近失败明细 = retry+archived 两类行按时间
//! 倒序合并截断（与上游"retry+archived 合并"语义同形）。调度表尚不存在
//! （worker 未起过）时全部读空，不视为错误。

use std::sync::Arc;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};

use crate::state::{internal_error, AppState, StatusError};
use proto::proto::task::service::v1::{
    InspectSystemTasksRequest, InspectSystemTasksResponse, SystemTaskFailure,
    SystemTaskScheduleEntry, SystemTaskStateSummary,
};

/// 最近失败明细每任务类型保留条数（"截断"语义；上游截到常量 5）。
const RECENT_FAILURES_CAP: usize = 5;

const QUEUE_NAME: &str = "default";

pub struct TaskMonitorService {
    pub state: Arc<AppState>,
}

fn ts(t: chrono::NaiveDateTime) -> Option<pbjson_types::Timestamp> {
    crate::state::naive_to_ts(t)
}

/// 下一次触发点：从 now 起逐分钟前扫（调度粒度即分钟；最长扫一天）。
fn next_enqueue(
    spec: &rushwind_transport_cron::CronSpec,
    now: chrono::NaiveDateTime,
) -> chrono::NaiveDateTime {
    let mut t = now + chrono::Duration::minutes(1);
    for _ in 0..(24 * 60) {
        if spec.matches(t) {
            return t;
        }
        t += chrono::Duration::minutes(1);
    }
    now
}

async fn queue_table_exists(db: &DatabaseConnection) -> bool {
    let stmt = Statement::from_string(
        db.get_database_backend(),
        "SELECT 1 FROM information_schema.tables WHERE table_name = 'rushwind_apalis_jobs'",
    );
    matches!(db.query_one_raw(stmt).await, Ok(Some(_)))
}

#[async_trait::async_trait]
impl proto::gen::services::TaskMonitorServiceHandlers for TaskMonitorService {
    /// 一站式巡检：调度条目 + 各任务类型的队列状态与失败明细。
    async fn inspect_system_tasks(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: InspectSystemTasksRequest,
    ) -> Result<InspectSystemTasksResponse, StatusError> {
        let now = crate::data::now();

        // 调度条目：注册表 + 每类型最近一次入队时间（队列行 MAX(created_at)）。
        let mut schedules = Vec::new();
        for (name, spec_str) in crate::server::apalis::schedule_registry() {
            let spec = rushwind_transport_cron::CronSpec::parse(spec_str)
                .map_err(|e| internal_error(format!("cron spec {spec_str}: {e}")))?;
            schedules.push(SystemTaskScheduleEntry {
                task_type: name.to_string(),
                cron_spec: spec_str.to_string(),
                next_enqueue_at: ts(next_enqueue(&spec, now)),
                prev_enqueue_at: None, // below, from the queue table
            });
        }

        // 队列表未建（worker 从未起过）→ 全空面，不视为错误。
        if !queue_table_exists(&self.state.db).await {
            return Ok(InspectSystemTasksResponse {
                queue: QUEUE_NAME.into(),
                schedules,
                summaries: Vec::new(),
            });
        }

        // 每类型最近入队时间（prev）。
        let prev_rows = self
            .query(
                "SELECT payload::jsonb->>'type' AS task_type, MAX(created_at) AS last_at \
                 FROM rushwind_apalis_jobs GROUP BY 1",
            )
            .await?;
        let mut prev: Vec<(String, Option<chrono::NaiveDateTime>)> = Vec::new();
        for row in &prev_rows {
            let name: String = row.try_get("", "task_type").unwrap_or_default();
            let at: Option<chrono::NaiveDateTime> = row.try_get("", "last_at").ok();
            prev.push((name, at));
        }
        for entry in &mut schedules {
            if let Some((_, at)) = prev.iter().find(|(n, _)| *n == entry.task_type) {
                entry.prev_enqueue_at = at.and_then(ts);
            }
        }

        // 状态摘要：按类型 × 投影桶计数。
        let count_rows = self
            .query(
                "SELECT payload::jsonb->>'type' AS task_type, \
                        CASE WHEN status = 'running' THEN 'active' \
                             WHEN status = 'pending' AND attempts = 0 THEN 'pending' \
                             WHEN status = 'pending' THEN 'retry' \
                             WHEN status = 'killed' THEN 'archived' \
                             ELSE status END AS bucket, \
                        COUNT(*) AS cnt \
                 FROM rushwind_apalis_jobs GROUP BY 1, 2",
            )
            .await?;
        #[derive(Default)]
        struct Counts {
            active: u32,
            pending: u32,
            retry: u32,
            archived: u32,
        }
        let mut order: Vec<String> = Vec::new();
        let mut counts: std::collections::BTreeMap<String, Counts> =
            std::collections::BTreeMap::new();
        for row in &count_rows {
            let name: String = row.try_get("", "task_type").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let bucket: String = row.try_get("", "bucket").unwrap_or_default();
            let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
            let c = counts.entry(name.clone()).or_default();
            if !order.contains(&name) {
                order.push(name);
            }
            match bucket.as_str() {
                "active" => c.active = cnt as u32,
                "pending" => c.pending = cnt as u32,
                "retry" => c.retry = cnt as u32,
                "archived" => c.archived = cnt as u32,
                _ => {}
            }
        }

        // 最近失败明细：retry + archived 行按失败时间倒序，每类型截断。
        let fail_rows = self
            .query(
                "SELECT payload::jsonb->>'type' AS task_type, \
                        CASE WHEN status = 'killed' THEN 'archived' ELSE 'retry' END AS state, \
                        COALESCE(last_error, '') AS last_error, \
                        updated_at, attempts, max_attempts, run_at \
                 FROM rushwind_apalis_jobs \
                 WHERE status = 'killed' OR (status = 'pending' AND attempts > 0) \
                 ORDER BY updated_at DESC LIMIT 200",
            )
            .await?;
        let mut failures: std::collections::BTreeMap<String, Vec<SystemTaskFailure>> =
            std::collections::BTreeMap::new();
        for row in &fail_rows {
            let name: String = row.try_get("", "task_type").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let state: String = row.try_get("", "state").unwrap_or_default();
            let is_archived = state == "archived";
            let list = failures.entry(name.clone()).or_default();
            if list.len() >= RECENT_FAILURES_CAP {
                continue;
            }
            list.push(SystemTaskFailure {
                task_type: name,
                queue: QUEUE_NAME.into(),
                state,
                last_error: row.try_get("", "last_error").unwrap_or_default(),
                last_failed_at: row
                    .try_get::<Option<chrono::NaiveDateTime>>("", "updated_at")
                    .ok()
                    .flatten()
                    .and_then(ts),
                retried: row.try_get::<i32>("", "attempts").unwrap_or(0).max(0) as u32,
                max_retry: row.try_get::<i32>("", "max_attempts").unwrap_or(0).max(0) as u32,
                // 下次重试时间（archived 无）
                next_process_at: if is_archived {
                    None
                } else {
                    row.try_get::<Option<chrono::NaiveDateTime>>("", "run_at")
                        .ok()
                        .flatten()
                        .and_then(ts)
                },
            });
        }

        let mut summaries = Vec::new();
        for name in &order {
            let c = &counts[name];
            summaries.push(SystemTaskStateSummary {
                task_type: name.clone(),
                active: c.active,
                pending: c.pending,
                retry: c.retry,
                archived: c.archived,
                recent_failures: failures.remove(name).unwrap_or_default(),
            });
        }

        Ok(InspectSystemTasksResponse {
            queue: QUEUE_NAME.into(),
            schedules,
            summaries,
        })
    }
}

impl TaskMonitorService {
    /// 原生只读查询（巡检面全部走 SQL 投影，不建 SeaORM 实体）。
    async fn query(&self, sql: &str) -> Result<Vec<sea_orm::QueryResult>, StatusError> {
        self.state
            .db
            .query_all_raw(Statement::from_string(
                self.state.db.get_database_backend(),
                sql,
            ))
            .await
            .map_err(|e: DbErr| internal_error(format!("task inspect: {e}")))
    }
}
