//! Jobs page + partials.

use maud::{html, Markup};
use serde_json::Value;

use super::{esc_attr, s_at, status_class};

#[allow(clippy::too_many_arguments)]
pub fn page(
    source: &str,
    poll: u64,
    environment: Option<&str>,
    since: &str,
    status: &str,
    owner: &str,
    sort: &str,
    selected_profile: Option<&str>,
) -> Markup {
    let filters = query_suffix(source, since, status, owner, sort, selected_profile);
    html! {
        section.hero.jobs-shell data-testid="jobs-command-center" {
            div.hero-grid {
                div.hero-copy {
                    div.eyebrow { "Jobs" }
                    h2 { "Job activity" }
                    p { "This page shows running jobs, recent history, and workflows with the longest runtimes." }
                }
                aside.stack {
                    div.context-card {
                        header.panel-head {
                            h3 { "Operator context" }
                            span.muted { "jobs surface" }
                        }
                        (window_form("/jobs", source, since, status, owner, sort, selected_profile))
                        div.list {
                            div.list-item {
                                div.kpi-line { strong { "Source" } span.small { (source) } }
                                div.small { "Current telemetry source." }
                            }
                            div.list-item {
                                div.kpi-line { strong { "Refresh" } span.small { (poll) "s" } }
                                div.small { "Running jobs refresh on this interval." }
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
                h2 { "Execution snapshot" }
                span.muted { "jobs-summary-strip" }
            }
            div
                id="jobs-summary"
                hx-get={ "/jobs/summary?" (filters) }
                hx-trigger={ "load, every " (poll) "s" }
                hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Running jobs" }
                span.muted { "refreshes every " (poll) "s" }
            }
            div
                id="jobs-running"
                hx-get={ "/jobs/running?" (filters) }
                hx-trigger={ "load, every " (poll) "s" }
                hx-swap="innerHTML"
            { "Loading…" }
        }
        div.grid-2.overview-lower {
            section.panel {
                header.panel-head {
                    h2 { "Queue pressure" }
                    span.muted { "oldest queued jobs" }
                }
                div
                    id="jobs-queued"
                    hx-get={ "/jobs/queued?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
            section.panel {
                header.panel-head {
                    h2 { "Top users" }
                    span.muted { "runs, failures, queue age" }
                }
                div
                    id="jobs-owners"
                    hx-get={ "/jobs/owners?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
        }
        section.panel {
            header.panel-head {
                h2 { "Recent history" }
                span.muted { "filtered history" }
            }
            div
                id="jobs-history"
                hx-get={ "/jobs/history?" (filters) }
                hx-trigger="load"
                hx-swap="innerHTML"
            { "Loading…" }
        }
        section.panel {
            header.panel-head {
                h2 { "Longest-running workflows" }
                span.muted { "p95 and mean duration" }
            }
            div
                id="jobs-top"
                hx-get={ "/jobs/top?top=10&" (filters) }
                hx-trigger="load"
                hx-swap="innerHTML"
            { "Loading…" }
        }
    }
}

pub fn summary_strip(data: &Value) -> Markup {
    html! {
        div.metric-strip {
            (summary_metric("Running", &data["summary"]["running"], "Active job groups right now."))
            (summary_metric("Queued", &data["summary"]["queued"], "Jobs waiting to start."))
            (summary_metric("Recent failures", &data["summary"]["failed_recent"], "Failed jobs in the selected window."))
            (summary_metric("Throughput", &data["summary"]["throughput_runs"], "Total runs in the selected window."))
        }
    }
}

pub fn running_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No running jobs." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Flow" } th { "Status" } th { "Owner" } th { "Queued ms" } th { "Started" } th { "Job id" }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "flow_name")) }
                        td { span class={ "status " (status_class(&s_at(j, "status"))) } { (s_at(j, "status")) } }
                        td { (s_at(j, "owner_email")) }
                        td.num { (s_at(j, "wait_ms")) }
                        td.muted { (s_at(j, "started_at")) }
                        td.mono { (s_at(j, "id")) }
                    }
                }
            }
        }
    }
}

pub fn queued_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No queued jobs." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Flow" } th { "Owner" } th { "Queued" } th { "Wait ms" } th { "Job id" }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "flow_name")) }
                        td { (s_at(j, "owner_email")) }
                        td.muted { (s_at(j, "created_at")) }
                        td.num { (s_at(j, "wait_ms")) }
                        td.mono { (s_at(j, "id")) }
                    }
                }
            }
        }
    }
}

pub fn history_table(
    data: &Value,
    source: &str,
    since: &str,
    status: &str,
    owner: &str,
    selected_profile: Option<&str>,
) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No jobs in window." } };
    }
    html! {
        table.data {
            thead { tr {
                th { (sort_link("/jobs", "Flow", source, since, status, owner, "flow", selected_profile)) }
                th { "Status" }
                th { (sort_link("/jobs", "Duration (ms)", source, since, status, owner, "duration", selected_profile)) }
                th { "Owner" }
                th { (sort_link("/jobs", "Finished", source, since, status, owner, "finished", selected_profile)) }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "flow_name")) }
                        td { span class={ "status " (status_class(&s_at(j, "status"))) } { (s_at(j, "status")) } }
                        td.num { (s_at(j, "duration_ms")) }
                        td { (s_at(j, "owner_email")) }
                        td.muted { (s_at(j, "finished_at")) }
                    }
                }
            }
        }
    }
}

pub fn owners_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No owner data." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Owner" } th { "Runs" } th { "Failures" } th { "Longest run" } th { "Longest queue" }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "owner_email")) }
                        td.num { (s_at(j, "run_count")) }
                        td.num { (s_at(j, "failure_count")) }
                        td.num { (s_at(j, "longest_run_ms")) }
                        td.num { (s_at(j, "longest_queue_ms")) }
                    }
                }
            }
        }
    }
}

pub fn top_table(
    data: &Value,
    source: &str,
    since: &str,
    selected_profile: Option<&str>,
) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No data." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Flow" }
                th { (sort_link("/jobs", "Runs", source, since, "all", "", "runs", selected_profile)) }
                th { "Mean ms" }
                th { (sort_link("/jobs", "p95 ms", source, since, "all", "", "duration", selected_profile)) }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "flow_name")) }
                        td.num { (s_at(j, "run_count")) }
                        td.num { (s_at(j, "mean_ms")) }
                        td.num { (s_at(j, "p95_ms")) }
                    }
                }
            }
        }
    }
}

fn summary_metric(label: &str, value: &Value, detail: &str) -> Markup {
    html! {
        article.metric-card.mini {
            div.metric-value { (s_at(&serde_json::json!({ "v": value }), "v")) }
            div.metric-label { (label) }
            p.metric-copy { (detail) }
        }
    }
}

fn window_form(
    action: &str,
    source: &str,
    since: &str,
    status: &str,
    owner: &str,
    sort: &str,
    selected_profile: Option<&str>,
) -> Markup {
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
            label.control-group {
                span.small { "Status" }
                select name="status" {
                    (window_option("all", status))
                    (window_option("running", status))
                    (window_option("queued", status))
                    (window_option("succeeded", status))
                    (window_option("failed", status))
                }
            }
            label.control-group.owner-field {
                span.small { "Owner contains" }
                input type="text" name="owner" value=(owner) placeholder="email";
            }
            label.control-group {
                span.small { "History sort" }
                select name="sort" {
                    (window_option("finished", sort))
                    (window_option("duration", sort))
                    (window_option("flow", sort))
                }
            }
            button.button-chip type="submit" { "Apply" }
        }
    }
}

fn window_option(value: &str, current: &str) -> Markup {
    html! {
        option value=(value) selected[current == value] { (value) }
    }
}

fn query_suffix(
    source: &str,
    since: &str,
    status: &str,
    owner: &str,
    sort: &str,
    selected_profile: Option<&str>,
) -> String {
    let mut query = format!(
        "source={}&since={}&status={}&owner={}&sort={}",
        esc_attr(source),
        esc_attr(since),
        esc_attr(status),
        esc_attr(owner),
        esc_attr(sort)
    );
    if let Some(profile) = selected_profile.filter(|profile| !profile.trim().is_empty()) {
        query.push_str("&profile=");
        query.push_str(&esc_attr(profile));
    }
    query
}

#[allow(clippy::too_many_arguments)]
fn sort_link(
    base: &str,
    label: &str,
    source: &str,
    since: &str,
    status: &str,
    owner: &str,
    sort: &str,
    selected_profile: Option<&str>,
) -> Markup {
    html! {
        a href={ (base) "?source=" (esc_attr(source)) "&since=" (esc_attr(since)) "&status=" (esc_attr(status)) "&owner=" (esc_attr(owner)) "&sort=" (sort) @if let Some(profile) = selected_profile.filter(|profile| !profile.trim().is_empty()) { "&profile=" (esc_attr(profile)) } } { (label) }
    }
}
