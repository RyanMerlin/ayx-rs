//! Doc <-> inventory cross-check for `docs/one-endpoint-matrix.md`.
//!
//! That doc is a per-endpoint live-probe ledger for the Alteryx One surface. It is
//! authored by hand (the live evidence in its `Live status`/`Verified (UTC)`/`Notes`
//! columns cannot be derived mechanically), but its *rows* — every `(METHOD, path,
//! commands)` triple — must never drift from `ayx-one-api/src/inventory.rs`. A row
//! that names a command the inventory doesn't know about, or an inventory endpoint
//! missing from the doc, means the matrix is silently lying to whoever reads it next
//! — the exact failure mode `one_inventory_drift.rs` guards against for the CLI
//! wiring itself, applied here to this doc.
//!
//! Strategy: parse every markdown table row in the doc whose first cell is an HTTP
//! method, then assert bidirectionally against `ayx_one_api::inventory_endpoints()`:
//! every doc row's `(method, path)` exists in the inventory, and every inventory
//! `(method, path)` pair appears as a doc row. This reads the inventory at runtime
//! (never a hard-coded count), so it stays green as new endpoints land — as long as
//! the doc is regenerated alongside them (see `dump_inventory_for_doc_regeneration`
//! below, and the "How to re-verify" section of the doc itself).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ayx_core::profile::{
    AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile,
};

const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

fn doc_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is `ayx-rs/`; the doc lives at the workspace-root `docs/`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("one-endpoint-matrix.md")
}

fn read_doc() -> String {
    let path = doc_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// One parsed markdown-table row from the doc.
#[derive(Debug, Clone)]
struct DocRow {
    method: String,
    path: String,
    commands: Vec<String>,
}

/// Cells of a `| a | b | c |` markdown table line, trimmed, with the leading/trailing
/// empty cells produced by the outer pipes dropped. Pipe characters can't appear
/// inside these cells (none of the doc's method/path/command text contains one), so a
/// plain split is sufficient — no need for the string-literal-aware scanner
/// `one_inventory_drift.rs` uses for Rust source.
fn table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let mut cells: Vec<&str> = trimmed.split('|').collect();
    if cells.first().is_some_and(|c| c.trim().is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|c| c.trim().is_empty()) {
        cells.pop();
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

/// Strip a single layer of backticks/whitespace from a doc cell, e.g. `` `/v4/flows` ``
/// -> `/v4/flows`.
fn unbacktick(cell: &str) -> String {
    cell.trim().trim_matches('`').trim().to_string()
}

fn parse_doc_rows(doc: &str) -> Vec<DocRow> {
    let mut rows = Vec::new();
    for line in doc.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells = table_cells(line);
        // Every endpoint table in the doc has exactly 8 columns:
        // Method | Path | Live status | Verified (UTC) | ayx command(s) |
        // Response shape | Error-body flavor | Notes
        if cells.len() != 8 {
            continue;
        }
        let method = cells[0].to_string();
        if !HTTP_METHODS.contains(&method.as_str()) {
            // Skips the header row ("Method"), the separator row ("---"), and any
            // other markdown table in the doc (e.g. the two-column Column Legend).
            continue;
        }
        let path = unbacktick(&cells[1]);
        let commands: Vec<String> = cells[4]
            .split("<br>")
            .map(unbacktick)
            .filter(|c| !c.is_empty())
            .collect();
        rows.push(DocRow {
            method,
            path,
            commands,
        });
    }
    rows
}

fn inventory_keys() -> BTreeSet<(String, String)> {
    ayx_one_api::inventory_endpoints()
        .into_iter()
        .map(|(m, p)| (m.to_string(), p.to_string()))
        .collect()
}

#[test]
fn the_doc_parses_into_a_nonempty_row_set() {
    let doc = read_doc();
    let rows = parse_doc_rows(&doc);
    assert!(
        !rows.is_empty(),
        "parsed zero rows out of docs/one-endpoint-matrix.md — the parser or the \
         doc's table shape is broken, not the inventory"
    );
    // Sanity: every parsed row is a real ayx endpoint (`/`-rooted path) and names at
    // least one `one ...` command, exactly like the inventory itself requires
    // (see one_inventory_drift.rs / coverage.rs's equivalent guards).
    for row in &rows {
        assert!(
            row.path.starts_with('/'),
            "doc row {} {} has a path that doesn't start with '/' — parser likely \
             mis-split a table row",
            row.method,
            row.path
        );
        assert!(
            !row.commands.is_empty(),
            "doc row {} {} lists no ayx command(s) — every row in this doc's tables \
             is expected to carry at least one",
            row.method,
            row.path
        );
        for cmd in &row.commands {
            assert!(
                cmd.starts_with("one "),
                "doc row {} {} names command {cmd:?}, which is not an `ayx one ...` \
                 command path",
                row.method,
                row.path
            );
        }
    }
}

#[test]
fn every_doc_row_matches_a_wired_inventory_endpoint() {
    let doc = read_doc();
    let rows = parse_doc_rows(&doc);
    let inventory = inventory_keys();

    let mut missing = Vec::new();
    for row in &rows {
        let key = (row.method.clone(), row.path.clone());
        if !inventory.contains(&key) {
            missing.push(format!(
                "  {} {} (doc commands: {})",
                row.method,
                row.path,
                row.commands.join(", ")
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "docs/one-endpoint-matrix.md lists these rows, but they have no matching \
         (METHOD, path) entry in ayx_one_api::inventory_endpoints() — either the doc \
         is out of sync with the inventory, or it invented a row the CLI doesn't \
         actually dispatch:\n{}\n\nRegenerate the row from inventory.rs (see the \
         doc's \"How to re-verify\" section) rather than hand-editing method/path.",
        missing.join("\n")
    );
}

#[test]
fn every_wired_inventory_endpoint_appears_in_the_doc() {
    let doc = read_doc();
    let rows = parse_doc_rows(&doc);
    let doc_keys: BTreeSet<(String, String)> = rows
        .iter()
        .map(|r| (r.method.clone(), r.path.clone()))
        .collect();
    let inventory = inventory_keys();

    let mut missing: Vec<&(String, String)> = inventory
        .iter()
        .filter(|k| !doc_keys.contains(*k))
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "ayx_one_api::inventory_endpoints() has these (METHOD, path) pairs with no \
         matching row in docs/one-endpoint-matrix.md — a newly-wired endpoint landed \
         without a matrix row:\n{}\n\nAdd a row under the right surface section (see \
         the doc's \"How to re-verify\" section for the regeneration dump), evidence \
         starting as `unverified` / `not probed this session` is fine.",
        missing
            .iter()
            .map(|(m, p)| format!("  {m} {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every command the doc lists for a given `(method, path)` must be a subset of what
/// the inventory actually records for that same pair — the doc is allowed to be
/// incomplete about *evidence*, never wrong about *wiring*.
#[test]
fn doc_commands_never_invent_a_dispatcher_the_inventory_does_not_have() {
    use std::collections::HashMap;

    let doc = read_doc();
    let rows = parse_doc_rows(&doc);

    let mut inv_commands: HashMap<(String, String), BTreeSet<String>> = HashMap::new();
    for (method, path, commands) in ayx_one_api::inventory_endpoints_full() {
        inv_commands
            .entry((method.to_string(), path.to_string()))
            .or_default()
            .extend(commands.iter().map(|c| c.to_string()));
    }

    let mut bad = Vec::new();
    for row in &rows {
        let key = (row.method.clone(), row.path.clone());
        let Some(known) = inv_commands.get(&key) else {
            // Already reported by every_doc_row_matches_a_wired_inventory_endpoint.
            continue;
        };
        for cmd in &row.commands {
            if !known.contains(cmd) {
                bad.push(format!(
                    "  {} {}: doc names `{cmd}`, inventory.rs's commands for this \
                     row are {known:?}",
                    row.method, row.path
                ));
            }
        }
    }

    assert!(
        bad.is_empty(),
        "docs/one-endpoint-matrix.md names a dispatching command inventory.rs does \
         not record for that (method, path):\n{}",
        bad.join("\n")
    );
}

fn config() -> Config {
    Config {
        profile_name: "doc-regen".to_string(),
        mongo: MongoProfile {
            mode: MongoMode::Embedded,
            databases: MongoDatabases {
                gallery_name: "AlteryxGallery".to_string(),
                service_name: "AlteryxService".to_string(),
            },
            embedded: Some(MongoEmbedded {
                runtime_settings_path: None,
                alteryx_service_path: None,
                restore_target_path: None,
            }),
            managed: None,
        },
        alteryx_one: Some(AlteryxOneProfile {
            account_email: "doc-regen@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some("client-id".to_string()),
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some("https://example.invalid/token".to_string()),
            access_token: Some("placeholder".to_string()),
            access_token_ref: None,
            refresh_token: Some("placeholder".to_string()),
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: Default::default(),
        }),
        observability: None,
        server_api: None,
        api: None,
        server: None,
        sqlserver: None,
        upgrade: None,
    }
}

/// Not a drift assertion — a regeneration aid. `inventory_endpoints_full()` has no
/// network dependency (it's a static catalog read), so this is safe to run anywhere,
/// unlike `one_live_smoke.rs`. Run with `--nocapture` (as documented in the doc's
/// "How to re-verify" section) to print the live-wired inventory grouped exactly like
/// this doc's sections, for pasting into new/updated rows by hand.
#[test]
fn dump_inventory_for_doc_regeneration() {
    let env = ayx_one_api::one_surface_inventory_envelope(&config()).expect("inventory");
    println!(
        "{}",
        serde_json::to_string_pretty(&env.data).expect("serialize inventory dump")
    );
}
