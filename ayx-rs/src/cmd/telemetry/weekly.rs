//! `telemetry weekly run-counts` — emit a stable 7×24 matrix the next phase
//! (richer renderer or TUI heatmap) consumes directly.
//!
//! Per user direction the rendering of the heatmap itself is the next phase;
//! this command's job is to lock in a stable data contract.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use chrono::Utc;
use serde_json::json;

use super::jobs::weekly_matrix;
use super::window::Window;
use super::{OneTelemetryArgs, TelemetryArgs, load_and_pick_source};

pub fn run_counts(environment: Option<&str>, args: &OneTelemetryArgs) -> Result<Envelope> {
    let telemetry_args = TelemetryArgs::from(args);
    let (config, src) = load_and_pick_source(&telemetry_args, environment)?;
    let window = Window::parse(&args.since)?;
    let m = weekly_matrix(&config, &telemetry_args, &window)?;
    let matrix: Vec<_> = m
        .buckets
        .iter()
        .map(|b| {
            json!({
                "day_of_week": b.day_of_week,
                "hour": b.hour,
                "count": b.count,
            })
        })
        .collect();
    let total: u64 = m.buckets.iter().map(|b| b.count).sum();
    Ok(Envelope::ok_with_data(
        format!(
            "telemetry weekly run-counts ({}): {} runs across 168 buckets",
            window.label, total
        ),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "generated_at": Utc::now().to_rfc3339(),
            "total": total,
            "matrix": matrix,
        }),
    ))
}
