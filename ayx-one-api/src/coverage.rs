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
    /// Every CLI command that dispatches this endpoint. Was a single `command`
    /// string; widened because one endpoint can legitimately back several commands.
    pub commands: Vec<String>,
}

/// A wired inventory endpoint that lives outside the namespace this spec can
/// describe, and is therefore neither `covered`, `missing`, nor `stale`.
///
/// The One gateway spec (`GET /v4/open-api-spec`) documents `/v4` only, but the
/// CLI also speaks several sibling services (`/svc-workflow`, `/plans/v1`,
/// `/scheduling/v1`, `/billing/v1`, `/iam/v1`). Those rows used to be dropped on
/// the floor, which understated `inventory_operations` and made it impossible to
/// tell "we compared this and it matched" from "we never compared this at all".
#[derive(Debug, Clone, serde::Serialize)]
pub struct UncomparableEndpoint {
    pub method: String,
    pub path: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CoverageReport {
    /// Percentage of *comparable* spec operations that the inventory wires.
    ///
    /// `None` when the spec contributes nothing comparable (empty, malformed, or
    /// entirely outside `/v4`). Reporting `100.0` there would read as total
    /// coverage when the truth is that nothing was compared at all.
    ///
    /// Scoped to the namespace both sides can express — see
    /// `outside_spec_namespace` for wired endpoints this figure deliberately
    /// excludes. It is not "percent of the CLI's One surface that is covered".
    pub coverage_pct: Option<f64>,
    /// Raw HTTP operations in the spec, before canonical collapse.
    pub spec_operations: usize,
    /// Distinct *canonical* inventory keys comparable against this spec.
    ///
    /// This is deliberately not `inventory_total - outside_spec_namespace.len()`.
    /// `canonical_op` strips query strings, so wired rows that differ only by
    /// query — `/v4/people` vs `/v4/people?role=admin`, `/v4/workflows` vs
    /// `/v4/workflows?limit=1` — collapse to one comparable key while remaining
    /// two distinct wired rows. A spec cannot distinguish them, so matching
    /// them separately is impossible; counting them separately here would
    /// overstate what was compared.
    pub inventory_operations: usize,
    /// Distinct wired inventory rows, by raw `(method, path)`, comparable or not.
    ///
    /// Always `>= inventory_operations + outside_spec_namespace.len()`, and
    /// strictly greater whenever query-only variants are wired (see above).
    pub inventory_total: usize,
    pub covered: usize,
    pub missing: Vec<MissingEndpoint>,
    pub stale: Vec<StaleEndpoint>,
    pub unmatched_spec_paths: Vec<String>,
    /// Wired endpoints outside the spec's namespace. Not a defect — these are
    /// real, working commands against sibling services — but they are unverified
    /// by this diff, so they are reported rather than hidden.
    pub outside_spec_namespace: Vec<UncomparableEndpoint>,
}

const HTTP_METHODS: &[&str] = &[
    "get", "put", "post", "delete", "patch", "head", "options", "trace",
];

/// Canonicalize an operation to `(UPPER_METHOD, canonical_path)`.
///
/// Drops query/fragment and trailing slash, and replaces every `{param}`
/// segment with `{}`. Returns `None` unless the path is rooted at `/v4` — the
/// only namespace the One gateway spec describes.
///
/// The `/v4` must be the path's *first* segment. An earlier version searched
/// for `/v4/` anywhere, so a sibling service's own v4 would be folded into the
/// gateway namespace: `/svc-workflow/api/v4/workflows` matched the gateway's
/// `GET /v4/workflows` row and reported as spec-verified. `/svc-workflow`
/// already ships `/api/v0`, `/api/v1`, and `/api/v2`, so that collision was one
/// version bump away, and it would have inverted the meaning of
/// `outside_spec_namespace` for every row it hit.
///
/// A spec whose `servers[].url` carries a base path is already normalized by
/// the caller before it gets here (`{base}{path}`), so `/v4` still lands first.
pub fn canonical_op(method: &str, full_path: &str) -> Option<(String, String)> {
    let no_q = full_path.split(['?', '#']).next().unwrap_or(full_path);
    let trimmed = no_q.trim_end_matches('/');
    if !(trimmed.starts_with("/v4/") || trimmed == "/v4") {
        return None;
    }
    let from_v4 = trimmed;
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
    let mut inv_meta: HashMap<(String, String), (&'static str, &'static str, Vec<&'static str>)> =
        HashMap::new();
    // A row that cannot be canonicalized is outside this spec's namespace, not
    // absent. Recording it keeps `inventory_total` honest and stops a whole
    // sibling service (all of `/svc-workflow`, say) from silently vanishing
    // from the report.
    let mut outside: Vec<UncomparableEndpoint> = Vec::new();
    for (m, p, c) in &inv_full {
        match canonical_op(m, p) {
            Some(key) => {
                inv_keys.insert(key.clone());
                // Merge, never overwrite. Rows that differ only by query string
                // share a canonical key, and a plain `insert` kept only the
                // last one — so a stale `/v4/workflows` reported `one workflows
                // count` while `one workflows list` vanished from the report,
                // telling the operator that half the affected commands were
                // fine.
                let entry = inv_meta.entry(key).or_insert_with(|| (m, p, Vec::new()));
                for name in c.iter() {
                    if !entry.2.contains(name) {
                        entry.2.push(name);
                    }
                }
            }
            None => outside.push(UncomparableEndpoint {
                method: (*m).to_string(),
                path: (*p).to_string(),
                commands: c.iter().map(|name| (*name).to_string()).collect(),
            }),
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
            commands: c.iter().map(|name| (*name).to_string()).collect(),
        })
        .collect();

    // Deterministic ordering for stable output + tests.
    missing
        .sort_by(|a, b| (&a.resource, &a.path, &a.method).cmp(&(&b.resource, &b.path, &b.method)));
    stale.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    unmatched.sort();
    outside.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    outside.dedup_by(|a, b| a.path == b.path && a.method == b.method);

    // Counted from the raw rows, independently of either bucket. The two
    // buckets use different dedup keys (canonical vs raw), so deriving this as
    // their sum would silently undercount every query-only variant.
    let inventory_total = inv_full
        .iter()
        .map(|(m, p, _)| (*m, *p))
        .collect::<HashSet<_>>()
        .len();

    let covered = spec_keys.intersection(&inv_keys).count();
    // Nothing comparable in the spec means no coverage figure exists. `100.0`
    // here would report a garbage or empty spec as fully covered.
    let coverage_pct = if spec_keys.is_empty() {
        None
    } else {
        Some((covered as f64 / spec_keys.len() as f64 * 1000.0).round() / 10.0)
    };

    CoverageReport {
        coverage_pct,
        spec_operations: spec_ops,
        inventory_operations: inv_keys.len(),
        inventory_total,
        covered,
        missing,
        stale,
        unmatched_spec_paths: unmatched,
        outside_spec_namespace: outside,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

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
        assert!(r.stale.iter().all(|s| !s.commands.is_empty()));
        assert!(
            r.stale
                .iter()
                .all(|s| s.commands.iter().all(|c| c.starts_with("one "))),
            "every stale row must name real `ayx one ...` commands"
        );
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

    /// The inventory-side mirror of `non_v4_path_is_unmatched_not_dropped`.
    ///
    /// The spec side always recorded a non-`/v4` operation in
    /// `unmatched_spec_paths`, but the inventory side silently dropped the
    /// matching rows, so every endpoint on a sibling service — all of
    /// `/svc-workflow`, `/plans/v1`, `/scheduling/v1`, `/billing/v1`,
    /// `/iam/v1` — vanished from the report entirely: not covered, not stale,
    /// not counted. `inventory_operations` then reported 123 for a 150-row
    /// inventory while `coverage_pct` silently described only the `/v4` slice.
    #[test]
    fn wired_endpoints_outside_the_spec_namespace_are_reported_not_dropped() {
        let r = coverage(&spec_with(json!({ "/v4/flows": { "get": {} } })));

        assert!(
            !r.outside_spec_namespace.is_empty(),
            "the real inventory wires sibling services (/svc-workflow, /plans/v1, \
             /scheduling/v1, /billing/v1, /iam/v1); none were reported"
        );
        // Derive the expectation from the inventory independently, rather than
        // re-stating how `coverage()` computes the field. Asserting
        // `inventory_total == inventory_operations + outside.len()` would be a
        // tautology -- that is the definition -- and would still pass if the
        // partitioning silently started dropping rows on both sides at once.
        let expected_outside: BTreeSet<(String, String)> = inventory_endpoints_full()
            .iter()
            .filter(|(m, p, _)| canonical_op(m, p).is_none())
            .map(|(m, p, _)| ((*m).to_string(), (*p).to_string()))
            .collect();
        let reported_outside: BTreeSet<(String, String)> = r
            .outside_spec_namespace
            .iter()
            .map(|e| (e.method.clone(), e.path.clone()))
            .collect();
        assert_eq!(
            reported_outside, expected_outside,
            "every inventory row that cannot be canonicalized must be reported, \
             and nothing else"
        );
        let distinct_raw_rows: BTreeSet<(&str, &str)> = inventory_endpoints_full()
            .iter()
            .map(|(m, p, _)| (*m, *p))
            .collect();
        assert_eq!(
            r.inventory_total,
            distinct_raw_rows.len(),
            "inventory_total must count every distinct wired row"
        );
        assert!(
            r.inventory_total >= r.inventory_operations + expected_outside.len(),
            "the two buckets dedupe on different keys, so their sum can only \
             ever be <= the raw row count"
        );
        // Every reported row must name the command(s) that reach it, or the
        // report cannot be acted on.
        for e in &r.outside_spec_namespace {
            assert!(
                !e.commands.is_empty(),
                "{} {} is reported with no dispatching command",
                e.method,
                e.path
            );
            assert!(
                !e.path.contains("/v4/"),
                "{} {} is comparable and must not be listed as outside",
                e.method,
                e.path
            );
        }
        // Sanity-check the specific services this was written to catch.
        let paths: Vec<&str> = r
            .outside_spec_namespace
            .iter()
            .map(|e| e.path.as_str())
            .collect();
        assert!(
            paths.iter().any(|p| p.starts_with("/svc-workflow/")),
            "cloud-native workflows are wired but absent from the report: {paths:?}"
        );
    }

    /// A row outside the spec namespace must never be miscounted as drift.
    /// Reporting `/svc-workflow` as `stale` would send someone deleting live,
    /// working commands.
    #[test]
    fn outside_namespace_rows_are_not_reported_as_stale() {
        let r = coverage(&spec_with(json!({ "/v4/flows": { "get": {} } })));
        for e in &r.outside_spec_namespace {
            assert!(
                !r.stale
                    .iter()
                    .any(|s| s.path == e.path && s.method == e.method),
                "{} {} is outside the spec namespace but was reported as stale",
                e.method,
                e.path
            );
        }
    }

    /// `inventory_total` must count raw wired rows, not the sum of the two
    /// buckets. `canonical_op` strips query strings, so `/v4/people` and
    /// `/v4/people?role=admin` are two wired rows but one comparable key; an
    /// earlier version derived the total as `inv_keys.len() + outside.len()`
    /// and silently undercounted every such pair.
    #[test]
    fn query_only_variants_are_counted_as_distinct_wired_rows() {
        let collapsing: Vec<(&str, &str)> = inventory_endpoints_full()
            .iter()
            .filter(|(_, p, _)| p.contains('?'))
            .map(|(m, p, _)| (*m, *p))
            .collect();
        assert!(
            !collapsing.is_empty(),
            "this test is meaningless without a wired query-string endpoint; \
             the inventory has none, so re-derive the guard"
        );

        let r = coverage(&spec_with(json!({ "/v4/flows": { "get": {} } })));
        // Each query variant shares a canonical key with its bare form, so it
        // adds to the raw total without adding a comparable key.
        for (method, path) in &collapsing {
            let bare = path.split('?').next().unwrap();
            let bare_is_wired = inventory_endpoints_full()
                .iter()
                .any(|(m, p, _)| m == method && *p == bare);
            assert!(
                bare_is_wired,
                "{method} {path} has no bare counterpart; the collapse premise \
                 does not hold and this guard needs rewriting"
            );
        }
        assert!(
            r.inventory_total > r.inventory_operations + r.outside_spec_namespace.len(),
            "with {} query-only variant(s) wired, the raw total must exceed the \
             sum of the deduped buckets",
            collapsing.len()
        );
    }

    /// A sibling service's own `/v4` must not be folded into the gateway
    /// namespace. `/svc-workflow` already ships `/api/v0`, `/api/v1`, and
    /// `/api/v2`; when it reaches v4, an unanchored `find("/v4/")` would match
    /// the gateway's `GET /v4/workflows` row and report those paths as
    /// spec-verified instead of outside the namespace.
    #[test]
    fn only_a_leading_v4_segment_anchors_a_path() {
        assert_eq!(
            canonical_op("GET", "/v4/workflows"),
            Some(("GET".into(), "/v4/workflows".into()))
        );
        assert_eq!(
            canonical_op("GET", "/svc-workflow/api/v4/workflows"),
            None,
            "a sibling service's v4 must stay outside the gateway namespace"
        );
        assert_eq!(canonical_op("GET", "/plans/v1/plans"), None);
        assert_eq!(
            canonical_op("get", "/v4/flows/{flowId}"),
            Some(("GET".into(), "/v4/flows/{}".into()))
        );
    }

    /// Rows that collapse to one canonical key must contribute *all* their
    /// commands to the stale report. `inv_meta.insert` overwrote, so a stale
    /// `/v4/workflows` named only `one workflows count` and silently dropped
    /// `one workflows list` — telling an operator that half the affected
    /// commands were unaffected.
    #[test]
    fn stale_rows_name_every_command_that_shares_a_canonical_key() {
        let r = coverage(&spec_with(json!({ "/v4/flows": { "get": {} } })));
        let workflows = r
            .stale
            .iter()
            .find(|s| s.path.starts_with("/v4/workflows"))
            .expect("the workflows listing route is absent from the gateway spec, so it is stale");
        for expected in ["one workflows list", "one workflows count"] {
            assert!(
                workflows.commands.iter().any(|c| c == expected),
                "{expected} shares the canonical key but is missing from {:?}",
                workflows.commands
            );
        }
    }

    #[test]
    fn empty_or_uncomparable_spec_reports_no_coverage_figure() {
        // A spec with nothing on /v4 compares nothing. Reporting 100.0 would
        // read as "fully covered" when the real answer is "not measured".
        let r = coverage(
            &json!({ "servers": [{ "url": "https://host" }], "paths": { "/health": { "get": {} } } }),
        );
        assert_eq!(
            r.coverage_pct, None,
            "nothing comparable must not report a percentage"
        );
        assert_eq!(r.covered, 0);

        let r = coverage(&spec_with(json!({ "/v4/flows": { "get": {} } })));
        assert!(
            r.coverage_pct.is_some(),
            "a comparable spec must still report a figure"
        );
    }

    #[test]
    fn non_v4_path_is_unmatched_not_dropped() {
        let spec = json!({ "servers": [{ "url": "https://host" }], "paths": { "/health": { "get": {} } } });
        let r = coverage(&spec);
        assert!(r.unmatched_spec_paths.iter().any(|p| p.contains("/health")));
    }

    /// The same raw `(METHOD, path)` may be cross-listed under more than one surface
    /// (`GET /v4/people` is both an `iam` and a `person` endpoint). When it is, every
    /// row must declare the **identical** command set, so `one inventory` never tells
    /// two different stories about who calls an endpoint.
    ///
    /// This replaces an allowlist of colliding canonical keys. Two points matter:
    ///   - Grouping is by RAW path, not canonical. `canonical_op` strips query strings,
    ///     so `/v4/people` and `/v4/people?role=admin` collapse together — but they are
    ///     genuinely different requests (`one workspace admins` filters server-side).
    ///     Collapsing them is a coverage-matching artifact, not a wiring bug, and
    ///     forcing their command sets to match would be a lie.
    ///   - Now that a row carries every command that dispatches it, a true alias is
    ///     expressed inside one row and needs no exemption.
    #[test]
    fn cross_listed_endpoints_must_agree_on_their_command_set() {
        use std::collections::{BTreeSet, HashMap};

        let mut by_raw: HashMap<(&str, &str), Vec<&'static [&'static str]>> = HashMap::new();
        for (m, p, commands) in inventory_endpoints_full() {
            by_raw.entry((m, p)).or_default().push(commands);
        }

        for (key, rows) in &by_raw {
            if rows.len() <= 1 {
                continue;
            }
            let first: BTreeSet<&str> = rows[0].iter().copied().collect();
            for other in &rows[1..] {
                let other_set: BTreeSet<&str> = other.iter().copied().collect();
                assert_eq!(
                    first, other_set,
                    "endpoint {} {} is cross-listed under multiple surfaces with different \
                     command sets ({first:?} vs {other_set:?}); every row for one endpoint \
                     must list every command that dispatches it",
                    key.0, key.1
                );
            }
        }
    }

    /// Every row must name at least one dispatching command, and each name must be a
    /// real `ayx one ...` path. An empty slice would silently vanish from `one
    /// inventory` and from the stale-endpoint report.
    #[test]
    fn every_endpoint_row_names_at_least_one_one_command() {
        for (method, path, commands) in inventory_endpoints_full() {
            assert!(
                !commands.is_empty(),
                "inventory row {method} {path} lists no dispatching command"
            );
            for name in commands {
                assert!(
                    name.starts_with("one "),
                    "inventory row {method} {path} names command {name:?}, which is not an \
                     `ayx one ...` command path"
                );
            }
        }
    }
}
