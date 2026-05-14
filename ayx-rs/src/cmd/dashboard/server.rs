//! axum router + shared state + graceful shutdown.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use super::handlers;

#[derive(Clone)]
pub struct AppState {
    pub profile_path: PathBuf,
    pub default_source: String,
    pub poll_secs: u64,
    pub environment: Option<String>,
}

pub type SharedState = Arc<AppState>;

#[derive(Embed)]
#[folder = "src/cmd/dashboard/assets/"]
struct Assets;

pub fn router(state: AppState) -> Router {
    let shared: SharedState = Arc::new(state);
    Router::new()
        .route("/", get(handlers::overview::index))
        .route("/overview/weekly", get(handlers::overview::weekly_partial))
        .route("/jobs", get(handlers::jobs::page))
        .route("/jobs/summary", get(handlers::jobs::summary_partial))
        .route("/jobs/running", get(handlers::jobs::running_partial))
        .route("/jobs/queued", get(handlers::jobs::queued_partial))
        .route("/jobs/history", get(handlers::jobs::history_partial))
        .route("/jobs/top", get(handlers::jobs::top_partial))
        .route("/jobs/owners", get(handlers::jobs::owners_partial))
        .route("/failures", get(handlers::failures::page))
        .route("/failures/recent", get(handlers::failures::recent_partial))
        .route("/failures/queued", get(handlers::failures::queued_partial))
        .route("/failures/owners", get(handlers::failures::owners_partial))
        .route(
            "/failures/workflows",
            get(handlers::failures::workflow_failures_partial),
        )
        .route("/workflows", get(handlers::workflows::page))
        .route(
            "/workflows/summary",
            get(handlers::workflows::summary_partial),
        )
        .route("/workflows/top", get(handlers::workflows::top_partial))
        .route(
            "/workflows/performance",
            get(handlers::workflows::performance_partial),
        )
        .route(
            "/workflows/errors",
            get(handlers::workflows::errors_partial),
        )
        .route("/workflows/:id", get(handlers::workflows::drilldown))
        .route("/healthz", get(healthz))
        .route("/static/*path", get(static_handler))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(shared)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let app = router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

pub(crate) async fn healthz() -> &'static str {
    "ok"
}

pub(crate) async fn static_handler_inner(path: String) -> Response {
    match Assets::get(path.as_str()) {
        Some(file) => {
            let mime = mime_guess::from_path(path.as_str())
                .first_or_octet_stream()
                .to_string();
            (
                StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_str(&mime)
                        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
                )],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub(crate) async fn static_handler(AxumPath(path): AxumPath<String>) -> Response {
    static_handler_inner(path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_state() -> AppState {
        AppState {
            profile_path: PathBuf::from("config.yaml"),
            default_source: "one".to_owned(),
            poll_secs: 10,
            environment: None,
        }
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        assert_eq!(healthz().await, "ok");
    }

    #[tokio::test]
    async fn router_serves_static_assets_and_healthz() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(test_state());

        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        // small wait for bind
        tokio::time::sleep(Duration::from_millis(25)).await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let base = format!("http://{addr}");

        let healthz = client.get(format!("{base}/healthz")).send().await.unwrap();
        assert_eq!(healthz.status(), 200);
        assert_eq!(healthz.text().await.unwrap(), "ok");

        let htmx = client
            .get(format!("{base}/static/htmx.min.js"))
            .send()
            .await
            .unwrap();
        assert_eq!(htmx.status(), 200);
        let body = htmx.text().await.unwrap();
        assert!(body.contains("htmx"), "htmx body should mention htmx");

        let css = client
            .get(format!("{base}/static/app.css"))
            .send()
            .await
            .unwrap();
        assert_eq!(css.status(), 200);

        let missing = client
            .get(format!("{base}/static/does-not-exist.txt"))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), 404);

        handle.abort();
    }

    #[tokio::test]
    async fn router_serves_redesigned_dashboard_pages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(test_state());

        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        tokio::time::sleep(Duration::from_millis(25)).await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let base = format!("http://{addr}");

        let overview = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(overview.status(), 200);
        let overview_body = overview.text().await.unwrap();
        assert!(overview_body.contains("overview-hero"));
        assert!(overview_body.contains("Attention queue"));

        let jobs = client.get(format!("{base}/jobs")).send().await.unwrap();
        assert_eq!(jobs.status(), 200);
        let jobs_body = jobs.text().await.unwrap();
        assert!(jobs_body.contains("jobs-command-center"));
        assert!(jobs_body.contains("Execution snapshot"));

        let failures = client.get(format!("{base}/failures")).send().await.unwrap();
        assert_eq!(failures.status(), 200);
        let failures_body = failures.text().await.unwrap();
        assert!(failures_body.contains("failures-command-center"));
        assert!(failures_body.contains("Recent errors"));

        let workflows = client
            .get(format!("{base}/workflows"))
            .send()
            .await
            .unwrap();
        assert_eq!(workflows.status(), 200);
        let workflows_body = workflows.text().await.unwrap();
        assert!(workflows_body.contains("workflow-rankings"));
        assert!(workflows_body.contains("Workflow summary"));

        let workflow_detail = client
            .get(format!("{base}/workflows/test-flow"))
            .send()
            .await
            .unwrap();
        assert_eq!(workflow_detail.status(), 200);

        handle.abort();
    }
}
