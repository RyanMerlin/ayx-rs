//! maud HTML views — layout chrome + per-panel renderers.
//!
//! All renderers take `serde_json::Value` (the `Envelope::data` payload) so
//! the views are decoupled from the per-surface struct definitions. If a
//! field is missing the renderer prints "—" rather than failing.

use ayx_core::profile::RuntimeProfileResolution;
use maud::{html, Markup, DOCTYPE};
use serde_json::Value;

pub mod failures;
pub mod jobs;
pub mod overview;
pub mod workflows;

/// Page shell shared by every full-page route. Partial routes return a
/// bare `Markup` fragment instead so htmx can swap into the page in place.
#[allow(clippy::too_many_arguments)]
pub fn layout(
    title: &str,
    route: &str,
    body: Markup,
    source: &str,
    poll: u64,
    environment: Option<&str>,
    selected_profile: Option<&str>,
    profile_resolution: Option<&RuntimeProfileResolution>,
    profiles: &[String],
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="dark";
                meta name="theme-color" content="#050a14";
                title { "Alteryx One · " (title) }
                style { r#"
                    html, body {
                        margin: 0;
                        padding: 0;
                        min-height: 100%;
                        background: #050a14;
                        color: #ecf2ff;
                    }
                "# }
                link rel="stylesheet" href="/static/app.css";
                link rel="icon" type="image/svg+xml" href="/static/favicon.svg";
                script src="/static/htmx.min.js" {}
            }
            body {
                header.topbar {
                    div.brand {
                        a.wordmark href=(profile_href("/", selected_profile)) {
                            span.brand-blue { "Alteryx" }
                            span.brand-white { " One" }
                        }
                        div.brand-copy {
                            strong { "dashboard" }
                            span.muted { "operator telemetry surface" }
                        }
                    }
                    nav.tabs {
                        a class=(if title == "overview" { "active" } else { "" }) href=(profile_href("/", selected_profile)) { "Overview" }
                        a class=(if title == "jobs" { "active" } else { "" }) href=(profile_href("/jobs", selected_profile)) { "Jobs" }
                        a class=(if title == "failures" { "active" } else { "" }) href=(profile_href("/failures", selected_profile)) { "Failures" }
                        a class=(if title == "workflows" || title == "workflow" { "active" } else { "" }) href=(profile_href("/workflows", selected_profile)) { "Workflows" }
                    }
                    div.controls {
                        span.pill { "source: " strong { (source) } }
                        @if let Some(profile) = selected_profile {
                            span.pill { "profile: " strong { (profile) } }
                        }
                        @if let Some(resolution) = profile_resolution {
                            span.pill { "selection: " strong { (resolution.selection_source.as_str()) } }
                            span.pill { "active: " strong { (resolution.active_profile.as_deref().unwrap_or("—")) } }
                        }
                        @if let Some(env) = environment {
                            span.pill { "env: " strong { (env) } }
                        }
                        span.muted { "poll: " (poll) "s" }
                    }
                    @if !profiles.is_empty() {
                        nav.profile-switcher {
                            span.muted { "profile" }
                            @for profile in profiles {
                                a
                                    class=(if selected_profile == Some(profile.as_str()) { "active" } else { "" })
                                    href=(profile_href(route, Some(profile.as_str())))
                                {
                                    (profile)
                                }
                            }
                        }
                    }
                }
                main { (body) }
                footer.foot {
                    span.muted { "Alteryx One dashboard — central profile runtime surface · " (poll) "s panel polling" }
                }
            }
        }
    }
}

/// Render an error envelope into a small card so handlers don't have to
/// branch in every match arm.
pub fn error_card(message: &str) -> Markup {
    error_card_with_title("Telemetry error", message, None)
}

pub fn profile_error_card(message: &str, details: &Value) -> Markup {
    error_card_with_title("Profile resolution error", message, Some(details))
}

fn error_card_with_title(title: &str, message: &str, details: Option<&Value>) -> Markup {
    html! {
        div.error-card {
            div.eyebrow { (title) }
            pre { (message) }
            @if let Some(details) = details {
                dl.meta-grid {
                    @for (key, value) in details.as_object().into_iter().flat_map(|map| map.iter()) {
                        dt { (key) }
                        dd { (s(value)) }
                    }
                }
            }
        }
    }
}

pub fn profile_href(path: &str, selected_profile: Option<&str>) -> String {
    match selected_profile {
        Some(profile) if !profile.trim().is_empty() => {
            format!("{path}?profile={}", esc_attr(profile))
        }
        _ => path.to_string(),
    }
}

/// Helpers for safe optional-field rendering.
pub fn s(v: &Value) -> String {
    match v {
        Value::Null => "—".to_owned(),
        Value::String(s) => {
            if s.is_empty() {
                "—".to_owned()
            } else {
                s.clone()
            }
        }
        other => other.to_string(),
    }
}

pub fn s_at(obj: &Value, key: &str) -> String {
    s(obj.get(key).unwrap_or(&Value::Null))
}

pub fn fmt_f64(v: &Value) -> String {
    v.as_f64()
        .map(|n| format!("{n:.1}"))
        .unwrap_or_else(|| s(v))
}

pub fn esc_attr(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('@', "%40")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('?', "%3F")
}

pub fn status_class(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "running" => "running",
        "queued" => "warn",
        "succeeded" | "completed" | "success" => "ok",
        "failed" | "error" | "errored" => "err",
        "cancelled" => "muted",
        _ => "muted",
    }
}
