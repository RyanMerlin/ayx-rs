//! `one open`: deep-link the Alteryx One web console for a resource.
//!
//! Only kinds whose web path has been verified in a browser are wired. Others
//! return a validation envelope that tells the caller where to look.

use std::io::IsTerminal;

use anyhow::{Result, anyhow};
use ayx_core::envelope::{Envelope, ErrorCode};
use serde_json::json;

use super::RuntimeCtx;

/// Kinds with a browser-verified web path. Add a kind here only after opening
/// the constructed URL in a browser against a real tenant.
pub const VERIFIED_KINDS: &[&str] = &["workspace", "workflow"];

/// Build the deep-link URL for `kind`/`id` under the One console `base` URL.
///
/// Boxes the `Envelope` error: `Envelope` is large enough (several `String`s,
/// a `serde_json::Value`, and an `Option<Remediation>`) to trip clippy's
/// `result_large_err` — the same fix already applied to
/// `apply_jq_or_passthrough` in `main.rs`.
pub fn build_url(base: &str, kind: &str, id: &str) -> Result<String, Box<Envelope>> {
    let base = base.trim_end_matches('/');
    match kind {
        "workspace" => Ok(format!("{base}/?workspaceGid={id}")),
        "workflow" => Ok(format!("{base}/ayx-one/cloud-native/workflows/{id}")),
        other => Err(Box::new(
            Envelope::err_coded(
                ErrorCode::Validation,
                format!("no verified web path for kind `{other}`"),
                json!({ "kind": other, "id": id, "verified_kinds": VERIFIED_KINDS }),
            )
            .with_remediation(
                format!("Open {base} in a browser and search for {id}"),
                Vec::new(),
            ),
        )),
    }
}

pub fn execute(
    runtime: &RuntimeCtx<'_>,
    kind: String,
    id: Option<String>,
    print: bool,
) -> Result<Envelope> {
    let config = runtime.load_profile_lenient(None)?;
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("validation: `one open` requires an alteryx_one profile"))?;
    let base = ayx_one_api::configured_one_base_url(&config).ok_or_else(|| {
        anyhow!(
            "validation: this profile has no Alteryx One base URL, so `one open` cannot pick a tenant; set `alteryx_one.base_url` in the profile or export AYX_ONE_BASE_URL"
        )
    })?;
    let id = match (kind.as_str(), id) {
        ("workspace", None) => one
            .resolved_workspace_gid()
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow!(
                    "validation: this profile has no active workspace GID; pass the GID explicitly"
                )
            })?,
        (_, Some(id)) => id,
        (kind, None) => return Err(anyhow!("validation: `one open {kind}` requires an id")),
    };
    let url = match build_url(&base, &kind, &id) {
        Ok(url) => url,
        Err(envelope) => return Ok(*envelope),
    };
    let launch = !print && !runtime.no_input && std::io::stdout().is_terminal();
    let launched = launch && open::that(&url).is_ok();
    Ok(Envelope::ok_with_data(
        if launched {
            format!("opened {url}")
        } else {
            url.clone()
        },
        json!({ "kind": kind, "id": id, "url": url, "launched": launched }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_kinds_build_their_paths() {
        assert_eq!(
            build_url("https://us1.alteryxcloud.com/", "workspace", "01GID").unwrap(),
            "https://us1.alteryxcloud.com/?workspaceGid=01GID"
        );
        assert_eq!(
            build_url("https://us1.alteryxcloud.com", "workflow", "01ULID").unwrap(),
            "https://us1.alteryxcloud.com/ayx-one/cloud-native/workflows/01ULID"
        );
    }

    #[test]
    fn unverified_kind_is_a_validation_envelope_with_a_hint() {
        let env = build_url("https://us1.alteryxcloud.com", "flow", "42").unwrap_err();
        assert!(!env.ok);
        assert_eq!(
            env.error_code,
            Some(ayx_core::envelope::ErrorCode::Validation)
        );
        let remediation = env.remediation.expect("remediation present");
        assert!(remediation.summary.contains("https://us1.alteryxcloud.com"));
        assert!(remediation.summary.contains("42"));
        assert_eq!(
            env.data["verified_kinds"],
            serde_json::json!(VERIFIED_KINDS)
        );
    }
}
