//! `telemetry summary` — one envelope composing running/recent/top/errors.
//!
//! Useful as a first call for an operator or agent that wants the headline
//! shape of cluster activity in a single hop.

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use chrono::Utc;
use serde_json::{Value, json};

use super::jobs::{fetch_job_groups, is_failure_status, is_running_status, within_window};
use super::source::TelemetrySource;
use super::window::Window;
use super::{TelemetryArgs, load_and_pick_source};

pub fn summary(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry summary on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let in_window: Vec<_> = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .collect();
    let total = in_window.len();
    let running = page
        .items
        .iter()
        .filter(|j| is_running_status(j.status.as_deref()))
        .count();
    let failed = in_window
        .iter()
        .filter(|j| is_failure_status(j.status.as_deref()))
        .count();
    let succeeded = in_window
        .iter()
        .filter(|j| {
            matches!(
                j.status
                    .as_deref()
                    .map(|s| s.to_ascii_lowercase())
                    .as_deref(),
                Some("succeeded" | "completed" | "success")
            )
        })
        .count();
    let distinct_flows: std::collections::BTreeSet<_> =
        in_window.iter().filter_map(|j| j.flow_id.clone()).collect();

    let summary: Value = json!({
        "total_runs": total,
        "running": running,
        "succeeded": succeeded,
        "failed": failed,
        "distinct_flows": distinct_flows.len(),
        "failure_rate_pct": if total == 0 { 0.0 } else { 100.0 * failed as f64 / total as f64 },
    });

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry summary ({}): {} runs, {} running, {} failed",
            window.label, total, running, failed
        ),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "generated_at": Utc::now().to_rfc3339(),
            "summary": summary,
        }),
    ))
}
