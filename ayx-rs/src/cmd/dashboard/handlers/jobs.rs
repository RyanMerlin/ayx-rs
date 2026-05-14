use axum::extract::State;
use axum::response::Response;

use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{build_args, run_envelope, PanelQ};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::jobs;

use super::{err_card, html};

pub async fn page(State(state): State<SharedState>, q: PanelQ) -> Response {
    html(views::layout(
        "jobs",
        views::jobs::page(
            &state.default_source,
            state.poll_secs,
            state.environment.as_deref(),
            q.0.since.as_deref().unwrap_or("7d"),
            q.0.status.as_deref().unwrap_or("all"),
            q.0.owner.as_deref().unwrap_or(""),
            q.0.sort.as_deref().unwrap_or("finished"),
        ),
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
    ))
}

pub async fn summary_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::dashboard_summary(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::summary_strip(&env_.data)),
        Err(e) => err_card(format!("jobs summary: {e}")),
    }
}

pub async fn running_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::running(env.as_deref(), &args)).await {
        Ok(mut env_) => {
            filter_job_items(&mut env_.data, q.0.status.as_deref(), q.0.owner.as_deref());
            html(views::jobs::running_table(&env_.data))
        }
        Err(e) => err_card(format!("jobs running: {e}")),
    }
}

pub async fn history_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::history(env.as_deref(), &args)).await {
        Ok(mut env_) => {
            filter_job_items(&mut env_.data, q.0.status.as_deref(), q.0.owner.as_deref());
            sort_job_history(&mut env_.data, q.0.sort.as_deref().unwrap_or("finished"));
            html(views::jobs::history_table(
                &env_.data,
                q.0.source.as_deref().unwrap_or(&state.default_source),
                q.0.since.as_deref().unwrap_or("7d"),
                q.0.status.as_deref().unwrap_or("all"),
                q.0.owner.as_deref().unwrap_or(""),
            ))
        }
        Err(e) => err_card(format!("jobs history: {e}")),
    }
}

pub async fn top_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::top(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::top_table(
            &env_.data,
            q.0.source.as_deref().unwrap_or(&state.default_source),
            q.0.since.as_deref().unwrap_or("7d"),
        )),
        Err(e) => err_card(format!("jobs top: {e}")),
    }
}

pub async fn queued_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::queued(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::queued_table(&env_.data)),
        Err(e) => err_card(format!("jobs queued: {e}")),
    }
}

pub async fn owners_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::owners(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::owners_table(&env_.data)),
        Err(e) => err_card(format!("jobs owners: {e}")),
    }
}

fn filter_job_items(data: &mut serde_json::Value, status: Option<&str>, owner: Option<&str>) {
    let Some(items) = data
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let status = status.unwrap_or("all").to_ascii_lowercase();
    let owner = owner.unwrap_or("").trim().to_ascii_lowercase();
    items.retain(|item| {
        let item_status = item
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let item_owner = item
            .get("owner_email")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let status_ok = status == "all" || item_status == status;
        let owner_ok = owner.is_empty() || item_owner.contains(&owner);
        status_ok && owner_ok
    });
}

fn sort_job_history(data: &mut serde_json::Value, sort: &str) {
    let Some(items) = data
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    match sort {
        "duration" => items.sort_by(|a, b| {
            b.get("duration_ms")
                .and_then(serde_json::Value::as_u64)
                .cmp(&a.get("duration_ms").and_then(serde_json::Value::as_u64))
        }),
        "flow" => items.sort_by(|a, b| {
            a.get("flow_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .cmp(
                    b.get("flow_name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                )
        }),
        _ => items.sort_by(|a, b| {
            b.get("finished_at")
                .and_then(serde_json::Value::as_str)
                .cmp(&a.get("finished_at").and_then(serde_json::Value::as_str))
        }),
    }
}
