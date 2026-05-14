//! Dispatch for `ayx workflow ...`.
//!
//! Wraps the `ayx_workflow` crate helpers (inspect, unpack, validate,
//! replace, repackage, recurse, scan, migrate, yxdb, convert-cloud) and
//! the `workflow publish` arm that builds a .yxzp on the fly and uploads
//! via `ayx_server_api::workflow_version_upload_envelope`.

use std::fs;

use anyhow::{bail, Context, Result};
use ayx_core::envelope::Envelope;
use ayx_server_api::workflow_version_upload_envelope;
use ayx_workflow::{
    convert_desktop_to_cloud, inspect as inspect_workflow, load_rules as load_workflow_rules,
    migrate as migrate_workflow, read_yxdb as read_yxdb_workflow, recurse as recurse_workflow,
    repackage_dir as repackage_workflow, replace as replace_workflow, scan as scan_workflow,
    unpack_package as unpack_workflow, validate as validate_workflow, CloudConversionOptions,
    WorkflowReplacement,
};
use chrono::Utc;
use serde_json::json;

use crate::{load_profile_with_env, WorkflowCommand};

pub fn execute(environment: Option<&str>, command: Option<WorkflowCommand>) -> Result<Envelope> {
    fn load_profile<'a, P>(p: P, environment: Option<&str>) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        load_profile_with_env(p, environment)
    }
    match command {
        None => Ok(Envelope::ok(
            "workflow commands available: inspect, unpack, validate, replace, repackage, recurse, scan, convert-cloud, publish, migrate, yxdb",
        )),
        Some(WorkflowCommand::Inspect { input }) => {
            let detail = inspect_workflow(&input)?;
            Ok(Envelope::ok_with_data(
                "workflow inspection completed",
                json!({ "input": input.display().to_string(), "data": detail }),
            ))
        }
        Some(WorkflowCommand::Unpack { input, output_dir }) => {
            let detail = unpack_workflow(&input, &output_dir)?;
            Ok(Envelope::ok_with_data(
                "workflow package unpacked",
                json!({
                    "input": input.display().to_string(),
                    "output_dir": output_dir.display().to_string(),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Validate { input }) => {
            let detail = validate_workflow(&input)?;
            Ok(Envelope::ok_with_data(
                "workflow validation completed",
                json!({ "input": input.display().to_string(), "data": detail }),
            ))
        }
        Some(WorkflowCommand::Replace {
            input,
            output,
            find,
            replace,
            validate,
        }) => {
            let detail = replace_workflow(
                &input,
                &output,
                &[WorkflowReplacement { find, replace }],
                validate,
            )?;
            Ok(Envelope::ok_with_data(
                "workflow replacement completed",
                json!({
                    "input": input.display().to_string(),
                    "output": output.display().to_string(),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Repackage { input_dir, output }) => {
            let detail = repackage_workflow(&input_dir, &output)?;
            Ok(Envelope::ok_with_data(
                "workflow package rebuilt",
                json!({
                    "input_dir": input_dir.display().to_string(),
                    "output": output.display().to_string(),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Recurse {
            input,
            output,
            rules,
            find,
            replace,
            validate,
        }) => {
            let replacements = build_replacements(rules.as_ref(), find, replace, "recurse")?;
            let detail = recurse_workflow(&input, &output, &replacements, validate)?;
            Ok(Envelope::ok_with_data(
                "workflow recursion completed",
                json!({
                    "input": input.display().to_string(),
                    "output": output.display().to_string(),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Migrate {
            input,
            output,
            find,
            replace,
            validate,
        }) => {
            let detail = migrate_workflow(
                &input,
                &output,
                &[WorkflowReplacement { find, replace }],
                validate,
            )?;
            Ok(Envelope::ok_with_data(
                "workflow migration completed",
                json!({
                    "input": input.display().to_string(),
                    "output": output.display().to_string(),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Yxdb { input, csv }) => {
            let detail = read_yxdb_workflow(&input, csv.as_deref())?;
            Ok(Envelope::ok_with_data(
                "workflow yxdb read completed",
                json!({
                    "input": input.display().to_string(),
                    "csv": csv.as_ref().map(|path| path.display().to_string()),
                    "data": detail,
                }),
            ))
        }
        Some(WorkflowCommand::Scan {
            input,
            rules,
            find,
            replace,
        }) => {
            let replacements = build_replacements(rules.as_ref(), find, replace, "scan")?;
            let detail = scan_workflow(&input, &replacements)?;
            Ok(Envelope::ok_with_data(
                "workflow scan completed",
                json!({ "input": input.display().to_string(), "data": detail }),
            ))
        }
        Some(WorkflowCommand::ConvertCloud {
            input,
            output,
            fail_on_unsupported,
        }) => {
            let report = convert_desktop_to_cloud(
                &input,
                CloudConversionOptions {
                    fail_on_unsupported,
                },
            )?;
            fs::write(&output, serde_json::to_string_pretty(&report.content)? + "\n")
                .with_context(|| format!("failed to write '{}'", output.display()))?;
            Ok(Envelope::ok_with_data(
                "workflow cloud conversion completed",
                json!({
                    "input": input.display().to_string(),
                    "output": output.display().to_string(),
                    "content_checksum": report.content_checksum,
                    "warning_count": report.warnings.len(),
                    "warnings": report.warnings,
                    "unsupported_tools": report.unsupported_tools,
                    "removed_tools": report.removed_tools,
                    "converted_tool_count": report.converted_tool_count,
                }),
            ))
        }
        #[allow(clippy::too_many_arguments)]
        Some(WorkflowCommand::Publish {
            profile,
            input,
            workflow_id,
            name,
            owner_id,
            others_may_download,
            others_can_execute,
            execution_mode,
            has_private_data_exemption,
            comments,
            make_published,
            workflow_credential_type,
            credential_id,
            bypass_workflow_version_check,
        }) => {
            let config = load_profile(profile.as_deref(), environment)?;
            // Accept either a pre-built .yxzp or a directory we'll zip in a
            // tempfile. The temp filename includes pid + nanos so concurrent
            // publishes don't collide.
            let package_path = if input
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|s| s.eq_ignore_ascii_case("yxzp"))
                .unwrap_or(false)
            {
                input.clone()
            } else if input.is_dir() {
                let temp_package = std::env::temp_dir().join(format!(
                    "ayx-workflow-publish-{}-{}.yxzp",
                    std::process::id(),
                    Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                repackage_workflow(&input, &temp_package)?;
                temp_package
            } else {
                bail!("workflow publish expects a .yxzp package or directory");
            };
            let detail = workflow_version_upload_envelope(
                &config,
                &workflow_id,
                &name,
                &owner_id,
                &package_path,
                others_may_download,
                others_can_execute,
                &execution_mode,
                has_private_data_exemption,
                comments.as_deref(),
                make_published,
                &workflow_credential_type,
                credential_id.as_deref(),
                bypass_workflow_version_check,
            )?;
            Ok(Envelope::ok_with_data(
                "workflow publish requested",
                json!({
                    "input": input.display().to_string(),
                    "package_path": package_path.display().to_string(),
                    "data": detail,
                }),
            ))
        }
    }
}

/// Shared replacement-list builder used by `scan` and `recurse`. Accepts
/// either a rules YAML or parallel `--find`/`--replace` arrays; mismatched
/// lengths bail with a precise error.
fn build_replacements(
    rules: Option<&std::path::PathBuf>,
    find: Vec<String>,
    replace: Vec<String>,
    label: &str,
) -> Result<Vec<WorkflowReplacement>> {
    if let Some(rules) = rules {
        return Ok(load_workflow_rules(rules)?.replacements);
    }
    if find.len() != replace.len() {
        bail!("workflow {label} requires the same number of --find and --replace values");
    }
    Ok(find
        .into_iter()
        .zip(replace)
        .map(|(find, replace)| WorkflowReplacement { find, replace })
        .collect())
}
