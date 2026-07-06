//! `ayx one api coverage` — diff the live One OpenAPI spec vs. wired inventory.
//! Alteryx One only.
use std::path::PathBuf;

use anyhow::{Context, Result};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{coverage, one_api_live_request};

use crate::cmd::RuntimeCtx;

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
            env.data.clone()
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
