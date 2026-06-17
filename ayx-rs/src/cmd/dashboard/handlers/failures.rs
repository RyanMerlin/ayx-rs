use axum::extract::State;
use axum::response::Response;

use crate::cmd::dashboard::resolve_dashboard_profile;
use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{PanelQ, build_args, run_envelope};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::{errors, jobs, workflows};

use super::{err_card, html, profile_err_card};

pub async fn page(State(state): State<SharedState>, q: PanelQ) -> Response {
    let selected_profile = q.0.profile.as_deref().or(state.selected_profile.as_deref());
    if let Err(error) = resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        return html(views::layout(
            "failures",
            "/failures",
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
    html(views::layout(
        "failures",
        "/failures",
        views::failures::page(
            &state.default_source,
            state.poll_secs,
            state.environment.as_deref(),
            q.0.since.as_deref().unwrap_or("7d"),
            q.0.owner.as_deref().unwrap_or(""),
            selected_profile,
        ),
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
        selected_profile,
        state.profile_resolution.as_ref(),
        &state.available_profiles,
        state.remote_mode,
    ))
}

pub async fn recent_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
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
    match run_envelope(move || errors::recent(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::recent_errors_table(&env_.data)),
        Err(e) => err_card(format!("failures recent: {e}")),
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
        Ok(env_) => html(views::failures::queued_table(&env_.data)),
        Err(e) => err_card(format!("failures queued: {e}")),
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
        Ok(env_) => html(views::failures::owners_table(&env_.data)),
        Err(e) => err_card(format!("failures owners: {e}")),
    }
}

pub async fn workflow_failures_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
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
    match run_envelope(move || workflows::errors(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::workflow_failures_table(&env_.data)),
        Err(e) => err_card(format!("failures workflows: {e}")),
    }
}
