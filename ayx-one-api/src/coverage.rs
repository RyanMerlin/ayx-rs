//! Alteryx One API coverage diff: live OpenAPI spec vs. wired inventory.
//! Alteryx One only — this module never references Alteryx Server.

use serde_json::Value;

use crate::inventory::inventory_endpoints_full;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MissingEndpoint {
    pub method: String,
    pub path: String,
    pub resource: String,
    pub summary: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StaleEndpoint {
    pub method: String,
    pub path: String,
    pub command: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CoverageReport {
    pub coverage_pct: f64,
    pub spec_operations: usize,
    pub inventory_operations: usize,
    pub covered: usize,
    pub missing: Vec<MissingEndpoint>,
    pub stale: Vec<StaleEndpoint>,
    pub unmatched_spec_paths: Vec<String>,
}

const HTTP_METHODS: &[&str] = &[
    "get", "put", "post", "delete", "patch", "head", "options", "trace",
];

/// Canonicalize an operation to `(UPPER_METHOD, canonical_path)`.
/// Anchors the path at `/v4`, drops query/fragment and trailing slash, and
/// replaces every `{param}` segment with `{}`. Returns `None` if no `/v4`.
pub fn canonical_op(method: &str, full_path: &str) -> Option<(String, String)> {
    let no_q = full_path.split(['?', '#']).next().unwrap_or(full_path);
    let trimmed = no_q.trim_end_matches('/');
    let idx = trimmed.find("/v4/").or_else(|| {
        if trimmed.ends_with("/v4") {
            Some(trimmed.len() - 3)
        } else {
            None
        }
    })?;
    let from_v4 = &trimmed[idx..];
    let canon = from_v4
        .split('/')
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 2 {
                "{}"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    Some((method.to_ascii_uppercase(), canon))
}

/// Base path component from the spec's first `servers[].url` (or empty).
fn spec_base_path(spec: &Value) -> String {
    let url = spec
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // Strip scheme://host, keep path. Cheap parse: take substring after the 3rd '/'.
    if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        match rest.find('/') {
            Some(i) => rest[i..].trim_end_matches('/').to_string(),
            None => String::new(),
        }
    } else {
        url.trim_end_matches('/').to_string()
    }
}

fn resource_of(canonical_path: &str) -> String {
    // /v4/<resource>/... -> <resource>
    canonical_path
        .strip_prefix("/v4/")
        .unwrap_or(canonical_path)
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

pub fn coverage(spec: &Value) -> CoverageReport {
    use std::collections::{HashMap, HashSet};

    let base = spec_base_path(spec);

    // Inventory canonical set + a lookup back to (method, path, command) for stale.
    let inv_full = inventory_endpoints_full();
    let mut inv_keys: HashSet<(String, String)> = HashSet::new();
    let mut inv_meta: HashMap<(String, String), (&'static str, &'static str, &'static str)> =
        HashMap::new();
    for (m, p, c) in &inv_full {
        if let Some(key) = canonical_op(m, p) {
            inv_keys.insert(key.clone());
            inv_meta.insert(key, (m, p, c));
        }
    }

    let mut spec_keys: HashSet<(String, String)> = HashSet::new();
    let mut missing: Vec<MissingEndpoint> = Vec::new();
    let mut unmatched: Vec<String> = Vec::new();
    let mut spec_ops = 0usize;

    if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
        for (path_key, item) in paths {
            let full = format!("{base}{path_key}");
            let Some(methods) = item.as_object() else {
                continue;
            };
            for (verb, op) in methods {
                if !HTTP_METHODS.contains(&verb.to_ascii_lowercase().as_str()) {
                    continue;
                }
                spec_ops += 1;
                let Some((m, canon)) = canonical_op(verb, &full) else {
                    unmatched.push(format!("{} {}", verb.to_ascii_uppercase(), path_key));
                    continue;
                };
                let key = (m.clone(), canon.clone());
                spec_keys.insert(key.clone());
                if !inv_keys.contains(&key) {
                    missing.push(MissingEndpoint {
                        method: m,
                        path: path_key.clone(),
                        resource: resource_of(&canon),
                        summary: op
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        operation_id: op
                            .get("operationId")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
            }
        }
    }

    let mut stale: Vec<StaleEndpoint> = inv_keys
        .difference(&spec_keys)
        .filter_map(|key| inv_meta.get(key))
        .map(|(m, p, c)| StaleEndpoint {
            method: (*m).to_string(),
            path: (*p).to_string(),
            command: (*c).to_string(),
        })
        .collect();

    // Deterministic ordering for stable output + tests.
    missing
        .sort_by(|a, b| (&a.resource, &a.path, &a.method).cmp(&(&b.resource, &b.path, &b.method)));
    stale.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    unmatched.sort();

    let covered = spec_keys.intersection(&inv_keys).count();
    let coverage_pct = if spec_keys.is_empty() {
        100.0
    } else {
        (covered as f64 / spec_keys.len() as f64 * 1000.0).round() / 10.0
    };

    CoverageReport {
        coverage_pct,
        spec_operations: spec_ops,
        inventory_operations: inv_keys.len(),
        covered,
        missing,
        stale,
        unmatched_spec_paths: unmatched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec_with(paths: Value) -> Value {
        json!({ "openapi": "3.0.0", "servers": [{ "url": "https://x/" }], "paths": paths })
    }

    #[test]
    fn canonical_collapses_params_and_case() {
        assert_eq!(
            canonical_op("get", "/v4/flows/{flowId}"),
            Some(("GET".into(), "/v4/flows/{}".into()))
        );
    }

    #[test]
    fn param_name_drift_is_covered_not_missing() {
        // Inventory has GET /v4/flows/{id}; spec exposes GET /v4/flows/{flowId}.
        let spec = spec_with(json!({ "/v4/flows/{flowId}": { "get": { "summary": "Get flow" } } }));
        let r = coverage(&spec);
        assert!(r.missing.iter().all(|m| m.path != "/v4/flows/{flowId}"));
    }

    #[test]
    fn spec_only_op_is_missing() {
        let spec = spec_with(json!({
            "/v4/importedDatasets": { "post": { "summary": "Upload", "operationId": "createImported" } }
        }));
        let r = coverage(&spec);
        let m = r
            .missing
            .iter()
            .find(|m| m.path == "/v4/importedDatasets" && m.method == "POST")
            .expect("should be missing");
        assert_eq!(m.resource, "importedDatasets");
        assert_eq!(m.summary.as_deref(), Some("Upload"));
        assert_eq!(m.operation_id.as_deref(), Some("createImported"));
    }

    #[test]
    fn inventory_only_op_is_stale() {
        // A spec that exposes nothing the inventory has -> everything wired is stale.
        let spec = spec_with(json!({ "/v4/nonexistent": { "get": {} } }));
        let r = coverage(&spec);
        assert!(
            !r.stale.is_empty(),
            "wired endpoints absent from spec must be stale"
        );
        assert!(r.stale.iter().all(|s| !s.command.is_empty()));
    }

    #[test]
    fn base_path_relative_spec_is_anchored() {
        // servers URL carries /v4; paths are relative.
        let spec = json!({
            "servers": [{ "url": "https://host/v4" }],
            "paths": { "/flows/{id}": { "get": {} } }
        });
        let r = coverage(&spec);
        assert!(
            r.unmatched_spec_paths.is_empty(),
            "relative /flows must anchor to /v4/flows"
        );
        assert!(r.missing.iter().all(|m| m.path != "/flows/{id}"));
    }

    #[test]
    fn non_v4_path_is_unmatched_not_dropped() {
        let spec = json!({ "servers": [{ "url": "https://host" }], "paths": { "/health": { "get": {} } } });
        let r = coverage(&spec);
        assert!(r.unmatched_spec_paths.iter().any(|p| p.contains("/health")));
    }

    #[test]
    fn inventory_has_no_duplicate_canonical_keys() {
        use std::collections::{HashMap, HashSet};

        // Known, intentional command aliases that legitimately share a canonical
        // (METHOD, path) key. Two distinct CLI commands wire the same live
        // endpoint on purpose:
        //   - `one platform user` / `one platform person current` -> GET /v4/people/current
        //   - `one platform workspace configuration` / `...configuration-v4` -> GET /v4/workspaces/{}/configuration
        // This allowlist exists so the test still catches *new, accidental*
        // collisions (a real bug) without weakening detection or silently
        // deduping pre-existing, deliberate aliases. Any duplicate key not in
        // this list fails the test.
        let allowlisted_duplicates: HashSet<(&str, &str)> = [
            ("GET", "/v4/people/current"),
            ("GET", "/v4/workspaces/{}/configuration"),
        ]
        .into_iter()
        .collect();

        let mut by_key: HashMap<(String, String), Vec<&'static str>> = HashMap::new();
        for (m, p, c) in inventory_endpoints_full() {
            // Not every wired endpoint belongs to the /v4 API family — `iam/v1`,
            // `plans/v1`, `scheduling/v1`, and `billing/v1` are separate versioned
            // surfaces within Alteryx One, outside the /v4 OpenAPI spec this tool
            // diffs against. Those are out of canonical-coverage scope by design
            // (`canonical_op` returns `None`), so skip them rather than assume
            // every path anchors at /v4.
            let Some(key) = canonical_op(m, p) else {
                continue;
            };
            by_key.entry(key).or_default().push(c);
        }

        for (key, commands) in &by_key {
            if commands.len() <= 1 {
                continue;
            }
            let key_ref = (key.0.as_str(), key.1.as_str());
            assert!(
                allowlisted_duplicates.contains(&key_ref),
                "unexpected duplicate canonical inventory key {key:?} -> {commands:?} \
                 (not in the known-alias allowlist; either fix the wiring or add it \
                 to `allowlisted_duplicates` with a documented reason)"
            );
        }
    }
}
