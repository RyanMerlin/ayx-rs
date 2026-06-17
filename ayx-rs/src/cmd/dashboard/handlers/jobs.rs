use axum::extract::State;
use axum::response::Response;

use crate::cmd::dashboard::resolve_dashboard_profile;
use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{PanelQ, build_args, run_envelope};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::jobs;

use super::{err_card, html, profile_err_card};

pub async fn page(State(state): State<SharedState>, q: PanelQ) -> Response {
    let selected_profile = q.0.profile.as_deref().or(state.selected_profile.as_deref());
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => {
            return html(views::layout(
                "jobs",
                "/jobs",
                views::profile_error_card(&error.message, &error.to_value()),
                &state.default_source,
                state.poll_secs,
                state.environment.as_deref(),
                selected_profile,
                state.profile_resolution.as_ref(),
                &state.available_profiles,
                state.remote_mode,
            ));
        }
    };
    html(views::layout(
        "jobs",
        "/jobs",
        views::jobs::page(
            &state.default_source,
            state.poll_secs,
            state.environment.as_deref(),
            q.0.since.as_deref().unwrap_or("7d"),
            q.0.status.as_deref().unwrap_or("all"),
            q.0.owner.as_deref().unwrap_or(""),
            q.0.sort.as_deref().unwrap_or("finished"),
            selected_profile,
        ),
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
        selected_profile,
        Some(&resolved),
        &state.available_profiles,
        state.remote_mode,
    ))
}

pub async fn summary_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
    let env = state.environment.clone();
    match run_envelope(move || jobs::dashboard_summary(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::summary_strip(&env_.data)),
        Err(e) => err_card(format!("jobs summary: {e}")),
    }
}

pub async fn running_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
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
    let selected_profile = q.0.profile.as_deref().or(state.selected_profile.as_deref());
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
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
                selected_profile,
            ))
        }
        Err(e) => err_card(format!("jobs history: {e}")),
    }
}

pub async fn top_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let selected_profile = q.0.profile.as_deref().or(state.selected_profile.as_deref());
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
    let env = state.environment.clone();
    match run_envelope(move || jobs::top(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::top_table(
            &env_.data,
            q.0.source.as_deref().unwrap_or(&state.default_source),
            q.0.since.as_deref().unwrap_or("7d"),
            selected_profile,
        )),
        Err(e) => err_card(format!("jobs top: {e}")),
    }
}

pub async fn queued_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
    let env = state.environment.clone();
    match run_envelope(move || jobs::queued(env.as_deref(), &args)).await {
        Ok(env_) => html(views::jobs::queued_table(&env_.data)),
        Err(e) => err_card(format!("jobs queued: {e}")),
    }
}

pub async fn owners_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return profile_err_card(&error),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
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
