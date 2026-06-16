//! `ayx discover` — progressive disclosure for the live CLI tree.
//!
//! This is the agent-facing entry point for inspecting the actual `clap`
//! command graph. The default view is intentionally shallow so a harness can
//! enumerate top-level capabilities quickly; `--deep` expands the full
//! subtree, and a path drill-down narrows to one branch.

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use clap::{ArgAction, CommandFactory};
use serde::Serialize;

use crate::Cli;

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

    let root = Cli::command();
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
        cmd.get_subcommands()
            .filter(|sub| !sub.is_hide_set())
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
