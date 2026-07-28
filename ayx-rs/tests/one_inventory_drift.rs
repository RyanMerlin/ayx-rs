//! Guards the One endpoint inventory against the CLI's actual wiring.
//!
//! `ayx-one-api/src/inventory.rs` is hand-maintained, and `one inventory` /
//! `one api coverage` present it to operators as fact. When a dispatcher sends a
//! request the inventory does not describe — or describes with the wrong method —
//! those commands lie.
//!
//! That is not hypothetical. All four `one connections permissions` leaves shipped
//! wired to `/v4/connections/{id}/permissions`, which the live API answers with
//! `RouteNotFoundException`; the inventory faithfully mirrored the same wrong path,
//! so neither `one inventory` nor `one api coverage` could surface it. This test is
//! the gate that would have caught it at authoring time.
//!
//! It did not, the first time: `telemetry/permissions.rs` shipped wired to the
//! very same dead route and this gate stayed green, because file discovery only
//! walked files/dirs whose *name* started with `one` — `telemetry/` doesn't.
//! Discovery below scans every `.rs` file under `src/cmd` by *content* instead
//! (does it call a One transport function?), so a dispatcher's filename or
//! location can no longer hide it from this gate.
//!
//! That widening was still not enough: it left `src/main.rs` out of scope, and
//! the `one_doctor_*_envelope` / `one_platform_auth_*_envelope` dispatchers live
//! there and issue real One transport calls. Discovery now covers `main.rs` too
//! — scoping to a *directory* was the same mistake as scoping to a filename.
//!
//! Strategy: parse every file under `cmd/**/*.rs`, plus `src/main.rs`, for calls into the One
//! transport, recover the `(METHOD, ENDPOINT, MUTATING)` each one passes by
//! position (resolving simple same-file `const NAME: &str = "...";` endpoints
//! along the way), and assert:
//!   1. every `(METHOD, ENDPOINT)` pair exists in `ayx_one_api::inventory_endpoints()`
//!   2. every mutating-HTTP-method call site actually passes `mutating: true`
//!      (this is the class of bug that let `wrangle-to-python` run for real with
//!      no `--apply` gate until it was caught separately)
//!   3. the catalog's own `mutating`/`safety` metadata for a command agrees with
//!      what that command is actually wired to send (this is the class of bug
//!      that then let `catalog list` keep reporting `wrangle-to-python` as
//!      `safety: "read-only"` after the fix above — the catalog is hand-authored
//!      too, and nothing previously checked it against reality)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Transport entry points, and the zero-based positional-argument index (0 is
/// the leading `config` argument) of METHOD, ENDPOINT, and MUTATING.
///
/// `one_api_live_request*(config, surface, operation, method, endpoint, mutating, ...)`
/// `one_api_list_request(config, surface, operation, endpoint, path_params, params)`
///   -> method is always GET, mutating is always false (never passed).
const CALL_SHAPES: &[(&str, Option<usize>, usize, Option<usize>)] = &[
    ("one_api_live_request_with_body", Some(3), 4, Some(5)),
    ("one_api_live_request", Some(3), 4, Some(5)),
    ("one_api_list_request", None, 3, None),
];

/// `(method, endpoint)` pairs that use a normally-mutating HTTP verb but are
/// genuinely safe — validation/dry-run routes that make no state change. This
/// is the *only* escape hatch from "POST/PATCH/PUT/DELETE must pass
/// `mutating: true`" below; every entry needs a reason.
const SAFE_NON_MUTATING_ALLOWLIST: &[(&str, &str)] = &[
    // Schema-validates a connection payload against the API; makes no change
    // (the sibling of a plan/dry-run route, just spelled as its own endpoint).
    ("POST", "/v4/connections/dryRun"),
];

/// Endpoints built at runtime (`format!`, or resolved from a value this parser
/// cannot see statically) rather than a literal or a same-file `const`, so this
/// test cannot recover them by parsing. Each entry is `(file, method, endpoint)`
/// and must still appear in the inventory — asserted separately below, so an
/// allowlisted call is exempt from *parsing*, never from *being inventoried*.
const DYNAMIC_ENDPOINTS: &[(&str, &str, &str)] = &[
    // `flows parameters` appends `?outputObjectType=` when the flag is present.
    ("one_flows.rs", "GET", "/v4/flows/{id}/recipeParameters"),
    // library / folder listings append `?limit=&offset=` via a local helper.
    ("one_flows.rs", "GET", "/v4/flowsLibrary"),
    ("one_flows.rs", "GET", "/v4/folders"),
    ("one_flows.rs", "GET", "/v4/folders/{id}/flows"),
    ("one_datasets.rs", "GET", "/v4/datasetLibrary"),
    ("one_datasets.rs", "GET", "/v4/wrangledDatasets"),
    // `connections permissions delete` builds the unshare query (ids +
    // subject type) via `build_connection_unshare_query`.
    ("one_connections.rs", "DELETE", "/v4/connections/share"),
];

/// `(method, endpoint)` pairs dispatched only by a non-`one`-namespace
/// surface (currently just `ayx telemetry ...`), so they cannot be added to
/// `ayx-one-api/src/inventory.rs`: its `commands` field is contractually
/// `ayx one ...`-only (see
/// `ayx_one_api::coverage::tests::every_endpoint_row_names_at_least_one_one_command`).
///
/// Found when discovery below widened from a `one*.rs` filename convention to
/// scanning every file by content: `telemetry permissions workflows` and
/// `telemetry permissions summary` dispatch `GET
/// /iam/v1/workspaces/{id}/people`, which no `one` command uses, so it was
/// never in the inventory at all. Listed here — not silently added to the
/// `one`-only inventory (would break its own contract) and not silently
/// dropped from this gate either.
const NON_ONE_SURFACE_ENDPOINTS: &[(&str, &str)] = &[("GET", "/iam/v1/workspaces/{id}/people")];

fn cmd_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cmd")
}

/// Render a discovered source key as a real, repo-relative path.
///
/// Keys are relative to `src/cmd`, so most render as `src/cmd/<key>`. `main.rs`
/// is discovered as `../main.rs` (it lives a level up); normalize it rather than
/// emitting `cmd/../main.rs` in a failure message someone has to act on.
fn display_path(file: &str) -> String {
    match file.strip_prefix("../") {
        Some(rest) => format!("src/{rest}"),
        None => format!("src/cmd/{file}"),
    }
}

/// Every `.rs` file under `src/cmd`, **plus `src/main.rs`**, found by walking the
/// tree — no filename or directory-name filter. See the module doc: filtering by a
/// `one*` naming convention is exactly what let `telemetry/permissions.rs` hide a
/// wrong route from this gate. Paths are relative to `src/cmd` with `/` separators
/// (e.g. `"telemetry/permissions.rs"`), not bare filenames, so files that share a
/// basename across subdirectories (several `mod.rs`) stay distinguishable.
///
/// `main.rs` is included because the same class of blind spot survived the last
/// widening: `one_doctor_*_envelope` and `one_platform_auth_*_envelope` live in
/// `main.rs`, not under `src/cmd`, and issue real One transport calls. Scoping
/// discovery to a directory is the same mistake as scoping it to a filename
/// prefix — a dispatcher's *location* must not be able to hide it from this gate.
fn cmd_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![cmd_dir()];
    let root = cmd_dir();

    let main_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    if let Ok(text) = std::fs::read_to_string(&main_rs) {
        out.push(("../main.rs".to_string(), text));
    }
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((rel, text));
            }
        }
    }
    out.sort();
    out
}

/// Argument text of a call starting at `open_paren`, up to its matching close paren.
/// Skips parens inside string literals so `"/v4/x?(y)"` cannot unbalance the scan.
fn call_args(src: &str, open_paren: usize) -> Option<&str> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open_paren;
    let mut in_str = false;
    let mut escaped = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return src.get(open_paren + 1..i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Brace-delimited block starting at `open_brace`, up to its matching close
/// brace. Same string-skipping discipline as [`call_args`].
fn brace_block(src: &str, open_brace: usize) -> Option<&str> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open_brace;
    let mut in_str = false;
    let mut escaped = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return src.get(open_brace + 1..i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Split a call's argument text on top-level commas (depth 0 across
/// `()`/`[]`/`{}`, and never inside a string literal), returning each
/// positional argument's trimmed source text in order.
fn split_top_level_args(args: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut buf = String::new();
    for c in args.chars() {
        if in_str {
            buf.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                buf.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                buf.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                buf.push(c);
            }
            ',' if depth == 0 => {
                parts.push(buf.trim().to_string());
                buf.clear();
            }
            _ => buf.push(c),
        }
    }
    if !buf.trim().is_empty() {
        parts.push(buf.trim().to_string());
    }
    parts
}

/// If `text` is exactly a quoted string literal, its decoded contents.
fn single_literal(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut buf = String::new();
    let mut j = 1usize;
    let mut escaped = false;
    while j < bytes.len() {
        let c = bytes[j] as char;
        if escaped {
            buf.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(buf);
        } else {
            buf.push(c);
        }
        j += 1;
    }
    None
}

/// Simple same-file `const NAME: &str = "value";` declarations (single line,
/// single string literal). Several dispatchers hoist a shared endpoint literal
/// into a named const instead of repeating it at every call site — without
/// resolving those, this parser would silently drop every call site that uses
/// one (which is exactly how `one_workflows.rs`'s list/assets calls, wired via
/// `WORKFLOWS_LIST_ENDPOINT`/`ASSETS_LIST_ENDPOINT`, went unparsed).
fn local_str_consts(src: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("const ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let name = rest[..colon].trim();
        let Some(eq) = rest.find('=') else {
            continue;
        };
        if let Some(lit) = single_literal(rest[eq + 1..].trim()) {
            out.insert(name.to_string(), lit);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WiredCall {
    file: String,
    method: String,
    endpoint: String,
    /// `None` when neither a literal `true`/`false` nor implied by the call
    /// shape (`one_api_list_request` implies `Some(false)`) — see
    /// `parser_gaps` in [`wired_calls`].
    mutating: Option<bool>,
}

/// Parsed calls, plus two parser-gap diagnostics: endpoints this parser could
/// not resolve to a literal path (must be covered by `DYNAMIC_ENDPOINTS`), and
/// `mutating` arguments that were not a literal `true`/`false` (must be empty —
/// unlike the endpoint case there is no allowlist for this one; every call site
/// observed at the time this test was written passes a literal).
fn wired_calls() -> (Vec<WiredCall>, Vec<String>, Vec<String>) {
    let mut calls = Vec::new();
    let mut dynamic_endpoints = Vec::new();
    let mut unresolved_mutating = Vec::new();

    for (file, src) in cmd_sources() {
        let consts = local_str_consts(&src);
        for (func, method_idx, endpoint_idx, mutating_idx) in CALL_SHAPES {
            let mut from = 0usize;
            while let Some(rel) = src[from..].find(func) {
                let at = from + rel;
                from = at + func.len();

                // Only match a real call: the next non-space char must be '('.
                // Skips the `use` import line and the longer sibling name.
                let rest = &src[at + func.len()..];
                let Some(paren_off) = rest.find(|c: char| !c.is_whitespace()) else {
                    continue;
                };
                if rest.as_bytes()[paren_off] != b'(' {
                    continue;
                }
                // `one_api_live_request` is a prefix of
                // `one_api_live_request_with_body`; the char before '(' rules that out
                // because the longer name would leave `_with_body` in `rest`.
                let open = at + func.len() + paren_off;
                let Some(args) = call_args(&src, open) else {
                    continue;
                };
                let parts = split_top_level_args(args);

                let endpoint_raw = parts.get(*endpoint_idx).map(String::as_str).unwrap_or("");
                let endpoint =
                    single_literal(endpoint_raw).or_else(|| consts.get(endpoint_raw).cloned());
                let Some(endpoint) = endpoint else {
                    dynamic_endpoints.push(format!(
                        "{file}: {func} endpoint is not a literal path ({endpoint_raw})"
                    ));
                    continue;
                };
                if !endpoint.starts_with('/') {
                    dynamic_endpoints.push(format!("{file}: {func} endpoint not a literal path"));
                    continue;
                }

                let method = match method_idx {
                    Some(idx) => parts
                        .get(*idx)
                        .and_then(|s| single_literal(s))
                        .unwrap_or_default(),
                    None => "GET".to_string(),
                };

                let mutating = match mutating_idx {
                    Some(idx) => match parts.get(*idx).map(String::as_str) {
                        Some("true") => Some(true),
                        Some("false") => Some(false),
                        other => {
                            unresolved_mutating.push(format!(
                                "{file}: {func} mutating argument is not a literal bool ({:?})",
                                other.unwrap_or("<missing>")
                            ));
                            None
                        }
                    },
                    None => Some(false),
                };

                calls.push(WiredCall {
                    file: file.clone(),
                    method,
                    endpoint,
                    mutating,
                });
            }
        }
    }
    calls.sort();
    calls.dedup();
    (calls, dynamic_endpoints, unresolved_mutating)
}

/// Compare ignoring query strings — the inventory records `/v4/people?role=admin`
/// while a dispatcher may build the query separately.
fn strip_query(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

#[test]
fn every_wired_one_endpoint_is_in_the_inventory() {
    let inventory: BTreeSet<(String, String)> = ayx_one_api::inventory_endpoints()
        .into_iter()
        .map(|(m, p)| (m.to_ascii_uppercase(), strip_query(p).to_string()))
        .collect();

    let (calls, _dynamic_endpoints, unresolved_mutating) = wired_calls();
    assert!(
        !calls.is_empty(),
        "parsed zero One transport calls — the parser is broken, not the wiring"
    );
    assert!(
        unresolved_mutating.is_empty(),
        "could not statically determine the `mutating` argument for these calls; extend the \
         parser or make the call site pass a literal `true`/`false`:\n{}",
        unresolved_mutating.join("\n")
    );

    let mut missing = Vec::new();
    for call in &calls {
        let key = (
            call.method.to_ascii_uppercase(),
            strip_query(&call.endpoint).to_string(),
        );
        if inventory.contains(&key) {
            continue;
        }
        if NON_ONE_SURFACE_ENDPOINTS.contains(&(call.method.as_str(), strip_query(&call.endpoint)))
        {
            continue;
        }
        missing.push(format!(
            "  {} {} (dispatched by {})",
            call.method,
            call.endpoint,
            display_path(&call.file)
        ));
    }

    assert!(
        missing.is_empty(),
        "these endpoints are wired in the CLI but absent from \
         ayx-one-api/src/inventory.rs, so `one inventory` and `one api coverage` \
         under-report them:\n{}\n\nAdd an EndpointSpec row (or fix the wiring).",
        missing.join("\n")
    );
}

/// `NON_ONE_SURFACE_ENDPOINTS` must stay accurate on both sides: an entry must
/// actually be absent from the `one`-only inventory (else it belongs there
/// instead, with a real `commands` entry) and must actually be dispatched
/// only by non-`one_*` dispatcher files (else it belongs in the main
/// endpoint-inventory check instead of this carve-out).
#[test]
fn non_one_surface_allowlist_is_accurate_not_stale() {
    let inventory: BTreeSet<(String, String)> = ayx_one_api::inventory_endpoints()
        .into_iter()
        .map(|(m, p)| (m.to_ascii_uppercase(), strip_query(p).to_string()))
        .collect();
    for (method, endpoint) in NON_ONE_SURFACE_ENDPOINTS {
        assert!(
            !inventory.contains(&(method.to_string(), endpoint.to_string())),
            "{method} {endpoint} is listed in NON_ONE_SURFACE_ENDPOINTS as absent from the \
             inventory, but it is actually there now -- remove it from the carve-out"
        );
    }

    let (calls, _dynamic_endpoints, _unresolved_mutating) = wired_calls();
    for (method, endpoint) in NON_ONE_SURFACE_ENDPOINTS {
        let dispatchers: Vec<&str> = calls
            .iter()
            .filter(|c| c.method == *method && strip_query(&c.endpoint) == *endpoint)
            .map(|c| c.file.as_str())
            .collect();
        assert!(
            !dispatchers.is_empty(),
            "{method} {endpoint} is listed in NON_ONE_SURFACE_ENDPOINTS but nothing dispatches \
             it anymore -- remove the stale entry"
        );
        for file in dispatchers {
            assert!(
                !(file.starts_with("one") || file.contains("/one")),
                "{method} {endpoint} is listed in NON_ONE_SURFACE_ENDPOINTS as non-`one`-surface, \
                 but {} is a `one` dispatcher -- add a real inventory row instead",
                display_path(file)
            );
        }
    }
}

#[test]
fn dynamic_endpoint_allowlist_is_inventoried_and_not_stale() {
    let inventory: BTreeSet<(String, String)> = ayx_one_api::inventory_endpoints()
        .into_iter()
        .map(|(m, p)| (m.to_ascii_uppercase(), strip_query(p).to_string()))
        .collect();

    for (file, method, endpoint) in DYNAMIC_ENDPOINTS {
        assert!(
            inventory.contains(&(method.to_ascii_uppercase(), (*endpoint).to_string())),
            "allowlisted dynamic endpoint {method} {endpoint} ({}) is not in the \
             inventory; the allowlist exempts a call from static parsing, never from \
             being inventoried",
            display_path(file)
        );
    }
}

/// Every file with a call whose endpoint this parser could not resolve to a
/// literal must be represented in `DYNAMIC_ENDPOINTS` — otherwise that call is
/// invisible to `every_wired_one_endpoint_is_in_the_inventory` with nothing
/// tracking the gap. (This is a file-level check, not a call-level one: static
/// analysis cannot always tell which `DYNAMIC_ENDPOINTS` row a given
/// `format!`-built path resolves to, but "this file has an unresolvable call
/// and nobody documented it" is exactly the widened-discovery gap this test
/// exists to close — it is how `one_connections.rs`'s unshare-query DELETE call
/// was found still missing from the allowlist.)
#[test]
fn every_file_with_an_unresolved_endpoint_is_represented_in_the_allowlist() {
    let (_calls, dynamic_endpoints, _unresolved_mutating) = wired_calls();
    let allowlisted_files: BTreeSet<&str> = DYNAMIC_ENDPOINTS.iter().map(|(f, _, _)| *f).collect();

    let mut missing = Vec::new();
    for entry in &dynamic_endpoints {
        let Some(file) = entry.split(':').next() else {
            continue;
        };
        if !allowlisted_files.contains(file) {
            missing.push(entry.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "these files dispatch a One transport call whose endpoint could not be resolved \
         statically, and are not represented in DYNAMIC_ENDPOINTS:\n{}",
        missing.join("\n")
    );
}

/// A POST/PATCH/PUT/DELETE call that does not pass `mutating: true` runs for
/// real with no `--apply` gate and up to 4 retries on 5xx. This is the class of
/// bug `wrangle-to-python` shipped as (fixed alongside this test): the two arms
/// disagreed about whether the endpoint mutated, and the wrong one won.
#[test]
fn mutating_http_methods_pass_mutating_true_except_the_allowlist() {
    let (calls, _dynamic_endpoints, _unresolved_mutating) = wired_calls();
    assert!(
        !calls.is_empty(),
        "parsed zero One transport calls — the parser is broken, not the wiring"
    );
    let allow: BTreeSet<(&str, &str)> = SAFE_NON_MUTATING_ALLOWLIST.iter().copied().collect();

    let mut violations = Vec::new();
    for call in &calls {
        if !matches!(call.method.as_str(), "POST" | "PATCH" | "PUT" | "DELETE") {
            continue;
        }
        let key = (call.method.as_str(), strip_query(&call.endpoint));
        if allow.contains(&key) {
            continue;
        }
        if call.mutating != Some(true) {
            violations.push(format!(
                "  {} {} mutating={:?} (dispatched by {})",
                call.method,
                call.endpoint,
                call.mutating,
                display_path(&call.file)
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "these calls use a mutating HTTP method but do not pass mutating: true, so they run \
         for real with no --apply gate; if genuinely safe (e.g. a validate/dry-run route), add \
         them to SAFE_NON_MUTATING_ALLOWLIST with a reason:\n{}",
        violations.join("\n")
    );
}

// ─── Catalog cross-check ────────────────────────────────────────────────────
//
// `ayx-rs/src/cmd/catalog.rs` is a second, independently hand-maintained
// classification of the same commands (`safety`, `mutating`) that `catalog
// list` reports to agent/tool consumers. Nothing previously checked it against
// the live wiring above, which is exactly how `wrangle-to-python` went
// mutating at the transport layer while the catalog kept calling it
// `safety: "read-only"`.

#[derive(Debug, Clone)]
struct CatalogRow {
    path: String,
    safety: String,
    mutating: bool,
}

fn catalog_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cmd/catalog.rs");
    std::fs::read_to_string(&path).expect("ayx-rs/src/cmd/catalog.rs must be readable")
}

fn field_str(block: &str, field: &str) -> Option<String> {
    let pat = format!("{field}:");
    let idx = block.find(&pat)?;
    single_literal(block[idx + pat.len()..].trim_start())
}

fn field_bool(block: &str, field: &str) -> Option<bool> {
    let pat = format!("{field}:");
    let idx = block.find(&pat)?;
    let rest = block[idx + pat.len()..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_catalog_metadata(src: &str) -> Vec<CatalogRow> {
    let marker = "CatalogMetadata {";
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(marker) {
        let at = from + rel;
        let open_brace = at + marker.len() - 1;
        let Some(block) = brace_block(src, open_brace) else {
            break;
        };
        from = open_brace + block.len() + 2;
        if let (Some(path), Some(safety), Some(mutating)) = (
            field_str(block, "path"),
            field_str(block, "safety"),
            field_bool(block, "mutating"),
        ) {
            out.push(CatalogRow {
                path,
                safety,
                mutating,
            });
        }
    }
    out
}

/// A row's own `safety` label must agree with its own `mutating` flag. The one
/// deliberate exception is `"mutating-or-read-only"` (`server api call`, whose
/// HTTP method is caller-supplied at runtime — genuinely ambiguous, and
/// honestly labeled as such rather than picking one and being wrong half the
/// time).
#[test]
fn catalog_safety_label_matches_its_own_mutating_flag() {
    let rows = parse_catalog_metadata(&catalog_source());
    assert!(
        !rows.is_empty(),
        "parsed zero CatalogMetadata rows — the parser is broken, not the metadata"
    );

    const AMBIGUOUS: &str = "mutating-or-read-only";
    const READ_ONLY_LABELS: &[&str] = &["read-only", "read-only-or-safe-local-fix"];

    let mut violations = Vec::new();
    for row in &rows {
        if row.safety == AMBIGUOUS {
            continue;
        }
        let expected_mutating = !READ_ONLY_LABELS.contains(&row.safety.as_str());
        if row.mutating != expected_mutating {
            violations.push(format!(
                "{}: safety={:?} mutating={} (expected mutating={expected_mutating})",
                row.path, row.safety, row.mutating
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "catalog rows whose safety label disagrees with their own mutating flag:\n{}",
        violations.join("\n")
    );
}

/// The catalog's `mutating` metadata for a command must agree with what that
/// command is actually wired to send. Joined through
/// `ayx_one_api::inventory_endpoints_full()`, which already maps
/// `(method, endpoint)` to every command name that dispatches it.
#[test]
fn catalog_mutating_metadata_matches_the_wired_transport_call() {
    let catalog_rows = parse_catalog_metadata(&catalog_source());
    let catalog_by_path: BTreeMap<&str, &CatalogRow> =
        catalog_rows.iter().map(|r| (r.path.as_str(), r)).collect();

    let (calls, _dynamic_endpoints, _unresolved_mutating) = wired_calls();
    let mut wired_mutating: BTreeMap<(String, String), bool> = BTreeMap::new();
    for call in &calls {
        if let Some(mutating) = call.mutating {
            wired_mutating.insert(
                (
                    call.method.to_ascii_uppercase(),
                    strip_query(&call.endpoint).to_string(),
                ),
                mutating,
            );
        }
    }

    // A single command can dispatch more than one (method, endpoint) pair
    // depending on its own flags -- e.g. `one plans permissions` is a GET
    // unless `--subject-id` is set, in which case it's a DELETE. The
    // catalog's `mutating` is leaf-level, worst-case truth ("can invoking
    // this command mutate state at all"), so reduce every wired call a
    // command can reach with OR before comparing, rather than requiring
    // every individual endpoint to match.
    let mut command_can_mutate: BTreeMap<String, bool> = BTreeMap::new();
    for (method, path, commands) in ayx_one_api::inventory_endpoints_full() {
        let key = (method.to_ascii_uppercase(), strip_query(path).to_string());
        let Some(&wired) = wired_mutating.get(&key) else {
            continue;
        };
        for command in commands {
            let entry = command_can_mutate
                .entry((*command).to_string())
                .or_insert(false);
            *entry |= wired;
        }
    }

    let mut mismatches = Vec::new();
    for (command, wired_can_mutate) in &command_can_mutate {
        let catalog_path = command.replace(' ', "/");
        let Some(row) = catalog_by_path.get(catalog_path.as_str()) else {
            continue;
        };
        if row.mutating != *wired_can_mutate {
            mismatches.push(format!(
                "{catalog_path}: catalog says mutating={} (safety={:?}) but the wired \
                 transport call(s) for `{command}` reduce to mutating={wired_can_mutate}",
                row.mutating, row.safety
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "catalog `mutating` metadata disagrees with the actual wired transport call — an \
         agent/tool consumer of `catalog list` would make the wrong safety decision:\n{}",
        mismatches.join("\n")
    );
}
