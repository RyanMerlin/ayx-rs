//! Server-side telemetry dispatchers (Phase 2).
//!
//! Composes existing pieces — the bundled Mongo query templates and the
//! Server-API V3 read endpoints — rather than introducing new transport.
//! Heavy aggregation runs client-side (`aggregate.rs`) over capped `find()`
//! result sets, matching the Phase-1 One-side pattern.

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use ayx_server::mongo::{MongoQuerySpec, mongo_query_spec_from_name, query_envelope};
use chrono::Utc;
use serde_json::{Value, json};

use super::TelemetryArgs;
use super::window::Window;

/// Build a Mongo spec for the named template and substitute the profile's
/// gallery/service database into it. The bundled templates default to the
/// canonical names (AlteryxService / AlteryxGallery); operators with custom
/// deploys can rename via `mongo.databases.{gallery_name,service_name}`.
pub(super) fn spec_for(config: &Config, template: &str) -> Result<MongoQuerySpec> {
    let mut spec = mongo_query_spec_from_name(template)
        .map_err(|e| anyhow!("failed to load Mongo template '{template}': {e}"))?;
    spec.database = match spec.database.as_str() {
        "AlteryxService" => config.mongo.databases.service_name.clone(),
        "AlteryxGallery" => config.mongo.databases.gallery_name.clone(),
        other => other.to_string(),
    };
    Ok(spec)
}

/// Wrap a query plan/execution envelope in the canonical telemetry shape
/// (`source`, `window`, `generated_at`) so the renderer treats it uniformly
/// with the One-side envelopes. When `mongosh` isn't on the operator's
/// PATH the underlying `query_envelope` returns the plan; we surface that
/// alongside a hint pointing at `ayx mongo query --template <name>`.
fn wrap_server(
    inner: Envelope,
    template: &str,
    window_label: Option<&str>,
    message: &str,
) -> Envelope {
    let mut data = match inner.data {
        Value::Object(_) => inner.data,
        other => json!({"raw": other}),
    };
    if let Value::Object(map) = &mut data {
        map.insert("source".into(), Value::String("server".into()));
        if let Some(label) = window_label {
            map.insert("window".into(), Value::String(label.to_string()));
        }
        map.insert(
            "generated_at".into(),
            Value::String(Utc::now().to_rfc3339()),
        );
        map.insert("template".into(), Value::String(template.to_string()));
        map.insert(
            "hint".into(),
            Value::String(format!(
                "execute with: ayx mongo query --template {template}"
            )),
        );
    }
    Envelope::ok_with_data(message.to_string(), data)
}

pub fn jobs_running(config: &Config) -> Result<Envelope> {
    let spec = spec_for(config, "queue_running")?;
    let env = query_envelope(
        config, &spec, /*print_query=*/ true, /*apply=*/ false,
    )?;
    Ok(wrap_server(
        env,
        "queue_running",
        None,
        "telemetry jobs running (server): plan generated",
    ))
}

pub fn jobs_history(config: &Config, args: &TelemetryArgs) -> Result<Envelope> {
    let window = Window::parse(&args.since)?;
    let spec = spec_for(config, "queue_recent")?;
    let env = query_envelope(config, &spec, true, false)?;
    Ok(wrap_server(
        env,
        "queue_recent",
        Some(&window.label),
        "telemetry jobs history (server): plan generated",
    ))
}

pub fn errors_recent(config: &Config, args: &TelemetryArgs) -> Result<Envelope> {
    let window = Window::parse(&args.since)?;
    let spec = spec_for(config, "results_errored")?;
    let env = query_envelope(config, &spec, true, false)?;
    Ok(wrap_server(
        env,
        "results_errored",
        Some(&window.label),
        "telemetry errors recent (server): plan generated",
    ))
}

pub fn queue_status(config: &Config) -> Result<Envelope> {
    let spec = spec_for(config, "queue_running")?;
    let env = query_envelope(config, &spec, true, false)?;
    Ok(wrap_server(
        env,
        "queue_running",
        None,
        "telemetry queue status (server): plan generated",
    ))
}

pub fn queue_wait_time(config: &Config, args: &TelemetryArgs) -> Result<Envelope> {
    let window = Window::parse(&args.since)?;
    let spec = spec_for(config, "queue_recent")?;
    let env = query_envelope(config, &spec, true, false)?;
    Ok(wrap_server(
        env,
        "queue_recent",
        Some(&window.label),
        "telemetry queue wait-time (server): plan generated",
    ))
}

pub fn plans_history(config: &Config, args: &TelemetryArgs) -> Result<Envelope> {
    let window = Window::parse(&args.since)?;
    let spec = spec_for(config, "schedule_run_history_raw")?;
    let env = query_envelope(config, &spec, true, false)?;
    Ok(wrap_server(
        env,
        "schedule_run_history_raw",
        Some(&window.label),
        "telemetry plans history (server): plan generated",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{MongoDatabases, MongoMode, MongoProfile};

    fn cfg() -> Config {
        Config {
            profile_name: "t".into(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "MyGallery".into(),
                    service_name: "MyService".into(),
                },
                embedded: None,
                managed: None,
            },
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn queue_running_targets_service_db_and_filters_by_state() {
        let spec = spec_for(&cfg(), "queue_running").unwrap();
        assert_eq!(spec.database, "MyService");
        assert_eq!(spec.collection, "AS_Queue");
        // State in [0, 1] = Queued | Running.
        let states = spec.filter["State"]["$in"].as_array().unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.contains(&json!(0)));
        assert!(states.contains(&json!(1)));
    }

    #[test]
    fn queue_recent_sorts_by_entered_queue_datetime_desc() {
        let spec = spec_for(&cfg(), "queue_recent").unwrap();
        assert_eq!(spec.collection, "AS_Queue");
        assert_eq!(
            spec.sort.as_ref().unwrap()["EnteredQueueDateTime"],
            json!(-1)
        );
        assert!(spec.limit.unwrap() >= 1000);
    }

    #[test]
    fn results_errored_filters_to_failure_statuses() {
        let spec = spec_for(&cfg(), "results_errored").unwrap();
        assert_eq!(spec.collection, "AS_Results");
        let statuses = spec.filter["Status"]["$in"].as_array().unwrap();
        let expected = ["Error", "Failed", "Failure"];
        for want in expected {
            assert!(
                statuses.iter().any(|v| v == &json!(want)),
                "expected status '{want}' in $in filter"
            );
        }
    }

    #[test]
    fn schedule_run_history_targets_gallery_db_with_projection() {
        let spec = spec_for(&cfg(), "schedule_run_history_raw").unwrap();
        assert_eq!(spec.database, "MyGallery");
        assert_eq!(spec.collection, "Schedules");
        let proj = spec.projection.as_ref().expect("projection set");
        // Ensure the headline run-history fields are projected.
        for f in ["frequency", "lastRunTime", "runCount", "lastError"] {
            assert_eq!(proj[f], json!(1), "expected projection field '{f}'");
        }
    }

    #[test]
    fn results_recent_targets_service_db_without_filter() {
        let spec = spec_for(&cfg(), "results_recent").unwrap();
        assert_eq!(spec.database, "MyService");
        assert_eq!(spec.collection, "AS_Results");
        // Permissive filter — client-side aggregation handles the window.
        assert_eq!(spec.filter, json!({}));
    }

    #[test]
    fn unknown_template_errors() {
        assert!(spec_for(&cfg(), "no_such_template").is_err());
    }
}
