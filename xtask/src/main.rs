use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Repo maintenance tasks for ayx-rs")]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    #[command(about = "Regenerate docs/command-surface.md from the live catalog")]
    RefreshCommandSurface {
        #[arg(long, default_value = "docs/command-surface.md")]
        output: PathBuf,
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        CommandKind::RefreshCommandSurface { output, check } => {
            refresh_command_surface(&output, check)
        }
    }
}

fn refresh_command_surface(output: &Path, check: bool) -> Result<()> {
    let repo_root = workspace_root()?;
    let catalog = run_catalog_list(&repo_root)?;
    let generated = render_command_surface(&catalog)?;
    let output_path = repo_root.join(output);
    if check {
        let existing = fs::read_to_string(&output_path).with_context(|| {
            format!(
                "failed to read existing command surface '{}'",
                output_path.display()
            )
        })?;
        if normalize_generated_surface(&existing) != normalize_generated_surface(&generated) {
            bail!(
                "{} is stale; run `cargo run -q -p xtask -- refresh-command-surface`",
                output_path.display()
            );
        }
        println!("{} is fresh", output_path.display());
        return Ok(());
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }
    fs::write(&output_path, generated)
        .with_context(|| format!("failed to write '{}'", output_path.display()))?;
    println!("Wrote {}", output_path.display());
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("xtask must live inside the workspace"))
}

fn run_catalog_list(repo_root: &Path) -> Result<Value> {
    let output = Command::new("cargo")
        .current_dir(repo_root)
        .args([
            "run", "-q", "-p", "ayx-rs", "--", "--output", "json", "catalog", "list", "--format",
            "full",
        ])
        .output()
        .with_context(|| "failed to run catalog generation command")?;

    if !output.status.success() {
        bail!(
            "catalog generation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8(output.stdout).context("catalog output was not valid UTF-8")?;
    let json: Value = serde_json::from_str(&stdout).context("failed to parse catalog JSON")?;
    Ok(json)
}

fn render_command_surface(catalog: &Value) -> Result<String> {
    let data = catalog
        .get("data")
        .ok_or_else(|| anyhow!("catalog JSON missing data object"))?;
    let commands = data
        .get("commands")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("catalog JSON missing commands array"))?;
    let capabilities = data
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("catalog JSON missing capabilities array"))?;

    let mut commands_by_group: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for command in commands {
        let path = command
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let group = path.split('/').next().unwrap_or("").to_string();
        commands_by_group.entry(group).or_default().push(command);
    }
    for group in commands_by_group.values_mut() {
        group.sort_by(|a, b| {
            a.get("path")
                .and_then(Value::as_str)
                .cmp(&b.get("path").and_then(Value::as_str))
        });
    }

    let mut capabilities_sorted: Vec<&Value> = capabilities.iter().collect();
    capabilities_sorted.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .cmp(&b.get("id").and_then(Value::as_str))
    });

    let generated_utc = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let mut lines = Vec::new();
    lines.push("# AYX Command Surface".to_string());
    lines.push(String::new());
    lines.push(format!(
        "_Generated from_ `cargo run -q -p ayx-rs -- --output json catalog list --format full` _on {}._",
        generated_utc
    ));
    lines.push(String::new());
    lines.push("This file is generated. Refresh it with:".to_string());
    lines.push(String::new());
    lines.push("```powershell".to_string());
    lines.push("cargo run -q -p xtask -- refresh-command-surface".to_string());
    lines.push("```".to_string());
    lines.push(String::new());
    lines.push("## Summary".to_string());
    lines.push(String::new());
    lines.push(format!(
        "- Commands: {}",
        data.get("command_count")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    ));
    lines.push(format!(
        "- Capabilities: {}",
        data.get("capability_count")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    ));
    lines.push(String::new());
    lines.push("## Commands".to_string());
    lines.push(String::new());

    for (group, group_commands) in commands_by_group {
        lines.push(format!("### `{}`", group));
        lines.push(String::new());
        lines.push("| Name | Path | Safety | Mutating | Summary |".to_string());
        lines.push("| --- | --- | --- | --- | --- |".to_string());
        for command in group_commands {
            let name = md_cell(command.get("name").and_then(Value::as_str));
            let path = md_cell(command.get("path").and_then(Value::as_str));
            let safety = md_cell(command.get("safety").and_then(Value::as_str));
            let mutating = yes_no(command.get("mutating").and_then(Value::as_bool));
            let summary = md_cell(command.get("summary").and_then(Value::as_str));
            lines.push(format!(
                "| {} | `{}` | {} | {} | {} |",
                name, path, safety, mutating, summary
            ));
        }
        lines.push(String::new());
    }

    lines.push("## Capabilities".to_string());
    lines.push(String::new());
    lines.push("| Id | Provider | Safety | Available | Tags | Summary |".to_string());
    lines.push("| --- | --- | --- | --- | --- | --- |".to_string());
    for capability in capabilities_sorted {
        let id = md_cell(capability.get("id").and_then(Value::as_str));
        let provider = md_cell(capability.get("provider").and_then(Value::as_str));
        let safety = md_cell(capability.get("safety").and_then(Value::as_str));
        let available = yes_no(capability.get("available").and_then(Value::as_bool));
        let tags = capability
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .map(md_text)
                    .collect::<Vec<_>>()
                    .join("<br>")
            })
            .unwrap_or_default();
        let summary = md_cell(capability.get("summary").and_then(Value::as_str));
        lines.push(format!(
            "| `{}` | {} | {} | {} | {} | {} |",
            id, provider, safety, available, tags, summary
        ));
    }
    lines.push(String::new());
    lines.push("## Non-goals for This Doc".to_string());
    lines.push(String::new());
    lines.push("This spec intentionally does not duplicate:".to_string());
    lines.push(String::new());
    lines.push("- every leaf command".to_string());
    lines.push("- every payload schema".to_string());
    lines.push("- every API endpoint path".to_string());
    lines.push("- every implementation detail of module layout".to_string());
    lines.push(String::new());
    lines.push("Those details belong in command help, the catalog surface, targeted handoff docs, or generated references.".to_string());
    lines.push(String::new());

    Ok(lines.join("\n"))
}

fn md_text(value: &str) -> String {
    value
        .replace('\n', "<br>")
        .replace('|', "\\|")
        .trim()
        .to_string()
}

fn md_cell(value: Option<&str>) -> String {
    md_text(value.unwrap_or(""))
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "",
    }
}

fn normalize_generated_surface(text: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with("_Generated from_ "))
        .collect::<Vec<_>>()
        .join("\n")
}
