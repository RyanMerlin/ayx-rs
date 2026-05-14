use axum::extract::{Path, State};
use axum::response::Response;

use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{build_args, run_envelope, PanelQ};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::workflows;

use super::{err_card, html};

pub async fn page(State(state): State<SharedState>, q: PanelQ) -> Response {
    html(views::layout(
        "workflows",
        views::workflows::page(
            &state.default_source,
            state.poll_secs,
            state.environment.as_deref(),
            q.0.since.as_deref().unwrap_or("7d"),
            q.0.sort.as_deref().unwrap_or("runs"),
            q.0.owner.as_deref().unwrap_or(""),
            q.0.health.as_deref().unwrap_or("all"),
        ),
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
    ))
}

pub async fn summary_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || workflows::dashboard_summary(env.as_deref(), &args)).await {
        Ok(env_) => html(views::workflows::summary_strip(&env_.data)),
        Err(e) => err_card(format!("workflows summary: {e}")),
    }
}

pub async fn top_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    let sort = q.0.sort.clone().unwrap_or_else(|| "runs".to_owned());
    match run_envelope(move || workflows::top(env.as_deref(), &args, &sort)).await {
        Ok(mut env_) => {
            filter_workflow_items(&mut env_.data, q.0.owner.as_deref(), q.0.health.as_deref());
            html(views::workflows::top_table(&env_.data))
        }
        Err(e) => err_card(format!("workflows top: {e}")),
    }
}

pub async fn performance_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || workflows::performance(env.as_deref(), &args)).await {
        Ok(mut env_) => {
            filter_workflow_items(&mut env_.data, q.0.owner.as_deref(), q.0.health.as_deref());
            sort_performance_items(&mut env_.data, q.0.sort.as_deref().unwrap_or("runs"));
            html(views::workflows::performance_table(
                &env_.data,
                q.0.source.as_deref().unwrap_or(&state.default_source),
                q.0.since.as_deref().unwrap_or("7d"),
                q.0.owner.as_deref().unwrap_or(""),
                q.0.health.as_deref().unwrap_or("all"),
            ))
        }
        Err(e) => err_card(format!("workflows performance: {e}")),
    }
}

pub async fn errors_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || workflows::errors(env.as_deref(), &args)).await {
        Ok(mut env_) => {
            filter_error_items(&mut env_.data, q.0.owner.as_deref());
            html(views::workflows::errors_table(&env_.data))
        }
        Err(e) => err_card(format!("workflows errors: {e}")),
    }
}

pub async fn drilldown(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    q: PanelQ,
) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    let result = run_envelope(move || workflows::detail(env.as_deref(), &args, &id)).await;
    let body = match result {
        Ok(env_) => views::workflows::drilldown(&env_.data),
        Err(e) => views::error_card(&format!("workflow drilldown: {e}")),
    };
    html(views::layout(
        "workflow",
        body,
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
    ))
}

fn filter_workflow_items(data: &mut serde_json::Value, owner: Option<&str>, health: Option<&str>) {
    let Some(items) = data
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let owner = owner.unwrap_or("").trim().to_ascii_lowercase();
    let health = health.unwrap_or("all").to_ascii_lowercase();
    items.retain(|item| {
        let item_owner = item
            .get("owner_email")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let failures = item
            .get("failure_count")
            .or_else(|| item.get("failed"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let owner_ok = owner.is_empty() || item_owner.contains(&owner);
        let health_ok = match health.as_str() {
            "unhealthy" => failures > 0,
            "healthy" => failures == 0,
            _ => true,
        };
        owner_ok && health_ok
    });
}

fn sort_performance_items(data: &mut serde_json::Value, sort: &str) {
    let Some(items) = data
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    match sort {
        "duration" => items.sort_by(|a, b| {
            b.get("p95_ms")
                .and_then(serde_json::Value::as_f64)
                .partial_cmp(&a.get("p95_ms").and_then(serde_json::Value::as_f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "failure-rate" => items.sort_by(|a, b| {
            b.get("failed")
                .and_then(serde_json::Value::as_u64)
                .cmp(&a.get("failed").and_then(serde_json::Value::as_u64))
        }),
        _ => items.sort_by(|a, b| {
            b.get("run_count")
                .and_then(serde_json::Value::as_u64)
                .cmp(&a.get("run_count").and_then(serde_json::Value::as_u64))
        }),
    }
}

fn filter_error_items(data: &mut serde_json::Value, owner: Option<&str>) {
    let Some(items) = data
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let owner = owner.unwrap_or("").trim().to_ascii_lowercase();
    if owner.is_empty() {
        return;
    }
    items.retain(|item| {
        item.get("owner_email")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains(&owner)
    });
}
