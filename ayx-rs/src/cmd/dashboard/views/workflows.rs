//! Workflows page + partials.

use maud::{html, Markup};
use serde_json::Value;

use super::{esc_attr, fmt_f64, s_at, status_class};

pub fn page(
    source: &str,
    poll: u64,
    environment: Option<&str>,
    since: &str,
    sort: &str,
    owner: &str,
    health: &str,
) -> Markup {
    let filters = query_suffix(source, since, sort, owner, health);
    html! {
        section.hero.workflow-shell data-testid="workflow-rankings" {
            div.hero-grid {
                div.hero-copy {
                    div.eyebrow { "Workflows" }
                    h2 { "Workflow activity" }
                    p { "This page shows workflow volume, runtime performance, and recent errors." }
                }
                aside.stack {
                    div.context-card {
                        header.panel-head {
                            h3 { "Workflow context" }
                            span.muted { "source: " (source) }
                        }
                        (controls_form("/workflows", source, since, sort, owner, health))
                        div.list {
                            div.list-item {
                                div.kpi-line { strong { "Window" } span.small { "7d default" } }
                                div.small { "Default telemetry window for this view." }
                            }
                            div.list-item {
                                div.kpi-line { strong { "Refresh" } span.small { (poll) "s" } }
                                div.small { "Panels refresh on this interval." }
                            }
                            div.list-item {
                                div.kpi-line { strong { "Environment" } span.small { (environment.unwrap_or("—")) } }
                                div.small { "Current profile environment." }
                            }
                        }
                    }
                }
            }
        }
        section.panel {
            header.panel-head {
                h2 { "Workflow summary" }
                span.muted { "workflow-summary-strip" }
            }
            div
                id="wf-summary"
                hx-get={ "/workflows/summary?source=" (source) "&since=" (since) }
                hx-trigger={ "load, every " (poll) "s" }
                hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Top workflows" }
                span.muted { "ranked by run count" }
            }
            div
                id="wf-top"
                hx-get={ "/workflows/top?top=20&" (filters) }
                hx-trigger={ "load, every " (poll) "s" }
                hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Performance table" }
                span.muted { "p50 / p95 / p99 / max" }
            }
            div
                id="wf-perf"
                hx-get={ "/workflows/performance?" (filters) }
                hx-trigger="load"
                hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Recent errors" }
                span.muted { "most recent failed runs" }
            }
            div
                id="wf-errors"
                hx-get={ "/workflows/errors?top=8&" (filters) }
                hx-trigger={ "load, every " (poll) "s" }
                hx-swap="innerHTML"
            { "Loading…" }
        }
    }
}

pub fn summary_strip(data: &Value) -> Markup {
    html! {
        div.metric-strip {
            (summary_metric("Active workflows", &data["summary"]["active_flows"], "Distinct workflows in the selected window."))
            (summary_metric("Unhealthy", &data["summary"]["unhealthy_flows"], "Workflows with recent failures."))
            (summary_metric("Total runs", &data["summary"]["total_runs"], "All workflow runs in the selected window."))
            (summary_metric("Avg p95 ms", &data["summary"]["avg_p95_ms"], "Average p95 runtime across workflows."))
        }
    }
}

pub fn top_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No workflow activity in window." } };
    }
    let max_runs = items
        .iter()
        .filter_map(|w| w["run_count"].as_u64())
        .max()
        .unwrap_or(1);
    html! {
        div.rank-list {
            @for w in &items {
                @let id = s_at(w, "flow_id");
                @let runs = w["run_count"].as_u64().unwrap_or(0);
                @let width = ((runs as f64 / max_runs as f64) * 100.0).round();
                a.rank-row href={ "/workflows/" (id) } {
                    div.rank-main {
                        div.rank-title { (s_at(w, "flow_name")) }
                        div.rank-meta {
                            span { (s_at(w, "owner_email")) }
                            span { "last run " (s_at(w, "last_run_at")) }
                            span class={ "status " (status_class(&s_at(w, "last_status"))) } { (s_at(w, "last_status")) }
                        }
                    }
                    div.rank-bar {
                        span style={ "width:" (width) "%" } {}
                    }
                    div.rank-stats {
                        strong { (s_at(w, "run_count")) }
                        span.muted { (s_at(w, "failure_count")) " failed" }
                    }
                }
            }
        }
    }
}

pub fn performance_table(
    data: &Value,
    source: &str,
    since: &str,
    owner: &str,
    health: &str,
) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No performance data." } };
    }
    html! {
        table.data {
            thead { tr {
                th { (sort_link("/workflows", "Workflow", source, since, owner, health, "runs")) }
                th { (sort_link("/workflows", "Count", source, since, owner, health, "runs")) }
                th { "p50" }
                th { (sort_link("/workflows", "p95", source, since, owner, health, "duration")) }
                th { "p99" }
                th { "Max" }
            }}
            tbody {
                @for w in &items {
                    tr {
                        td { (s_at(w, "flow_name")) }
                        td.num { (s_at(w, "run_count")) }
                        td.num { (s_at(w, "p50_ms")) }
                        td.num { (s_at(w, "p95_ms")) }
                        td.num { (s_at(w, "p99_ms")) }
                        td.num { (s_at(w, "max_ms")) }
                    }
                }
            }
        }
    }
}

pub fn errors_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No recent errors." } };
    }
    html! {
        div.list {
            @for e in &items {
                article.list-item.error-item {
                    div.kpi-line {
                        strong { (s_at(e, "flow_name")) }
                        span class={ "status " (status_class(&s_at(e, "status"))) } { (s_at(e, "status")) }
                    }
                    div.small { (s_at(e, "owner_email")) " · " (s_at(e, "finished_at")) }
                    pre { (s_at(e, "error")) }
                }
            }
        }
    }
}

pub fn drilldown(data: &Value) -> Markup {
    let workflow = &data["workflow"];
    html! {
        section.hero.workflow-detail-shell data-testid="workflow-detail-shell" {
            header.panel-head {
                    div {
                        div.eyebrow { "Workflow detail" }
                        h2 { (s_at(workflow, "flow_name")) }
                    p.muted.mono { (s_at(workflow, "flow_id")) }
                }
                a.muted href="/workflows" { "← all workflows" }
            }
            div.metric-strip {
                (summary_metric("Runs", &workflow["run_count"], "Runs in the selected window."))
                (summary_metric("Failures", &workflow["failure_count"], "Failed runs in the same window."))
                (summary_metric("Avg ms", &workflow["mean_ms"], "Average runtime for runs with duration data."))
                (summary_metric("p95 ms", &workflow["p95_ms"], "p95 runtime for this workflow."))
                (summary_metric("Max ms", &workflow["max_ms"], "Longest observed runtime."))
                (summary_metric("Owner", &workflow["owner_email"], "Most recent owner email in telemetry."))
            }
        }
        section.panel {
            header.panel-head {
                h2 { "Recent trend" }
                span.muted { "runtime sparkline" }
            }
            (trend_chart(&data["recent_runs"]))
        }
        div.grid-2.detail-grid {
            section.panel {
                header.panel-head {
                    h2 { "Recent runs" }
                    span.muted { "latest 12" }
                }
                (runs_table(&data["recent_runs"]))
            }
            section.panel {
                header.panel-head {
                    h2 { "Performance" }
                    span.muted { "runtime profile" }
                }
                (performance_summary(&data["performance"], workflow))
            }
        }
        section.panel {
            header.panel-head {
                h2 { "Recent errors" }
                span.muted { "latest failures for this workflow" }
            }
            (errors_table(&serde_json::json!({ "items": data["recent_errors"].clone() })))
        }
    }
}

fn runs_table(data: &Value) -> Markup {
    let items = data.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No recent runs." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Job id" } th { "Status" } th { "Owner" } th { "Started" } th { "Duration ms" }
            }}
            tbody {
                @for run in &items {
                    tr {
                        td.mono { (s_at(run, "id")) }
                        td { span class={ "status " (status_class(&s_at(run, "status"))) } { (s_at(run, "status")) } }
                        td { (s_at(run, "owner_email")) }
                        td.muted { (s_at(run, "started_at")) }
                        td.num { (s_at(run, "duration_ms")) }
                    }
                }
            }
        }
    }
}

fn performance_summary(perf: &Value, workflow: &Value) -> Markup {
    html! {
        div.list {
            div.list-item {
                div.kpi-line { strong { "Last run" } span.small { (s_at(workflow, "last_run_at")) } }
                div.small { "Most recent run time for this workflow." }
            }
            div.list-item {
                div.kpi-line { strong { "Last status" } span class={ "status " (status_class(&s_at(workflow, "last_status"))) } { (s_at(workflow, "last_status")) } }
                div.small { "Status of the most recent run." }
            }
            div.list-item {
                div.kpi-line { strong { "Median / p99" } span.small { (s_at(perf, "p50_ms")) " / " (s_at(perf, "p99_ms")) " ms" } }
                div.small { "Median and p99 runtime." }
            }
            div.list-item {
                div.kpi-line { strong { "Min / max" } span.small { (s_at(perf, "min_ms")) " / " (s_at(perf, "max_ms")) " ms" } }
                div.small { "Shortest and longest observed runtime." }
            }
        }
    }
}

fn trend_chart(data: &Value) -> Markup {
    let items = data.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No recent runs." } };
    }
    let values: Vec<f64> = items
        .iter()
        .filter_map(|run| run.get("duration_ms").and_then(Value::as_f64))
        .collect();
    if values.is_empty() {
        return html! { p.empty { "No duration data." } };
    }
    let max = values.iter().copied().fold(0.0_f64, f64::max).max(1.0);
    let width = 640.0;
    let height = 140.0;
    let step = if values.len() > 1 {
        width / (values.len() as f64 - 1.0)
    } else {
        width
    };
    let points: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            let x = if values.len() > 1 {
                idx as f64 * step
            } else {
                width / 2.0
            };
            let y = height - (value / max * (height - 20.0)) - 10.0;
            format!("{x:.1},{y:.1}")
        })
        .collect();
    let fill_points = format!("0,{height} {} {width},{height}", points.join(" "));
    let stroke_points = points.join(" ");
    html! {
        div.trend-shell {
            svg.trend viewBox="0 0 640 140" role="img" aria-label="Recent workflow runtime trend" {
                defs {
                    linearGradient id="trend-fill" x1="0%" y1="0%" x2="0%" y2="100%" {
                        stop offset="0%" stop-color="#38bdf8" stop-opacity="0.35";
                        stop offset="100%" stop-color="#38bdf8" stop-opacity="0";
                    }
                }
                polyline fill="url(#trend-fill)" stroke="none" points=(fill_points);
                polyline fill="none" stroke="#7dd3fc" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" points=(stroke_points);
                @for (idx, run) in items.iter().enumerate() {
                    @let x = if values.len() > 1 { idx as f64 * step } else { width / 2.0 };
                    @let y = height - (run.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0) / max * (height - 20.0)) - 10.0;
                    @let fill = if run.get("status").and_then(Value::as_str).map(|s| s.to_ascii_lowercase()).as_deref() == Some("failed") {
                        "#fb7185"
                    } else {
                        "#ecf2ff"
                    };
                    circle cx=(format!("{x:.1}")) cy=(format!("{y:.1}")) r="3.5" fill=(fill);
                }
            }
            p.small { "Newest run is leftmost. Failures are marked in rose." }
        }
    }
}

fn summary_metric(label: &str, value: &Value, detail: &str) -> Markup {
    let rendered =
        if value.is_number() && value.as_f64().is_some() && !value.is_u64() && !value.is_i64() {
            fmt_f64(value)
        } else {
            s_at(&serde_json::json!({ "v": value }), "v")
        };
    html! {
        article.metric-card.mini {
            div.metric-value { (rendered) }
            div.metric-label { (label) }
            p.metric-copy { (detail) }
        }
    }
}

fn controls_form(
    action: &str,
    source: &str,
    since: &str,
    sort: &str,
    owner: &str,
    health: &str,
) -> Markup {
    html! {
        form.control-row method="get" action=(action) {
            input type="hidden" name="source" value=(source);
            label.control-group {
                span.small { "Window" }
                select name="since" {
                    (select_option("24h", since))
                    (select_option("7d", since))
                    (select_option("30d", since))
                }
            }
            label.control-group {
                span.small { "Rank by" }
                select name="sort" {
                    (select_option("runs", sort))
                    (select_option("failure-rate", sort))
                    (select_option("duration", sort))
                }
            }
            label.control-group {
                span.small { "Health" }
                select name="health" {
                    (select_option("all", health))
                    (select_option("unhealthy", health))
                    (select_option("healthy", health))
                }
            }
            label.control-group.owner-field {
                span.small { "Owner contains" }
                input type="text" name="owner" value=(owner) placeholder="email";
            }
            button.button-chip type="submit" { "Apply" }
        }
    }
}

fn select_option(value: &str, current: &str) -> Markup {
    html! {
        option value=(value) selected[current == value] { (value) }
    }
}

fn query_suffix(source: &str, since: &str, sort: &str, owner: &str, health: &str) -> String {
    format!(
        "source={}&since={}&sort={}&owner={}&health={}",
        esc_attr(source),
        esc_attr(since),
        esc_attr(sort),
        esc_attr(owner),
        esc_attr(health)
    )
}

fn sort_link(
    base: &str,
    label: &str,
    source: &str,
    since: &str,
    owner: &str,
    health: &str,
    next_sort: &str,
) -> Markup {
    html! {
        a href={ (base) "?source=" (esc_attr(source)) "&since=" (esc_attr(since)) "&sort=" (next_sort) "&owner=" (esc_attr(owner)) "&health=" (esc_attr(health)) } { (label) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drilldown_renders_detail_shell() {
        let payload = json!({
            "workflow": {
                "flow_id": "wf-123",
                "flow_name": "Invoice Sync",
                "owner_email": "ops@example.com",
                "last_run_at": "2026-05-12T12:00:00Z",
                "last_status": "Failed",
                "run_count": 42,
                "failure_count": 3,
                "mean_ms": 1200.0,
                "p95_ms": 2400.0,
                "max_ms": 4200.0
            },
            "recent_runs": [
                {
                    "id": "job-1",
                    "status": "Succeeded",
                    "owner_email": "ops@example.com",
                    "started_at": "2026-05-12T11:00:00Z",
                    "duration_ms": 1000
                }
            ],
            "recent_errors": [
                {
                    "flow_name": "Invoice Sync",
                    "status": "Failed",
                    "owner_email": "ops@example.com",
                    "finished_at": "2026-05-12T10:00:00Z",
                    "error": "timeout"
                }
            ],
            "performance": {
                "p50_ms": 900.0,
                "p99_ms": 3000.0,
                "min_ms": 500.0,
                "max_ms": 4200.0
            }
        });

        let rendered = drilldown(&payload).into_string();
        assert!(rendered.contains("workflow-detail-shell"));
        assert!(rendered.contains("Recent runs"));
        assert!(rendered.contains("Recent errors"));
        assert!(rendered.contains("Invoice Sync"));
    }
}
