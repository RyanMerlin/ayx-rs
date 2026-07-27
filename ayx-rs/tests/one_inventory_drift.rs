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
//! Strategy: parse every `cmd/one*.rs` dispatcher for calls into the One transport,
//! recover the `(METHOD, endpoint)` string literals each one passes, and assert the
//! pair exists in `ayx_one_api::inventory_endpoints()`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Transport entry points and the zero-based index, among a call's string literal
/// arguments, of the METHOD and ENDPOINT.
///
/// `one_api_live_request*(config, surface, operation, method, endpoint, ...)`
///   -> literals: [surface, operation, method, endpoint]
/// `one_api_list_request(config, surface, operation, endpoint, ...)`
///   -> literals: [surface, operation, endpoint]; method is always GET.
const CALL_SHAPES: &[(&str, Option<usize>, usize)] = &[
    ("one_api_live_request_with_body", Some(2), 3),
    ("one_api_live_request", Some(2), 3),
    ("one_api_list_request", None, 2),
];

/// Endpoints built at runtime (`format!`) rather than passed as a literal, so this
/// test cannot read them statically. Each entry is `(file, endpoint_template)` and
/// must still appear in the inventory — that is asserted separately below, so an
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
];

fn cmd_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cmd")
}

/// Every `one*.rs` dispatcher, including the `one_platform/` and `one_api/` subdirs.
fn one_dispatcher_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![cmd_dir()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("one") {
                    stack.push(path);
                }
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".rs") {
                continue;
            }
            let in_one_subdir = dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("one"));
            if !(name.starts_with("one") || in_one_subdir) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((name.to_string(), text));
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

/// String literals in `args`, in source order.
fn string_literals(args: &str) -> Vec<String> {
    let bytes = args.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let mut j = i + 1;
            let mut buf = String::new();
            let mut escaped = false;
            while j < bytes.len() {
                let c = bytes[j] as char;
                if escaped {
                    buf.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    break;
                } else {
                    buf.push(c);
                }
                j += 1;
            }
            out.push(buf);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WiredCall {
    file: String,
    method: String,
    endpoint: String,
}

fn wired_calls() -> (Vec<WiredCall>, Vec<String>) {
    let mut calls = Vec::new();
    let mut dynamic = Vec::new();

    for (file, src) in one_dispatcher_sources() {
        for (func, method_idx, endpoint_idx) in CALL_SHAPES {
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
                let literals = string_literals(args);
                let Some(endpoint) = literals.get(*endpoint_idx) else {
                    continue;
                };
                if !endpoint.starts_with('/') {
                    dynamic.push(format!("{file}: {func} endpoint not a literal path"));
                    continue;
                }
                let method = match method_idx {
                    Some(idx) => literals.get(*idx).cloned().unwrap_or_default(),
                    None => "GET".to_string(),
                };
                calls.push(WiredCall {
                    file: file.clone(),
                    method,
                    endpoint: endpoint.clone(),
                });
            }
        }
    }
    calls.sort();
    calls.dedup();
    (calls, dynamic)
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

    let (calls, _dynamic) = wired_calls();
    assert!(
        !calls.is_empty(),
        "parsed zero One transport calls — the parser is broken, not the wiring"
    );

    let mut missing = Vec::new();
    for call in &calls {
        let key = (
            call.method.to_ascii_uppercase(),
            strip_query(&call.endpoint).to_string(),
        );
        if !inventory.contains(&key) {
            missing.push(format!(
                "  {} {} (dispatched by cmd/{})",
                call.method, call.endpoint, call.file
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "these endpoints are wired in the CLI but absent from \
         ayx-one-api/src/inventory.rs, so `one inventory` and `one api coverage` \
         under-report them:\n{}\n\nAdd an EndpointSpec row (or fix the wiring).",
        missing.join("\n")
    );
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
            "allowlisted dynamic endpoint {method} {endpoint} (cmd/{file}) is not in the \
             inventory; the allowlist exempts a call from static parsing, never from \
             being inventoried"
        );
    }
}
