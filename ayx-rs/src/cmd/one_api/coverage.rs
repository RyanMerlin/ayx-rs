//! `ayx one api coverage` — diff the live One OpenAPI spec vs. wired inventory.
//! Alteryx One only.
use std::path::PathBuf;

use anyhow::{Context, Result};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{coverage, one_api_live_request};

use crate::cmd::RuntimeCtx;

/// Unwrap the OpenAPI document from a live transport envelope.
///
/// `one_api_live_request` returns the parsed body nested under `response`,
/// alongside transport metadata (`elapsed_ms`, `error_code`, `response_shape`,
/// ...). Handing `env.data` straight to `coverage()` passed that *wrapper* as
/// the spec: it has no `paths` key, so every run reported
/// `spec_operations: 0` with an empty `missing` list, and `--check` could not
/// fail against the live spec no matter how far the CLI had drifted. The gate
/// was reporting success for work it had never done.
fn spec_body(data: &serde_json::Value) -> Option<&serde_json::Value> {
    data.get("response").filter(|v| !v.is_null())
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    profile: Option<String>,
    spec: Option<PathBuf>,
    check: bool,
) -> Result<Envelope> {
    // Obtain the OpenAPI document: from --spec file, or live.
    let spec_json: serde_json::Value = match spec {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading spec file {}", path.display()))?;
            serde_json::from_str(&text).context("parsing spec file as JSON")?
        }
        None => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let env = one_api_live_request(
                &config,
                "api",
                "open-api-spec",
                "GET",
                "/v4/open-api-spec",
                false,
                &[],
            )?;
            // Propagate an auth/network failure as-is rather than diffing garbage.
            if !env.ok {
                return Ok(env);
            }
            let Some(spec) = spec_body(&env.data) else {
                return Ok(Envelope::err_coded(
                    ErrorCode::Upstream,
                    "coverage failed: the open-api-spec response carried no body",
                    env.data.clone(),
                ));
            };
            spec.clone()
        }
    };

    let report = coverage(&spec_json);
    let missing_n = report.missing.len();
    let data = serde_json::to_value(&report).context("serializing coverage report")?;

    // `err_coded` yields ok=false, which `exit_code_for_envelope` maps to exit 1.
    // That is the `--check` CI regression gate.
    if check && missing_n > 0 {
        Ok(Envelope::err_coded(
            ErrorCode::Validation,
            format!("coverage incomplete: {missing_n} endpoint(s) missing"),
            data,
        ))
    } else {
        Ok(Envelope::ok_with_data("one api coverage", data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape `one_api_live_request` actually returns on success.
    fn live_envelope_data(body: serde_json::Value) -> serde_json::Value {
        json!({
            "elapsed_ms": 12,
            "error_code": null,
            "mutating": false,
            "response_shape": "json",
            "status": 200,
            "response": body,
        })
    }

    #[test]
    fn spec_is_read_from_the_response_body_not_the_transport_wrapper() {
        let spec = json!({ "openapi": "3.0.0", "paths": { "/v4/flows": { "get": {} } } });
        let data = live_envelope_data(spec.clone());

        assert_eq!(
            spec_body(&data),
            Some(&spec),
            "the spec must be unwrapped from `response`"
        );

        // The regression this guards: feeding the wrapper to `coverage()` finds
        // no `paths`, so the report is empty and `--check` cannot fail.
        let from_wrapper = coverage(&data);
        assert_eq!(
            from_wrapper.spec_operations, 0,
            "sanity: the wrapper has no `paths`, which is why passing it was silent"
        );
        assert!(from_wrapper.missing.is_empty());

        let from_body = coverage(spec_body(&data).unwrap());
        assert!(
            from_body.spec_operations > 0,
            "unwrapped, the same envelope must yield real spec operations"
        );
    }

    #[test]
    fn a_null_or_absent_response_body_is_not_treated_as_an_empty_spec() {
        assert!(spec_body(&live_envelope_data(serde_json::Value::Null)).is_none());
        assert!(spec_body(&json!({ "elapsed_ms": 3 })).is_none());
    }
}
