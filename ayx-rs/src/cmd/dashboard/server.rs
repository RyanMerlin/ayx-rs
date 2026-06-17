//! axum router + shared state + graceful shutdown.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine as _;
use rust_embed::Embed;
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use super::handlers;
use ayx_core::profile::RuntimeProfileResolution;

#[derive(Clone)]
pub struct AppState {
    pub available_profiles: Vec<String>,
    pub selected_profile: Option<String>,
    pub profile_resolution: Option<RuntimeProfileResolution>,
    pub default_source: String,
    pub poll_secs: u64,
    pub environment: Option<String>,
    pub remote_mode: bool,
    pub auth_password: Option<String>,
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
        .route("/workflows/{id}", get(handlers::workflows::drilldown))
        .route("/healthz", get(healthz))
        .route("/static/{*path}", get(static_handler))
        .layer(from_fn_with_state(shared.clone(), auth_middleware))
        .layer(from_fn_with_state(
            shared.clone(),
            security_headers_middleware,
        ))
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

async fn security_headers_middleware(
    State(state): State<SharedState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private, max-age=0"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    if state.remote_mode {
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    }
    response
}

async fn auth_middleware(
    State(state): State<SharedState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path == "/healthz" || path.starts_with("/static/") {
        return next.run(request).await;
    }
    if let Some(expected_password) = state.auth_password.as_deref()
        && !authorization_is_valid(request.headers(), expected_password)
    {
        let mut response = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Basic realm="ayx dashboard""#),
        );
        return response;
    }
    next.run(request).await
}

fn authorization_is_valid(headers: &HeaderMap, expected_password: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(encoded) = value.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(credentials) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((_username, password)) = credentials.split_once(':') else {
        return false;
    };
    password == expected_password
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::RuntimeProfileResolution;
    use std::time::Duration;

    fn test_state() -> AppState {
        let resolution = RuntimeProfileResolution {
            config_home: "/tmp/ayx-home".to_owned(),
            selected_profile: "prod".to_owned(),
            selection_source: "state".to_owned(),
            resolved_profile_path: "/tmp/profiles/prod.yaml".to_owned(),
            active_profile: Some("prod".to_owned()),
        };
        AppState {
            available_profiles: vec!["default".to_owned(), "prod".to_owned()],
            selected_profile: Some("prod".to_owned()),
            profile_resolution: Some(resolution),
            default_source: "one".to_owned(),
            poll_secs: 10,
            environment: None,
            remote_mode: false,
            auth_password: None,
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
        let csp = htmx
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = htmx.text().await.unwrap();
        assert!(body.contains("htmx"), "htmx body should mention htmx");
        assert_eq!(
            csp.as_deref(),
            Some(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
            )
        );

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
    async fn router_matches_workflow_drilldown_param_route() {
        // Guards the axum 0.8 path-param conversion `/workflows/:id` -> `/workflows/{id}`.
        // axum 0.8 rejects the old `:param` syntax at router-build time (a runtime panic
        // the compiler cannot catch); a mis-converted route would 404 at the router.
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

        let resp = client
            .get(format!("http://{addr}/workflows/example-id"))
            .send()
            .await
            .unwrap();
        // Route matched and the handler rendered (200), rather than a router 404.
        assert_eq!(resp.status(), 200);

        handle.abort();
    }

    #[tokio::test]
    async fn router_requires_basic_auth_when_password_configured() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut state = test_state();
        state.auth_password = Some("secret-pass".to_owned());
        state.remote_mode = true;
        let app = router(state);

        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        tokio::time::sleep(Duration::from_millis(25)).await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let base = format!("http://{addr}");

        let unauthorized = client.get(format!("{base}/jobs")).send().await.unwrap();
        assert_eq!(unauthorized.status(), 401);
        assert_eq!(
            unauthorized
                .headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok()),
            Some(r#"Basic realm="ayx dashboard""#)
        );

        let authorized = client
            .get(format!("{base}/jobs"))
            .basic_auth("ayx", Some("secret-pass"))
            .send()
            .await
            .unwrap();
        assert_eq!(authorized.status(), 200);

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

    #[tokio::test]
    async fn dashboard_preserves_selected_profile_in_navigation() {
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

        let body = client
            .get(format!("{base}/jobs?profile=prod"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        assert!(body.contains("href=\"/?profile=prod\""));
        assert!(body.contains("href=\"/jobs?profile=prod\""));
        assert!(body.contains("href=\"/failures?profile=prod\""));
        assert!(body.contains("href=\"/workflows?profile=prod\""));
        assert!(body.contains("href=\"/jobs?profile=default\""));
        assert!(body.contains("profile-switcher"));

        handle.abort();
    }

    #[tokio::test]
    async fn dashboard_renders_structured_profile_resolution_errors() {
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

        let body = client
            .get(format!("{base}/?profile=config.yaml"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        assert!(body.contains("Profile resolution error"));
        assert!(body.contains("selected_profile"));
        assert!(body.contains("selection_source"));
        assert!(body.contains("config.yaml"));
        assert!(body.contains("must be a central profile name"));

        handle.abort();
    }
}
