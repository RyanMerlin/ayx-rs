//! `ayx discover` — progressive disclosure for the live CLI tree.
//!
//! This is the agent-facing entry point for inspecting the actual `clap`
//! command graph. The default view is intentionally shallow so a harness can
//! enumerate top-level capabilities quickly; `--deep` expands the full
//! subtree, and a path drill-down narrows to one branch.

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use clap::ArgAction;
use serde::Serialize;

use super::command_surface;

#[derive(Debug, Serialize)]
pub struct DiscoverNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<DiscoverArg>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<DiscoverOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subcommands: Vec<DiscoverNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DiscoverArg {
    pub name: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscoverOption {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub takes_value: bool,
}

#[derive(Debug, Serialize)]
struct DiscoverPayload {
    schema_version: u32,
    binary: String,
    version: String,
    path: Vec<String>,
    deep: bool,
    tree: DiscoverNode,
}

pub fn execute(path: Vec<String>, deep: bool) -> Result<Envelope> {
    let path_tokens: Vec<String> = path
        .into_iter()
        .flat_map(|segment| {
            segment
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    let root = command_surface::root_command();
    let mut current: &clap::Command = &root;
    for token in &path_tokens {
        current = current
            .find_subcommand(token)
            .ok_or_else(|| anyhow!("unknown discover path: {}", path_tokens.join(" ")))?;
    }

    let depth = if deep { usize::MAX } else { 1 };
    let node = build_node(current, depth);

    Ok(Envelope::ok_with_data(
        if path_tokens.is_empty() {
            "ayx discover completed"
        } else {
            "ayx discover path completed"
        },
        serde_json::to_value(DiscoverPayload {
            schema_version: 1,
            binary: "ayx".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            path: path_tokens,
            deep,
            tree: node,
        })?,
    ))
}

fn build_node(cmd: &clap::Command, remaining_depth: usize) -> DiscoverNode {
    let aliases: Vec<String> = cmd.get_all_aliases().map(|s| s.to_string()).collect();

    let mut args = Vec::new();
    let mut options = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let name = arg.get_id().to_string();
        let help = arg.get_help().map(|h| h.to_string());
        if arg.is_positional() {
            args.push(DiscoverArg {
                name,
                required: arg.is_required_set(),
                help,
            });
        } else {
            options.push(DiscoverOption {
                name,
                short: arg.get_short(),
                help,
                default: arg
                    .get_default_values()
                    .first()
                    .map(|value| value.to_string_lossy().into_owned()),
                takes_value: matches!(arg.get_action(), ArgAction::Set | ArgAction::Append),
            });
        }
    }

    let subcommands = if remaining_depth == 0 {
        Vec::new()
    } else {
        command_surface::visible_subcommands(cmd)
            .map(|sub| build_node(sub, remaining_depth.saturating_sub(1)))
            .collect()
    };

    DiscoverNode {
        name: cmd.get_name().to_string(),
        about: cmd.get_about().map(|a| a.to_string()),
        aliases,
        args,
        options,
        subcommands,
        hidden: cmd.is_hide_set().then_some(true),
    }
}

/// Flatten a full-depth discover tree into its canonical `path` set
/// (slash-joined, matching `command_surface::LiveCommand::path`). Test-only:
/// this is the narrow helper `command_surface`'s source-of-truth test uses to
/// cross-check discover's tree against `visible_commands()`, without making
/// `DiscoverNode` part of any broader public API.
#[cfg(test)]
pub(crate) fn flatten_deep_tree_paths() -> std::collections::BTreeSet<String> {
    let root = command_surface::root_command();
    let tree = build_node(&root, usize::MAX);

    fn walk(
        node: &DiscoverNode,
        tokens: &mut Vec<String>,
        out: &mut std::collections::BTreeSet<String>,
    ) {
        for sub in &node.subcommands {
            tokens.push(sub.name.clone());
            out.insert(tokens.join("/"));
            walk(sub, tokens, out);
            tokens.pop();
        }
    }

    let mut out = std::collections::BTreeSet::new();
    let mut tokens = Vec::new();
    walk(&tree, &mut tokens, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_node_omits_hidden_subcommands_via_shared_predicate() {
        let synthetic = clap::Command::new("root")
            .subcommand(clap::Command::new("visible").about("Visible child."))
            .subcommand(
                clap::Command::new("secret")
                    .about("Hidden child.")
                    .hide(true),
            );

        let node = build_node(&synthetic, usize::MAX);

        assert_eq!(node.subcommands.len(), 1);
        assert_eq!(node.subcommands[0].name, "visible");
    }

    #[test]
    fn shallow_depth_stops_at_one_level() {
        let root = command_surface::root_command();
        let node = build_node(&root, 1);
        assert!(!node.subcommands.is_empty());
        for sub in &node.subcommands {
            assert!(
                sub.subcommands.is_empty(),
                "{} should have no grandchildren at depth 1",
                sub.name
            );
        }
    }

    #[test]
    fn execute_reports_unknown_path() {
        let result = execute(vec!["definitely-not-a-real-command".to_string()], false);
        assert!(result.is_err());
    }

    #[test]
    fn execute_default_depth_is_shallow() {
        let envelope = execute(Vec::new(), false).expect("execute should succeed");
        let subcommands = envelope.data["tree"]["subcommands"]
            .as_array()
            .expect("tree.subcommands should be an array")
            .clone();
        assert!(!subcommands.is_empty());
        for sub in &subcommands {
            let grandchildren = sub
                .get("subcommands")
                .and_then(|s| s.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            assert_eq!(
                grandchildren, 0,
                "default depth must not include grandchildren"
            );
        }
    }

    #[test]
    fn execute_deep_flatten_matches_command_surface_paths() {
        let discover_paths = flatten_deep_tree_paths();
        let surface_paths = crate::cmd::command_surface::visible_command_paths();
        assert_eq!(discover_paths, surface_paths);
    }
}
