//! `telemetry errors recent` — failed job groups in the window, newest first,
//! with error messages truncated to a sane length for table rendering.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use chrono::Utc;
use serde_json::{Value, json};

use super::jobs::{duration_ms, fetch_job_groups, is_failure_status, within_window};
use super::server;
use super::source::TelemetrySource;
use super::window::Window;
use super::{TelemetryArgs, load_and_pick_source};

const MAX_ERROR_PREVIEW: usize = 160;

pub fn recent(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src == TelemetrySource::Server {
        return server::errors_recent(&config, args);
    }
    let window = Window::parse(&args.since)?;
    let page = fetch_job_groups(&config, args)?;
    let mut failed: Vec<&_> = page
        .items
        .iter()
        .filter(|j| within_window(j, &window))
        .filter(|j| is_failure_status(j.status.as_deref()))
        .collect();
    failed.sort_by(|a, b| {
        b.started_at
            .as_deref()
            .or(b.created_at.as_deref())
            .cmp(&a.started_at.as_deref().or(a.created_at.as_deref()))
    });
    let take = args.top.max(1);
    let items: Vec<Value> = failed
        .into_iter()
        .take(take)
        .map(|j| {
            let preview = j
                .error
                .as_deref()
                .map(|s| truncate_for_table(s, MAX_ERROR_PREVIEW));
            json!({
                "id": j.id,
                "flow_id": j.flow_id,
                "flow_name": j.flow_name,
                "started_at": j.started_at,
                "finished_at": j.finished_at,
                "duration_ms": duration_ms(j),
                "owner_email": j.owner_email,
                "error": preview,
            })
        })
        .collect();

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry errors recent ({}): {} failure{}",
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

fn truncate_for_table(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.replace('\n', " ");
    }
    let mut out = String::with_capacity(max + 1);
    for ch in s.chars().take(max) {
        out.push(if ch == '\n' { ' ' } else { ch });
    }
    out.push('…');
    out
}
