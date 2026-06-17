use axum::extract::State;
use axum::response::Response;

use crate::cmd::dashboard::resolve_dashboard_profile;
use crate::cmd::dashboard::server::SharedState;
use crate::cmd::dashboard::telemetry_bridge::{PanelQ, build_args, run_envelope};
use crate::cmd::dashboard::views;
use crate::cmd::telemetry::summary;
use crate::cmd::telemetry::weekly;
use serde_json::json;

use super::html;

pub async fn index(State(state): State<SharedState>, q: PanelQ) -> Response {
    let selected_profile = q.0.profile.as_deref().or(state.selected_profile.as_deref());
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => {
            let body = views::profile_error_card(&error.message, &error.to_value());
            return html(views::layout(
                "overview",
                "/",
                body,
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
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
    let env = state.environment.clone();
    let result = run_envelope(move || summary::summary(env.as_deref(), &args)).await;

    let (summary_data, error_message) = match result {
        Ok(envelope) => (envelope.data, None),
        Err(e) => (
            json!({
                "source": state.default_source,
                "window": "7d",
                "generated_at": "—",
                "summary": {
                    "total_runs": "—",
                    "running": "—",
                    "failed": "—",
                    "distinct_flows": "—",
                    "succeeded": "—",
                    "failure_rate_pct": 0.0
                }
            }),
            Some(format!("overview: {e}")),
        ),
    };
    let body = views::overview::render(
        &summary_data,
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
        q.0.since.as_deref().unwrap_or("7d"),
        selected_profile,
        error_message.as_deref(),
    );
    html(views::layout(
        "overview",
        "/",
        body,
        &state.default_source,
        state.poll_secs,
        state.environment.as_deref(),
        selected_profile,
        Some(&resolved),
        &state.available_profiles,
        state.remote_mode,
    ))
}

pub async fn weekly_partial(State(state): State<SharedState>, q: PanelQ) -> Response {
    let resolved = match resolve_dashboard_profile(&state, q.0.profile.as_deref()) {
        Ok(resolution) => resolution,
        Err(error) => return html(views::profile_error_card(&error.message, &error.to_value())),
    };
    let args = build_args(
        &q.0,
        &state.default_source,
        Some(&resolved.selected_profile),
    );
    let env = state.environment.clone();
    match run_envelope(move || weekly::run_counts(env.as_deref(), &args)).await {
        Ok(env_) => html(views::overview::weekly_heatmap(&env_.data)),
        Err(e) => html(views::error_card(&format!("overview weekly: {e}"))),
    }
}
