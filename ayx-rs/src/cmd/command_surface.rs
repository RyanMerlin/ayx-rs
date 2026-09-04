//! Canonical live-command index — the single source of truth for "what
//! commands does `ayx` actually expose right now."
//!
//! This module owns the visibility policy (`Command::is_hide_set()`) shared
//! by `ayx discover` (human/agent-facing tree walk) and, from a follow-up
//! task on, `ayx catalog` (machine-readable registry). Canonical identity is
//! derived purely from the live `clap::Command` tree built from
//! `Cli::command()` — never from help text, a spawned binary, or a second
//! generated file.
//!
//! `visible_commands()` / `LiveCommand` are the read-side API this module
//! exists to provide; `discover` consumes `root_command()` and
//! `visible_subcommands()`, `cmd::catalog` and `cmd::registry` consume
//! `visible_commands()`/`root_command()`. `visible_command_paths()` remains
//! test-only (cross-checked against `discover --deep` and `catalog list
//! --scope all`), hence the narrow allow below.

use std::collections::BTreeSet;

use clap::CommandFactory;

use crate::Cli;

/// One canonical, owned record for a visible `ayx` command — root and
/// leaf/branch nodes alike (anything reachable and not hidden).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveCommand {
    /// Canonical whitespace-joined identity, e.g. `"one flows list"`.
    pub name: String,
    /// Canonical slash-joined identity, e.g. `"one/flows/list"`.
    pub path: String,
    /// The command's clap `about` text, verbatim.
    pub summary: String,
}

/// Build a fresh root of the live `ayx` command tree.
///
/// Constructed fresh on every call (mirrors how `discover` always built its
/// own `Cli::command()` before this module existed) rather than cached, so
/// callers can hold and walk it without lifetime entanglement with any other
/// caller.
pub(crate) fn root_command() -> clap::Command {
    Cli::command()
}

/// The visible (non-hidden) direct subcommands of `command`. This is the
/// single shared definition of "hidden" for the live command surface —
/// `discover` filters through this instead of re-implementing the predicate.
pub(crate) fn visible_subcommands(command: &clap::Command) -> impl Iterator<Item = &clap::Command> {
    command.get_subcommands().filter(|sub| !sub.is_hide_set())
}

/// Every visible command in the live tree (branch nodes and leaves alike —
/// both are invocable/help-visible surfaces), sorted lexicographically by
/// canonical `path`. Excludes the root itself and anything `is_hide_set()`.
///
/// # Panics
///
/// Panics if a visible node has no non-empty clap `about`, or if two visible
/// nodes resolve to the same canonical path. Both are CLI-authoring bugs,
/// not runtime/input errors — the whole point of this module is that the
/// live tree is trusted to satisfy these invariants.
pub(crate) fn visible_commands() -> Vec<LiveCommand> {
    let root = root_command();
    let mut records = collect_from(&root);

    records.sort_by(|a, b| a.path.cmp(&b.path));

    let mut seen_paths = BTreeSet::new();
    for record in &records {
        if !seen_paths.insert(record.path.clone()) {
            panic!(
                "duplicate canonical command path `{}` — two visible clap \
                 nodes resolved to the same path/name identity",
                record.path
            );
        }
    }

    records
}

/// `visible_commands()`, projected to just the canonical `path` set. Useful
/// for membership checks (e.g. "does the live tree still expose X") without
/// carrying summaries around.
///
/// Used by `remediation_for_error_code` (`main.rs`) to confirm a `<family>
/// list` command actually exists before naming it in a `NotFound`
/// remediation, and cross-checked in tests against `discover --deep` and
/// `catalog list --scope all`.
pub(crate) fn visible_command_paths() -> BTreeSet<String> {
    visible_commands().into_iter().map(|c| c.path).collect()
}

/// Walk `root`'s visible subtree and return every record, unsorted and
/// without duplicate-checking. `visible_commands()` sorts and validates this
/// output; the raw collector is exposed to tests so the synthetic-tree case
/// can exercise it directly against a hand-built `clap::Command` instead of
/// the real `Cli`.
fn collect_from(root: &clap::Command) -> Vec<LiveCommand> {
    let mut tokens = Vec::new();
    let mut records = Vec::new();
    collect_visible(root, &mut tokens, &mut records);
    records
}

/// Recursive step shared by `collect_from`. Carries the canonical
/// `get_name()` token vector down the tree; never consults aliases or
/// `find_subcommand` — those belong to `discover`'s path *resolution*, not
/// canonical id *generation*.
fn collect_visible(command: &clap::Command, tokens: &mut Vec<String>, out: &mut Vec<LiveCommand>) {
    for child in visible_subcommands(command) {
        tokens.push(child.get_name().to_string());

        let summary = child
            .get_about()
            .map(|about| about.to_string())
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "command `{}` is visible but has no non-empty clap `about` — every \
                     visible node must carry a one-line #[command(about = ...)] or /// \
                     doc comment",
                    tokens.join(" ")
                )
            });

        out.push(LiveCommand {
            name: tokens.join(" "),
            path: tokens.join("/"),
            summary,
        });

        // Recurse regardless of whether this child also has its own default
        // action — branch nodes are visible/invocable surfaces too, not just
        // leaves.
        collect_visible(child, tokens, out);

        tokens.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattened_discover_tree_matches_visible_commands() {
        let discover_paths = crate::cmd::discover::flatten_deep_tree_paths();
        let surface_paths = visible_command_paths();
        assert_eq!(
            discover_paths, surface_paths,
            "discover --deep's flattened path set must exactly equal visible_commands()"
        );
    }

    #[test]
    fn no_record_is_root_hidden_or_duplicated() {
        let root = root_command();
        let records = visible_commands();
        assert!(
            !records.is_empty(),
            "expected a non-empty live command tree"
        );

        let mut seen_paths = BTreeSet::new();
        let mut seen_names = BTreeSet::new();
        for record in &records {
            // Root itself is never returned: an included root would carry an
            // empty path/name (zero tokens).
            assert!(!record.path.is_empty(), "root must not be returned");
            assert!(!record.name.is_empty(), "root must not be returned");

            assert!(
                seen_paths.insert(record.path.clone()),
                "duplicate path: {}",
                record.path
            );
            assert!(
                seen_names.insert(record.name.clone()),
                "duplicate name: {}",
                record.name
            );

            assert_eq!(
                record.name.replace(' ', "/"),
                record.path,
                "name/path identity mismatch for {}",
                record.name
            );

            // Independently re-walk the raw tree (via find_subcommand, which
            // is fine for a *test assertion* — the ban on find_subcommand is
            // about canonical id generation, not verification) to confirm
            // this path never resolves to a hidden node.
            let tokens: Vec<&str> = record.path.split('/').collect();
            let mut node = &root;
            for token in &tokens {
                node = node.find_subcommand(token).unwrap_or_else(|| {
                    panic!("path {} did not resolve in the raw tree", record.path)
                });
            }
            assert!(
                !node.is_hide_set(),
                "{} resolved to a hidden node",
                record.path
            );
        }
    }

    #[test]
    fn every_visible_record_has_a_nonempty_single_line_summary() {
        let records = visible_commands();
        assert!(
            !records.is_empty(),
            "expected a non-empty live command tree"
        );
        for record in records {
            assert!(
                !record.summary.trim().is_empty(),
                "{} has an empty summary",
                record.path
            );
            assert!(
                !record.summary.contains('\n'),
                "{} summary is not single-line: {:?}",
                record.path,
                record.summary
            );
        }
    }

    #[test]
    fn synthetic_tree_omits_hidden_child_and_ignores_alias() {
        let synthetic = clap::Command::new("root")
            .subcommand(
                clap::Command::new("visible-child")
                    .about("A visible child command.")
                    .alias("vc")
                    .subcommand(
                        clap::Command::new("hidden-grandchild")
                            .about("Should never appear.")
                            .hide(true),
                    ),
            )
            .subcommand(
                clap::Command::new("hidden-root-child")
                    .about("Should never appear either.")
                    .hide(true),
            );

        let records = collect_from(&synthetic);

        let paths: Vec<&str> = records.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["visible-child"],
            "hidden nodes must be omitted and aliases must not create extra records"
        );
        assert_eq!(records[0].name, "visible-child");
        assert_eq!(records[0].summary, "A visible child command.");
    }

    /// The One endpoint inventory names the CLI commands that dispatch each endpoint,
    /// and `one inventory` prints those names to operators. Nothing kept them honest,
    /// so they drifted: the inventory said `one job-group ...`, `one output-object ...`,
    /// `one write-setting ...` and `one webhooks test` long after those commands were
    /// renamed to their plural/current forms.
    ///
    /// Validate every name against the live clap tree. A rename now fails here instead
    /// of silently making `one inventory` uncopy-pasteable.
    #[test]
    fn inventory_command_names_exist_in_the_live_clap_tree() {
        let live = visible_command_paths();
        let mut unknown: Vec<String> = Vec::new();

        for (method, path, commands) in ayx_one_api::inventory_endpoints_full() {
            for name in commands {
                let as_path = name.replace(' ', "/");
                if !live.contains(&as_path) {
                    unknown.push(format!("  {name:?} (from {method} {path})"));
                }
            }
        }
        unknown.sort();
        unknown.dedup();

        assert!(
            unknown.is_empty(),
            "ayx-one-api/src/inventory.rs names commands that do not exist in the live \
             clap tree:\n{}\n\nEither the command was renamed (update the inventory) or \
             the name is a typo.",
            unknown.join("\n")
        );
    }
}
