//! Plan telemetry: top by run-count, per-plan performance percentiles.
//!
//! Plans are first-class on the One side; jobs reference their parent plan
//! via the `plan_id` field on `JobGroupSummary`. We aggregate by `plan_id`.

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::aggregate::DurationStats;
use super::jobs::{duration_ms, fetch_job_groups, is_failure_status, pct, within_window};
use super::source::TelemetrySource;
use super::window::Window;
use super::{load_and_pick_source, TelemetryArgs};

pub fn top(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    aggregate_plans(environment, args, /*by_p95=*/ false)
}

pub fn performance(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    aggregate_plans(environment, args, /*by_p95=*/ true)
}

fn aggregate_plans(
    environment: Option<&str>,
    args: &TelemetryArgs,
    by_p95: bool,
) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry plans on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for j in page.items.iter().filter(|j| within_window(j, &window)) {
        let key = match &j.plan_id {
            Some(p) => p.clone(),
            None => continue,
        };
        groups.entry(key).or_default().push(j);
    }
    let mut rows: Vec<Value> = groups
        .into_iter()
        .map(|(plan_id, jobs)| {
            let durations: Vec<f64> = jobs
                .iter()
                .filter_map(|j| duration_ms(j))
                .map(|d| d as f64)
                .collect();
            let stats = DurationStats::from_durations_ms(&durations);
            let failed = jobs
                .iter()
                .filter(|j| is_failure_status(j.status.as_deref()))
                .count();
            json!({
                "plan_id": plan_id,
                "run_count": jobs.len(),
                "failed": failed,
                "failure_rate_pct": pct(failed, jobs.len()),
                "p50_ms": stats.p50_ms,
                "p95_ms": stats.p95_ms,
                "p99_ms": stats.p99_ms,
                "mean_ms": stats.mean_ms,
            })
        })
        .collect();

    if by_p95 {
        rows.sort_by(|a, b| {
            b["p95_ms"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["p95_ms"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        rows.sort_by(|a, b| {
            b["run_count"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["run_count"].as_u64().unwrap_or(0))
        });
    }
    let take = args.top.max(1);
    let items: Vec<Value> = rows.into_iter().take(take).collect();
    let label = if by_p95 { "performance" } else { "top" };
    Ok(Envelope::ok_with_data(
        format!(
            "telemetry plans {} ({}): {} plan{}",
            label,
            window.label,
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "generated_at": Utc::now().to_rfc3339(),
            "items": items,
        }),
    ))
}
