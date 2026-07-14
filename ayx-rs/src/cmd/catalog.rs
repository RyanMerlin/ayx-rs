//! Dispatch for `ayx catalog ...`.
//!
//! The catalog surface is the machine-readable registry view: a stable index
//! of commands and capabilities that complements `ayx discover` rather than
//! replacing the live CLI tree.

use anyhow::{Context, Result, anyhow, bail};
use ayx_core::envelope::Envelope;
use serde_json::Value;
use serde_json::json;
use std::fs;

use crate::capability;
use crate::{COMMAND_SPECS, CatalogCommand};

pub fn execute(command: Option<CatalogCommand>) -> Result<Envelope> {
    Ok(match command {
        Some(CatalogCommand::List { tag, format }) => {
            catalog_list_envelope(tag.as_deref(), &format)?
        }
        Some(CatalogCommand::Describe { target, command }) => {
            let target = target.as_deref().or(command.as_deref()).ok_or_else(|| {
                anyhow!("catalog describe requires a command or capability identifier")
            })?;
            catalog_describe_envelope(target)?
        }
        Some(CatalogCommand::Run {
            capability,
            json_input,
            dry_run,
        }) => catalog_run_envelope(&capability, &json_input, dry_run)?,
        None => Envelope::ok("catalog registry commands available: list, describe, run"),
    })
}

pub(crate) fn catalog_list_envelope(tag: Option<&str>, format: &str) -> Result<Envelope> {
    let full = match format {
        "compact" => false,
        "full" => true,
        other => bail!(
            "unsupported catalog format '{}'; use compact or full",
            other
        ),
    };
    let commands: Vec<Value> = COMMAND_SPECS
        .iter()
        .map(|spec| {
            let mut entry = json!({
                "kind": "command",
                "name": spec.name,
                "path": spec.path,
                "summary": spec.summary,
                "output": spec.output,
                "safety": spec.safety,
                "mutating": spec.mutating,
            });
            if full {
                entry["prerequisites"] = json!(spec.prerequisites);
                entry["notes"] = json!(spec.notes);
            }
            entry
        })
        .collect();
    let capabilities = capability::list_capabilities(tag, full)?;

    Ok(Envelope::ok_with_data(
        "catalog entries listed",
        json!({
            "format": format,
            "tag": tag,
            "count": commands.len() + capabilities.len(),
            "command_count": commands.len(),
            "capability_count": capabilities.len(),
            "commands": commands,
            "capabilities": capabilities,
        }),
    ))
}

pub(crate) fn catalog_describe_envelope(identifier: &str) -> Result<Envelope> {
    if let Some(capability) = capability::describe(identifier)? {
        return Ok(Envelope::ok_with_data(
            "catalog capability described",
            capability,
        ));
    }

    let spec = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == identifier || spec.path == identifier)
        .ok_or_else(|| anyhow!("catalog entry '{}' not found", identifier))?;

    Ok(Envelope::ok_with_data(
        "catalog entry described",
        json!({
            "kind": "command",
            "name": spec.name,
            "path": spec.path,
            "summary": spec.summary,
            "output": spec.output,
            "safety": spec.safety,
            "mutating": spec.mutating,
            "prerequisites": spec.prerequisites,
            "notes": spec.notes,
        }),
    ))
}

pub(crate) fn catalog_run_envelope(
    capability_id: &str,
    json_input: &str,
    dry_run: bool,
) -> Result<Envelope> {
    let input = parse_json_arg(json_input)?;
    capability::run(capability_id, &input, dry_run)
}

fn parse_json_arg(raw: &str) -> Result<Value> {
    let text = if let Some(path) = raw.strip_prefix('@') {
        fs::read_to_string(path)
            .with_context(|| format!("failed to read json input file '{}'", path))?
    } else {
        raw.to_string()
    };
    serde_json::from_str(&text).context("failed to parse --json input")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_one_api::format_refresh_token_response;

    #[test]
    fn catalog_list_includes_core_commands() {
        let env = catalog_list_envelope(None, "compact").expect("catalog list should succeed");
        let commands = env.data["commands"].as_array().expect("commands array");
        let names: Vec<&str> = commands
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"profile list"));
        assert!(names.contains(&"profile current"));
        assert!(names.contains(&"profile use"));
        assert!(names.contains(&"doctor"));
        assert!(names.contains(&"doctor config"));
        assert!(names.contains(&"mongo status"));
        assert!(names.contains(&"catalog list"));
        assert!(names.contains(&"license api status"));
        assert!(names.contains(&"license status"));
        assert!(names.contains(&"discover"));
        assert!(names.contains(&"one login"));
        assert!(names.contains(&"one logout"));
        assert!(names.contains(&"one inventory"));
        assert!(names.contains(&"one whoami"));
        assert!(names.contains(&"one person list"));
        assert!(names.contains(&"one person current"));
        assert!(names.contains(&"one person count"));
        assert!(names.contains(&"one person detail"));
        assert!(names.contains(&"one person create"));
        assert!(names.contains(&"one person update"));
        assert!(names.contains(&"one person patch"));
        assert!(names.contains(&"one person delete"));
        assert!(names.contains(&"one person update-password"));
        assert!(names.contains(&"one person password-reset-request"));
        assert!(names.contains(&"one api status"));
        assert!(names.contains(&"one auth status"));
        assert!(names.contains(&"one workspace current"));
        assert!(names.contains(&"one workspace list"));
        assert!(names.contains(&"one workspace current-configuration"));
        assert!(names.contains(&"one workspace configuration-v4"));
        assert!(names.contains(&"one workspace save-current-configuration"));
        assert!(names.contains(&"one workspace save-configuration-v4"));
        assert!(names.contains(&"one role list-assignments"));
        assert!(names.contains(&"one plans status"));
        assert!(names.contains(&"one plans list"));
        assert!(names.contains(&"one plans create"));
        assert!(names.contains(&"one plans full"));
        assert!(names.contains(&"one plans update"));
        assert!(names.contains(&"one plans delete"));
        assert!(names.contains(&"one plans share"));
        assert!(names.contains(&"one flows list"));
        assert!(names.contains(&"one flows count"));
        assert!(names.contains(&"one flows library list"));
        assert!(names.contains(&"one flows library count"));
        assert!(names.contains(&"one flows folders list"));
        assert!(names.contains(&"one flows folders count"));
        assert!(names.contains(&"one flows folders detail"));
        assert!(names.contains(&"one flows folders create"));
        assert!(names.contains(&"one flows folders update"));
        assert!(names.contains(&"one flows folders delete"));
        assert!(names.contains(&"one flows folders flows list"));
        assert!(names.contains(&"one flows folders flows count"));
        assert!(names.contains(&"one flows detail"));
        assert!(names.contains(&"one flows create"));
        assert!(names.contains(&"one flows update"));
        assert!(names.contains(&"one flows delete"));
        assert!(names.contains(&"one flows copy"));
        assert!(names.contains(&"one flows run"));
        assert!(names.contains(&"one flows validate"));
        assert!(names.contains(&"one flows parameters"));
        assert!(names.contains(&"one flows inputs"));
        assert!(names.contains(&"one flows outputs"));
        assert!(names.contains(&"one flows permissions"));
        assert!(names.contains(&"one flows move"));
        assert!(names.contains(&"one flows replace-dataset"));
        assert!(names.contains(&"one flows import"));
        assert!(names.contains(&"one flows import-dry-run"));
        assert!(names.contains(&"one flows export"));
        assert!(names.contains(&"one flows export-dry-run"));
        assert!(names.contains(&"one connections list"));
        assert!(names.contains(&"one connections create"));
        assert!(names.contains(&"one connections dry-run"));
        assert!(names.contains(&"one connections permissions list"));
        assert!(names.contains(&"one connections connector-metadata defaults"));
        assert!(names.contains(&"one connections connector-metadata detail"));
        assert!(names.contains(&"one connections connector-metadata publish-info"));
        assert!(names.contains(&"one connections connector-metadata overrides list"));
        assert!(names.contains(&"one connections connector-metadata overrides create"));
        assert!(names.contains(&"one job-group list"));
        assert!(names.contains(&"one job-group pdf-results"));
        assert!(names.contains(&"one job-group run"));
        assert!(names.contains(&"one job-group publish"));
        assert!(names.contains(&"one output-object list"));
        assert!(names.contains(&"one output-object count"));
        assert!(names.contains(&"one output-object create"));
        assert!(names.contains(&"one output-object detail"));
        assert!(names.contains(&"one output-object update"));
        assert!(names.contains(&"one output-object delete"));
        assert!(names.contains(&"one output-object inputs"));
        assert!(names.contains(&"one output-object wrangle-to-python"));
        assert!(names.contains(&"one webhook-flow-task create"));
        assert!(names.contains(&"one webhook-flow-task detail"));
        assert!(names.contains(&"one webhook-flow-task delete"));
        assert!(names.contains(&"one write-setting create"));
        assert!(names.contains(&"one write-setting list"));
        assert!(names.contains(&"one write-setting count"));
        assert!(names.contains(&"one write-setting detail"));
        assert!(names.contains(&"one write-setting update"));
        assert!(names.contains(&"one write-setting delete"));
        assert!(names.contains(&"one api open-api-spec"));
        assert!(names.contains(&"one api coverage"));
        assert!(names.contains(&"one scheduling list"));
        assert!(names.contains(&"one billing current-account"));
        assert!(names.contains(&"one token list"));
        assert!(names.contains(&"one token create"));
        assert!(names.contains(&"one token detail"));
        assert!(names.contains(&"one token delete"));
        assert!(!names.contains(&"one group"));
        assert!(!names.contains(&"one sso"));
        assert!(!names.contains(&"one audit"));
        assert!(!names.contains(&"one session"));
        assert!(!names.contains(&"one oauth-client"));
        assert!(!names.contains(&"one env-param"));
        assert!(!names.contains(&"one pdh"));
        assert!(!names.contains(&"one app"));
        assert!(!names.contains(&"one health"));
        assert!(!names.contains(&"one status"));
        assert!(!names.contains(&"one user"));
        let capabilities = env.data["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert!(
            capabilities
                .iter()
                .any(|item| item["id"] == "designer.workflow.context")
        );
    }

    #[test]
    fn catalog_describe_finds_path_or_name() {
        let env = catalog_describe_envelope("mongo backup").expect("catalog describe should work");
        assert_eq!(env.data["name"], "mongo backup");
        assert_eq!(env.data["mutating"], true);

        let env = catalog_describe_envelope("server/api/import-swagger")
            .expect("catalog describe should work by path");
        assert_eq!(env.data["name"], "server api import-swagger");

        let env = catalog_describe_envelope("license api diagnose")
            .expect("catalog describe should work for license");
        assert_eq!(env.data["path"], "license/api/diagnose");

        let env = catalog_describe_envelope("one auth diagnose").expect("describe one auth");
        assert_eq!(env.data["path"], "one/auth/diagnose");

        let env = catalog_describe_envelope("designer.workflow.run")
            .expect("catalog describe should work for capability");
        assert_eq!(env.data["kind"], "capability");
        assert_eq!(env.data["provider"], "designer_local");
    }

    #[test]
    fn catalog_list_filters_capabilities_by_tag() {
        let env =
            catalog_list_envelope(Some("cloud"), "compact").expect("catalog list should work");
        let capabilities = env.data["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert!(capabilities.iter().all(|item| {
            item["tags"]
                .as_array()
                .expect("tags")
                .iter()
                .filter_map(Value::as_str)
                .any(|tag| tag == "cloud")
        }));
    }

    #[test]
    fn catalog_run_executes_designer_capability() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("sample.yxmd");
        fs::write(
            &input,
            r#"<AlteryxDocument yxmdVer="2025.2"><Nodes><Node ToolID="1"><GuiSettings Plugin="AlteryxBasePluginsGui.TextInput.TextInput"/></Node></Nodes><Connections/></AlteryxDocument>"#,
        )
        .expect("write sample");

        let json_input = serde_json::to_string(&json!({
            "workflow_path": input.display().to_string(),
        }))
        .expect("serialize");
        let env = catalog_run_envelope("designer.workflow.context", &json_input, false)
            .expect("catalog run should succeed");
        assert_eq!(env.data["capability"]["id"], "designer.workflow.context");
        assert_eq!(env.data["result"]["workflow"]["tool_count"], 1);
    }

    #[test]
    fn one_refresh_token_response_formats_access_token() {
        let token = format_refresh_token_response(&serde_json::json!({
            "token_type": "Bearer",
            "access_token": "fresh-token"
        }))
        .expect("response should format");
        assert_eq!(token, "Bearer fresh-token");
    }
}
