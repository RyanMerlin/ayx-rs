//! Overview landing — summary cards + a small running-jobs preview.

use maud::{html, Markup};
use serde_json::Value;

use super::{error_card, esc_attr, fmt_f64, profile_href, s_at};

pub fn render(
    summary: &Value,
    source: &str,
    poll: u64,
    environment: Option<&str>,
    since: &str,
    selected_profile: Option<&str>,
    error: Option<&str>,
) -> Markup {
    html! {
        section.hero.overview-hero data-testid="overview-hero" {
            div.hero-grid {
                div.hero-copy {
                    div.eyebrow { "Overview" }
                    h2 { "Dashboard overview" }
                    p { "This page summarizes current workload, active jobs, and recent failures." }
                    div.metrics {
                        (card("Runs In Window", &summary["summary"]["total_runs"], "Total runs in the selected window."))
                        (card("Running Jobs", &summary["summary"]["running"], "Jobs currently running or in progress."))
                        (card("Failed Runs", &summary["summary"]["failed"], "Runs that ended in failure."))
                        (card("Active Workflows", &summary["summary"]["distinct_flows"], "Distinct workflows with recent activity."))
                    }
                }
                aside.stack {
                    div.context-card {
                        header.panel-head {
                            h3 { "Current context" }
                            span.muted { "source: " strong { (summary["source"].as_str().unwrap_or(source)) } }
                        }
                        (window_form("/", source, since, selected_profile))
                        div.list {
                            div.list-item {
                                div.kpi-line {
                                    strong { "Environment" }
                                    span.small { (environment.unwrap_or("—")) }
                                }
                                div.small { "Current profile environment." }
                            }
                            div.list-item {
                                div.kpi-line {
                                    strong { "Window" }
                                    span.small { (summary["window"].as_str().unwrap_or("—")) }
                                }
                                div.small { "Telemetry time window for this page." }
                            }
                            div.list-item {
                                div.kpi-line {
                                    strong { "Refresh" }
                                    span.small { (poll) "s" }
                                }
                                div.small { "Auto-refresh interval for live panels." }
                            }
                            div.list-item {
                                div.kpi-line {
                                    strong { "Generated" }
                                    span.small.mono { (summary["generated_at"].as_str().unwrap_or("—")) }
                                }
                                div.small { "Last dashboard summary update." }
                            }
                        }
                    }
                    div.context-card {
                        header.panel-head {
                            h3 { "Health snapshot" }
                            span class={ "status " (health_class(summary["summary"]["failure_rate_pct"].as_f64().unwrap_or(0.0))) } { (health_label(summary["summary"]["failure_rate_pct"].as_f64().unwrap_or(0.0))) }
                        }
                        div.stacked {
                            div.kpi-line {
                                span.small { "Failure rate" }
                                strong { (fmt_f64(&summary["summary"]["failure_rate_pct"])) "%" }
                            }
                            div.kpi-line {
                                span.small { "Succeeded" }
                                strong { (s_at(&summary["summary"], "succeeded")) }
                            }
                            div.kpi-line {
                                span.small { "Failed / running" }
                                strong { (s_at(&summary["summary"], "failed")) " / " (s_at(&summary["summary"], "running")) }
                            }
                        }
                    }
                }
            }
        }
        @if let Some(message) = error {
            section.panel {
                (error_card(message))
            }
        }
        div.grid-2.overview-lower {
            section.panel {
                header.panel-head {
                    h2 { "Running jobs" }
                    span.muted { "live · refreshes every " (poll) "s" }
                }
                div
                    id="running-jobs"
                    hx-get={ "/jobs/running?" (query_suffix(source, since, selected_profile)) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
            section.panel {
                header.panel-head {
                    h2 { "Top workflows" }
                    a.muted href=(profile_href("/workflows", selected_profile)) { "view all →" }
                }
                div
                    id="top-workflows"
                    hx-get={ "/workflows/top?" (query_suffix(source, since, selected_profile)) "&top=5" }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
        }
        section.panel {
            header.panel-head {
                h2 { "Weekly activity" }
                span.muted { "run density by day and hour" }
            }
                div
                    id="overview-weekly"
                    hx-get={ "/overview/weekly?" (query_suffix(source, since, selected_profile)) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Attention queue" }
                span.muted { "recent failed jobs" }
            }
                div
                    id="overview-errors"
                    hx-get={ "/workflows/errors?" (query_suffix(source, since, selected_profile)) "&top=4" }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
            { "Loading…" }
        }
    }
}

pub fn weekly_heatmap(data: &Value) -> Markup {
    let matrix = data["matrix"].as_array().cloned().unwrap_or_default();
    if matrix.is_empty() {
        return html! { p.empty { "No weekly activity data." } };
    }
    let max = matrix
        .iter()
        .filter_map(|cell| cell["count"].as_u64())
        .max()
        .unwrap_or(1);
    let cell_w = 18u32;
    let cell_h = 16u32;
    let label_w = 30u32;
    let label_h = 22u32;
    let width = label_w + (24 * cell_w) + 8;
    let height = label_h + (7 * cell_h) + 12;
    let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    html! {
        div.heatmap-shell {
            svg.heatmap viewBox={ "0 0 " (width) " " (height) } role="img" aria-label="Weekly workflow activity heatmap" {
                @for hour in 0..24u32 {
                    text x=(label_w + hour * cell_w + 2) y="14" class="heatmap-label" { (hour) }
                }
                @for (day_idx, day_name) in days.iter().enumerate() {
                    text x="0" y=(label_h + (day_idx as u32 * cell_h) + 12) class="heatmap-label" { (day_name) }
                }
                @for cell in &matrix {
                    @let day = cell["day_of_week"].as_u64().unwrap_or(0) as u32;
                    @let hour = cell["hour"].as_u64().unwrap_or(0) as u32;
                    @let count = cell["count"].as_u64().unwrap_or(0);
                    @let x = label_w + hour * cell_w;
                    @let y = label_h + day * cell_h;
                    rect
                        x=(x)
                        y=(y)
                        width=(cell_w - 2)
                        height=(cell_h - 2)
                        rx="4"
                        fill=(heat_color(count, max));
                    title { (days[day as usize]) " " (hour) ":00 · " (count) " run" @if count != 1 { "s" } }
                }
            }
            p.small { "Darker cells indicate more runs in that day/hour bucket for the selected window." }
        }
    }
}

fn card(label: &str, val: &Value, detail: &str) -> Markup {
    html! {
        article.metric-card {
            div.metric-value { (s_at(&serde_json::json!({ "v": val }), "v")) }
            div.metric-label { (label) }
            p.metric-copy { (detail) }
        }
    }
}

fn health_class(rate: f64) -> &'static str {
    if rate >= 15.0 {
        "err"
    } else if rate >= 5.0 {
        "warn"
    } else {
        "ok"
    }
}

fn health_label(rate: f64) -> &'static str {
    if rate >= 15.0 {
        "degraded"
    } else if rate >= 5.0 {
        "watch"
    } else {
        "healthy"
    }
}

fn window_form(action: &str, source: &str, since: &str, selected_profile: Option<&str>) -> Markup {
    html! {
        form.control-row method="get" action=(action) {
            input type="hidden" name="source" value=(source);
            @if let Some(profile) = selected_profile {
                input type="hidden" name="profile" value=(profile);
            }
            label.control-group {
                span.small { "Window" }
                select name="since" {
                    (window_option("24h", since))
                    (window_option("7d", since))
                    (window_option("30d", since))
                }
            }
            button.button-chip type="submit" { "Apply" }
        }
    }
}

fn query_suffix(source: &str, since: &str, selected_profile: Option<&str>) -> String {
    match selected_profile {
        Some(profile) if !profile.trim().is_empty() => format!(
            "source={}&since={}&profile={}",
            esc_attr(source),
            esc_attr(since),
            esc_attr(profile),
        ),
        _ => format!("source={}&since={}", esc_attr(source), esc_attr(since)),
    }
}

fn window_option(value: &str, current: &str) -> Markup {
    html! {
        option value=(value) selected[current == value] { (value) }
    }
}

fn heat_color(count: u64, max: u64) -> String {
    if count == 0 || max == 0 {
        return "rgba(33, 47, 78, 0.65)".to_owned();
    }
    let ratio = count as f64 / max as f64;
    if ratio >= 0.8 {
        "#7dd3fc".to_owned()
    } else if ratio >= 0.5 {
        "#4fa9de".to_owned()
    } else if ratio >= 0.25 {
        "#326d9d".to_owned()
    } else {
        "#1f3557".to_owned()
    }
}
