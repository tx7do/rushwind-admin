//! The 500 simulation over the REAL assembled edge — not a `oneshot`
//! router probe: the request rides a live TCP listener through the
//! bootstrap http server's full middleware chain (request-id →
//! recovery → logging), the same stack the deployment faces.
//!
//! Three pins:
//! 1. a panicking handler answers **500 with reason `INTERNAL_PANIC`**
//!    (handler detail never reaches the wire), the `rushwind.panic`
//!    report lands with the request identity attached (method, path,
//!    request_id — the recovery layer's scoped context), and the
//!    server keeps serving afterwards (the liveness probe behind the
//!    incident question "one 500 wedged the whole service");
//! 2. a business 500 (a handler RETURNING an error, no panic) leaves
//!    no panic report — 500 and panic report are different events,
//!    the no-double-report split stays intact;
//! 3. the signalled lifecycle run ends (the test harness can tear the
//!    assembled server down deterministically).
//!
//! The panic-report delivery is deterministic here: the hook's flush
//! handshake runs inside the panic machinery, so the unwind (and the
//! recovery's 500) proceeds only after the report reached every
//! reporter — by the time the client sees the response, the sink
//! already holds the report. No sleeps, no polling on that assertion.

use std::sync::{Arc, Mutex};

use rushwind_bootstrap::{Bootstrap, BootstrapError, RouteInput, RouteSurface};
use rushwind_panic::{PanicReport, PanicReporter};
use rushwind_transport::StopSignal;

/// Collects every delivered report behind a lock — `report` runs on
/// the delivery worker thread and must be quick; the assertions read
/// the Vec from the test task afterwards.
#[derive(Clone, Default)]
struct ReportSink(Arc<Mutex<Vec<PanicReport>>>);

impl PanicReporter for ReportSink {
    fn report(&self, report: &PanicReport) {
        self.0
            .lock()
            .expect("report sink lock")
            .push(report.clone());
    }
}

fn context_of<'a>(report: &'a PanicReport, key: &str) -> Option<&'a str> {
    report
        .context
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// An ephemeral port for this test's assembly (the bind-then-drop race
/// window is the usual test-side trade; collisions are vanishingly
/// rare and self-healing on retry).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind :0")
        .local_addr()
        .expect("local addr")
        .port()
}

/// The simulation surface: one healthy probe, one handler that
/// panics with the test's marker (markers differ per test — reporters
/// are process-global and the two tests may run concurrently), and
/// one handler that RETURNING a 500 without panicking.
fn sim_router(marker: &'static str) -> axum::Router {
    axum::Router::new()
        .route("/ping", axum::routing::get(|| async { "pong" }))
        .route(
            "/panic",
            axum::routing::get(move || async move { panic!("{marker}") }),
        )
        .route(
            "/biz-500",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "dependency down",
                )
            }),
        )
}

/// Boots the real assembly (one http server, one route pack, no
/// storage/authn — the minimal document the assembler accepts), runs
/// the lifecycle on a spawned task, and waits for /ping to answer.
async fn boot(
    marker: &'static str,
) -> (
    u16,
    StopSignal,
    tokio::task::JoinHandle<Result<(), rushwind_transport::ServerError>>,
) {
    let port = free_port();
    let yaml = format!(
        "app:\n  name: panic-sim\nservers:\n  - kind: http\n    bind: \"127.0.0.1:{port}\"\n    route_packs:\n      - name: sim\n"
    );
    let assembler = Bootstrap::from_yaml_str(&yaml)
        .expect("assembly document parses")
        .route_pack(
            "sim",
            move |_settings: serde_json::Value, _input: RouteInput| {
                Ok::<_, BootstrapError>(RouteSurface::new(sim_router(marker)))
            },
        );
    let booted = assembler.build().await.expect("assembly builds");
    let stop = StopSignal::new();
    let run_stop = stop.clone();
    let run = tokio::spawn(async move { booted.app.run(run_stop).await });
    wait_ready(port).await;
    (port, stop, run)
}

/// Polls the probe until the listener answers — the readiness gate
/// that keeps the tests free of start-up sleeps.
async fn wait_ready(port: u16) {
    let url = format!("http://127.0.0.1:{port}/ping");
    for _ in 0..100 {
        if let Ok(resp) = reqwest::get(&url).await {
            if resp.status() == 200 {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the assembled server never answered /ping on :{port}");
}

/// Signs the lifecycle off and asserts the run ends. The aggregation
/// filters the cooperative-cancellation marker out, so a signalled
/// stop comes back `Ok` — Panicked/Timeout/Failed are the
/// non-cooperative shapes this assertion guards against.
async fn shutdown(
    stop: StopSignal,
    run: tokio::task::JoinHandle<Result<(), rushwind_transport::ServerError>>,
) {
    stop.signal();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .expect("the signalled run ends within the deadline")
        .expect("the run task joins");
    assert!(
        outcome.is_ok(),
        "a signalled run ends cooperatively, got {:?}",
        outcome.err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panicking_handler_answers_500_with_the_report_and_stays_alive() {
    let marker = "boom: simulated handler panic (panic-sim-a)";
    let sink = ReportSink::default();
    let _guard = rushwind_panic::plug(Arc::new(sink.clone()));
    let (port, stop, run) = boot(marker).await;

    // ---- the simulated 500 ----
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/panic"))
        .await
        .expect("the panicking request answers (recovery, not a dead connection)");
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("the envelope is json");
    assert_eq!(body["code"].as_str(), Some("INTERNAL"));
    assert_eq!(body["reason"].as_str(), Some("INTERNAL_PANIC"));
    assert_eq!(
        body["message"].as_str(),
        Some("internal server error"),
        "handler detail never reaches the wire"
    );

    // ---- the report: delivered before the unwind, so already here ----
    {
        let reports = sink.0.lock().expect("report sink lock");
        let mine = reports
            .iter()
            .find(|r| r.message == marker)
            .expect("the rushwind.panic report is delivered");
        assert!(
            mine.location
                .as_deref()
                .unwrap_or("")
                .contains("panic_500_simulation.rs"),
            "the report names the panic site, got {:?}",
            mine.location
        );
        assert_eq!(context_of(mine, "method"), Some("GET"));
        assert_eq!(context_of(mine, "path"), Some("/panic"));
        let request_id = context_of(mine, "request_id").expect("request id rides the context");
        assert!(!request_id.is_empty());
    }

    // ---- the liveness probe: the 500 did not wedge the service ----
    let ping = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reqwest::get(format!("http://127.0.0.1:{port}/ping")),
    )
    .await
    .expect("the probe answers within the deadline — the service did not wedge")
    .expect("the probe request succeeds");
    assert_eq!(ping.status(), 200);
    assert_eq!(ping.text().await.expect("probe body"), "pong");

    shutdown(stop, run).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_business_500_without_panic_leaves_no_panic_report() {
    let sink = ReportSink::default();
    let _guard = rushwind_panic::plug(Arc::new(sink.clone()));
    let (port, stop, run) = boot("boom: unused marker (panic-sim-b)").await;

    let resp = reqwest::get(format!("http://127.0.0.1:{port}/biz-500"))
        .await
        .expect("the business-500 request answers");
    assert_eq!(resp.status(), 500);

    // The absence is structural, not timing: a non-panicking request
    // never enters the panic hook, so nothing can enqueue later — the
    // brief poll just makes the assertion patient.
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    {
        let reports = sink.0.lock().expect("report sink lock");
        assert!(
            !reports
                .iter()
                .any(|r| context_of(r, "path") == Some("/biz-500")),
            "a business 500 must not produce a rushwind.panic report, got {reports:?}"
        );
    }

    let ping = reqwest::get(format!("http://127.0.0.1:{port}/ping"))
        .await
        .expect("the service still answers after the business 500");
    assert_eq!(ping.status(), 200);

    shutdown(stop, run).await;
}
