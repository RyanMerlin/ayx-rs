//! Failures-first dashboard surface.

use maud::{html, Markup};
use serde_json::Value;

use super::{esc_attr, s_at};

pub fn page(
    source: &str,
    poll: u64,
    environment: Option<&str>,
    since: &str,
    owner: &str,
) -> Markup {
    let filters = query_suffix(source, since, owner);
    html! {
        section.hero.overview-hero data-testid="failures-command-center" {
            div.hero-grid {
                div.hero-copy {
                    div.eyebrow { "Failures" }
                    h2 { "Failures, queue pressure, and long runners." }
                    p { "This page is tuned for diagnosis: actual errors, oldest queued jobs, and owners with the most activity are surfaced together." }
                }
                aside.stack {
                    div.context-card {
                        header.panel-head {
                            h3 { "Failure context" }
                            span.muted { "source: " (source) }
                        }
                        form.control-row method="get" action="/failures" {
                            input type="hidden" name="source" value=(source);
                            label.control-group.owner-field {
                                span.small { "Owner contains" }
                                input type="text" name="owner" value=(owner) placeholder="email";
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
                        div.list {
                            div.list-item {
                                div.kpi-line { strong { "Environment" } span.small { (environment.unwrap_or("—")) } }
                                div.small { "Profile-bound environment." }
                            }
                            div.list-item {
                                div.kpi-line { strong { "Window" } span.small { (since) } }
                                div.small { "Selected analysis window." }
                            }
                            div.list-item {
                                div.kpi-line { strong { "Refresh" } span.small { (poll) "s" } }
                                div.small { "Panels refresh on this cadence." }
                            }
                        }
                    }
                }
            }
        }
        div.grid-2.overview-lower {
            section.panel {
                header.panel-head {
                    h2 { "Recent errors" }
                    span.muted { "actual failure messages" }
                }
                div
                    id="failures-recent"
                    hx-get={ "/failures/recent?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
            section.panel {
                header.panel-head {
                    h2 { "Queue pressure" }
                    span.muted { "oldest queued jobs" }
                }
                div
                    id="failures-queued"
                    hx-get={ "/failures/queued?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
        }
        div.grid-2.overview-lower {
            section.panel {
                header.panel-head {
                    h2 { "Top users" }
                    span.muted { "runs, failures, queue age" }
                }
                div
                    id="failures-owners"
                    hx-get={ "/failures/owners?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
            section.panel {
                header.panel-head {
                    h2 { "Failed workflows" }
                    span.muted { "workflow metadata and failure rate" }
                }
                div
                    id="failures-workflows"
                    hx-get={ "/failures/workflows?" (filters) }
                    hx-trigger={ "load, every " (poll) "s" }
                    hx-swap="innerHTML"
                { "Loading…" }
            }
        }
    }
}

pub fn recent_errors_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No recent errors." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Flow" } th { "Owner" } th { "Started" } th { "Duration ms" } th { "Error" }
            }}
            tbody {
                @for e in &items {
                    tr {
                        td { (s_at(e, "flow_name")) }
                        td { (s_at(e, "owner_email")) }
                        td.muted { (s_at(e, "started_at")) }
                        td.num { (s_at(e, "duration_ms")) }
                        td.err { pre { (s_at(e, "error")) } }
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
                th { "Flow" } th { "Owner" } th { "Queued ms" } th { "Created" }
            }}
            tbody {
                @for j in &items {
                    tr {
                        td { (s_at(j, "flow_name")) }
                        td { (s_at(j, "owner_email")) }
                        td.num { (s_at(j, "wait_ms")) }
                        td.muted { (s_at(j, "created_at")) }
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

pub fn workflow_failures_table(data: &Value) -> Markup {
    let items = data["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return html! { p.empty { "No workflow failures." } };
    }
    html! {
        table.data {
            thead { tr {
                th { "Workflow" } th { "Owner" } th { "Failures" } th { "Failure rate" } th { "Last run" }
            }}
            tbody {
                @for w in &items {
                    tr {
                        td { (s_at(w, "flow_name")) }
                        td { (s_at(w, "owner_email")) }
                        td.num { (s_at(w, "failure_count")) }
                        td.num { (s_at(w, "failure_rate_pct")) "%" }
                        td.muted { (s_at(w, "last_run_at")) }
                    }
                }
            }
        }
    }
}

fn window_option(value: &str, current: &str) -> Markup {
    html! {
        option value=(value) selected[current == value] { (value) }
    }
}

fn query_suffix(source: &str, since: &str, owner: &str) -> String {
    format!(
        "source={}&since={}&owner={}",
        esc_attr(source),
        esc_attr(since),
        esc_attr(owner),
    )
}
