//! The task-queue transport pair, both wired as lifecycle transports
//! through the assembler's factory and cron-job registries:
//! * the worker — claims and executes queued jobs over the queue
//!   storage (`rushwind-apalis-postgres`, a Postgres queue table with
//!   the same push/claim/ack shape, framework-native);
//! * the enqueue side — the shared storage handle the cron producer's
//!   periodic wildcard pushes through.
//!
//! The producer itself (the two system crons plus the DB-driven
//! wildcard) registers as cron jobs by name.
use std::sync::Arc;

use apalis_core::backend::Backend;
use apalis_core::error::{BoxDynError, Error as ApalisError};
use apalis_core::layers::Ack;
use apalis_core::response::Response;
use apalis_core::storage::Storage;
use apalis_core::worker::{Context as WorkerContext, Worker, WorkerId};
use futures_util::StreamExt as _;
use rushwind_apalis_postgres::PostgresStorage;
use rushwind_transport::{Server, ServerError, ServerFuture, StopSignal};
use serde_json::json;

use crate::state::AppState;

/// The job envelope carried in the queue (JsonCodec<String> args).
#[derive(serde::Deserialize)]
struct Job {
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    payload: serde_json::Value,
}

/// The enqueue side: the shared queue-storage handle. The worker claims
/// from the same storage; the cron producer's periodic wildcard pushes
/// through this side.
pub struct TaskQueue {
    storage: PostgresStorage<String>,
    queue_name: String,
}

impl TaskQueue {
    /// Builds the queue storage over the `queue` (default queue). The
    /// queue table itself is created idempotently in the worker's
    /// start.
    pub fn new(dsn: &str, queue: &str) -> Result<Self, String> {
        let storage = PostgresStorage::<String>::from_settings(json!({
            "url": dsn,
            "queue": queue,
        }))
        .map_err(|e| format!("apalis storage: {e}"))?;
        Ok(Self {
            storage,
            queue_name: queue.to_string(),
        })
    }

    /// Enqueues a job (the producer side).
    pub async fn enqueue(&self, type_name: &str, payload: serde_json::Value) -> Result<(), String> {
        let job = json!({ "type": type_name, "payload": payload }).to_string();
        let mut storage = self.storage.clone();
        storage
            .push(job)
            .await
            .map_err(|e| format!("apalis push: {e}"))?;
        Ok(())
    }
}

/// The worker transport: claims queued jobs from the shared queue
/// storage and executes their handlers.
pub struct ApalisServer {
    state: Arc<AppState>,
    queue: Arc<TaskQueue>,
}

impl ApalisServer {
    /// Wraps the shared queue-storage handle.
    pub fn new(state: Arc<AppState>, queue: Arc<TaskQueue>) -> Self {
        Self { state, queue }
    }

    /// The task handlers (module registrations).
    async fn run_handler(
        db: &sea_orm::DatabaseConnection,
        type_name: &str,
        payload: &serde_json::Value,
    ) {
        match type_name {
            "tenant_expiry_scan" => {
                use sea_orm::{
                    ActiveModelTrait as _, ColumnTrait as _, EntityTrait as _, QueryFilter as _,
                    Set,
                };
                let expired = crate::data::sys_tenants::Entity::find()
                    .filter(
                        sea_orm::sea_query::Condition::all()
                            .add(crate::data::sys_tenants::Column::Status.eq("ON"))
                            .add(crate::data::sys_tenants::Column::ExpiredAt.is_not_null())
                            .add(
                                crate::data::sys_tenants::Column::ExpiredAt.lte(crate::data::now()),
                            ),
                    )
                    .all(db)
                    .await
                    .unwrap_or_default();
                let count = expired.len();
                for row in expired {
                    let mut t: crate::data::sys_tenants::ActiveModel = row.into();
                    t.status = Set(Some("EXPIRED".into()));
                    let _ = t.update(db).await;
                }
                eprintln!("[scheduler] tenant_expiry_scan: {count} expired");
            }
            "audit_log_archive" => {
                eprintln!(
                    "[scheduler] audit_log_archive: retention export skipped (no archive dir)"
                );
            }
            "backup" | "broadcast_message" | "script_task" => {
                eprintln!("[scheduler] {type_name} fired with payload {payload}");
            }
            other => eprintln!("[scheduler] unknown task type {other}"),
        }
    }
}

#[async_trait::async_trait]
impl Server for ApalisServer {
    fn endpoint(&self) -> Result<String, ServerError> {
        Ok(format!("apalis-postgres://{}", self.queue.queue_name))
    }

    fn start(&self, stop: StopSignal) -> ServerFuture<'_> {
        Box::pin(async move {
            PostgresStorage::<()>::setup(self.queue.storage.pool())
                .await
                .map_err(|e| ServerError::Failed(format!("apalis setup: {e}")))?;

            let worker = Worker::new(WorkerId::new("admin-task-worker"), WorkerContext::default());
            worker.start();
            let poller = self.queue.storage.clone().poll(&worker);
            tokio::spawn(poller.heartbeat);
            let mut stream = poller.stream;

            loop {
                tokio::select! {
                    _ = stop.wait() => return Err(ServerError::Cancelled),
                    item = stream.next() => match item {
                        Some(Ok(Some(req))) => {
                            let job: Result<Job, _> = serde_json::from_str(&req.args);
                            let ok = match &job {
                                Ok(job) => {
                                    Self::run_handler(&self.state.db, &job.type_name, &job.payload)
                                        .await;
                                    true
                                }
                                Err(e) => {
                                    eprintln!("[scheduler] job decode failed: {e}");
                                    false
                                }
                            };
                            let mut acker = self.queue.storage.clone();
                            let res = if ok {
                                Response::success(
                                    (),
                                    req.parts.task_id.clone(),
                                    req.parts.attempt.clone(),
                                )
                            } else {
                                let err: BoxDynError =
                                    "job decode/execute failed".to_string().into();
                                Response::failure(
                                    ApalisError::Failed(Arc::new(err)),
                                    req.parts.task_id.clone(),
                                    req.parts.attempt.clone(),
                                )
                            };
                            let _ = acker.ack(&req.parts.context, &res).await;
                        }
                        Some(Ok(None)) | None => break,
                        Some(Err(e)) => {
                            eprintln!("[scheduler] poll error: {e}");
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn stop(&self) -> ServerFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

/// The worker factory: the task-queue consumer transport. The storage
/// handle is pre-built by the assembly entry; the factory wraps it.
pub fn worker_factory(
    state: Arc<AppState>,
    queue: Arc<TaskQueue>,
) -> impl Fn(
    serde_json::Value,
    rushwind_bootstrap::RouteInput,
) -> rushwind_bootstrap::BoxFuture<
    'static,
    Result<std::sync::Arc<dyn rushwind_transport::Server>, rushwind_bootstrap::BootstrapError>,
> + Send
       + Sync
       + 'static {
    move |_settings, _input| {
        let state = Arc::clone(&state);
        let queue = Arc::clone(&queue);
        Box::pin(async move {
            Ok(Arc::new(ApalisServer::new(state, queue))
                as std::sync::Arc<dyn rushwind_transport::Server>)
        })
    }
}

// ---------------------------------------------------------------------------
// The cron producer: the two static system crons are registered
// directly; the DB-driven PERIODIC rows ride a wildcard job whose
// handler scans `sys_tasks` each tick, so ControlTask/Update/Delete
// take effect without a restart.
// ---------------------------------------------------------------------------

/// Builds the cron producer registrations: the static system crons and
/// the DB-driven PERIODIC wildcard, keyed by their registered names.
/// The full schedule registry (static crons + the DB-driven wildcard),
/// exposed read-only for the task-monitor inspect surface.
pub fn schedule_registry() -> Vec<(&'static str, &'static str)> {
    vec![
        ("tenant_expiry_scan", "0 * * * *"),
        ("audit_log_archive", "30 3 * * *"),
        ("sys_tasks_periodic", "* * * * *"),
    ]
}

pub fn cron_jobs(
    state: Arc<AppState>,
    queue: Arc<TaskQueue>,
) -> Vec<(&'static str, rushwind_transport_cron::CronJob)> {
    use rushwind_transport_cron::{CronJob, CronSpec};

    fn spec(spec: &str) -> CronSpec {
        CronSpec::parse(spec).expect("static cron spec parses")
    }

    // Static system crons: one table row per job — name, spec, and the
    // handler the worker dispatch shares.
    const STATIC_CRONS: &[(&str, &str)] = &[
        ("tenant_expiry_scan", "0 * * * *"),
        ("audit_log_archive", "30 3 * * *"),
    ];

    // The wildcard job: DB-driven PERIODIC rows. The handler re-reads
    // `sys_tasks` each tick, so ControlTask/Update/Delete take effect
    // without a restart.
    let sys_tasks_periodic = CronJob::new("sys_tasks_periodic", spec("* * * * *"), {
        let state = Arc::clone(&state);
        let queue = Arc::clone(&queue);
        move || {
            let state = Arc::clone(&state);
            let queue = Arc::clone(&queue);
            Box::pin(async move {
                use chrono::Timelike as _;
                use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _};
                let now = crate::data::now();
                if now.second() != 0 {
                    return;
                }
                let rows = crate::data::sys_tasks::Entity::find()
                    .filter(
                        sea_orm::sea_query::Condition::all()
                            .add(crate::data::sys_tasks::Column::Enable.eq(true))
                            .add(crate::data::sys_tasks::Column::TypeColumn.eq("PERIODIC")),
                    )
                    .all(&state.db)
                    .await
                    .unwrap_or_default();
                let mut fired: Vec<String> = Vec::new();
                for row in rows {
                    let Some(s) = row.cron_spec.as_deref() else {
                        continue;
                    };
                    let Ok(spec) = CronSpec::parse(s) else {
                        eprintln!("[scheduler] invalid cron spec '{s}' for {}", row.type_name);
                        continue;
                    };
                    if !spec.matches(now) || fired.contains(&row.type_name) {
                        continue;
                    }
                    fired.push(row.type_name.clone());
                    let payload = row
                        .task_payload
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let _ = queue.enqueue(&row.type_name, payload).await;
                }
            })
        }
    });

    let mut jobs: Vec<(&'static str, CronJob)> = STATIC_CRONS
        .iter()
        .map(|&(name, spec_str)| {
            let state = Arc::clone(&state);
            let job = CronJob::new(name, spec(spec_str), move || {
                let state = Arc::clone(&state);
                let name = name.to_string();
                Box::pin(async move {
                    ApalisServer::run_handler(&state.db, &name, &serde_json::json!({})).await;
                })
            });
            (name, job)
        })
        .collect();
    jobs.push(("sys_tasks_periodic", sys_tasks_periodic));
    jobs
}
