use axum::extract::State;
use axum::response::Response;

use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{build_args, run_envelope, PanelQ};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::{errors, jobs, workflows};

use super::{err_card, html};

pub async fn page(State(state): State<SharedState>, q: PanelQ) -> Response {
    html(views::layout(
        "failures",
        views::failures::page(
            &state.default_source,
            state.poll_secs,
            state.environment.as_deref(),
            q.0.since.as_deref().unwrap_or("7d"),
            q.0.owner.as_deref().unwrap_or(""),
        ),
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
    ))
}

pub async fn recent_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || errors::recent(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::recent_errors_table(&env_.data)),
        Err(e) => err_card(format!("failures recent: {e}")),
    }
}

pub async fn queued_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::queued(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::queued_table(&env_.data)),
        Err(e) => err_card(format!("failures queued: {e}")),
    }
}

pub async fn owners_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || jobs::owners(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::owners_table(&env_.data)),
        Err(e) => err_card(format!("failures owners: {e}")),
    }
}

pub async fn workflow_failures_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let args = build_args(&q.0, &state.default_source, &state.profile_path);
    let env = state.environment.clone();
    match run_envelope(move || workflows::errors(env.as_deref(), &args)).await {
        Ok(env_) => html(views::failures::workflow_failures_table(&env_.data)),
        Err(e) => err_card(format!("failures workflows: {e}")),
    }
}
