//! Workflow-centric telemetry: top by run-count / failure-rate / duration,
//! per-workflow performance percentiles, error-ranked listings.
//!
//! All flavors pull job groups via `jobs::fetch_job_groups` and group by
//! `flow_id` client-side.

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::types::JobGroupSummary;
use chrono::Utc;
use serde_json::{json, Value};

use super::aggregate::DurationStats;
use super::jobs::{
    duration_ms, fetch_job_groups, is_failure_status, job_to_row, pct, per_flow_stats,
    within_window,
};
use super::source::TelemetrySource;
use super::window::Window;
use super::{load_and_pick_source, TelemetryArgs};

pub fn top(environment: Option<&str>, args: &TelemetryArgs, by: &str) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry workflows on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let stats = per_flow_stats(&page, &window);

    let mut rows: Vec<Value> = stats
        .iter()
        .map(|(flow_id, name, stats, run_count, failed)| {
            let meta = flow_meta(&page.items, flow_id);
            json!({
                "flow_id": flow_id,
                "flow_name": name,
                "run_count": run_count,
                "failure_count": failed,
                "failure_rate_pct": pct(*failed, *run_count),
                "p50_ms": stats.p50_ms,
                "p95_ms": stats.p95_ms,
                "p99_ms": stats.p99_ms,
                "owner_email": meta.owner_email,
                "last_run_at": meta.last_run_at,
                "last_status": meta.last_status,
            })
        })
        .collect();

    let key = by.to_ascii_lowercase();
    rows.sort_by(|a, b| {
        let score = |row: &Value| -> f64 {
            match key.as_str() {
                "failure-rate" | "failure_rate" => row["failure_rate_pct"].as_f64().unwrap_or(0.0),
                "p95-duration" | "p95_duration" | "duration" => {
                    row["p95_ms"].as_f64().unwrap_or(0.0)
                }
                _ => row["run_count"].as_u64().unwrap_or(0) as f64,
            }
        };
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let take = args.top.max(1);
    let items: Vec<Value> = rows.into_iter().take(take).collect();

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry workflows top --by {} ({}): {} flow{}",
            key,
            window.label,
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "sort_by": key,
            "generated_at": Utc::now().to_rfc3339(),
            "items": items,
        }),
    ))
}

pub fn performance(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry workflows on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let stats = per_flow_stats(&page, &window);
    let mut rows: Vec<Value> = stats
        .iter()
        .filter(|(_, _, s, _, _)| s.count > 0)
        .map(|(flow_id, name, stats, run_count, failed)| {
            json!({
                "flow_id": flow_id,
                "flow_name": name,
                "run_count": run_count,
                "failed": failed,
                "mean_ms": stats.mean_ms,
                "p50_ms": stats.p50_ms,
                "p95_ms": stats.p95_ms,
                "p99_ms": stats.p99_ms,
                "min_ms": stats.min_ms,
                "max_ms": stats.max_ms,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b["p95_ms"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["p95_ms"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let take = args.top.max(1);
    let items: Vec<Value> = rows.into_iter().take(take).collect();
    Ok(Envelope::ok_with_data(
        format!(
            "telemetry workflows performance ({}): {} flow{}",
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

pub fn errors(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry workflows on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut failed_jobs: Vec<&JobGroupSummary> = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .filter(|j| is_failure_status(j.status.as_deref()))
        .collect();
    failed_jobs.sort_by_key(|job| std::cmp::Reverse(stamp_of(job)));
    let take = args.top.max(1);
    let items: Vec<Value> = failed_jobs.into_iter().take(take).map(job_to_row).collect();

    let total_failed = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .filter(|j| is_failure_status(j.status.as_deref()))
        .count();

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry workflows errors ({}): {} failed job{}",
            window.label,
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "total_failed_jobs": total_failed,
            "generated_at": Utc::now().to_rfc3339(),
            "items": items,
        }),
    ))
}

pub fn dashboard_summary(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry workflows on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let stats = per_flow_stats(&page, &window);
    let active_flows = stats.len();
    let unhealthy_flows = stats
        .iter()
        .filter(|(_, _, _, _, failed)| *failed > 0)
        .count();
    let total_runs: usize = stats.iter().map(|(_, _, _, run_count, _)| *run_count).sum();
    let avg_p95 = average(stats.iter().filter_map(|(_, _, s, _, _)| s.p95_ms));
    let hot = stats
        .iter()
        .filter(|(_, _, _, run_count, _)| *run_count >= 3)
        .count();

    Ok(Envelope::ok_with_data(
        format!("telemetry workflows summary ({})", window.label),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "generated_at": Utc::now().to_rfc3339(),
            "summary": {
                "active_flows": active_flows,
                "unhealthy_flows": unhealthy_flows,
                "total_runs": total_runs,
                "avg_p95_ms": avg_p95,
                "hot_workflows": hot,
            },
        }),
    ))
}

pub fn detail(environment: Option<&str>, args: &TelemetryArgs, flow_id: &str) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::One {
        return Err(anyhow!(
            "validation: telemetry workflows on `server` source not implemented in this phase; pass --source one"
        ));
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut jobs: Vec<&JobGroupSummary> = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .filter(|j| j.flow_id.as_deref() == Some(flow_id))
        .collect();
    jobs.sort_by_key(|job| std::cmp::Reverse(stamp_of(job)));
    let durations: Vec<f64> = jobs
        .iter()
        .filter_map(|j| duration_ms(j))
        .map(|x| x as f64)
        .collect();
    let perf = DurationStats::from_durations_ms(&durations);
    let failures = jobs
        .iter()
        .filter(|j| is_failure_status(j.status.as_deref()))
        .count();
    let meta = flow_meta(&page.items, flow_id);
    let recent_runs: Vec<Value> = jobs.iter().take(12).map(|j| job_to_row(j)).collect();
    let recent_errors: Vec<Value> = jobs
        .iter()
        .filter(|j| is_failure_status(j.status.as_deref()))
        .take(6)
        .map(|j| job_to_row(j))
        .collect();

    Ok(Envelope::ok_with_data(
        format!("telemetry workflow detail {} ({})", flow_id, window.label,),
        json!({
            "source": src.as_str(),
            "window": window.label,
            "generated_at": Utc::now().to_rfc3339(),
            "workflow": {
                "flow_id": flow_id,
                "flow_name": meta.flow_name,
                "owner_email": meta.owner_email,
                "last_run_at": meta.last_run_at,
                "last_status": meta.last_status,
                "run_count": jobs.len(),
                "failure_count": failures,
                "failure_rate_pct": pct(failures, jobs.len()),
                "mean_ms": perf.mean_ms,
                "p50_ms": perf.p50_ms,
                "p95_ms": perf.p95_ms,
                "p99_ms": perf.p99_ms,
                "min_ms": perf.min_ms,
                "max_ms": perf.max_ms,
            },
            "recent_runs": recent_runs,
            "recent_errors": recent_errors,
            "performance": {
                "count": perf.count,
                "mean_ms": perf.mean_ms,
                "p50_ms": perf.p50_ms,
                "p95_ms": perf.p95_ms,
                "p99_ms": perf.p99_ms,
                "min_ms": perf.min_ms,
                "max_ms": perf.max_ms,
            }
        }),
    ))
}

struct FlowMeta {
    flow_name: String,
    owner_email: Option<String>,
    last_run_at: Option<String>,
    last_status: Option<String>,
}

fn flow_meta(items: &[JobGroupSummary], flow_id: &str) -> FlowMeta {
    let mut jobs: Vec<&JobGroupSummary> = items
        .iter()
        .filter(|j| j.flow_id.as_deref() == Some(flow_id))
        .collect();
    jobs.sort_by_key(|job| std::cmp::Reverse(stamp_of(job)));
    let flow_name = jobs
        .iter()
        .find_map(|j| j.flow_name.clone())
        .unwrap_or_else(|| "<unknown workflow>".to_owned());
    let owner_email = jobs.iter().find_map(|j| j.owner_email.clone());
    let last_run_at = jobs.iter().find_map(|j| stamp_of(j));
    let last_status = jobs.iter().find_map(|j| j.status.clone());
    FlowMeta {
        flow_name,
        owner_email,
        last_run_at,
        last_status,
    }
}

fn stamp_of(j: &JobGroupSummary) -> Option<String> {
    j.started_at
        .clone()
        .or_else(|| j.finished_at.clone())
        .or_else(|| j.created_at.clone())
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut count = 0usize;
    let mut sum = 0.0f64;
    for value in values {
        count += 1;
        sum += value;
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}
