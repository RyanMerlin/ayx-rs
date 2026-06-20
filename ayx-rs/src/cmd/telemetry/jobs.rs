//! Job-group telemetry: running / history / top.
//!
//! Pages `/v4/jobLibrary`, normalizes via `JobGroupListPage`, then filters
//! and aggregates in Rust. All read-only — no `--apply` gate.

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::types::{JobGroupListPage, JobGroupSummary};
use ayx_one_api::{OneListParams, one_api_list_request};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::aggregate::{DurationStats, WeeklyMatrix};
use super::server;
use super::source::TelemetrySource;
use super::window::Window;
use super::{TelemetryArgs, load_and_pick_source};

pub fn running(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src == TelemetrySource::Server {
        return server::jobs_running(&config);
    }
    let page = fetch_job_groups(&config, args)?;
    let running: Vec<&JobGroupSummary> = page
        .items
        .iter()
        .filter(|j| is_running_status(j.status.as_deref()))
        .collect();

    let items: Vec<Value> = running
        .iter()
        .map(|j| {
            json!({
                "id": j.id,
                "flow_id": j.flow_id,
                "flow_name": j.flow_name,
                "status": j.status,
                "created_at": j.created_at,
                "started_at": j.started_at,
                "owner_email": j.owner_email,
                "wait_ms": wait_ms(j),
            })
        })
        .collect();

    Ok(Envelope::ok_with_data(
        format!("telemetry jobs running: {} active", items.len()),
        json!({
            "source": src.as_str(),
            "generated_at": Utc::now().to_rfc3339(),
            "items": items,
        }),
    ))
}

pub fn history(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src == TelemetrySource::Server {
        return server::jobs_history(&config, args);
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut in_window: Vec<&JobGroupSummary> = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .collect();
    in_window.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let take = args.top.max(1);
    let items: Vec<Value> = in_window.into_iter().take(take).map(job_to_row).collect();

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry jobs history ({}): {} item{}",
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

pub fn top(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src == TelemetrySource::Server {
        // Server-side top-N aggregation isn't wired yet — return the raw
        // results_recent plan and let the operator run it. Phase 3 will
        // close the gap with a results-side aggregator.
        return server::jobs_history(&config, args);
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut rows: Vec<Value> = per_flow_stats(&page, &window)
        .into_iter()
        .map(|(flow_id, name, stats, count, failed)| {
            json!({
                "flow_id": flow_id,
                "flow_name": name,
                "run_count": count,
                "failed": failed,
                "failure_rate_pct": pct(failed, count),
                "mean_ms": stats.mean_ms,
                "p95_ms": stats.p95_ms,
                "max_ms": stats.max_ms,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b["run_count"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["run_count"].as_u64().unwrap_or(0))
    });
    let take = args.top.max(1);
    let items: Vec<Value> = rows.into_iter().take(take).collect();

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry jobs top ({}): {} flow{}",
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

// ─── Helpers (shared with workflows/errors/weekly via super module) ────────

pub(super) fn fetch_job_groups(
    config: &ayx_core::profile::Config,
    args: &TelemetryArgs,
) -> Result<JobGroupListPage> {
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(args.all, args.max_pages);
    let env = one_api_list_request(
        config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &params,
    )?;
    let items = env
        .data
        .get("items")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let next = env
        .data
        .get("next_page_token")
        .cloned()
        .unwrap_or(Value::Null);
    let normalized = json!({
        "items": items,
        "nextPageToken": next,
    });
    let page = JobGroupListPage::from_value(&normalized)
        .map_err(|e| anyhow!("failed to parse job-library response into JobGroupListPage: {e}"))?;
    Ok(page)
}

pub(super) fn is_running_status(s: Option<&str>) -> bool {
    matches!(
        s.map(|x| x.to_ascii_lowercase()).as_deref(),
        Some("running" | "queued" | "in_progress" | "inprogress")
    )
}

pub(super) fn is_failure_status(s: Option<&str>) -> bool {
    matches!(
        s.map(|x| x.to_ascii_lowercase()).as_deref(),
        Some("failed" | "error" | "errored")
    )
}

pub(super) fn within_window(j: &JobGroupSummary, window: &Window) -> bool {
    let ts = j.started_at.as_deref().or(j.created_at.as_deref());
    let Some(ts) = ts else { return false };
    match DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => dt.with_timezone(&Utc) >= window.since,
        Err(_) => false,
    }
}

pub(super) fn job_to_row(j: &JobGroupSummary) -> Value {
    json!({
        "id": j.id,
        "flow_id": j.flow_id,
        "flow_name": j.flow_name,
        "status": j.status,
        "started_at": j.started_at,
        "finished_at": j.finished_at,
        "duration_ms": duration_ms(j),
        "owner_email": j.owner_email,
        "error": j.error,
    })
}

pub(super) fn duration_ms(j: &JobGroupSummary) -> Option<u64> {
    if let Some(d) = j.duration_ms {
        return Some(d);
    }
    let start = DateTime::parse_from_rfc3339(j.started_at.as_deref()?).ok()?;
    let end = DateTime::parse_from_rfc3339(j.finished_at.as_deref()?).ok()?;
    let ms = (end - start).num_milliseconds();
    if ms < 0 { None } else { Some(ms as u64) }
}

pub(super) fn wait_ms(j: &JobGroupSummary) -> Option<u64> {
    let created = DateTime::parse_from_rfc3339(j.created_at.as_deref()?).ok()?;
    let end_ts = j
        .started_at
        .as_deref()
        .or(j.finished_at.as_deref())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let ms = (end_ts - created.with_timezone(&Utc)).num_milliseconds();
    if ms < 0 { None } else { Some(ms as u64) }
}

pub(super) fn pct(num: usize, denom: usize) -> f64 {
    if denom == 0 {
        0.0
    } else {
        100.0 * (num as f64) / (denom as f64)
    }
}

/// Build a weekly run-count matrix from the most recent job groups in the
/// given window. Used by `weekly run-counts` and `summary`.
pub(super) fn weekly_matrix(
    config: &ayx_core::profile::Config,
    args: &TelemetryArgs,
    window: &Window,
) -> Result<WeeklyMatrix> {
    let page = fetch_job_groups(config, args)?;
    let stamps: Vec<DateTime<Utc>> = page
        .items
        .iter()
        .filter(|j| within_window(j, window))
        .filter_map(|j| {
            j.started_at
                .as_deref()
                .or(j.created_at.as_deref())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
        })
        .collect();
    Ok(WeeklyMatrix::from_timestamps(&stamps))
}

/// Per-flow `DurationStats`. Returns `(flow_id, flow_name, stats, run_count, failure_count)`.
pub(super) fn per_flow_stats(
    page: &JobGroupListPage,
    window: &Window,
) -> Vec<(String, String, DurationStats, usize, usize)> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, Vec<&JobGroupSummary>> = BTreeMap::new();
    for j in page.items.iter().filter(|j| within_window(j, window)) {
        let key = j.flow_id.clone().unwrap_or_else(|| "<unknown>".into());
        groups.entry(key).or_default().push(j);
    }
    groups
        .into_iter()
        .map(|(flow_id, jobs)| {
            let durations: Vec<f64> = jobs
                .iter()
                .filter_map(|j| duration_ms(j))
                .map(|x| x as f64)
                .collect();
            let stats = DurationStats::from_durations_ms(&durations);
            let failed = jobs
                .iter()
                .filter(|j| is_failure_status(j.status.as_deref()))
                .count();
            let name = jobs
                .iter()
                .find_map(|j| j.flow_name.clone())
                .unwrap_or_default();
            (flow_id, name, stats, jobs.len(), failed)
        })
        .collect()
}
