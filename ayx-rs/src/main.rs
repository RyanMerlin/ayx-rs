use std::collections::HashMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::builder::styling::{Color, RgbColor, Style, Styles};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use roxmltree::Document;
use serde_json::{Value, json};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_core::observability::transport_error_summary;
use ayx_core::profile::{
    AyxState, Config, ServerProfile, ayx_config_home, ayx_profiles_dir, ayx_state_path,
    ayx_workspaces_dir, list_central_profiles, load_ayx_state, profile_resolution_detail,
    profile_shape_label, profile_storage_path, resolve_runtime_profile, save_ayx_state,
};
// Most ayx_one + ayx_one_api helpers used by the One dispatch are now imported
// directly in cmd/one.rs. These re-exports stay so the License surface and the
// doctor envelope helpers (still in main.rs) and the TUI can use them.
use ayx_one::{
    api_diagnose_envelope, api_inventory_envelope, api_status_envelope,
    one_surface_inventory_envelope,
};
use ayx_one_api::one_api_live_request;
// server (logs/upgrade/util/api) helpers moved to cmd/server.rs.
// mongo to cmd/mongo.rs. sqlserver to cmd/sqlserver.rs.
use ayx_server::logs::discover_log_inventory;
// workflow helpers + workflow_version_upload_envelope moved to cmd/workflow.rs.
use self_update::Status;
use self_update::backends::github::Update as GitHubUpdate;

mod capability;
mod cmd;
mod onboard;
mod render;
pub(crate) mod secret;
mod tui;

const ALTERYX_BLUE: Color = Color::Rgb(RgbColor(0, 103, 185));
const ALTERYX_CYAN: Color = Color::Rgb(RgbColor(0, 169, 224));
const AYX_STYLES: Styles = Styles::styled()
    .header(Style::new().fg_color(Some(ALTERYX_BLUE)).bold())
    .usage(Style::new().fg_color(Some(ALTERYX_BLUE)).bold())
    .literal(Style::new().fg_color(Some(ALTERYX_CYAN)).bold())
    .placeholder(Style::new().fg_color(Some(ALTERYX_CYAN)));

fn decode_token_claims(access_token: &str) -> Option<Value> {
    let mut parts = access_token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
    serde_json::from_slice::<Value>(&decoded)
        .ok()
        .filter(|value| value.is_object())
}

fn verification_record_matches_claims(
    record: &Value,
    sub: Option<&str>,
    email: Option<&str>,
) -> bool {
    fn matches_value(value: &Value, expected: &str, casefold: bool) -> bool {
        match value {
            Value::Object(map) => map
                .values()
                .any(|nested| matches_value(nested, expected, casefold)),
            Value::Array(items) => items
                .iter()
                .any(|nested| matches_value(nested, expected, casefold)),
            Value::String(observed) => {
                if casefold {
                    observed.eq_ignore_ascii_case(expected)
                } else {
                    observed == expected
                }
            }
            Value::Number(number) => number.to_string() == expected,
            Value::Bool(boolean) => boolean.to_string() == expected,
            Value::Null => false,
        }
    }

    let Some(obj) = record.as_object() else {
        return false;
    };

    let subject_keys = [
        "sub",
        "subject",
        "userId",
        "createdBy",
        "createdById",
        "ownerId",
        "accountId",
    ];
    let email_keys = [
        "email",
        "createdByEmail",
        "ownerEmail",
        "userEmail",
        "createdByUserEmail",
    ];

    if let Some(expected_sub) = sub
        && subject_keys.iter().any(|key| {
            obj.get(*key)
                .is_some_and(|value| matches_value(value, expected_sub, false))
        })
    {
        return true;
    }

    if let Some(expected_email) = email
        && email_keys.iter().any(|key| {
            obj.get(*key)
                .is_some_and(|value| matches_value(value, expected_email, true))
        })
    {
        return true;
    }

    false
}

fn sanitize_verification_payload_for_user(
    verification_payload: &Value,
    access_token: &str,
) -> Value {
    let Some(obj) = verification_payload.as_object() else {
        return verification_payload.clone();
    };
    let Some(data) = obj.get("data").and_then(Value::as_array) else {
        return verification_payload.clone();
    };
    let Some(claims) = decode_token_claims(access_token) else {
        return verification_payload.clone();
    };
    let sub = claims.get("sub").and_then(Value::as_str);
    let email = claims.get("email").and_then(Value::as_str);

    let filtered_data: Vec<Value> = data
        .iter()
        .filter(|item| verification_record_matches_claims(item, sub, email))
        .cloned()
        .collect();

    let mut sanitized = obj.clone();
    sanitized.insert("data".to_string(), Value::Array(filtered_data.clone()));
    sanitized.insert(
        "count".to_string(),
        Value::Number(serde_json::Number::from(filtered_data.len() as u64)),
    );
    Value::Object(sanitized)
}

fn sanitize_live_probe_for_user(probe_data: &Value, access_token: &str) -> Value {
    let Some(obj) = probe_data.as_object() else {
        return probe_data.clone();
    };
    let Some(response) = obj.get("response") else {
        return probe_data.clone();
    };

    let sanitized_response = sanitize_verification_payload_for_user(response, access_token);
    if sanitized_response == *response {
        return probe_data.clone();
    }

    let mut sanitized = obj.clone();
    sanitized.insert("response".to_string(), sanitized_response);
    Value::Object(sanitized)
}

fn access_token_claim_summary(access_token: Option<&str>) -> Option<Value> {
    let claims = decode_token_claims(access_token?)?;
    let exp = claims.get("exp").and_then(Value::as_i64)?;
    let iat = claims.get("iat").and_then(Value::as_i64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)?;
    let mut summary = serde_json::Map::new();
    summary.insert("expired".to_string(), Value::Bool(now >= exp));
    summary.insert("exp".to_string(), Value::from(exp));
    summary.insert(
        "seconds_remaining".to_string(),
        Value::from((exp - now).max(0)),
    );
    if let Some(iat) = iat {
        summary.insert("iat".to_string(), Value::from(iat));
    }
    for key in ["iss", "sub", "email"] {
        if let Some(value) = claims.get(key).cloned() {
            summary.insert(key.to_string(), value);
        }
    }
    if let Some(aud) = claims.get("aud").cloned() {
        summary.insert("aud".to_string(), aud);
    }
    Some(Value::Object(summary))
}

#[derive(Parser, Debug)]
#[command(
    name = "ayx",
    version,
    about = "Operator CLI and TUI for the Alteryx ecosystem (Server, One, Mongo, Designer workflows).",
    long_about = "ayx is a single-binary, agent-friendly CLI for Alteryx administrators and \
                  automation. It produces a uniform JSON envelope (use --output json), gates \
                  mutating One API calls behind --apply (dry-run by default), records audit \
                  artifacts for destructive operations, and resolves profiles from a central \
                  config home so promotion-style workflows can switch environments cleanly. \
                  See `ayx <command> --help` for branch-specific help, `ayx discover` for the \
                  live CLI tree, and `ayx catalog list` for the machine-readable registry.",
    disable_help_subcommand = true,
    styles = AYX_STYLES
)]
struct Cli {
    /// Output format for the result envelope.
    #[arg(
        long,
        default_value = "text",
        value_parser = ["text", "json", "yaml", "table"],
        global = true
    )]
    output: String,
    /// Select a named environment from environments.yaml for this run.
    #[arg(long = "env", alias = "environment", global = true)]
    environment_flag: Option<String>,
    #[arg(value_name = "ENV", hide = true, last = true, global = true)]
    environment_tail: Option<String>,
    /// Global apply flag for mutating One API commands.
    ///
    /// Without `--apply`, mutating One requests (POST/PUT/PATCH/DELETE) return
    /// a structured dry-run envelope describing the request that would be sent
    /// but do not contact the server. Read-only commands ignore this flag.
    /// Per-command `--apply` flags (e.g. on `mongo backup`) take precedence.
    #[arg(long, global = true)]
    apply: bool,
    /// Enable verbose human-readable progress output to stderr. Independent
    /// of `--output`; useful with `--output json` to see what's happening
    /// without polluting the structured stdout payload.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
    /// Enable debug-level logging to stderr. Implies `--verbose`. Use this
    /// when reporting bugs or for in-depth troubleshooting.
    #[arg(long, global = true)]
    debug: bool,
    /// Disable TLS certificate verification globally. Use only for local
    /// development / lab environments; never in production. Equivalent to
    /// setting `verify_tls = false` on every API surface.
    #[arg(long, global = true)]
    no_verify_tls: bool,
    /// Skip the TTY confirmation prompt on destructive operations. Required
    /// for non-interactive automation (CI / pipes) that runs destructive
    /// commands. Has no effect on read-only or non-destructive flows.
    #[arg(long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    fn resolved_environment(&self) -> Option<&str> {
        self.environment_flag
            .as_deref()
            .or(self.environment_tail.as_deref())
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Central profile registry and active profile management")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    #[command(about = "Alteryx One command surface", arg_required_else_help = true)]
    One {
        #[command(subcommand)]
        command: OneCommand,
    },
    #[command(
        about = "Cross-environment tools for environments.yaml source/target workflows",
        arg_required_else_help = true
    )]
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    #[command(about = "Keyring secret inspection and maintenance")]
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    #[command(
        about = "Workflow package and XML tooling for .yxmd, .yxmc, .yxzp, and .yxdb",
        arg_required_else_help = true
    )]
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    #[command(
        about = "Server discovery, logs, auth, diagnose, doctor, upgrade, and low-level API calls"
    )]
    Server {
        #[command(subcommand)]
        command: Option<ServerCommand>,
    },
    #[command(about = "Mongo inventory, backup, restore, query, and doctor helpers")]
    Mongo {
        #[command(subcommand)]
        command: MongoCommand,
    },
    #[command(
        about = "SQL Server status, prechecks, connection helpers, and migration planning",
        arg_required_else_help = true
    )]
    Sqlserver {
        #[command(subcommand)]
        command: SqlserverCommand,
    },
    #[command(
        about = "Interactive first-run setup for config.yaml or environments.yaml with validation and secret reuse"
    )]
    Onboard {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long, alias = "workspace")]
        environments: bool,
        #[arg(long)]
        non_interactive: bool,
    },
    #[command(
        about = "Interactive TUI for central profile selection, explicit file editing, One credentials, and connectivity checks"
    )]
    Tui,
    #[command(
        about = "Machine-readable command registry",
        arg_required_else_help = true
    )]
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    #[command(
        about = "Audit artifact management — list, sweep, retention. Audit files live under ${AYX_CONFIG_HOME}/audits/ by default."
    )]
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    #[command(
        about = "Tactical registry — named playbooks with safety, validation, and rollback notes"
    )]
    Tactics {
        #[command(subcommand)]
        command: TacticsCommand,
    },
    #[command(about = "Workflow registry — higher-order skills composing tactics")]
    Workflows {
        #[command(subcommand)]
        command: WorkflowsCommand,
    },
    #[command(
        about = "Licensing portal branch and API surface",
        arg_required_else_help = true
    )]
    License {
        #[command(subcommand)]
        command: LicenseCommand,
    },
    #[command(
        about = "Show active profile, account email, workspace, and environment in one shot."
    )]
    Whoami {
        /// Override the central profile name. Defaults to `AYX_PROFILE` or the active profile.
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Run configuration, auth, network, and product health diagnostics")]
    Doctor {
        #[command(subcommand)]
        command: Option<DoctorCommand>,
        #[arg(long)]
        fix: bool,
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Self-update from GitHub releases")]
    Update {
        #[arg(long, default_value = "RyanMerlin")]
        repo_owner: String,
        #[arg(long, default_value = "ayx-rs")]
        repo_name: String,
        #[arg(long, default_value = "ayx")]
        bin_name: String,
        #[arg(long)]
        target_version: Option<String>,
        #[arg(long)]
        skip_confirm: bool,
    },
    #[command(about = "Generate shell completion scripts (bash, zsh, fish, powershell, elvish)")]
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    #[command(
        about = "Operational telemetry: running jobs, run history, top workflows/plans, errors, weekly run-counts"
    )]
    Telemetry {
        #[command(subcommand)]
        command: cmd::telemetry::TelemetryCommand,
    },
    #[command(about = "Progressive live discovery of the CLI tree")]
    Discover {
        #[arg(long)]
        deep: bool,
        #[arg(value_name = "PATH")]
        path: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AuditCommand {
    /// Show the resolved audit directory and a quick file count / size summary.
    Status {
        /// Override the audit dir. Defaults to ${AYX_CONFIG_HOME}/audits.
        #[arg(long)]
        audit_dir: Option<PathBuf>,
    },
    /// Delete audit artifacts older than `--retain-days`. Dry-run by default.
    Sweep {
        /// Audit dir to sweep. Defaults to ${AYX_CONFIG_HOME}/audits.
        #[arg(long)]
        audit_dir: Option<PathBuf>,
        /// Keep files newer than this many days; delete the rest.
        #[arg(long, default_value = "30")]
        retain_days: u32,
        /// Actually delete. Without --apply the command reports what *would* be removed.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum TacticsCommand {
    /// List every tactic, with title, safety classification, and tags.
    List {
        /// Filter by tag (substring match).
        #[arg(long)]
        tag: Option<String>,
        /// Filter by safety classification: read_only | mutating | destructive.
        #[arg(long)]
        safety: Option<String>,
    },
    /// Describe a single tactic: steps, validations, rollback.
    Describe {
        /// Tactic id, e.g. `mongo.backup-restore`.
        id: String,
    },
    /// Resolve a free-text task description to a ranked list of candidate tactics.
    Resolve {
        /// The task description, e.g. "back up mongo before a migration".
        #[arg(long)]
        task: String,
        /// Cap returned hits.
        #[arg(long, default_value = "5")]
        limit: usize,
    },
    /// Execute a tactic. Without `--apply`, mutating/destructive tactics
    /// emit a structured plan and never invoke a subprocess. Read-only
    /// tactics always run.
    Run {
        /// Tactic id.
        id: String,
        /// Provide a placeholder value, e.g. `--param profile=prod`. Repeat
        /// for each placeholder referenced by the tactic.
        #[arg(long = "param", value_parser = parse_param_kv, action = clap::ArgAction::Append)]
        param: Vec<(String, String)>,
        /// Load params from a YAML file (`key: value` map). Merged with
        /// `--param` flags; explicit `--param` wins on conflict.
        #[arg(long)]
        param_file: Option<PathBuf>,
        /// Write per-step audit JSON to this directory (mode 0o700/0o600).
        /// Default: ${AYX_CONFIG_HOME}/audits/.
        #[arg(long)]
        audit_dir: Option<PathBuf>,
        /// On a TTY, prompt interactively for any params that the tactic
        /// requires but were not provided via --param or --param-file.
        /// Always off on stdin redirection / CI (we detect TTY).
        #[arg(long)]
        prompt_missing: bool,
    },
    /// Cross-check every step in every loaded tactic against the catalog.
    /// Emits warnings for unknown command paths, capability ids, and
    /// dangling workflow → tactic references. Read-only.
    Validate,
    /// Print a tactic's full YAML so an operator can fork it into their
    /// config home (`${AYX_CONFIG_HOME}/registry/`) to override the bundled
    /// stdlib version.
    Export {
        /// Tactic id.
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum WorkflowsCommand {
    /// List every workflow with its title, safety, and tactic count.
    List {
        #[arg(long)]
        tag: Option<String>,
    },
    /// Explain a workflow: title, safety, ordered tactic ids with summaries.
    Explain {
        /// Workflow id, e.g. `governance.go-live`.
        id: String,
    },
    /// Execute a workflow as an ordered chain of tactics. Honors the same
    /// `--apply` semantics as `tactics run`.
    Run {
        /// Workflow id.
        id: String,
        #[arg(long = "param", value_parser = parse_param_kv, action = clap::ArgAction::Append)]
        param: Vec<(String, String)>,
        #[arg(long)]
        param_file: Option<PathBuf>,
        #[arg(long)]
        audit_dir: Option<PathBuf>,
        #[arg(long)]
        prompt_missing: bool,
    },
}

fn parse_param_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("expected key=value, got '{s}'"))?;
    if k.is_empty() {
        return Err(format!("empty key in '{s}'"));
    }
    Ok((k.to_string(), v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_after_nested_subcommand() {
        let cli = Cli::try_parse_from(["ayx", "one", "flows", "list", "--env", "prod"])
            .expect("parser should accept trailing --env");
        assert_eq!(cli.resolved_environment(), Some("prod"));
    }

    #[test]
    fn missing_argument_errors_classify_as_validation() {
        // A required-argument error must classify as Validation (not Internal)
        // so the user gets the input/`--help` hint instead of a fabricated
        // transport diagnosis.
        let err = anyhow::anyhow!("id is required");
        assert!(matches!(classify_anyhow_error(&err), ErrorCode::Validation));
    }

    #[test]
    fn upstream_5xx_wins_over_body_validation_phrase() {
        // A 5xx whose body echoes a validation phrase is an upstream fault, not
        // a client-side validation error — status code beats body keywords.
        let err = anyhow::anyhow!("HTTP 500 from upstream: client_id is required");
        assert!(matches!(classify_anyhow_error(&err), ErrorCode::Upstream));
    }

    #[test]
    fn rejects_unknown_output_format() {
        // A typo'd --output must be rejected by clap, not silently rendered as
        // text with exit 0 (which would hand an agent unparseable output).
        let parsed = Cli::try_parse_from(["ayx", "--output", "jsn", "profile", "current"]);
        assert!(
            parsed.is_err(),
            "clap should reject an unknown --output value"
        );
    }

    #[test]
    fn accepts_known_output_formats() {
        for fmt in ["text", "json", "yaml", "table"] {
            let parsed = Cli::try_parse_from(["ayx", "--output", fmt, "profile", "current"]);
            assert!(parsed.is_ok(), "clap should accept --output {fmt}");
        }
    }

    #[test]
    fn one_only_telemetry_commands_reject_server_source_at_parse_time() {
        for args in [
            ["ayx", "telemetry", "summary", "--source", "server"].as_slice(),
            [
                "ayx",
                "telemetry",
                "weekly",
                "run-counts",
                "--source",
                "server",
            ]
            .as_slice(),
            ["ayx", "telemetry", "workflows", "top", "--source", "server"].as_slice(),
        ] {
            let err = Cli::try_parse_from(args).expect_err("server source should not parse");
            let rendered = err.to_string();
            assert!(rendered.contains("invalid value 'server'"));
            assert!(rendered.contains("possible values: one"));
        }
    }

    #[test]
    fn server_capable_telemetry_commands_still_accept_server_source() {
        let parsed =
            Cli::try_parse_from(["ayx", "telemetry", "queue", "status", "--source", "server"]);
        assert!(
            parsed.is_ok(),
            "server-capable telemetry commands should still parse --source server"
        );
    }
}

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    #[command(about = "List centrally managed profiles and show the active profile.")]
    List,
    #[command(about = "Show the active central profile pointer.")]
    Current,
    #[command(about = "Show the resolved central profile and configured sections")]
    Show { name: Option<String> },
    #[command(about = "Set the active central profile.")]
    Use { name: String },
    #[command(about = "Show central profile storage paths")]
    Path,
    #[command(about = "Migrate a legacy profile into the central registry")]
    Migrate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum SecretCommand {
    #[command(
        about = "Remove orphaned keyring accounts from the pre-v0.11.0 profile_name-scoped naming scheme",
        long_about = "Identifies keyring accounts written by ayx < v0.11.0 where the \
                      profile_name field differs from the on-disk file stem.  Dry-run by \
                      default; use --apply to delete."
    )]
    Prune {
        #[arg(long, help = "Limit to a single profile by file stem (e.g. 'default')")]
        profile: Option<String>,
        #[arg(long, help = "Delete the orphaned accounts (default: dry-run only)")]
        apply: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DoctorCommand {
    #[command(
        about = "Validate config home, active profile resolution, and inline secret posture."
    )]
    Config,
    #[command(about = "Check One and Server credential posture")]
    Auth,
    #[command(about = "Check configured One and Server network targets")]
    Network,
    #[command(about = "Check One auth and workspace probe posture")]
    One,
    #[command(about = "Check Server configuration posture and next-step guidance")]
    Server,
    #[command(about = "Check Mongo mode and managed connection posture")]
    Mongo,
    /// Run every applicable diagnostic in sequence and return one merged envelope
    /// with per-check status/summary fields plus an overall rollup.
    All,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MongoCommand {
    #[command(about = "Resolve the configured Mongo connection and database names.")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Generate an inventory plan for the Mongo-backed databases.")]
    Inventory {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Back up the Gallery and Service Mongo databases.")]
    Backup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "backups")]
        output_dir: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
    #[command(about = "Restore Mongo data from a backup input path.")]
    Restore {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        input_path: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
    #[command(about = "Run a read-only Mongo query against a Server collection.")]
    Query {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        projection: Option<String>,
        #[arg(long)]
        sort: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        print: bool,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        template: Option<String>,
    },
    #[command(about = "Apply a guarded Mongo update to a Server collection")]
    Mutate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        update: Option<String>,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        print: bool,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        accept_mutation_risk: bool,
    },
    #[command(about = "Run the default support query suite across critical Mongo collections.")]
    Doctor {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerCommand {
    #[command(about = "Server API status, diagnostics, and OpenAPI-driven calls")]
    Api {
        #[command(subcommand)]
        command: ServerApiCommand,
    },
    #[command(about = "Capture host system information to JSON")]
    SystemInfo {
        #[arg(
            long = "output-file",
            value_name = "FILE",
            default_value = "system_info.json"
        )]
        output_file: PathBuf,
    },
    #[command(about = "Summarize RuntimeSettings.xml and export JSON")]
    RuntimeSettings {
        #[arg(long, default_value = DEFAULT_RUNTIME_SETTINGS_PATH)]
        path: PathBuf,
        #[arg(long = "output-file", value_name = "FILE")]
        output_file: Option<PathBuf>,
    },
    #[command(about = "Show common Alteryx Server filesystem paths")]
    AyxPaths,
    #[command(about = "Discover, summarize, and parse Server logs")]
    ServerLogs {
        #[command(subcommand)]
        command: ServerLogsCommand,
    },
    #[command(about = "Run targeted Server diagnostics")]
    Diagnose {
        #[command(subcommand)]
        command: ServerDiagnoseCommand,
    },
    #[command(about = "Server SSO/SAML auth diagnosis and simulation")]
    Auth {
        #[command(subcommand)]
        command: ServerAuthCommand,
    },
    #[command(about = "Guided Server troubleshooting workflows")]
    Doctor {
        #[command(subcommand)]
        command: ServerDoctorCommand,
    },
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    #[command(about = "Generate a Server backup file plan")]
    BackupPlan {
        #[arg(long)]
        backup_dir: PathBuf,
    },
    #[command(about = "Run or simulate a full Server backup")]
    Backup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "backups")]
        backup_dir: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SqlserverCommand {
    #[command(about = "Summarize configured SQL Server connection posture")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Summarize SQL Server inventory and database posture")]
    Inventory {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Run SQL Server migration prechecks")]
    Precheck {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        collation: Option<String>,
    },
    #[command(about = "Validate configured SQL Server connection strings")]
    ValidateStrings {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Generate a SQL Server connection string")]
    ConnectionString {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "controller")]
        scope: String,
        #[arg(long, default_value = "sql")]
        auth: String,
        #[arg(long)]
        server: Option<String>,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        encrypt: bool,
        #[arg(long)]
        trust_server_certificate: bool,
        #[arg(long)]
        multi_subnet_failover: bool,
    },
    #[command(about = "Generate a SQL Server migration plan")]
    Migrate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        target_version: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    #[command(about = "Generate SQL Server migration preparation guidance")]
    Prepare {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        target_version: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum WorkflowCommand {
    #[command(about = "Inspect Alteryx workflow, macro, package, or data artifacts.")]
    Inspect {
        #[arg(long)]
        input: PathBuf,
    },
    #[command(about = "Unpack a .yxzp workflow package.")]
    Unpack {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    #[command(about = "Validate workflow and macro XML structures.")]
    Validate {
        #[arg(long)]
        input: PathBuf,
    },
    #[command(about = "Find and replace text in workflow XML or packages.")]
    Replace {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "output-path")]
        output_path: PathBuf,
        #[arg(long)]
        find: String,
        #[arg(long)]
        replace: String,
        #[arg(long)]
        validate: bool,
    },
    #[command(about = "Rebuild a .yxzp package from a directory tree.")]
    Repackage {
        #[arg(long)]
        input_dir: PathBuf,
        #[arg(long = "output-path")]
        output_path: PathBuf,
    },
    #[command(about = "Recursively apply XML replacement rules across workflow artifacts.")]
    Recurse {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "output-path")]
        output_path: PathBuf,
        #[arg(long)]
        rules: Option<PathBuf>,
        #[arg(long = "find")]
        find: Vec<String>,
        #[arg(long = "replace")]
        replace: Vec<String>,
        #[arg(long)]
        validate: bool,
    },
    #[command(about = "Preflight scan workflow artifacts for rule matches without rewriting.")]
    Scan {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        rules: Option<PathBuf>,
        #[arg(long = "find")]
        find: Vec<String>,
        #[arg(long = "replace")]
        replace: Vec<String>,
    },
    #[command(about = "Convert a desktop workflow into cloud JSON")]
    ConvertCloud {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "output-path")]
        output_path: PathBuf,
        #[arg(long, default_value_t = false)]
        fail_on_unsupported: bool,
    },
    #[command(about = "Republish a workflow package through the Server API.")]
    Publish {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        workflow_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        owner_id: String,
        #[arg(long, default_value_t = true)]
        others_may_download: bool,
        #[arg(long, default_value_t = true)]
        others_can_execute: bool,
        #[arg(long, default_value = "Standard")]
        execution_mode: String,
        #[arg(long, default_value_t = false)]
        has_private_data_exemption: bool,
        #[arg(long)]
        comments: Option<String>,
        #[arg(long, default_value_t = true)]
        make_published: bool,
        #[arg(long, default_value = "Default")]
        workflow_credential_type: String,
        #[arg(long)]
        credential_id: Option<String>,
        #[arg(long, default_value_t = false)]
        bypass_workflow_version_check: bool,
    },
    #[command(about = "Perform an end-to-end workflow XML migration pass.")]
    Migrate {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "output-path")]
        output_path: PathBuf,
        #[arg(long)]
        find: String,
        #[arg(long)]
        replace: String,
        #[arg(long)]
        validate: bool,
    },
    #[command(
        about = "Read and export .yxdb data; use --csv for export and top-level --output json for machine-readable envelopes"
    )]
    Yxdb {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        csv: Option<PathBuf>,
    },
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiCommand {
    /// Manage the experimental One UI browser session.
    #[command(arg_required_else_help = true)]
    Session {
        #[command(subcommand)]
        command: UiSessionCommand,
    },
    /// Open and inspect workflows in the experimental One UI.
    #[command(arg_required_else_help = true)]
    Workflow {
        #[command(subcommand)]
        command: UiWorkflowCommand,
    },
    /// Browse datasets and connections in the experimental One UI.
    #[command(arg_required_else_help = true)]
    Data {
        #[command(subcommand)]
        command: UiDataCommand,
    },
    /// Inventory the experimental One UI library page.
    #[command(arg_required_else_help = true)]
    Library {
        #[command(subcommand)]
        command: UiLibraryCommand,
    },
    /// Inventory the experimental One UI schedules page.
    #[command(arg_required_else_help = true)]
    Schedules {
        #[command(subcommand)]
        command: UiSchedulesCommand,
    },
    /// Inventory the experimental One UI jobs page.
    #[command(arg_required_else_help = true)]
    Jobs {
        #[command(subcommand)]
        command: UiJobsCommand,
    },
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiSessionCommand {
    /// Report the experimental One visual interface session policy and reuse posture.
    Status,
    /// Ensure the experimental One UI browser session is warm and ready.
    Ensure,
    /// Attach to a specific experimental One UI browser tab.
    Attach {
        #[arg(long)]
        tab: Option<String>,
    },
    /// Inventory the experimental One UI session's open tabs.
    Inventory,
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiWorkflowCommand {
    /// Open a workflow canvas in the experimental One UI.
    Open {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        foreground: bool,
    },
    /// Create a new workflow in the experimental One UI.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        foreground: bool,
    },
    /// Inventory the experimental workflow page canvas, config pane, and results pane.
    Inventory {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        foreground: bool,
    },
    /// Read a tool's configuration pane in the experimental One UI.
    PaneConfig {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: Option<String>,
    },
    /// Read a tool's results pane in the experimental One UI.
    PaneResults {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: Option<String>,
    },
    /// List tools on the workflow canvas in the experimental One UI.
    ToolList {
        #[arg(long)]
        workflow_id: Option<String>,
    },
    /// Select a tool on the workflow canvas in the experimental One UI.
    ToolSelect {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: String,
    },
    /// Inspect a tool's configuration in the experimental One UI.
    ToolInspect {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: String,
    },
    /// Read the workflow's tool graph in the experimental One UI.
    GraphGet {
        #[arg(long)]
        workflow_id: Option<String>,
    },
    /// Write the workflow's tool graph in the experimental One UI.
    GraphPut {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        input: PathBuf,
    },
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiDataCommand {
    /// List available One datasets from the visual data page.
    ListDatasets {
        #[arg(long)]
        foreground: bool,
    },
    /// Inspect a dataset's detail in the experimental One UI.
    DatasetDetail {
        #[arg(long)]
        dataset_id: String,
        #[arg(long)]
        foreground: bool,
    },
    /// Preview a dataset's rows in the experimental One UI.
    DatasetPreview {
        #[arg(long)]
        dataset_id: String,
        #[arg(long)]
        foreground: bool,
    },
    /// Upload a dataset file in the experimental One UI.
    Upload {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        foreground: bool,
    },
    /// List available One connections from the visual data page (experimental).
    ListConnections {
        #[arg(long)]
        foreground: bool,
    },
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiLibraryCommand {
    /// Inventory the experimental One UI library page.
    Inventory,
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiSchedulesCommand {
    /// Inventory the experimental One UI schedules page.
    Inventory,
}

#[cfg(feature = "ui")]
#[derive(Subcommand, Debug)]
pub(crate) enum UiJobsCommand {
    /// Inventory the experimental One UI jobs page.
    Inventory,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ToolsCommand {
    #[command(
        about = "Cross-environment workspace scaffolding and comparison",
        arg_required_else_help = true
    )]
    Workspace {
        #[command(subcommand)]
        command: ToolsWorkspaceCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ToolsWorkspaceCommand {
    #[command(about = "Write an environments.yaml workspace template")]
    Init {
        #[arg(
            long = "output-file",
            value_name = "FILE",
            default_value = "environments.yaml"
        )]
        output_file: PathBuf,
        #[arg(long, default_value = "dev")]
        active_environment: String,
        #[arg(long, default_value = "dev")]
        source_environment: String,
        #[arg(long, default_value = "prod")]
        target_environment: String,
    },
    #[command(about = "Resolve source and target environments from a workspace")]
    Resolve {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    #[command(about = "Compare source and target workspace profiles")]
    Compare {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    #[command(about = "Scaffold cross-environment workflow migration")]
    MigrateWorkflows {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    #[command(about = "Scaffold cross-environment DCM connection checks")]
    CheckDcmConnections {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneCommand {
    /// Authenticate with Alteryx One and store credentials.
    ///
    /// Default (no flags): email OTP flow — sends a one-time passcode to your
    /// account email address, then completes the Alteryx One OIDC workspace
    /// handshake via a pure-HTTP reqwest flow (no browser or Python required).
    ///
    /// With --device: device-code flow — prints a short URL and code; open
    /// the URL on any device, enter the code, and the CLI stores your tokens
    /// automatically.
    ///
    /// With --browser: PKCE authorization-code flow — opens your default
    /// browser and captures tokens via a local redirect.
    ///
    /// With --refresh-token / --access-token: store tokens you already have
    /// (for scripted / CI use).
    Login {
        #[arg(long)]
        profile: Option<String>,
        /// OAuth client ID (defaults to the profile's oauth_client_id).
        #[arg(long)]
        client_id: Option<String>,
        /// Use the browser-redirect PKCE flow instead of email OTP.
        #[arg(long)]
        browser: bool,
        /// Use device-code grant instead of email OTP.
        #[arg(long)]
        device: bool,
        /// Refresh token to store and exchange (bypasses interactive flow).
        #[arg(long)]
        refresh_token: Option<String>,
        /// Access token to store directly (no exchange; bypasses interactive flow).
        #[arg(long)]
        access_token: Option<String>,
        /// Token endpoint URL (defaults to the profile's configured endpoint).
        #[arg(long)]
        token_endpoint: Option<String>,
        /// Workspace id to bind these credentials to (key in workspace_credentials).
        #[arg(long)]
        workspace_id: Option<String>,
        /// Workspace ULID (gid) — stored as workspace_gid for SP scope.
        #[arg(long)]
        workspace_gid: Option<String>,
    },
    /// Clear stored Alteryx One credentials from the active profile.
    Logout {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Show the current One user profile.
    Whoami,
    #[command(
        about = "Summarize One API token posture for managed IAM",
        arg_required_else_help = true
    )]
    Auth {
        #[command(subcommand)]
        command: OneAuthCommand,
    },
    #[command(
        about = "Alteryx One workspace inspection and administration",
        arg_required_else_help = true
    )]
    Workspace {
        #[command(subcommand)]
        command: OneWorkspaceCommand,
    },
    #[command(
        about = "Alteryx One managed-IAM role assignments",
        arg_required_else_help = true
    )]
    Role {
        #[command(subcommand)]
        command: OneRoleCommand,
    },
    // NOTE: Token/Person stay `Option<...>` -- bare `ayx one token` / `ayx one
    // person` have real default behavior (list), not just a dead help
    // string, so they're intentionally excluded from the
    // arg_required_else_help conversion (see cmd/one_platform/token.rs and
    // cmd/one_platform/person.rs).
    #[command(about = "Alteryx One API access token management")]
    Token {
        #[command(subcommand)]
        command: Option<OneTokenCommand>,
    },
    #[command(about = "Alteryx One person (user) management")]
    Person {
        #[command(subcommand)]
        command: Option<OnePersonCommand>,
    },
    /// Summarize the current One API surface registry.
    Inventory {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Alteryx One API introspection (spec + coverage).
    #[command(arg_required_else_help = true)]
    Api {
        #[command(subcommand)]
        command: OneApiCommand,
    },
    #[command(
        about = "Alteryx One configuration, auth, and product health diagnostics",
        arg_required_else_help = true
    )]
    Doctor {
        #[command(subcommand)]
        command: OneDoctorCommand,
    },
    #[command(
        about = "Alteryx One plans — list, run, share, and manage",
        long_about = "Alteryx One plans — list, run, share, and manage. Note: the Plans API \
                      requires an enterprise-tier workspace — returns 404 on some workspace \
                      tiers.",
        arg_required_else_help = true
    )]
    Plans {
        #[command(subcommand)]
        command: OnePlansCommand,
    },
    #[command(
        about = "Alteryx One flows — list, run, import, and export",
        arg_required_else_help = true
    )]
    Flows {
        #[command(subcommand)]
        command: OneFlowsCommand,
    },
    #[command(
        about = "Read datasets from the Alteryx One dataset APIs",
        arg_required_else_help = true
    )]
    Datasets {
        #[command(subcommand)]
        command: OneDatasetsCommand,
    },
    #[command(
        about = "Alteryx One connections — list, create, and manage credentials",
        arg_required_else_help = true
    )]
    Connections {
        #[command(subcommand)]
        command: OneConnectionsCommand,
    },
    #[command(
        about = "Alteryx One job groups — run, publish, and inspect",
        arg_required_else_help = true
    )]
    JobGroups {
        #[command(subcommand)]
        command: OneJobGroupCommand,
    },
    #[command(
        about = "Alteryx One output objects — list, create, and manage",
        arg_required_else_help = true
    )]
    OutputObjects {
        #[command(subcommand)]
        command: OneOutputObjectCommand,
    },
    #[command(
        about = "Alteryx One webhook flow tasks — create, inspect, and test",
        arg_required_else_help = true
    )]
    WebhookFlowTasks {
        #[command(subcommand)]
        command: OneWebhookFlowTaskCommand,
    },
    #[command(
        about = "Alteryx One write settings — list, create, and manage",
        arg_required_else_help = true
    )]
    WriteSettings {
        #[command(subcommand)]
        command: OneWriteSettingCommand,
    },
    #[command(
        about = "Alteryx One schedules — list, enable, and disable",
        long_about = "Alteryx One schedules — list, enable, and disable. Note: the Scheduling \
                      API requires an enterprise-tier workspace — returns 404 on some workspace \
                      tiers.",
        arg_required_else_help = true
    )]
    Scheduling {
        #[command(subcommand)]
        command: OneSchedulingCommand,
    },
    #[command(
        about = "Alteryx One billing account and usage export",
        long_about = "Alteryx One billing account and usage export. Note: the Billing API \
                      requires an enterprise-tier workspace — returns 404 on some workspace \
                      tiers.",
        arg_required_else_help = true
    )]
    Billing {
        #[command(subcommand)]
        command: OneBillingCommand,
    },
    #[cfg(feature = "ui")]
    #[command(
        about = "Experimental Alteryx One visual interface surface",
        arg_required_else_help = true
    )]
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneTokenCommand {
    /// List One API access tokens.
    List,
    /// Create a One API access token from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One API access token by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Delete a One API access token by id.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePersonCommand {
    /// List One people.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Inspect the current One person record.
    Current,
    /// Count One people.
    Count,
    /// Inspect a One person record by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Create a One person from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Replace a One person record from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Patch a One person record from JSON payload.
    Patch {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One person record.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update the current One person's password from JSON payload.
    UpdatePassword {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Request a One password reset from JSON payload.
    PasswordResetRequest {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWorkspaceCommand {
    /// List accessible One workspaces.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Inspect the current One workspace posture.
    Current,
    /// Inspect the current One workspace configuration.
    CurrentConfiguration,
    /// Inspect a One workspace configuration by id.
    ConfigurationV4 {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update the current One workspace configuration from JSON payload.
    SaveCurrentConfiguration {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Update a One workspace configuration by id from JSON payload.
    SaveConfigurationV4 {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One workspace configuration by id.
    Configuration {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect the workspace configuration schema.
    ConfigurationSchema {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect the current workspace configuration schema.
    CurrentConfigurationSchema,
    /// Reset the current workspace configuration.
    DeleteCurrentConfiguration {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Reset a workspace configuration by workspace id.
    DeleteConfiguration {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List people in the current One workspace.
    People,
    /// List workspace admins.
    Admins,
    /// Select which authenticated workspace is active for this profile.
    Switch {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Invite users to a One workspace.
    InviteUsers {
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Remove a user from a One workspace.
    RemoveUser {
        #[arg(long)]
        workspace_id: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Suspend users in a One workspace.
    SuspendUsers {
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Unsuspend users in a One workspace.
    UnsuspendUsers {
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Start a transfer for a One workspace.
    Transfer {
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Transfer assets from the current One workspace from JSON payload.
    TransferAssets {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneRoleCommand {
    /// Inspect role assignments for One managed IAM.
    ListAssignments {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Assign a subject to a One managed IAM role.
    Assign {
        #[arg(value_name = "ROLE-ID")]
        role_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
    },
    /// Unassign a subject from a One managed IAM role.
    Unassign {
        #[arg(value_name = "ROLE-ID")]
        role_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneApiCommand {
    /// Summarize the Alteryx One API posture.
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Validate Alteryx One API reachability and auth posture.
    Diagnose {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Fetch the Alteryx One OpenAPI specification.
    OpenApiSpec {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Diff the live One OpenAPI spec against the wired-command inventory.
    Coverage {
        #[arg(long)]
        profile: Option<String>,
        /// Diff a saved OpenAPI spec JSON file instead of fetching live.
        #[arg(long, value_name = "FILE")]
        spec: Option<std::path::PathBuf>,
        /// Exit non-zero if any endpoint is missing (CI regression gate).
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneAuthCommand {
    /// Summarize One API token posture for managed IAM.
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Validate One API token reachability and workspace scope.
    Diagnose {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlansCommand {
    /// List One plans.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Create a One plan.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One plan.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect a One plan with the full documented payload.
    Full {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Run a One plan.
    Run {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Count One plans.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Inspect run parameters for a One plan.
    RunParameters {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List schedules for a One plan.
    Schedules {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Fetch a One plan package.
    Export {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One plan from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One plan.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Share a One plan from JSON payload.
    Share {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Import a One plan package.
    Import {
        #[arg(long)]
        profile: Option<String>,
    },
    /// List plan permissions, or delete one when `--subject-id` is provided.
    Permissions {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        subject_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneFlowsCommand {
    /// List One flows.
    List {
        #[arg(long)]
        profile: Option<String>,
        /// Cap results per page (server-side limit). Default is the server's
        /// own page size (typically 100 for /v4/flows).
        #[arg(long)]
        limit: Option<u32>,
        /// Fetch a specific page; pass the `nextPageToken` returned by a
        /// previous call.
        #[arg(long)]
        page_token: Option<String>,
        /// Automatically follow `nextPageToken` until all pages are fetched.
        /// Capped by `--max-pages` (default 50).
        #[arg(long)]
        all: bool,
        /// Hard cap on pages when `--all` is set. Prevents runaway loops
        /// against very large tenants.
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count One flows.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(arg_required_else_help = true)]
    /// Browse the One flow library (list, count).
    Library {
        #[command(subcommand)]
        command: OneFlowLibraryCommand,
    },
    #[command(arg_required_else_help = true)]
    /// Manage One flow folders (list, create, update, delete, nested flows).
    Folders {
        #[command(subcommand)]
        command: OneFlowFoldersCommand,
    },
    /// Create a One flow from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One flow by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One flow from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One flow.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Copy a One flow using a JSON payload.
    Copy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        body: Option<PathBuf>,
    },
    /// Run a One flow using a JSON payload.
    Run {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        body: Option<PathBuf>,
    },
    /// Validate a One flow.
    Validate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect flow-level parameters and overrides.
    Parameters {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        output_object_type: Option<String>,
    },
    /// List inputs for a One flow.
    Inputs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List outputs for a One flow.
    Outputs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List permissions for a One flow.
    PermissionsGet {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Share a flow from JSON payload.
    Permissions {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Move a One flow from JSON payload.
    Move {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Replace a dataset in a One flow from JSON payload.
    ReplaceDataset {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Import a flow package.
    Import {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        #[arg(long)]
        folder_id: Option<String>,
        #[arg(long)]
        from_ui: bool,
        #[arg(long)]
        override_js_udfs: bool,
    },
    /// Dry-run import of a flow package.
    ImportDryRun {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        #[arg(long)]
        folder_id: Option<String>,
        #[arg(long)]
        from_ui: bool,
        #[arg(long)]
        override_js_udfs: bool,
    },
    /// Export a flow package to disk.
    Export {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(
            long = "output-file",
            value_name = "FILE",
            help = "path to write the exported .yxzp package"
        )]
        output_file: PathBuf,
    },
    /// Dry-run export of a flow package.
    ExportDryRun {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneDatasetsCommand {
    /// List datasets in the user-facing One dataset library.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count datasets in the user-facing One dataset library.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Read wrangled-dataset resources.
    #[command(arg_required_else_help = true)]
    Wrangled {
        #[command(subcommand)]
        command: OneDatasetsWrangledCommand,
    },
    /// Read imported-dataset resources.
    #[command(arg_required_else_help = true)]
    Imported {
        #[command(subcommand)]
        command: OneDatasetsImportedCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneDatasetsWrangledCommand {
    /// List wrangled datasets.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count wrangled datasets.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Inspect a wrangled dataset by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneDatasetsImportedCommand {
    /// Inspect an imported dataset by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneFlowLibraryCommand {
    /// List the One flow library.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count the One flow library.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneFlowFoldersCommand {
    /// List flow folders.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count flow folders.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Inspect a flow folder by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Create a flow folder from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Update a flow folder from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a flow folder.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    #[command(arg_required_else_help = true)]
    /// List or count flows within a folder.
    Flows {
        #[command(subcommand)]
        command: OneFlowFolderFlowsCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneFlowFolderFlowsCommand {
    /// List flows in a folder.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count flows in a folder.
    Count {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectionsCommand {
    /// List One connections.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count One connections.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Create a One connection from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Dry-run creation of a One connection.
    DryRun {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One connection.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect connection status.
    Status {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One connection from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One connection.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    #[command(
        arg_required_else_help = true,
        long_about = "Inspect connector metadata — defaults, detail, publish info, and \
                      overrides. Note: connector enumeration (list) is not available via the \
                      Alteryx One v4 API — use a known connector slug (e.g. 'gsheetsuser', \
                      'remotefile') with 'detail' to discover the schema."
    )]
    /// Inspect connector metadata — defaults, detail, publish info, and overrides.
    ConnectorMetadata {
        #[command(subcommand)]
        command: OneConnectorMetadataCommand,
    },
    #[command(arg_required_else_help = true)]
    /// Manage permissions for a One connection.
    Permissions {
        #[command(subcommand)]
        command: OneConnectionPermissionCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectorMetadataCommand {
    /// Inspect connector defaults.
    Defaults {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
    /// Inspect connector publish information.
    PublishInfo {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
    /// Inspect current connector metadata.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
    #[command(arg_required_else_help = true)]
    /// Manage connector metadata overrides.
    Overrides {
        #[command(subcommand)]
        command: OneConnectorMetadataOverridesCommand,
    },
    /// Fetch connector metadata defaults and emit a fillable JSON template
    /// for use with `connections create --body <file>`.
    ///
    /// The `type` field is derived from the connector category:
    /// `relational` -> `jdbc`, everything else -> `remotefile`.
    Template {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectorMetadataOverridesCommand {
    /// Create connector metadata overrides from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect connector metadata overrides.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
    /// Delete connector metadata overrides.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTOR")]
        connector: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectionPermissionCommand {
    /// List permissions for a One connection.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Create permissions for a One connection.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One connection permission by subject id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTION-ID")]
        connection_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
    },
    /// Delete a One connection permission by subject id.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTION-ID")]
        connection_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneJobGroupCommand {
    /// List One job groups.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count One job groups.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run a One job group.
    Run {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Publish job-group results to a target.
    Publish {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One job group.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Cancel a One job group.
    Cancel {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect a One job group status.
    Status {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List One job group inputs.
    Inputs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List One job group outputs.
    Outputs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List jobs for a One job group.
    Jobs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List publications for a One job group.
    Publications {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect profile data for a One job group.
    Profile {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect profile results for a One job group.
    ProfileResults {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Inspect PDF results for a One job group.
    PdfResults {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneOutputObjectCommand {
    /// List One output objects.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count One output objects.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Create a One output object from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One output object.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One output object from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One output object.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List inputs for a One output object.
    Inputs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Generate Python from a One output object.
    WrangleToPython {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long)]
        body: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWebhookFlowTaskCommand {
    /// Create a webhook flow task from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a webhook flow task.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Delete a webhook flow task.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Send a test webhook from JSON payload.
    Test {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWriteSettingCommand {
    /// List One write settings.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count One write settings.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Create a One write setting from JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One write setting.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One write setting from JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One write setting.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneSchedulingCommand {
    /// List One schedules.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Inspect a One schedule by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Enable a One schedule.
    Enable {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Disable a One schedule.
    Disable {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Count One schedules.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneBillingCommand {
    /// Inspect the current One billing account.
    CurrentAccount {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Export One billing usage data.
    UsageExport {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneDoctorCommand {
    /// Run the One auth doctor workflow.
    Auth {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run the One discovery doctor workflow.
    Discover {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run the One identity doctor workflow.
    Identity {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run the One plans doctor workflow.
    Plans {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run the One scheduling doctor workflow.
    Scheduling {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run the One billing doctor workflow.
    Billing {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum LicenseCommand {
    #[command(about = "Licensing portal API status and diagnostics")]
    Api {
        #[command(subcommand)]
        command: LicenseApiCommand,
    },
    #[command(about = "Summarize the Licensing branch posture.")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Summarize Licensing branch inventory candidates.")]
    Inventory {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum LicenseApiCommand {
    #[command(about = "Summarize the Licensing portal API posture.")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Validate Licensing API reachability and auth posture.")]
    Diagnose {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum CatalogCommand {
    #[command(about = "List machine-readable command metadata.")]
    List {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "compact")]
        format: String,
    },
    #[command(about = "Describe a single command in the catalog.")]
    Describe {
        target: Option<String>,
        #[arg(long)]
        command: Option<String>,
    },
    #[command(about = "Run a registered capability with JSON input")]
    Run {
        capability: String,
        #[arg(long = "json")]
        json_input: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug)]
pub(crate) struct CommandSpec {
    pub name: &'static str,
    pub path: &'static str,
    pub summary: &'static str,
    pub output: &'static str,
    pub safety: &'static str,
    pub mutating: bool,
    pub prerequisites: &'static [&'static str],
    pub notes: &'static [&'static str],
}

pub(crate) const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "profile list",
        path: "profile/list",
        summary: "List centrally managed profiles and show the active profile.",
        output: "profile registry envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["ayx config home"],
        notes: &["Use this to discover centrally managed profiles."],
    },
    CommandSpec {
        name: "profile current",
        path: "profile/current",
        summary: "Show the active central profile pointer.",
        output: "active profile envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["ayx config home"],
        notes: &["Use this to see which profile ayx will use by default."],
    },
    CommandSpec {
        name: "profile use",
        path: "profile/use",
        summary: "Set the active central profile.",
        output: "state update envelope",
        safety: "mutating-local",
        mutating: true,
        prerequisites: &["existing central profile"],
        notes: &["Updates ayx state only; no remote systems are changed."],
    },
    CommandSpec {
        name: "doctor",
        path: "doctor",
        summary: "Run the full ayx health sequence for config, auth, network, and product posture.",
        output: "doctor aggregate envelope",
        safety: "read-only-or-safe-local-fix",
        mutating: false,
        prerequisites: &["active or explicit profile"],
        notes: &["Use --fix for safe local remediation such as creating the central config home."],
    },
    CommandSpec {
        name: "doctor config",
        path: "doctor/config",
        summary: "Validate config home, active profile resolution, and inline secret posture.",
        output: "config doctor envelope",
        safety: "read-only-or-safe-local-fix",
        mutating: false,
        prerequisites: &["ayx config home or legacy config"],
        notes: &["Use this first when profile resolution or local state is unclear."],
    },
    CommandSpec {
        name: "mongo status",
        path: "mongo/status",
        summary: "Resolve the configured Mongo connection and database names.",
        output: "connection detail and database metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongo.mode", "mongo.databases"],
        notes: &["Use this first to validate embedded or managed Mongo configuration."],
    },
    CommandSpec {
        name: "mongo inventory",
        path: "mongo/inventory",
        summary: "Generate an inventory plan for the Mongo-backed databases.",
        output: "database inventory plan",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongo.databases"],
        notes: &["Use this before backup or restore planning."],
    },
    CommandSpec {
        name: "mongo backup",
        path: "mongo/backup",
        summary: "Back up the Gallery and Service Mongo databases.",
        output: "backup plan or execution artifacts",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "mongo.mode"],
        notes: &[
            "Requires --apply for a live backup.",
            "Writes audit artifacts.",
        ],
    },
    CommandSpec {
        name: "mongo restore",
        path: "mongo/restore",
        summary: "Restore Mongo data from a backup input path.",
        output: "restore execution artifacts",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "restore input path"],
        notes: &[
            "Requires --apply for a live restore.",
            "Writes audit artifacts.",
        ],
    },
    CommandSpec {
        name: "server api import-swagger",
        path: "server/api/import-swagger",
        summary: "Download and cache the Server OpenAPI document.",
        output: "cached swagger metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server.webapi_url"],
        notes: &["Use before server api call."],
    },
    CommandSpec {
        name: "server api status",
        path: "server/api/status",
        summary: "Summarize Server API credentials and base URL posture.",
        output: "server api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Useful before diagnostics, import, or call."],
    },
    CommandSpec {
        name: "server api diagnose",
        path: "server/api/diagnose",
        summary: "Validate token acquisition and API reachability for Server.",
        output: "diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use before server api import-swagger or server api call."],
    },
    CommandSpec {
        name: "server api call",
        path: "server/api/call",
        summary: "Invoke a Server API operation by operationId.",
        output: "call response envelope",
        safety: "mutating-or-read-only",
        mutating: false,
        prerequisites: &["cached Swagger document", "central runtime profile"],
        notes: &["Operation behavior depends on the selected endpoint."],
    },
    CommandSpec {
        name: "license status",
        path: "license/status",
        summary: "Summarize the Licensing branch posture.",
        output: "license status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Product branch ready; API subcommands are the primary entry point."],
    },
    CommandSpec {
        name: "license inventory",
        path: "license/inventory",
        summary: "Summarize Licensing branch inventory candidates.",
        output: "license inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Product branch ready; API subcommands are the primary entry point."],
    },
    CommandSpec {
        name: "workflow inspect",
        path: "workflow/inspect",
        summary: "Inspect Alteryx workflow, macro, package, or data artifacts.",
        output: "workflow inspection envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["workflow artifact path"],
        notes: &[
            "Use this to inspect .yxmd, .yxmc, .yxzp, or .yxdb files and directories.",
            "Recursive directory inspection is supported.",
        ],
    },
    CommandSpec {
        name: "workflow unpack",
        path: "workflow/unpack",
        summary: "Unpack a .yxzp workflow package.",
        output: "workflow unpack envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["input .yxzp package", "output directory"],
        notes: &["Preserves the archive contents in a directory tree for XML-level edits."],
    },
    CommandSpec {
        name: "workflow validate",
        path: "workflow/validate",
        summary: "Validate workflow and macro XML structures.",
        output: "workflow validation envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["workflow artifact path"],
        notes: &["Validates .yxmd, .yxmc, .yxzp, or directories of workflow artifacts."],
    },
    CommandSpec {
        name: "workflow replace",
        path: "workflow/replace",
        summary: "Find and replace text in workflow XML or packages.",
        output: "workflow replacement envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input artifact", "output path", "find/replace values"],
        notes: &[
            "Use --validate to check the rewritten XML after replacement.",
            "Package inputs are unpacked, rewritten, and re-packed.",
        ],
    },
    CommandSpec {
        name: "workflow repackage",
        path: "workflow/repackage",
        summary: "Rebuild a .yxzp package from a directory tree.",
        output: "workflow repackage envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input directory", "output package path"],
        notes: &["Useful after XML-level edits to workflow package contents."],
    },
    CommandSpec {
        name: "workflow migrate",
        path: "workflow/migrate",
        summary: "Perform an end-to-end workflow XML migration pass.",
        output: "workflow migration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input artifact", "output path", "find/replace values"],
        notes: &[
            "Combines inspect, replace, validate, and repackaging into one flow.",
            "Use this for NFS-style migration and other recursive XML updates.",
        ],
    },
    CommandSpec {
        name: "workflow recurse",
        path: "workflow/recurse",
        summary: "Recursively apply XML replacement rules across workflow artifacts.",
        output: "workflow recurse envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "input artifact or directory",
            "rules file or repeated find/replace pairs",
        ],
        notes: &[
            "Use --rules for YAML-driven migrations or repeat --find/--replace pairs.",
            "Recurses into packages and nested workflow artifacts.",
        ],
    },
    CommandSpec {
        name: "workflow scan",
        path: "workflow/scan",
        summary: "Preflight scan workflow artifacts for rule matches without rewriting.",
        output: "workflow scan envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &[
            "input artifact or directory",
            "rules file or repeated find/replace pairs",
        ],
        notes: &[
            "Reports candidate matches by file so migrations can be reviewed first.",
            "Use with the same rules you plan to pass to recurse.",
        ],
    },
    CommandSpec {
        name: "workflow publish",
        path: "workflow/publish",
        summary: "Republish a workflow package through the Server API.",
        output: "workflow publish envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "workflow package"],
        notes: &[
            "Uses the Server workflow upload API for the actual publish step.",
            "Accepts a ready .yxzp or a directory that can be repackaged first.",
        ],
    },
    #[cfg(feature = "ui")]
    CommandSpec {
        name: "one ui session status",
        path: "one/ui/session/status",
        summary: "Report the experimental One visual interface session policy and reuse posture.",
        output: "one ui session status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["browser session"],
        notes: &[
            "Use pinned visible tabs for operator-facing workflow and data pages.",
            "Background pages are allowed for read-only validation and refresh work.",
        ],
    },
    #[cfg(feature = "ui")]
    CommandSpec {
        name: "one ui workflow inventory",
        path: "one/ui/workflow/inventory",
        summary: "Inventory the experimental workflow page canvas, config pane, and results pane.",
        output: "one ui workflow inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["authenticated Cloud workflow page"],
        notes: &[
            "This is the deterministic capture point for UI-driven workflow debugging.",
            "Future commands should reuse the same tab/page when the workflow is already open.",
        ],
    },
    #[cfg(feature = "ui")]
    CommandSpec {
        name: "one ui data list-datasets",
        path: "one/ui/data/list-datasets",
        summary: "List available One datasets from the visual data page.",
        output: "one ui data inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["authenticated Cloud data page"],
        notes: &[
            "May use a pinned Data tab or a background page depending on the caller's policy.",
            "Useful as the first step before preview, detail, upload, or validation fan-out.",
        ],
    },
    CommandSpec {
        name: "one login",
        path: "one/login",
        summary: "Authenticate with Alteryx One and store credentials.",
        output: "one login envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one"],
        notes: &[
            "Default flow uses email OTP; browser, device, refresh-token, and access-token paths are also supported.",
            "Stores credentials in the active profile using the existing inline-secret policy.",
        ],
    },
    CommandSpec {
        name: "one logout",
        path: "one/logout",
        summary: "Clear stored Alteryx One credentials from the active profile.",
        output: "one logout envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one"],
        notes: &[
            "Clears top-level and workspace-scoped One access/refresh credential fields and refs.",
            "Does not revoke remote tokens or delete external secret-store entries.",
        ],
    },
    CommandSpec {
        name: "one inventory",
        path: "one/inventory",
        summary: "Summarize the current One API surface registry.",
        output: "one inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &[
            "Use this as the authoritative One endpoint registry.",
            "Implemented and partial surfaces are listed separately from documented-only gaps.",
        ],
    },
    CommandSpec {
        name: "one whoami",
        path: "one/whoami",
        summary: "Show the current One user profile.",
        output: "one whoami envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CommandSpec {
        name: "one person list",
        path: "one/person/list",
        summary: "List One people.",
        output: "one person list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people in the One API docs."],
    },
    CommandSpec {
        name: "one person current",
        path: "one/person/current",
        summary: "Inspect the current One person record.",
        output: "one person current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CommandSpec {
        name: "one person count",
        path: "one/person/count",
        summary: "Count One people.",
        output: "one person count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/count in the One API docs."],
    },
    CommandSpec {
        name: "one person detail",
        path: "one/person/detail",
        summary: "Inspect a One person record by id.",
        output: "one person detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one person create",
        path: "one/person/create",
        summary: "Create a One person from JSON payload.",
        output: "one person create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/people in the One API docs."],
    },
    CommandSpec {
        name: "one person update",
        path: "one/person/update",
        summary: "Replace a One person record from JSON payload.",
        output: "one person update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PUT /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one person patch",
        path: "one/person/patch",
        summary: "Patch a One person record from JSON payload.",
        output: "one person patch envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one person delete",
        path: "one/person/delete",
        summary: "Delete a One person record.",
        output: "one person delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one person update-password",
        path: "one/person/update-password",
        summary: "Update the current One person's password from JSON payload.",
        output: "one person update-password envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/people/current/updatePassword in the One API docs."],
    },
    CommandSpec {
        name: "one person password-reset-request",
        path: "one/person/password-reset-request",
        summary: "Request a One password reset from JSON payload.",
        output: "one person password reset request envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/passwordresetrequest in the One API docs."],
    },
    CommandSpec {
        name: "one workspace current",
        path: "one/workspace/current",
        summary: "Inspect the current One workspace posture.",
        output: "one workspace current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/workspaces/current in the One API docs."],
    },
    CommandSpec {
        name: "one workspace current-configuration",
        path: "one/workspace/current-configuration",
        summary: "Inspect the current One workspace configuration.",
        output: "one workspace current configuration envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace configuration-v4",
        path: "one/workspace/configuration-v4",
        summary: "Inspect a One workspace configuration by id.",
        output: "one workspace configuration-v4 envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace configuration",
        path: "one/workspace/configuration",
        summary: "Inspect a One workspace configuration by id.",
        output: "one workspace configuration envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace save-current-configuration",
        path: "one/workspace/save-current-configuration",
        summary: "Update the current One workspace configuration from JSON payload.",
        output: "one workspace save-current-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/workspaces/current/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace save-configuration-v4",
        path: "one/workspace/save-configuration-v4",
        summary: "Update a One workspace configuration by id from JSON payload.",
        output: "one workspace save-configuration-v4 envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace list",
        path: "one/workspace/list",
        summary: "List accessible One workspaces.",
        output: "one workspace list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces in the One API docs."],
    },
    CommandSpec {
        name: "one workspace configuration-schema",
        path: "one/workspace/configuration-schema",
        summary: "Inspect the workspace configuration schema.",
        output: "one workspace configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration-schema in the One API docs."],
    },
    CommandSpec {
        name: "one workspace current-configuration-schema",
        path: "one/workspace/current-configuration-schema",
        summary: "Inspect the current workspace configuration schema.",
        output: "one workspace current configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration-schema in the One API docs."],
    },
    CommandSpec {
        name: "one workspace delete-current-configuration",
        path: "one/workspace/delete-current-configuration",
        summary: "Reset the current workspace configuration.",
        output: "one workspace delete-current-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/current/delete-configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace delete-configuration",
        path: "one/workspace/delete-configuration",
        summary: "Reset a workspace configuration by workspace id.",
        output: "one workspace delete-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/delete-configuration in the One API docs."],
    },
    CommandSpec {
        name: "one workspace people",
        path: "one/workspace/people",
        summary: "List people in the current One workspace.",
        output: "one workspace people envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to GET /v4/people in the One API docs. Workspace context comes from the x-alteryx-workspace-gid header; /v4/workspaces/{id}/people returns 404.",
        ],
    },
    CommandSpec {
        name: "one workspace admins",
        path: "one/workspace/admins",
        summary: "List workspace admins.",
        output: "one workspace admins envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to GET /v4/people?role=admin in the One API docs. Workspace context comes from the x-alteryx-workspace-gid header; /v4/workspaces/{workspaceId}/admins returns 404.",
        ],
    },
    CommandSpec {
        name: "one workspace switch",
        path: "one/workspace/switch",
        summary: "Set the active One workspace in the local profile.",
        output: "one workspace switch envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "stored workspace credentials"],
        notes: &[
            "Updates `alteryx_one.expected_workspace_id` in the selected profile.",
            "Does not call a One API endpoint.",
        ],
    },
    CommandSpec {
        name: "one workspace invite-users",
        path: "one/workspace/invite-users",
        summary: "Invite users to a One workspace.",
        output: "one workspace invite-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/people/batch in the One API docs."],
    },
    CommandSpec {
        name: "one workspace remove-user",
        path: "one/workspace/remove-user",
        summary: "Remove a user from a One workspace.",
        output: "one workspace remove-user envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/workspaces/{workspaceId}/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one workspace suspend-users",
        path: "one/workspace/suspend-users",
        summary: "Suspend users in a One workspace.",
        output: "one workspace suspend-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /iam/v1/workspaces/{id}/people/suspend in the One API docs."],
    },
    CommandSpec {
        name: "one workspace unsuspend-users",
        path: "one/workspace/unsuspend-users",
        summary: "Unsuspend users in a One workspace.",
        output: "one workspace unsuspend-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /iam/v1/workspaces/{id}/people/unsuspend in the One API docs."],
    },
    CommandSpec {
        name: "one workspace transfer",
        path: "one/workspace/transfer",
        summary: "Start a transfer for a One workspace.",
        output: "one workspace transfer envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/transfer in the One API docs."],
    },
    CommandSpec {
        name: "one workspace transfer-assets",
        path: "one/workspace/transfer-assets",
        summary: "Transfer assets from the current One workspace from JSON payload.",
        output: "one workspace transfer-assets envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/workspaces/current/transfer in the One API docs."],
    },
    CommandSpec {
        name: "one role list-assignments",
        path: "one/role/list-assignments",
        summary: "Inspect role assignments for One managed IAM.",
        output: "one role assignments envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/authorization/roles/{id}/people in the One API docs."],
    },
    CommandSpec {
        name: "one role assign",
        path: "one/role/assign",
        summary: "Assign a subject to a One managed IAM role.",
        output: "one role assign envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to POST /v4/authorization/roles/{id}/people/{subjectId} in the One API docs.",
        ],
    },
    CommandSpec {
        name: "one role unassign",
        path: "one/role/unassign",
        summary: "Unassign a subject from a One managed IAM role.",
        output: "one role unassign envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to DELETE /v4/authorization/roles/{id}/people/{subjectId} in the One API docs.",
        ],
    },
    CommandSpec {
        name: "one auth status",
        path: "one/auth/status",
        summary: "Summarize One API token posture for managed IAM.",
        output: "one auth status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Confirms OAuth client ID, token endpoint, access token presence, refresh token presence, and whether the token can reach the token inventory surface.",
        ],
    },
    CommandSpec {
        name: "one token list",
        path: "one/token/list",
        summary: "List One API access tokens.",
        output: "one token list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens in the One API docs."],
    },
    CommandSpec {
        name: "one token create",
        path: "one/token/create",
        summary: "Create a One API access token from JSON payload.",
        output: "one token create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/apiAccessTokens in the One API docs."],
    },
    CommandSpec {
        name: "one token detail",
        path: "one/token/detail",
        summary: "Inspect a One API access token by id.",
        output: "one token detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CommandSpec {
        name: "one token delete",
        path: "one/token/delete",
        summary: "Delete a One API access token by id.",
        output: "one token delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CommandSpec {
        name: "one auth diagnose",
        path: "one/auth/diagnose",
        summary: "Validate One API token reachability and workspace scope.",
        output: "one auth diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Uses the token inventory endpoint as the safe validation target, while mutating operations still preflight workspace identity separately.",
        ],
    },
    CommandSpec {
        name: "one doctor auth",
        path: "one/doctor/auth",
        summary: "Run the One auth doctor workflow.",
        output: "one auth doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps token posture and workspace probe checks."],
    },
    CommandSpec {
        name: "one doctor discover",
        path: "one/doctor/discover",
        summary: "Run the One discovery doctor workflow.",
        output: "one discovery doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Surfaces workspace, plan, schedule, and billing discovery data."],
    },
    CommandSpec {
        name: "one doctor identity",
        path: "one/doctor/identity",
        summary: "Run the One identity doctor workflow.",
        output: "one identity doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps workspace and role discovery checks."],
    },
    CommandSpec {
        name: "one doctor plans",
        path: "one/doctor/plans",
        summary: "Run the One plans doctor workflow.",
        output: "one plans doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps list, count, and plan lookup checks."],
    },
    CommandSpec {
        name: "one doctor scheduling",
        path: "one/doctor/scheduling",
        summary: "Run the One scheduling doctor workflow.",
        output: "one scheduling doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps schedule list and count checks."],
    },
    CommandSpec {
        name: "one doctor billing",
        path: "one/doctor/billing",
        summary: "Run the One billing doctor workflow.",
        output: "one billing doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps billing account and usage export checks."],
    },
    CommandSpec {
        name: "one api status",
        path: "one/api/status",
        summary: "Summarize the Alteryx One API posture.",
        output: "one api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Use this to inspect One API posture before diagnostics.",
            "Treat this as the One managed IAM posture check.",
        ],
    },
    CommandSpec {
        name: "one api diagnose",
        path: "one/api/diagnose",
        summary: "Validate Alteryx One API reachability and auth posture.",
        output: "one api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Use before future One API call-style workflows.",
            "Route workflow guidance through the orchestration layer once the symptom is known.",
        ],
    },
    CommandSpec {
        name: "one api open-api-spec",
        path: "one/api/open-api-spec",
        summary: "Fetch the Alteryx One OpenAPI specification.",
        output: "one api open-api-spec envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/open-api-spec in the One API docs."],
    },
    CommandSpec {
        name: "one api coverage",
        path: "one/api/coverage",
        summary: "Diff the live One OpenAPI spec against wired commands (covered/missing/stale).",
        output: "one api coverage envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Fetches GET /v4/open-api-spec (or --spec <file>) and diffs it against the ayx-one-api inventory.",
            "--check exits non-zero when endpoints are missing.",
        ],
    },
    CommandSpec {
        name: "one plans status",
        path: "one/plans/status",
        summary: "Summarize the Alteryx One plans posture.",
        output: "one plans status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Reserved for plan lifecycle workflows.",
            "Managed Plans is wired from the documented One API surface.",
        ],
    },
    CommandSpec {
        name: "one plans list",
        path: "one/plans/list",
        summary: "List One plans.",
        output: "one plans list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans create",
        path: "one/plans/create",
        summary: "Create a One plan.",
        output: "one plans create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans in the One API docs."],
    },
    CommandSpec {
        name: "one plans detail",
        path: "one/plans/detail",
        summary: "Inspect a One plan.",
        output: "one plans detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans/{id} in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans run",
        path: "one/plans/run",
        summary: "Run a One plan.",
        output: "one plans run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /plans/v1/plans/{id}/run in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans count",
        path: "one/plans/count",
        summary: "Count One plans.",
        output: "one plans count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans/count in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans full",
        path: "one/plans/full",
        summary: "Inspect a One plan with the full documented payload.",
        output: "one plans full envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/full in the One API docs."],
    },
    CommandSpec {
        name: "one plans run-parameters",
        path: "one/plans/run-parameters",
        summary: "Inspect run parameters for a One plan.",
        output: "one plans run-parameters envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans/{id}/runParameters in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans schedules",
        path: "one/plans/schedules",
        summary: "List schedules for a One plan.",
        output: "one plans schedules envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans/{id}/schedules in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans export",
        path: "one/plans/export",
        summary: "Fetch a One plan package.",
        output: "one plans export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /plans/v1/plans/{id}/package in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans update",
        path: "one/plans/update",
        summary: "Update a One plan from JSON payload.",
        output: "one plans update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/plans/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one plans delete",
        path: "one/plans/delete",
        summary: "Delete a One plan.",
        output: "one plans delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/plans/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one plans share",
        path: "one/plans/share",
        summary: "Share a One plan from JSON payload.",
        output: "one plans share envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one plans import",
        path: "one/plans/import",
        summary: "Import a One plan package.",
        output: "one plans import envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /plans/v1/plans/package in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans permissions",
        path: "one/plans/permissions",
        summary: "List plan permissions, or delete one when `--subject-id` is provided.",
        output: "one plans permissions envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to GET /plans/v1/plans/{id}/permissions in managed-plans-v1.yaml.",
            "When `--subject-id` is set, maps to DELETE /plans/v1/plans/{id}/permissions/{subjectId}.",
        ],
    },
    CommandSpec {
        name: "one flows list",
        path: "one/flows/list",
        summary: "List One flows.",
        output: "one flows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows in the One API docs."],
    },
    CommandSpec {
        name: "one flows count",
        path: "one/flows/count",
        summary: "Count One flows.",
        output: "one flows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/count in the One API docs."],
    },
    CommandSpec {
        name: "one flows library list",
        path: "one/flows/library/list",
        summary: "List the One flow library.",
        output: "one flows library list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flowsLibrary in the One API docs."],
    },
    CommandSpec {
        name: "one flows library count",
        path: "one/flows/library/count",
        summary: "Count the One flow library.",
        output: "one flows library count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flowsLibrary/count in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders list",
        path: "one/flows/folders/list",
        summary: "List flow folders.",
        output: "one flows folders list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders count",
        path: "one/flows/folders/count",
        summary: "Count flow folders.",
        output: "one flows folders count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/count in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders detail",
        path: "one/flows/folders/detail",
        summary: "Inspect a flow folder by id.",
        output: "one flows folders detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders create",
        path: "one/flows/folders/create",
        summary: "Create a flow folder from JSON payload.",
        output: "one flows folders create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/folders in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders update",
        path: "one/flows/folders/update",
        summary: "Update a flow folder from JSON payload.",
        output: "one flows folders update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/folders/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders delete",
        path: "one/flows/folders/delete",
        summary: "Delete a flow folder.",
        output: "one flows folders delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/folders/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders flows list",
        path: "one/flows/folders/flows/list",
        summary: "List flows in a folder.",
        output: "one flows folders flows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id}/flows in the One API docs."],
    },
    CommandSpec {
        name: "one flows folders flows count",
        path: "one/flows/folders/flows/count",
        summary: "Count flows in a folder.",
        output: "one flows folders flows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id}/flows/count in the One API docs."],
    },
    CommandSpec {
        name: "one flows detail",
        path: "one/flows/detail",
        summary: "Inspect a One flow by id.",
        output: "one flows detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows create",
        path: "one/flows/create",
        summary: "Create a One flow from JSON payload.",
        output: "one flows create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows in the One API docs."],
    },
    CommandSpec {
        name: "one flows update",
        path: "one/flows/update",
        summary: "Update a One flow from JSON payload.",
        output: "one flows update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows delete",
        path: "one/flows/delete",
        summary: "Delete a One flow.",
        output: "one flows delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows copy",
        path: "one/flows/copy",
        summary: "Copy a One flow using a JSON payload.",
        output: "one flows copy envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/copy in the One API docs."],
    },
    CommandSpec {
        name: "one flows run",
        path: "one/flows/run",
        summary: "Run a One flow using a JSON payload.",
        output: "one flows run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/run in the One API docs."],
    },
    CommandSpec {
        name: "one flows validate",
        path: "one/flows/validate",
        summary: "Validate a One flow.",
        output: "one flows validate envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/validate in the One API docs."],
    },
    CommandSpec {
        name: "one flows parameters",
        path: "one/flows/parameters",
        summary: "Inspect flow-level parameters and overrides.",
        output: "one flows parameters envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/recipeParameters in the One API docs."],
    },
    CommandSpec {
        name: "one flows inputs",
        path: "one/flows/inputs",
        summary: "List inputs for a One flow.",
        output: "one flows inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one flows outputs",
        path: "one/flows/outputs",
        summary: "List outputs for a One flow.",
        output: "one flows outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/outputs in the One API docs."],
    },
    CommandSpec {
        name: "one flows permissions-get",
        path: "one/flows/permissions-get",
        summary: "List permissions for a One flow.",
        output: "one flows permissions-get envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one flows permissions",
        path: "one/flows/permissions",
        summary: "Share a flow from JSON payload.",
        output: "one flows permissions envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one flows move",
        path: "one/flows/move",
        summary: "Move a One flow from JSON payload.",
        output: "one flows move envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/move in the One API docs."],
    },
    CommandSpec {
        name: "one flows replace-dataset",
        path: "one/flows/replace-dataset",
        summary: "Replace a dataset in a One flow from JSON payload.",
        output: "one flows replace-dataset envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/flows/{id}/replaceDataset in the One API docs."],
    },
    CommandSpec {
        name: "one flows import",
        path: "one/flows/import",
        summary: "Import a flow package.",
        output: "one flows import envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package in the One API docs."],
    },
    CommandSpec {
        name: "one flows import-dry-run",
        path: "one/flows/import-dry-run",
        summary: "Dry-run import of a flow package.",
        output: "one flows import dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one flows export",
        path: "one/flows/export",
        summary: "Export a flow package to disk.",
        output: "one flows export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package in the One API docs."],
    },
    CommandSpec {
        name: "one flows export-dry-run",
        path: "one/flows/export-dry-run",
        summary: "Dry-run export of a flow package.",
        output: "one flows export dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one datasets list",
        path: "one/datasets/list",
        summary: "List datasets in the One dataset library.",
        output: "one datasets list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/datasetLibrary in the One API docs."],
    },
    CommandSpec {
        name: "one datasets count",
        path: "one/datasets/count",
        summary: "Count datasets in the One dataset library.",
        output: "one datasets count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/datasetLibrary/count in the One API docs."],
    },
    CommandSpec {
        name: "one datasets wrangled list",
        path: "one/datasets/wrangled/list",
        summary: "List One wrangled datasets.",
        output: "one datasets wrangled list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets in the One API docs."],
    },
    CommandSpec {
        name: "one datasets wrangled count",
        path: "one/datasets/wrangled/count",
        summary: "Count One wrangled datasets.",
        output: "one datasets wrangled count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets/count in the One API docs."],
    },
    CommandSpec {
        name: "one datasets wrangled detail",
        path: "one/datasets/wrangled/detail",
        summary: "Inspect a One wrangled dataset by id.",
        output: "one datasets wrangled detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one datasets imported detail",
        path: "one/datasets/imported/detail",
        summary: "Inspect a One imported dataset by id.",
        output: "one datasets imported detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/importedDatasets/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections list",
        path: "one/connections/list",
        summary: "List One connections.",
        output: "one connections list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections in the One API docs."],
    },
    CommandSpec {
        name: "one connections count",
        path: "one/connections/count",
        summary: "Count One connections.",
        output: "one connections count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/count in the One API docs."],
    },
    CommandSpec {
        name: "one connections create",
        path: "one/connections/create",
        summary: "Create a One connection from JSON payload.",
        output: "one connections create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections in the One API docs."],
    },
    CommandSpec {
        name: "one connections dry-run",
        path: "one/connections/dry-run",
        summary: "Dry-run creation of a One connection.",
        output: "one connections dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one connections detail",
        path: "one/connections/detail",
        summary: "Inspect a One connection.",
        output: "one connections detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections status",
        path: "one/connections/status",
        summary: "Inspect connection status.",
        output: "one connections status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/status in the One API docs."],
    },
    CommandSpec {
        name: "one connections update",
        path: "one/connections/update",
        summary: "Update a One connection from JSON payload.",
        output: "one connections update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections delete",
        path: "one/connections/delete",
        summary: "Delete a One connection.",
        output: "one connections delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions list",
        path: "one/connections/permissions/list",
        summary: "List permissions for a One connection.",
        output: "one connections permissions list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions create",
        path: "one/connections/permissions/create",
        summary: "Create permissions for a One connection.",
        output: "one connections permissions create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions detail",
        path: "one/connections/permissions/detail",
        summary: "Inspect a One connection permission by subject id.",
        output: "one connections permissions detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/permissions/{aid} in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions delete",
        path: "one/connections/permissions/delete",
        summary: "Delete a One connection permission by subject id.",
        output: "one connections permissions delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connections/{id}/permissions/{aid} in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata defaults",
        path: "one/connections/connector-metadata/defaults",
        summary: "Inspect connector defaults.",
        output: "one connections connector-metadata defaults envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/defaults in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata detail",
        path: "one/connections/connector-metadata/detail",
        summary: "Inspect current connector metadata.",
        output: "one connections connector-metadata detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector} in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata publish-info",
        path: "one/connections/connector-metadata/publish-info",
        summary: "Inspect connector publish information.",
        output: "one connections connector-metadata publish-info envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/publish/info in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides create",
        path: "one/connections/connector-metadata/overrides/create",
        summary: "Create connector metadata overrides from JSON payload.",
        output: "one connections connector-metadata overrides create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides list",
        path: "one/connections/connector-metadata/overrides/list",
        summary: "Inspect connector metadata overrides.",
        output: "one connections connector-metadata overrides list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides delete",
        path: "one/connections/connector-metadata/overrides/delete",
        summary: "Delete connector metadata overrides.",
        output: "one connections connector-metadata overrides delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one job-group list",
        path: "one/job-group/list",
        summary: "List One job groups.",
        output: "one job-group list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary in the One API docs."],
    },
    CommandSpec {
        name: "one job-group count",
        path: "one/job-group/count",
        summary: "Count One job groups.",
        output: "one job-group count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary/count in the One API docs."],
    },
    CommandSpec {
        name: "one job-group pdf-results",
        path: "one/job-group/pdf-results",
        summary: "Inspect PDF results for a One job group.",
        output: "one job-group pdf-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/pdfResults in the One API docs."],
    },
    CommandSpec {
        name: "one job-group run",
        path: "one/job-group/run",
        summary: "Run a One job group.",
        output: "one job-group run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/jobGroups in the One API docs."],
    },
    CommandSpec {
        name: "one job-group publish",
        path: "one/job-group/publish",
        summary: "Publish job-group results to a target.",
        output: "one job-group publish envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PUT /v4/jobGroups/{id}/publish in the One API docs."],
    },
    CommandSpec {
        name: "one job-group detail",
        path: "one/job-group/detail",
        summary: "Inspect a One job group.",
        output: "one job-group detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one job-group cancel",
        path: "one/job-group/cancel",
        summary: "Cancel a One job group.",
        output: "one job-group cancel envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /v4/jobGroups/{id}/cancel in the One API docs."],
    },
    CommandSpec {
        name: "one job-group status",
        path: "one/job-group/status",
        summary: "Inspect a One job group status.",
        output: "one job-group status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/status in the One API docs."],
    },
    CommandSpec {
        name: "one job-group inputs",
        path: "one/job-group/inputs",
        summary: "List One job group inputs.",
        output: "one job-group inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group outputs",
        path: "one/job-group/outputs",
        summary: "List One job group outputs.",
        output: "one job-group outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/outputs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group jobs",
        path: "one/job-group/jobs",
        summary: "List jobs for a One job group.",
        output: "one job-group jobs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/jobs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group publications",
        path: "one/job-group/publications",
        summary: "List publications for a One job group.",
        output: "one job-group publications envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/publications in the One API docs."],
    },
    CommandSpec {
        name: "one job-group profile",
        path: "one/job-group/profile",
        summary: "Inspect profile data for a One job group.",
        output: "one job-group profile envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profile in the One API docs."],
    },
    CommandSpec {
        name: "one job-group profile-results",
        path: "one/job-group/profile-results",
        summary: "Inspect profile results for a One job group.",
        output: "one job-group profile-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profileResults in the One API docs."],
    },
    CommandSpec {
        name: "one output-object list",
        path: "one/output-object/list",
        summary: "List One output objects.",
        output: "one output-object list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects in the One API docs."],
    },
    CommandSpec {
        name: "one output-object count",
        path: "one/output-object/count",
        summary: "Count One output objects.",
        output: "one output-object count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/count in the One API docs."],
    },
    CommandSpec {
        name: "one output-object create",
        path: "one/output-object/create",
        summary: "Create a One output object from JSON payload.",
        output: "one output-object create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/outputObjects in the One API docs."],
    },
    CommandSpec {
        name: "one output-object detail",
        path: "one/output-object/detail",
        summary: "Inspect a One output object.",
        output: "one output-object detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object update",
        path: "one/output-object/update",
        summary: "Update a One output object from JSON payload.",
        output: "one output-object update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object delete",
        path: "one/output-object/delete",
        summary: "Delete a One output object.",
        output: "one output-object delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object inputs",
        path: "one/output-object/inputs",
        summary: "List inputs for a One output object.",
        output: "one output-object inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one output-object wrangle-to-python",
        path: "one/output-object/wrangle-to-python",
        summary: "Generate Python from a One output object.",
        output: "one output-object wrangle-to-python envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /v4/outputObjects/{id}/wrangleToPython in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task create",
        path: "one/webhook-flow-task/create",
        summary: "Create a webhook flow task from JSON payload.",
        output: "one webhook-flow-task create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhookFlowTasks in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task detail",
        path: "one/webhook-flow-task/detail",
        summary: "Inspect a webhook flow task.",
        output: "one webhook-flow-task detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task delete",
        path: "one/webhook-flow-task/delete",
        summary: "Delete a webhook flow task.",
        output: "one webhook-flow-task delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-tasks test",
        path: "one/webhook-flow-tasks/test",
        summary: "Send a test webhook from JSON payload.",
        output: "one webhook-flow-tasks test envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhooks/test in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting list",
        path: "one/write-setting/list",
        summary: "List One write settings.",
        output: "one write-setting list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting count",
        path: "one/write-setting/count",
        summary: "Count One write settings.",
        output: "one write-setting count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/count in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting create",
        path: "one/write-setting/create",
        summary: "Create a One write setting from JSON payload.",
        output: "one write-setting create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/writeSettings in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting detail",
        path: "one/write-setting/detail",
        summary: "Inspect a One write setting.",
        output: "one write-setting detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting update",
        path: "one/write-setting/update",
        summary: "Update a One write setting from JSON payload.",
        output: "one write-setting update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting delete",
        path: "one/write-setting/delete",
        summary: "Delete a One write setting.",
        output: "one write-setting delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one scheduling list",
        path: "one/scheduling/list",
        summary: "List One schedules.",
        output: "one scheduling list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /scheduling/v1/schedules in managed-scheduling-v1.yaml."],
    },
    CommandSpec {
        name: "one scheduling detail",
        path: "one/scheduling/detail",
        summary: "Inspect a One schedule by id.",
        output: "one scheduling detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /scheduling/v1/schedules/{id} in managed-scheduling-v1.yaml."],
    },
    CommandSpec {
        name: "one scheduling enable",
        path: "one/scheduling/enable",
        summary: "Enable a One schedule.",
        output: "one scheduling enable envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to POST /scheduling/v1/schedules/{id}/enable in managed-scheduling-v1.yaml.",
        ],
    },
    CommandSpec {
        name: "one scheduling disable",
        path: "one/scheduling/disable",
        summary: "Disable a One schedule.",
        output: "one scheduling disable envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to POST /scheduling/v1/schedules/{id}/disable in managed-scheduling-v1.yaml.",
        ],
    },
    CommandSpec {
        name: "one scheduling count",
        path: "one/scheduling/count",
        summary: "Count One schedules.",
        output: "one scheduling count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /scheduling/v1/schedules/count in managed-scheduling-v1.yaml."],
    },
    CommandSpec {
        name: "one billing current-account",
        path: "one/billing/current-account",
        summary: "Inspect the current One billing account.",
        output: "one billing current-account envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /billing/v1/my/billing-accounts/current in managed-billing-v1.yaml."],
    },
    CommandSpec {
        name: "one billing usage-export",
        path: "one/billing/usage-export",
        summary: "Export One billing usage data.",
        output: "one billing usage-export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /billing/v1/usage/export in managed-billing-v1.yaml."],
    },
    CommandSpec {
        name: "license api status",
        path: "license/api/status",
        summary: "Summarize the Licensing portal API posture.",
        output: "license api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use to inspect licensing API posture before diagnostics."],
    },
    CommandSpec {
        name: "license api diagnose",
        path: "license/api/diagnose",
        summary: "Validate Licensing API reachability and auth posture.",
        output: "license api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use before future license api call-style workflows."],
    },
    CommandSpec {
        name: "server upgrade plan",
        path: "server/upgrade/plan",
        summary: "Compute an upgrade path between versions.",
        output: "upgrade plan manifest",
        safety: "read-only",
        mutating: false,
        prerequisites: &["source version", "target version"],
        notes: &["Use this to map supported upgrade hops."],
    },
    CommandSpec {
        name: "catalog list",
        path: "catalog/list",
        summary: "List machine-readable command metadata.",
        output: "command catalog entries",
        safety: "read-only",
        mutating: false,
        prerequisites: &["none"],
        notes: &["Use this when another tool needs to discover available commands."],
    },
    CommandSpec {
        name: "catalog describe",
        path: "catalog/describe",
        summary: "Describe a single command in the catalog.",
        output: "single command metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["catalog entry name or path"],
        notes: &["Accepts either a name or a path-like catalog key."],
    },
    CommandSpec {
        name: "discover",
        path: "discover",
        summary: "Progressively discover the live CLI tree and metadata.",
        output: "live cli discovery tree",
        safety: "read-only",
        mutating: false,
        prerequisites: &["none"],
        notes: &[
            "Top-level progressive disclosure entry point for agent harnesses.",
            "Use --deep to expand the full subtree or pass a path to drill down.",
        ],
    },
    CommandSpec {
        name: "server diagnose startup",
        path: "server/diagnose/startup",
        summary: "Run a guided startup failure diagnosis.",
        output: "startup diagnosis steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &[
            "central runtime profile",
            "optional startup error",
            "optional log file",
        ],
        notes: &["Wraps logs, runtime settings, and recent log candidate checks."],
    },
    CommandSpec {
        name: "server diagnose tls",
        path: "server/diagnose/tls",
        summary: "Inspect TLS, certificate, and proxy-related Server checks.",
        output: "tls diagnosis steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server.webapi_url"],
        notes: &[
            "Focuses on SSL/TLS, port binding, and proxy configuration.",
            "Use this for gallery binding, controller cert, and HTTPS setup issues.",
        ],
    },
    CommandSpec {
        name: "server-logs discover",
        path: "server-logs/discover",
        summary: "Inventory every Server log file the profile knows about.",
        output: "log inventory envelope (paths, sizes, mtimes)",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server install path"],
        notes: &[
            "First step in any log triage. Surfaces canonical paths so context queries can target them.",
        ],
    },
    CommandSpec {
        name: "server-logs context",
        path: "server-logs/context",
        summary: "Extract surrounding lines around every occurrence of a query string in a log file.",
        output: "context envelope (matches with before/after windows)",
        safety: "read-only",
        mutating: false,
        prerequisites: &["log file path", "query string"],
        notes: &[
            "Use --before / --after to widen the window.",
            "Pair with `server-logs discover` to enumerate log paths first.",
        ],
    },
    CommandSpec {
        name: "server-logs inventory",
        path: "server-logs/inventory",
        summary: "Aggregate counts and time ranges across all Server logs.",
        output: "inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &["Coarser than `discover`; intended for at-a-glance posture."],
    },
    CommandSpec {
        name: "server-logs summary",
        path: "server-logs/summary",
        summary: "Summarize a single log file (line count, error count, time range).",
        output: "summary envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["log file path"],
        notes: &["Quick triage before drilling in with `context`."],
    },
    CommandSpec {
        name: "server auth status",
        path: "server/auth/status",
        summary: "Summarize Server authentication configuration.",
        output: "auth status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server settings"],
        notes: &["Use this before SAML diagnosis or simulation."],
    },
    CommandSpec {
        name: "server auth diagnose saml",
        path: "server/auth/diagnose/saml",
        summary: "Inspect SAML configuration, metadata, and callback alignment.",
        output: "saml diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &[
            "central runtime profile",
            "metadata url or file when available",
        ],
        notes: &["Focuses on Server-side SAML configuration and common mismatch checks."],
    },
    CommandSpec {
        name: "mongo query",
        path: "mongo/query",
        summary: "Run a read-only Mongo query against a Server collection.",
        output: "mongo query envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongosh available on PATH"],
        notes: &["Use for targeted inspection of Gallery and Service collections."],
    },
    CommandSpec {
        name: "mongo doctor",
        path: "mongo/doctor",
        summary: "Run the default support query suite across critical Mongo collections.",
        output: "mongo doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongosh available on PATH"],
        notes: &["Targets queue, results, users, and app info collections."],
    },
    CommandSpec {
        name: "server auth diagnose saml-logs",
        path: "server/auth/diagnose/saml-logs",
        summary: "Collect and summarize SAML login logs.",
        output: "saml log diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "SAML login logs"],
        notes: &["Targets alteryx-sso and aas log families."],
    },
    CommandSpec {
        name: "server auth diagnose certificate",
        path: "server/auth/diagnose/certificate",
        summary: "Inspect certificate posture for SAML auth.",
        output: "certificate diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "certificate file when available"],
        notes: &["Focuses on certificate presence, parsing, and likely trust issues."],
    },
    CommandSpec {
        name: "server auth diagnose ad-legacy",
        path: "server/auth/diagnose/ad-legacy",
        summary: "Inspect legacy Active Directory auth support signals.",
        output: "legacy ad diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &["Kept intentionally narrow as a legacy troubleshooting path."],
    },
    CommandSpec {
        name: "server auth simulate saml",
        path: "server/auth/simulate/saml",
        summary: "Simulate a SAML auth flow using metadata and expected endpoints.",
        output: "saml simulation envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "metadata url or file"],
        notes: &["Designed as a diagnostic harness, not a full IdP emulator."],
    },
    CommandSpec {
        name: "server doctor startup",
        path: "server/doctor/startup",
        summary: "Run a guided startup doctor workflow.",
        output: "startup doctor steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &[
            "central runtime profile",
            "optional startup error",
            "optional log file",
        ],
        notes: &["Prescriptive version of server diagnose startup."],
    },
];

#[derive(Subcommand, Debug)]
pub(crate) enum ServerLogsCommand {
    #[command(about = "Discover Server log locations from the active profile")]
    Discover {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Inventory known Server log files and metadata")]
    Inventory {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Summarize a Server log file")]
    Summary {
        #[arg(long)]
        path: PathBuf,
    },
    #[command(about = "Extract matching context from a Server log file")]
    Context {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 25)]
        before: usize,
        #[arg(long, default_value_t = 25)]
        after: usize,
    },
    #[command(about = "Parse a Gallery log CSV export")]
    ParseCsv {
        #[arg(long)]
        path: PathBuf,
    },
    #[command(about = "Parse Service log events from a log file")]
    ServiceEvents {
        #[arg(long)]
        path: PathBuf,
    },
    #[command(about = "Parse Gallery log events from a log file")]
    GalleryEvents {
        #[arg(long)]
        path: PathBuf,
    },
    #[command(about = "Read the tail of a Server log file")]
    Tail {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    #[command(about = "List recent Server log candidates")]
    Recent {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerDiagnoseCommand {
    #[command(about = "Run a guided startup failure diagnosis.")]
    Startup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        error: Option<String>,
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    #[command(about = "Inspect Server log sources and triage targets")]
    Logs {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Inspect Server network and connectivity checks")]
    Network {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Inspect TLS, certificate, and proxy-related Server checks.")]
    Tls {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Inspect Server runtime settings and Mongo config")]
    RuntimeSettings {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthCommand {
    #[command(about = "Summarize Server authentication configuration.")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Inspect Server auth configuration and failure signals")]
    Diagnose {
        #[command(subcommand)]
        command: ServerAuthDiagnoseCommand,
    },
    #[command(about = "Simulate Server SAML authentication flows")]
    Simulate {
        #[command(subcommand)]
        command: ServerAuthSimulateCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthDiagnoseCommand {
    #[command(about = "Inspect SAML configuration, metadata, and callback alignment.")]
    Saml {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        metadata_url: Option<String>,
        #[arg(long)]
        metadata_file: Option<PathBuf>,
        #[arg(long)]
        acs_url: Option<String>,
        #[arg(long)]
        issuer: Option<String>,
    },
    #[command(about = "Collect and summarize SAML login logs.")]
    SamlLogs {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    #[command(about = "Inspect certificate posture for SAML auth.")]
    Certificate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        certificate_file: Option<PathBuf>,
    },
    #[command(about = "Inspect legacy Active Directory auth support signals.")]
    AdLegacy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        domain: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthSimulateCommand {
    #[command(about = "Simulate a SAML auth flow using metadata and expected endpoints.")]
    Saml {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        metadata_url: Option<String>,
        #[arg(long)]
        metadata_file: Option<PathBuf>,
        #[arg(long)]
        acs_url: Option<String>,
        #[arg(long)]
        issuer: Option<String>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        certificate_file: Option<PathBuf>,
        #[arg(long)]
        prompt: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerDoctorCommand {
    #[command(about = "Run a guided startup doctor workflow.")]
    Startup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        error: Option<String>,
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    #[command(about = "Guide Server log-family triage and next steps")]
    Logs {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Guide Server network troubleshooting checks")]
    Network {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Guide Server runtime settings validation")]
    RuntimeSettings {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerApiCommand {
    #[command(about = "Summarize Server API credentials and base URL posture.")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Validate token acquisition and API reachability for Server.")]
    Diagnose {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Download and cache the Server OpenAPI document.")]
    ImportSwagger {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "3")]
        version: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value = ".omni/swagger")]
        cache_dir: PathBuf,
    },
    #[command(about = "Invoke a Server API operation by operationId.")]
    Call {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        operation_id: String,
        #[arg(long, default_value = "3")]
        version: String,
        #[arg(long, default_value = ".omni/swagger")]
        cache_dir: PathBuf,
        #[arg(long)]
        swagger: Option<PathBuf>,
        #[arg(long)]
        body: Option<PathBuf>,
        #[arg(long, value_name = "KEY=VALUE")]
        param: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
#[command(about = "Server upgrade planning, backup, apply simulation, and postcheck helpers")]
pub(crate) enum UpgradeCommand {
    #[command(about = "Compute a supported Server upgrade path")]
    Path {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "embedded-mongo")]
        deployment: String,
    },
    #[command(about = "Run a Server upgrade precheck")]
    Precheck {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "upgrade-precheck")]
        out: PathBuf,
        #[arg(long, default_value = "embedded-mongo")]
        deployment: String,
    },
    #[command(about = "Run a Server upgrade backup")]
    Backup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        r#type: String,
        #[arg(long, default_value = "upgrade-backup")]
        out: PathBuf,
    },
    #[command(about = "Compute an upgrade path between versions.")]
    Plan {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "upgrade-plan")]
        out: PathBuf,
        #[arg(long, default_value = "embedded-mongo")]
        deployment: String,
    },
    #[command(about = "Run or simulate an upgrade manifest")]
    Apply {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        yes: bool,
    },
    #[command(about = "Run a Server upgrade postcheck")]
    Postcheck {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = "upgrade-postcheck")]
        out: PathBuf,
    },
    #[command(about = "Bundle upgrade artifacts into a package")]
    Bundle {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

pub(crate) fn load_payload(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read payload file '{}'", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON payload from '{}'", path.display()))?;
    Ok(value)
}

pub(crate) fn tools_workspace_init_envelope(
    output: &Path,
    active_environment: &str,
    source_environment: &str,
    target_environment: &str,
) -> Result<Envelope> {
    onboard::write_workspace_template(
        output,
        active_environment,
        source_environment,
        target_environment,
    )?;
    Ok(Envelope::ok_with_data(
        "environments template written",
        json!({
            "environments_file": output.display().to_string(),
            "active_environment": active_environment,
            "environments": [source_environment, target_environment],
            "notes": [
                "environments.yaml is the canonical multi-environment file",
                "Use --environment to override the active environment for a run",
            ],
        }),
    ))
}

pub(crate) fn tools_workspace_resolve_envelope(
    workspace: &Path,
    source: &str,
    target: &str,
) -> Result<Envelope> {
    let source_config = Config::load_from_path_with_environment(workspace, Some(source))?;
    let target_config = Config::load_from_path_with_environment(workspace, Some(target))?;
    Ok(Envelope::ok_with_data(
        "workspace environments resolved",
        json!({
            "workspace": workspace.display().to_string(),
            "source": {
                "environment": source,
                "profile": source_config.profile_name,
            },
            "target": {
                "environment": target,
                "profile": target_config.profile_name,
            },
        }),
    ))
}

pub(crate) fn tools_workspace_compare_envelope(
    workspace: &Path,
    source: &str,
    target: &str,
) -> Result<Envelope> {
    let source_config = Config::load_from_path_with_environment(workspace, Some(source))?;
    let target_config = Config::load_from_path_with_environment(workspace, Some(target))?;
    Ok(Envelope::ok_with_data(
        "workspace comparison scaffold",
        json!({
            "workspace": workspace.display().to_string(),
            "source": summarize_profile(&source_config),
            "target": summarize_profile(&target_config),
            "notes": [
                "This is the workspace-aware scaffold for future ayx tools operations",
                "Use source and target explicitly for cross-environment workflows",
            ],
        }),
    ))
}

pub(crate) fn tools_workspace_migrate_envelope(
    workspace: &Path,
    source: &str,
    target: &str,
    operation: &str,
) -> Result<Envelope> {
    let source_config = Config::load_from_path_with_environment(workspace, Some(source))?;
    let target_config = Config::load_from_path_with_environment(workspace, Some(target))?;
    Ok(Envelope::ok_with_data(
        format!("workspace {operation} scaffold"),
        json!({
            "workspace": workspace.display().to_string(),
            "operation": operation,
            "source": summarize_profile(&source_config),
            "target": summarize_profile(&target_config),
            "notes": [
                "The command currently resolves source and target environments explicitly",
                "This is the right hook for future cross-environment migration logic",
            ],
        }),
    ))
}

fn summarize_profile(config: &Config) -> Value {
    json!({
        "profile_name": config.profile_name,
        "server": config.server.as_ref().map(|server| json!({
            "webapi_url": server.webapi_url,
            "verify_tls": server.verify_tls(),
        })),
        "sqlserver": config.sqlserver.as_ref().map(|sql| json!({
            "controller": sql.controller.as_ref().map(|conn| conn.database.clone()),
            "server_ui": sql.server_ui.as_ref().map(|conn| conn.database.clone()),
        })),
        "mongo_mode": match config.mongo.mode {
            ayx_core::profile::MongoMode::Embedded => "embedded",
            ayx_core::profile::MongoMode::Managed => "managed",
        },
    })
}

pub(crate) fn server_profile(config: &Config) -> Result<&ServerProfile> {
    config.server.as_ref().ok_or_else(|| {
        anyhow!(
            "config missing server section; add server.webapi_url, curator_api_key, and curator_api_secret"
        )
    })
}

pub(crate) fn parse_key_value_params(items: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for item in items {
        let mut parts = item.splitn(2, '=');
        let key = parts
            .next()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| anyhow!("invalid --param '{}', expected key=value", item))?;
        let value = parts
            .next()
            .ok_or_else(|| anyhow!("invalid --param '{}', expected key=value", item))?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn execute(cli: Cli) -> Result<Envelope> {
    // Plumb the global gates to BOTH transports. Mutating requests
    // short-circuit to dry-run envelopes unless --apply was passed.
    ayx_one_api::set_one_apply(cli.apply);
    ayx_server_api::set_server_apply(cli.apply);
    ayx_one_api::set_no_verify_tls(cli.no_verify_tls);
    ayx_server_api::set_no_verify_tls(cli.no_verify_tls);
    if cli.debug {
        ayx_one_api::set_debug_trace(true);
        ayx_server_api::set_debug_trace(true);
        eprintln!(
            "[debug] apply={} environment={:?} no_verify_tls={} verbose={}",
            cli.apply,
            cli.resolved_environment(),
            cli.no_verify_tls,
            cli.verbose
        );
    }

    // `load_profile` is intentionally a tiny shim around the environment-aware
    // central runtime loader. Capturing the resolved environment here keeps the
    // runtime-only call-sites concise while the explicit path loaders below
    // remain available for onboarding/editor flows.
    let environment = cli
        .environment_flag
        .clone()
        .or(cli.environment_tail.clone());
    let load_profile = |profile: Option<&str>| -> Result<Config> {
        load_profile_with_env(profile, environment.as_deref())
    };
    let envelope = match cli.command {
        Command::Mongo { command } => cmd::mongo::execute(environment.as_deref(), command)?,
        Command::Server { command } => cmd::server::execute(environment.as_deref(), command)?,
        Command::Sqlserver { command } => cmd::sqlserver::execute(environment.as_deref(), command)?,
        Command::Workflow { command } => cmd::workflow::execute(environment.as_deref(), command)?,
        Command::Tools { command } => cmd::tools::execute(command)?,
        Command::Onboard {
            profile,
            environments,
            non_interactive,
        } => {
            let detail = onboard::run_onboarding(
                &profile,
                environment.as_deref(),
                non_interactive,
                environments,
            )?;
            Envelope::ok_with_data("onboarding completed", detail)
        }
        Command::Tui => return tui::run(),
        Command::Profile { command } => match command {
            ProfileCommand::List => profile_list_envelope()?,
            ProfileCommand::Current => profile_current_envelope()?,
            ProfileCommand::Show { name } => profile_show_envelope(name.as_deref())?,
            ProfileCommand::Use { name } => profile_use_envelope(&name)?,
            ProfileCommand::Path => profile_path_envelope()?,
            ProfileCommand::Migrate { profile, name } => {
                profile_migrate_envelope(&profile, name.as_deref())?
            }
        },
        Command::Doctor {
            command,
            fix,
            profile,
        } => doctor_envelope(
            command.as_ref(),
            profile.as_deref(),
            fix,
            environment.as_deref(),
        )?,
        Command::Discover { deep, path } => cmd::discover::execute(path, deep)?,
        Command::One { command } => cmd::one::execute(
            cmd::one::Ctx {
                apply: cli.apply,
                yes: cli.yes,
                environment: environment.as_deref(),
            },
            command,
        )?,
        Command::License { command } => match command {
            LicenseCommand::Api { command } => match command {
                LicenseApiCommand::Status { profile } => {
                    let config = load_profile(profile.as_deref())?;
                    api_status_envelope(&config, "license")?
                }
                LicenseApiCommand::Diagnose { profile } => {
                    let config = load_profile(profile.as_deref())?;
                    api_diagnose_envelope(&config, "license")?
                }
            },
            LicenseCommand::Status { profile } => {
                let config = load_profile(profile.as_deref())?;
                api_status_envelope(&config, "license")?
            }
            LicenseCommand::Inventory { profile } => {
                let config = load_profile(profile.as_deref())?;
                api_inventory_envelope(&config, "license")?
            }
        },
        Command::Catalog { command } => cmd::catalog::execute(command)?,
        Command::Update {
            repo_owner,
            repo_name,
            bin_name,
            target_version,
            skip_confirm,
        } => perform_self_update(
            &repo_owner,
            &repo_name,
            &bin_name,
            target_version.as_deref(),
            skip_confirm,
        )?,
        Command::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            let bin = cmd.get_name().to_string();
            let mut out: Vec<u8> = Vec::new();
            clap_complete::generate(shell, &mut cmd, &bin, &mut out);
            let script = String::from_utf8(out)?;
            // Print to stdout so users can `>` redirect to a completion file;
            // also return a small envelope for --output json.
            print!("{}", script);
            Envelope::ok_with_data(
                format!("{} completions generated", shell),
                json!({
                    "shell": format!("{}", shell),
                    "bytes": script.len(),
                    "usage_hint": match shell {
                        clap_complete::Shell::Bash => "ayx completions bash > ~/.local/share/bash-completion/completions/ayx",
                        clap_complete::Shell::Zsh => "ayx completions zsh > ${fpath[1]}/_ayx",
                        clap_complete::Shell::Fish => "ayx completions fish > ~/.config/fish/completions/ayx.fish",
                        clap_complete::Shell::PowerShell => "ayx completions powershell | Out-String | Invoke-Expression",
                        clap_complete::Shell::Elvish => "ayx completions elvish > ~/.elvish/lib/ayx.elv",
                        _ => "Redirect the printed script into your shell's completion location.",
                    },
                }),
            )
        }
        Command::Whoami { profile } => {
            // Identity in one shot. No network — purely what the local
            // profile + state knows. The operator can append `--output json`
            // for a structured payload or pipe through `ayx one
            // workspace current` for the live workspace.
            let resolution = resolve_runtime_profile(profile.as_deref()).ok();
            let config = load_profile(profile.as_deref()).ok();
            let state = load_ayx_state().ok();
            let active_profile = state.as_ref().and_then(|s| s.active_profile.clone());
            let active_workspace = state.as_ref().and_then(|s| s.active_workspace.clone());
            let account_email = config
                .as_ref()
                .and_then(|c| c.alteryx_one.as_ref())
                .map(|o| o.account_email.clone());
            let one_base_url = config
                .as_ref()
                .and_then(|c| c.alteryx_one.as_ref())
                .and_then(|o| o.normalized_base_url());
            let expected_workspace_id = config
                .as_ref()
                .and_then(|c| c.alteryx_one.as_ref())
                .and_then(|o| o.expected_workspace_id.clone());
            Envelope::ok_with_data(
                active_profile
                    .clone()
                    .unwrap_or_else(|| "(no active profile)".to_string()),
                json!({
                    "config_home": resolution.as_ref().map(|r| r.config_home.clone()),
                    "active_profile": active_profile,
                    "active_workspace": active_workspace,
                    "selected_profile": resolution.as_ref().map(|r| r.selected_profile.clone()),
                    "selection_source": resolution.as_ref().map(|r| r.selection_source.clone()),
                    "resolved_profile_path": resolution.as_ref().map(|r| r.resolved_profile_path.clone()),
                    "environment": environment.clone(),
                    "account_email": account_email,
                    "one_base_url": one_base_url,
                    "expected_workspace_id": expected_workspace_id,
                }),
            )
        }
        Command::Audit { command } => match command {
            AuditCommand::Status { audit_dir } => {
                let dir = audit_dir.unwrap_or_else(|| PathBuf::from("audits"));
                let resolved = ayx_core::audit::resolve_audit_dir(&dir);
                let (count, bytes) = if resolved.exists() {
                    fs::read_dir(&resolved)
                        .map(|entries| {
                            let mut c = 0u64;
                            let mut b = 0u64;
                            for e in entries.flatten() {
                                if let Ok(meta) = e.metadata()
                                    && meta.is_file()
                                {
                                    c += 1;
                                    b += meta.len();
                                }
                            }
                            (c, b)
                        })
                        .unwrap_or((0, 0))
                } else {
                    (0, 0)
                };
                Envelope::ok_with_data(
                    format!(
                        "audit dir resolved: {} ({} file(s), {} byte(s))",
                        resolved.display(),
                        count,
                        bytes
                    ),
                    json!({
                        "audit_dir": resolved.display().to_string(),
                        "file_count": count,
                        "bytes_total": bytes,
                        "exists": resolved.exists(),
                    }),
                )
            }
            AuditCommand::Sweep {
                audit_dir,
                retain_days,
                apply,
            } => {
                let dir = audit_dir.unwrap_or_else(|| PathBuf::from("audits"));
                let report = ayx_core::audit::sweep_audit_dir(&dir, retain_days, !apply)?;
                Envelope::ok_with_data(
                    format!(
                        "audit sweep {}: examined {}, {} {} (kept files newer than {} day(s))",
                        if apply {
                            "executed"
                        } else {
                            "dry-run (use --apply to delete)"
                        },
                        report.examined,
                        report.removed,
                        if apply { "removed" } else { "would be removed" },
                        retain_days
                    ),
                    json!({
                        "audit_dir": ayx_core::audit::resolve_audit_dir(&dir).display().to_string(),
                        "retain_days": retain_days,
                        "apply": apply,
                        "examined": report.examined,
                        "removed": report.removed,
                        "bytes_freed": report.bytes_freed,
                        "removed_paths": report.removed_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    }),
                )
            }
        },
        Command::Secret { command } => match command {
            SecretCommand::Prune { profile, apply } => {
                let config_home = ayx_config_home().map_err(|e| anyhow::anyhow!("{}", e))?;
                let profile_filter = profile.as_deref();

                let candidates = secret::prune_candidates(&config_home, profile_filter)?;

                if candidates.is_empty() {
                    return Ok(Envelope::ok_with_data(
                        "no orphaned accounts found",
                        json!({
                            "applied": apply,
                            "summary": { "candidates": 0, "deleted": 0, "skipped": 0, "not_found": 0, "failed": 0 },
                            "entries": [],
                        }),
                    ));
                }

                if !apply {
                    let entries: Vec<serde_json::Value> = candidates
                        .iter()
                        .map(|c| {
                            json!({
                                "profile": c.profile_stem,
                                "account": c.account,
                                "status": match c.status {
                                    secret::CandidateStatus::WouldDelete => "would_delete",
                                    secret::CandidateStatus::LiveRef => "live_ref",
                                    secret::CandidateStatus::NotFound => "not_found",
                                },
                            })
                        })
                        .collect();
                    let would_delete = candidates
                        .iter()
                        .filter(|c| c.status == secret::CandidateStatus::WouldDelete)
                        .count();
                    let skipped = candidates
                        .iter()
                        .filter(|c| c.status == secret::CandidateStatus::LiveRef)
                        .count();
                    return Ok(Envelope::ok_with_data(
                        format!(
                            "dry run: {} account(s) would be deleted; re-run with --apply",
                            would_delete
                        ),
                        json!({
                            "applied": false,
                            "summary": {
                                "candidates": would_delete,
                                "deleted": 0,
                                "skipped": skipped,
                                "not_found": 0,
                                "failed": 0,
                            },
                            "entries": entries,
                        }),
                    ));
                }

                let results = secret::apply_prune(candidates);
                let deleted = results
                    .iter()
                    .filter(|r| r.status == secret::ApplyStatus::Deleted)
                    .count();
                let not_found = results
                    .iter()
                    .filter(|r| r.status == secret::ApplyStatus::NotFound)
                    .count();
                let skipped = results
                    .iter()
                    .filter(|r| r.status == secret::ApplyStatus::LiveRef)
                    .count();
                let failed_count = results
                    .iter()
                    .filter(|r| matches!(r.status, secret::ApplyStatus::Failed(_)))
                    .count();

                let entries: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| {
                        json!({
                            "profile": r.profile_stem,
                            "account": r.account,
                            "status": match &r.status {
                                secret::ApplyStatus::Deleted   => "deleted",
                                secret::ApplyStatus::NotFound  => "not_found",
                                secret::ApplyStatus::LiveRef   => "live_ref",
                                secret::ApplyStatus::Failed(_) => "failed",
                            },
                        })
                    })
                    .collect();

                if failed_count > 0 {
                    anyhow::bail!(
                        "prune completed with {} failure(s): deleted {}, skipped {}, not_found {}",
                        failed_count,
                        deleted,
                        skipped,
                        not_found
                    );
                }

                Envelope::ok_with_data(
                    format!("deleted {} orphaned account(s)", deleted),
                    json!({
                        "applied": true,
                        "summary": {
                            "candidates": deleted + not_found,
                            "deleted": deleted,
                            "skipped": skipped,
                            "not_found": not_found,
                            "failed": 0,
                        },
                        "entries": entries,
                    }),
                )
            }
        },
        Command::Tactics { command } => cmd::registry::execute_tactics(cli.apply, command)?,
        Command::Workflows { command } => cmd::registry::execute_workflows(cli.apply, command)?,
        Command::Telemetry { command } => cmd::telemetry::execute(environment.as_deref(), command)?,
    };
    Ok(envelope)
}

pub(crate) fn one_doctor_identity_envelope(config: &Config) -> Result<Envelope> {
    let auth = one_platform_auth_status_envelope(config)?;
    let workspace = one_api_live_request(
        config,
        "identity",
        "doctor-workspace-current",
        "GET",
        "/v4/workspaces/current",
        false,
        &[],
    )?;
    Ok(Envelope::ok_with_data(
        "one identity doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                auth.data,
                workspace.data,
            ],
            "recommendations": [
                "Use one workspace people/admins to drill into workspace scope",
                "Route deeper symptom handling to the workflow guidance layer",
            ]
        }),
    ))
}

pub(crate) fn one_doctor_discover_envelope(config: &Config) -> Result<Envelope> {
    let workspace = one_api_live_request(
        config,
        "workspace",
        "discover-workspace-current",
        "GET",
        "/v4/workspaces/current",
        false,
        &[],
    )?;
    let plans = one_api_live_request(
        config,
        "plans",
        "discover-plans-list",
        "GET",
        "/plans/v1/plans",
        false,
        &[],
    )?;
    let schedules = one_api_live_request(
        config,
        "scheduling",
        "discover-schedules-list",
        "GET",
        "/scheduling/v1/schedules",
        false,
        &[],
    )?;
    let billing = one_api_live_request(
        config,
        "billing",
        "discover-billing-account",
        "GET",
        "/billing/v1/my/billing-accounts/current",
        false,
        &[],
    )?;

    Ok(Envelope::ok_with_data(
        "one discovery doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                workspace.data,
                plans.data,
                schedules.data,
                billing.data,
            ],
            "recommendations": [
                "Use one workspace current to identify the workspace context",
                "Use one plans list/detail/run to resolve plan ids",
                "Use one scheduling list/detail/enable/disable to resolve schedule ids",
                "Use the workflow guidance layer to decide whether a symptom belongs to identity, plans, scheduling, or billing",
            ]
        }),
    ))
}

pub(crate) fn one_doctor_plans_envelope(config: &Config) -> Result<Envelope> {
    let list = one_api_live_request(
        config,
        "plans",
        "doctor-plans-list",
        "GET",
        "/plans/v1/plans",
        false,
        &[],
    )?;
    let count = one_api_live_request(
        config,
        "plans",
        "doctor-plans-count",
        "GET",
        "/plans/v1/plans/count",
        false,
        &[],
    )?;
    Ok(Envelope::ok_with_data(
        "one plans doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                list.data,
                count.data,
            ],
            "recommendations": [
                "Use one plans detail/run when a specific plan id is known",
                "Use the workflow guidance layer for support-case sequencing and operator guidance",
            ]
        }),
    ))
}

pub(crate) fn one_doctor_scheduling_envelope(config: &Config) -> Result<Envelope> {
    let list = one_api_live_request(
        config,
        "scheduling",
        "doctor-schedules-list",
        "GET",
        "/scheduling/v1/schedules",
        false,
        &[],
    )?;
    let count = one_api_live_request(
        config,
        "scheduling",
        "doctor-schedules-count",
        "GET",
        "/scheduling/v1/schedules/count",
        false,
        &[],
    )?;
    Ok(Envelope::ok_with_data(
        "one scheduling doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                list.data,
                count.data,
            ],
            "recommendations": [
                "Use one scheduling detail/enable/disable when a schedule id is known",
                "Route operator selection and escalation guidance through the workflow guidance layer",
            ]
        }),
    ))
}

pub(crate) fn one_doctor_billing_envelope(config: &Config) -> Result<Envelope> {
    let account = one_api_live_request(
        config,
        "billing",
        "doctor-billing-account",
        "GET",
        "/billing/v1/my/billing-accounts/current",
        false,
        &[],
    )?;
    let usage = one_api_live_request(
        config,
        "billing",
        "doctor-billing-usage",
        "GET",
        "/billing/v1/usage/export",
        false,
        &[],
    )?;
    Ok(Envelope::ok_with_data(
        "one billing doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                account.data,
                usage.data,
            ],
            "recommendations": [
                "Keep billing reference-only unless a repeatable operator workflow appears",
                "Use the workflow guidance layer to decide whether billing belongs in CLI or documentation only",
            ]
        }),
    ))
}

fn profile_list_envelope() -> Result<Envelope> {
    let state = load_ayx_state()?;
    let profiles = list_central_profiles()?;
    Ok(Envelope::ok_with_data(
        "profiles listed",
        json!({
            "config_home": ayx_config_home()?.display().to_string(),
            "profiles_dir": ayx_profiles_dir()?.display().to_string(),
            "active_profile": state.active_profile,
            "profiles": profiles,
        }),
    ))
}

fn profile_current_envelope() -> Result<Envelope> {
    let resolution = resolve_runtime_profile(None)?;
    Ok(Envelope::ok_with_data(
        "current profile resolved",
        json!({
            "config_home": resolution.config_home,
            "selected_profile": resolution.selected_profile,
            "selection_source": resolution.selection_source,
            "resolved_profile_path": resolution.resolved_profile_path.clone(),
            "active_profile": resolution.active_profile,
            "exists": Path::new(&resolution.resolved_profile_path).exists(),
            "state_path": ayx_state_path()?.display().to_string(),
        }),
    ))
}

fn profile_show_envelope(name: Option<&str>) -> Result<Envelope> {
    let resolution = resolve_runtime_profile(name)?;
    let config = Config::load_runtime_profile_with_environment(name, None)?;
    Ok(Envelope::ok_with_data(
        "profile loaded",
        json!({
            "name": resolution.selected_profile,
            "config_home": resolution.config_home,
            "selected_profile": resolution.selected_profile,
            "selection_source": resolution.selection_source,
            "resolved_profile_path": resolution.resolved_profile_path,
            "active_profile": resolution.active_profile,
            "resolution": resolution,
            "profile_name": config.profile_name,
            "sections": {
                "alteryx_one": config.alteryx_one.is_some(),
                "server": config.server.is_some(),
                "server_api": config.server_api.is_some(),
                "sqlserver": config.sqlserver.is_some(),
                "observability": config.observability.is_some(),
            }
        }),
    ))
}

fn profile_use_envelope(name: &str) -> Result<Envelope> {
    let path = profile_storage_path(name)?;
    if !path.exists() {
        bail!("profile '{}' not found at '{}'", name, path.display());
    }
    let mut state = load_ayx_state()?;
    state.active_profile = Some(name.to_string());
    save_ayx_state(&state)?;
    Ok(Envelope::ok_with_data(
        "active profile updated",
        json!({
            "active_profile": name,
            "path": path.display().to_string(),
            "state_path": ayx_state_path()?.display().to_string(),
        }),
    ))
}

fn profile_path_envelope() -> Result<Envelope> {
    Ok(Envelope::ok_with_data(
        "profile storage paths",
        json!({
            "config_home": ayx_config_home()?.display().to_string(),
            "profiles_dir": ayx_profiles_dir()?.display().to_string(),
            "workspaces_dir": ayx_workspaces_dir()?.display().to_string(),
            "state_path": ayx_state_path()?.display().to_string(),
        }),
    ))
}

fn profile_migrate_envelope(profile: &Path, name: Option<&str>) -> Result<Envelope> {
    if !profile.exists() {
        bail!("profile source '{}' does not exist", profile.display());
    }
    let target_name = name
        .map(ToOwned::to_owned)
        .or_else(|| {
            profile
                .file_stem()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "default".to_string());
    let target = profile_storage_path(&target_name)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut config = Config::load_from_path(profile)?;
    // Derive the keyring scope from the *target* file stem, identical to how
    // `write_config_with_policy` does it.  Using `target_name` raw would produce
    // a different scope when the caller passes a name with a `.yaml` suffix or
    // surrounding whitespace (e.g. "prod.yaml" → stem "prod"), causing the
    // migrate-write scope to differ from every subsequent normal save scope.
    let migrate_scope = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&target_name);
    let secretize = onboard::secretize_config(
        &mut config,
        migrate_scope,
        onboard::InlineSecretPolicy::Allow,
    )?;
    let body = serde_yaml::to_string(&ayx_core::profile::canonical_profile_value(&config)?)?;
    onboard::write_restricted(&target, body.as_bytes())?;
    let mut state = load_ayx_state()?;
    // Use the same normalized stem that write_config_with_policy uses for scope so
    // active_profile matches what `profile list` shows (normalized, no .yaml suffix).
    state.active_profile = Some(migrate_scope.to_string());
    save_ayx_state(&state)?;
    Ok(Envelope::ok_with_data(
        "profile migrated",
        json!({
            "source": profile.display().to_string(),
            "target": target.display().to_string(),
            "active_profile": migrate_scope,
            "secret_refs": secretize.refs.keys().collect::<Vec<_>>(),
            "inline_secret_fields": secretize.inline_fields,
            "next_steps": [
                "Secrets were moved to the OS keyring when available; run `ayx doctor config` to verify refs",
                "Run `ayx doctor` to validate the migrated profile",
            ],
        }),
    ))
}

fn doctor_envelope(
    command: Option<&DoctorCommand>,
    profile: Option<&str>,
    fix: bool,
    environment: Option<&str>,
) -> Result<Envelope> {
    match command {
        // `None` (bare `ayx doctor`) and `All` (explicit `ayx doctor all`) both run the full suite.
        None | Some(DoctorCommand::All) => doctor_full_envelope(profile, fix, environment),
        Some(DoctorCommand::Config) => doctor_config_envelope(profile, fix),
        Some(DoctorCommand::Auth) => doctor_auth_envelope(profile, environment),
        Some(DoctorCommand::Network) => doctor_network_envelope(profile, environment),
        Some(DoctorCommand::One) => doctor_one_envelope(profile, environment),
        Some(DoctorCommand::Server) => doctor_server_envelope(profile, environment),
        Some(DoctorCommand::Mongo) => doctor_mongo_envelope(profile, environment),
    }
}

fn doctor_full_envelope(
    profile: Option<&str>,
    fix: bool,
    environment: Option<&str>,
) -> Result<Envelope> {
    let config = doctor_config_envelope(profile, fix)?;
    let auth = doctor_auth_envelope(profile, environment)?;
    let network = doctor_network_envelope(profile, environment)?;
    let one = doctor_one_envelope(profile, environment)?;
    let server = doctor_server_envelope(profile, environment)?;
    let mongo = doctor_mongo_envelope(profile, environment)?;
    let overall = doctor_rollup_status([
        doctor_status_from_data(&config.data),
        doctor_status_from_data(&auth.data),
        doctor_status_from_data(&network.data),
        doctor_status_from_data(&one.data),
        doctor_status_from_data(&server.data),
        doctor_status_from_data(&mongo.data),
    ]);
    Ok(Envelope::ok_with_data(
        "doctor completed",
        json!({
            "sequence": ["config", "auth", "network", "one", "server", "mongo"],
            "fix_applied": fix,
            "overall": overall,
            "checks": {
                "config": config.data,
                "auth": auth.data,
                "network": network.data,
                "one": one.data,
                "server": server.data,
                "mongo": mongo.data,
            }
        }),
    ))
}

fn doctor_config_envelope(profile: Option<&str>, fix: bool) -> Result<Envelope> {
    if fix {
        fs::create_dir_all(ayx_profiles_dir()?)?;
        fs::create_dir_all(ayx_workspaces_dir()?)?;
        if !ayx_state_path()?.exists() {
            save_ayx_state(&AyxState::default())?;
        }
    }
    let resolution = resolve_runtime_profile(profile)?;
    let (shape, inline_risks) = if Path::new(&resolution.resolved_profile_path).exists() {
        let raw = fs::read_to_string(&resolution.resolved_profile_path)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&raw)?;
        (
            profile_shape_label(&value),
            collect_inline_secret_warnings(&raw),
        )
    } else {
        ("missing", Vec::new())
    };
    let (status, summary) =
        doctor_config_status_summary(&resolution.selected_profile, shape, &inline_risks);
    Ok(Envelope::ok_with_data(
        "doctor config completed",
        json!({
            "config_home": ayx_config_home()?.display().to_string(),
            "profiles_dir": ayx_profiles_dir()?.display().to_string(),
            "workspaces_dir": ayx_workspaces_dir()?.display().to_string(),
            "state_path": ayx_state_path()?.display().to_string(),
            "resolution": resolution,
            "shape": shape,
            "inline_secret_risks": inline_risks,
            "fix_applied": fix,
            "status": status,
            "summary": summary,
        }),
    ))
}

pub(crate) fn doctor_config_envelope_from_path(profile: &Path, fix: bool) -> Result<Envelope> {
    if fix {
        fs::create_dir_all(ayx_profiles_dir()?)?;
        fs::create_dir_all(ayx_workspaces_dir()?)?;
        if !ayx_state_path()?.exists() {
            save_ayx_state(&AyxState::default())?;
        }
    }
    let resolution = profile_resolution_detail(profile)?;
    let (shape, inline_risks) = if Path::new(&resolution.resolved_path).exists() {
        let raw = fs::read_to_string(&resolution.resolved_path)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&raw)?;
        (
            profile_shape_label(&value),
            collect_inline_secret_warnings(&raw),
        )
    } else {
        ("missing", Vec::new())
    };
    let label = Path::new(&resolution.resolved_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("profile");
    let (status, summary) = doctor_config_status_summary(label, shape, &inline_risks);
    Ok(Envelope::ok_with_data(
        "doctor config completed",
        json!({
            "config_home": ayx_config_home()?.display().to_string(),
            "profiles_dir": ayx_profiles_dir()?.display().to_string(),
            "workspaces_dir": ayx_workspaces_dir()?.display().to_string(),
            "state_path": ayx_state_path()?.display().to_string(),
            "resolution": resolution,
            "shape": shape,
            "inline_secret_risks": inline_risks,
            "fix_applied": fix,
            "status": status,
            "summary": summary,
        }),
    ))
}

fn doctor_auth_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_runtime_profile_with_environment(profile, environment)?;
    let one = config.alteryx_one.as_ref();
    let server = config.server.as_ref();
    let one_configured = one.is_some();
    let one_access_token_present = one
        .and_then(|v| v.access_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_refresh_token_present = one
        .and_then(|v| v.refresh_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_oauth_client_id_present = one
        .and_then(|v| v.oauth_client_id.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let server_configured = server.is_some();
    let server_api_key_present = server.is_some_and(|v| !v.curator_api_key.trim().is_empty());
    let server_api_secret_present = server.is_some_and(|v| !v.curator_api_secret.trim().is_empty());
    let (status, summary) = doctor_auth_status_summary(
        one_configured,
        one_access_token_present,
        one_refresh_token_present,
        one_oauth_client_id_present,
        server_configured,
        server_api_key_present,
        server_api_secret_present,
    );
    Ok(Envelope::ok_with_data(
        "doctor auth completed",
        json!({
            "profile": config.profile_name,
            "status": status,
            "summary": summary,
            "one": {
                "configured": one_configured,
                "access_token_present": one_access_token_present,
                "refresh_token_present": one_refresh_token_present,
                "oauth_client_id_present": one_oauth_client_id_present,
                "access_token_source": secret_source(
                    one.and_then(|v| v.access_token_ref.as_ref()),
                    one.and_then(|v| v.access_token.as_deref()),
                ),
                "refresh_token_source": secret_source(
                    one.and_then(|v| v.refresh_token_ref.as_ref()),
                    one.and_then(|v| v.refresh_token.as_deref()),
                ),
            },
            "server": {
                "configured": server_configured,
                "curator_api_key_present": server_api_key_present,
                "curator_api_secret_present": server_api_secret_present,
                "curator_api_secret_source": secret_source(
                    server.and_then(|v| v.curator_api_secret_ref.as_ref()),
                    server.map(|v| v.curator_api_secret.as_str()),
                ),
            }
        }),
    ))
}

pub(crate) fn doctor_auth_envelope_from_path(
    profile: &Path,
    environment: Option<&str>,
) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
    let one = config.alteryx_one.as_ref();
    let server = config.server.as_ref();
    let one_configured = one.is_some();
    let one_access_token_present = one
        .and_then(|v| v.access_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_refresh_token_present = one
        .and_then(|v| v.refresh_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_oauth_client_id_present = one
        .and_then(|v| v.oauth_client_id.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let server_configured = server.is_some();
    let server_api_key_present = server.is_some_and(|v| !v.curator_api_key.trim().is_empty());
    let server_api_secret_present = server.is_some_and(|v| !v.curator_api_secret.trim().is_empty());
    let (status, summary) = doctor_auth_status_summary(
        one_configured,
        one_access_token_present,
        one_refresh_token_present,
        one_oauth_client_id_present,
        server_configured,
        server_api_key_present,
        server_api_secret_present,
    );
    Ok(Envelope::ok_with_data(
        "doctor auth completed",
        json!({
            "profile": config.profile_name,
            "status": status,
            "summary": summary,
            "one": {
                "configured": one_configured,
                "access_token_present": one_access_token_present,
                "refresh_token_present": one_refresh_token_present,
                "oauth_client_id_present": one_oauth_client_id_present,
                "access_token_source": secret_source(
                    one.and_then(|v| v.access_token_ref.as_ref()),
                    one.and_then(|v| v.access_token.as_deref()),
                ),
                "refresh_token_source": secret_source(
                    one.and_then(|v| v.refresh_token_ref.as_ref()),
                    one.and_then(|v| v.refresh_token.as_deref()),
                ),
            },
            "server": {
                "configured": server_configured,
                "curator_api_key_present": server_api_key_present,
                "curator_api_secret_present": server_api_secret_present,
                "curator_api_secret_source": secret_source(
                    server.and_then(|v| v.curator_api_secret_ref.as_ref()),
                    server.map(|v| v.curator_api_secret.as_str()),
                ),
            }
        }),
    ))
}

fn doctor_network_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_runtime_profile_with_environment(profile, environment)?;
    let one_base_url = config
        .alteryx_one
        .as_ref()
        .and_then(|v| v.normalized_base_url());
    let one_token_endpoint = config.alteryx_one.as_ref().and_then(|v| {
        let workspace_id = v.active_workspace_id();
        v.effective_token_endpoint_url_for_workspace(workspace_id)
    });
    let server_base_url = config.server.as_ref().map(|v| v.webapi_url.clone());
    let server_api_base_url = config.server_api.as_ref().map(|v| v.base_url.clone());
    let one_configured = one_base_url.is_some() || one_token_endpoint.is_some();
    let server_configured = server_base_url.is_some() || server_api_base_url.is_some();
    let (status, summary) = doctor_network_status_summary(one_configured, server_configured);
    Ok(Envelope::ok_with_data(
        "doctor network completed",
        json!({
            "profile": config.profile_name,
            "status": status,
            "summary": summary,
            "targets": {
                "one_base_url": one_base_url,
                "one_token_endpoint": one_token_endpoint,
                "server_base_url": server_base_url,
                "server_api_base_url": server_api_base_url,
            },
            "notes": [
                "Network checks currently validate configured endpoints rather than performing invasive probes",
            ],
        }),
    ))
}

fn doctor_one_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_runtime_profile_with_environment(profile, environment)?;
    if config.alteryx_one.is_none() {
        return Ok(Envelope::ok_with_data(
            "one auth diagnose",
            json!({
                "product": "one",
                "surface": "auth",
                "profile": config.profile_name,
                "status": "skip",
                "summary": "One not configured",
            }),
        ));
    }

    let mut envelope = one_platform_auth_diagnose_envelope(&config)?;
    let access_token_present = envelope
        .data
        .get("access_token_present")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let refresh_token_present = envelope
        .data
        .get("refresh_token_present")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let oauth_client_id_present = envelope
        .data
        .get("oauth_client_id_present")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let diagnosis = envelope
        .data
        .get("diagnosis")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (status, summary) = if diagnosis == "token present but workspace probe failed" {
        ("fail", "One workspace probe failed".to_string())
    } else if !access_token_present {
        ("warn", "One access token missing".to_string())
    } else if !refresh_token_present && !oauth_client_id_present {
        (
            "warn",
            "One refresh token and client id missing".to_string(),
        )
    } else if !refresh_token_present {
        ("warn", "One refresh token missing".to_string())
    } else if !oauth_client_id_present {
        ("warn", "One OAuth client id missing".to_string())
    } else if diagnosis == "token present and workspace probe executed" {
        ("ok", "One workspace probe succeeded".to_string())
    } else {
        ("warn", "One auth diagnostic incomplete".to_string())
    };
    if let Some(data) = envelope.data.as_object_mut() {
        data.insert("status".to_string(), json!(status));
        data.insert("summary".to_string(), json!(summary));
    }
    Ok(envelope)
}

fn doctor_server_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_runtime_profile_with_environment(profile, environment)?;
    let server_ready = config.server.is_some() || config.server_api.is_some();
    let status = if server_ready { "warn" } else { "skip" };
    let summary = if server_ready {
        "Server configured; live validation not run"
    } else {
        "Server not configured"
    };
    Ok(Envelope::ok_with_data(
        "doctor server completed",
        json!({
            "profile": config.profile_name,
            "configured": server_ready,
            "status": status,
            "summary": summary,
            "server_url": config.server.as_ref().map(|v| v.webapi_url.clone()),
            "server_api_url": config.server_api.as_ref().map(|v| v.base_url.clone()),
            "recommendations": if server_ready {
                vec!["Run `ayx server auth status` or `ayx server api status` for live validation"]
            } else {
                vec!["Add server or server_api settings to the active profile"]
            }
        }),
    ))
}

fn doctor_mongo_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_runtime_profile_with_environment(profile, environment)?;
    let managed_host_present = config
        .mongo
        .managed
        .as_ref()
        .and_then(|managed| managed.host.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let managed_url_present = config
        .mongo
        .managed
        .as_ref()
        .and_then(|managed| managed.url.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let (status, summary) = match config.mongo.mode {
        ayx_core::profile::MongoMode::Embedded => {
            ("ok", "Mongo embedded mode selected".to_string())
        }
        ayx_core::profile::MongoMode::Managed if !managed_host_present && !managed_url_present => {
            ("warn", "Managed Mongo missing host/url".to_string())
        }
        ayx_core::profile::MongoMode::Managed => (
            "warn",
            "Managed Mongo configured; connection not verified".to_string(),
        ),
    };
    Ok(Envelope::ok_with_data(
        "doctor mongo completed",
        json!({
            "profile": config.profile_name,
            "mode": match config.mongo.mode {
                ayx_core::profile::MongoMode::Embedded => "embedded",
                ayx_core::profile::MongoMode::Managed => "managed",
            },
            "gallery_database": config.mongo.databases.gallery_name,
            "service_database": config.mongo.databases.service_name,
            "managed_host_present": managed_host_present,
            "managed_url_present": managed_url_present,
            "status": status,
            "summary": summary,
        }),
    ))
}

fn doctor_config_status_summary(
    profile_label: &str,
    shape: &str,
    inline_risks: &[String],
) -> (&'static str, String) {
    if shape == "missing" {
        ("fail", format!("profile '{profile_label}' missing"))
    } else if !inline_risks.is_empty() {
        (
            "warn",
            format!("profile '{profile_label}' resolved; inline secrets found"),
        )
    } else {
        (
            "ok",
            format!("profile '{profile_label}' resolved; no inline secrets"),
        )
    }
}

fn doctor_auth_status_summary(
    one_configured: bool,
    one_access_token_present: bool,
    one_refresh_token_present: bool,
    one_oauth_client_id_present: bool,
    server_configured: bool,
    server_api_key_present: bool,
    server_api_secret_present: bool,
) -> (&'static str, String) {
    if !one_configured && !server_configured {
        return ("skip", "One and Server auth not configured".to_string());
    }

    let one_ready = !one_configured
        || (one_access_token_present && one_refresh_token_present && one_oauth_client_id_present);
    let server_ready = !server_configured || (server_api_key_present && server_api_secret_present);
    if !one_ready || !server_ready {
        let mut incomplete = Vec::new();
        if one_configured && !one_ready {
            incomplete.push("One");
        }
        if server_configured && !server_ready {
            incomplete.push("Server");
        }
        return (
            "warn",
            format!("{} auth incomplete", incomplete.join(" and ")),
        );
    }

    let summary = match (one_configured, server_configured) {
        (true, true) => "One and Server auth configured",
        (true, false) => "One auth configured",
        (false, true) => "Server auth configured",
        (false, false) => "One and Server auth not configured",
    };
    ("ok", summary.to_string())
}

fn doctor_network_status_summary(
    one_configured: bool,
    server_configured: bool,
) -> (&'static str, String) {
    match (one_configured, server_configured) {
        (false, false) => ("skip", "No One or Server endpoints configured".to_string()),
        (true, true) => (
            "warn",
            "One and Server endpoints configured; no live probes run".to_string(),
        ),
        (true, false) => (
            "warn",
            "One endpoints configured; no live probes run".to_string(),
        ),
        (false, true) => (
            "warn",
            "Server endpoints configured; no live probes run".to_string(),
        ),
    }
}

fn doctor_status_from_data(data: &Value) -> &str {
    data.get("status").and_then(Value::as_str).unwrap_or("warn")
}

fn doctor_rollup_status(statuses: [&str; 6]) -> &'static str {
    let mut best_rank = 0u8;
    for status in statuses {
        if status == "skip" {
            continue;
        }
        let rank = match status {
            "fail" => 3,
            "warn" => 2,
            "ok" => 1,
            _ => 2,
        };
        if rank > best_rank {
            best_rank = rank;
        }
    }
    match best_rank {
        3 => "fail",
        2 => "warn",
        1 => "ok",
        _ => "skip",
    }
}

fn collect_inline_secret_warnings(raw: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    for key in [
        "access_token:",
        "refresh_token:",
        "client_secret:",
        "curator_api_secret:",
        "password:",
    ] {
        if raw
            .lines()
            .any(|line| line.contains(key) && !line.contains("${"))
        {
            warnings.push(format!(
                "inline secret detected for {}",
                key.trim_end_matches(':')
            ));
        }
    }
    warnings
}

fn secret_source(reference: Option<&String>, value: Option<&str>) -> &'static str {
    if let Some(reference) = reference {
        if reference.starts_with("keyring:") {
            return "keyring";
        }
        if reference.starts_with("env:") {
            return "env";
        }
        return "reference";
    }
    if value.is_some_and(|value| !value.trim().is_empty()) {
        return "inline";
    }
    "missing"
}

pub(crate) fn one_platform_auth_status_envelope(config: &Config) -> Result<Envelope> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("config missing alteryx_one section"))?;
    let workspace_id = one.active_workspace_id();
    let access_token = one.resolved_access_token();
    let refresh_token = one.resolved_refresh_token();
    let oauth_client_id = one.resolved_oauth_client_id();
    let workspace_access_token_present =
        one.active_workspace_credential().is_some_and(|credential| {
            credential
                .access_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        });
    let api_logging = config.observability.as_ref().and_then(|obs| {
        obs.api_logging.as_ref().map(|logging| {
            json!({
                "enabled": logging.enabled,
                "path": logging.path,
                "redact_bodies": logging.redact_bodies,
                "log_requests": logging.log_requests,
                "log_responses": logging.log_responses,
            })
        })
    });
    let workspace_probe = if access_token.is_some() {
        Some(one_api_live_request(
            config,
            "auth",
            "auth-status",
            "GET",
            "/v4/apiAccessTokens",
            false,
            &[],
        )?)
    } else {
        None
    };

    Ok(Envelope::ok_with_data(
        "one auth status",
        json!({
            "product": "one",
            "surface": "auth",
            "profile": config.profile_name,
            "workspace_id": workspace_id,
            "oauth_client_id_present": oauth_client_id.is_some(),
            "base_url": one.normalized_base_url(),
            "token_endpoint_url": one.effective_token_endpoint_url_for_workspace(workspace_id),
            "access_token_present": access_token.is_some(),
            "refresh_token_present": refresh_token.is_some(),
            "observability": api_logging,
            "token_source": if workspace_access_token_present {
                "workspace"
            } else if access_token.is_some() {
                "profile"
            } else {
                "missing"
            },
            "access_token_claims": access_token_claim_summary(access_token),
            "validation_target": "/v4/apiAccessTokens",
            "workspace_probe": workspace_probe.as_ref().map(|probe| {
                sanitize_live_probe_for_user(
                    &probe.data,
                    access_token.unwrap_or(""),
                )
            }),
            "message": "One API token posture captured",
        }),
    ))
}

pub(crate) fn one_platform_auth_diagnose_envelope(config: &Config) -> Result<Envelope> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("config missing alteryx_one section"))?;
    let workspace_id = one.active_workspace_id();
    let access_token = one.resolved_access_token();
    let refresh_token = one.resolved_refresh_token();
    let oauth_client_id = one.resolved_oauth_client_id();
    let has_token = access_token.is_some();
    let has_refresh_token = refresh_token.is_some();
    if !has_token {
        return Ok(Envelope::ok_with_data(
            "one auth diagnose",
            json!({
                "product": "one",
                "surface": "auth",
                "profile": config.profile_name,
                "workspace_id": workspace_id,
                "oauth_client_id_present": oauth_client_id.is_some(),
                "base_url": one.normalized_base_url(),
                "token_endpoint_url": one.effective_token_endpoint_url_for_workspace(workspace_id),
                "access_token_present": false,
                "refresh_token_present": has_refresh_token,
                "diagnosis": "alteryx_one.access_token is missing",
                "recommendations": [
                    "Set AYX_ONE_BASE_URL to the API host, not the auth issuer",
                    "Set AYX_ONE_TOKEN_ENDPOINT_URL to the auth issuer root (for example https://pingauth.alteryxcloud.com/as)",
                    "Set AYX_ONE_API_ACCESS_TOKEN in .env",
                    "Populate alteryx_one.access_token in the active central profile if you prefer config-based storage"
                ],
            }),
        ));
    }

    let workspace_probe = match one_api_live_request(
        config,
        "auth",
        "auth-diagnose",
        "GET",
        "/v4/apiAccessTokens",
        false,
        &[],
    ) {
        Ok(probe) => Some(probe),
        Err(err) => {
            return Ok(Envelope::ok_with_data(
                "one auth diagnose",
                json!({
                    "product": "one",
                    "surface": "auth",
                    "profile": config.profile_name,
                    "workspace_id": workspace_id,
                    "oauth_client_id_present": oauth_client_id.is_some(),
                    "base_url": one.normalized_base_url(),
                    "token_endpoint_url": one.effective_token_endpoint_url_for_workspace(workspace_id),
                    "access_token_present": true,
                    "refresh_token_present": has_refresh_token,
                    "access_token_claims": access_token_claim_summary(access_token),
                    "diagnosis": "token present but workspace probe failed",
                    "workspace_probe_error": err.to_string(),
                    "recommendations": [
                        "If the access token is expired, mint a fresh one or repair refresh-token auth",
                        "Confirm the active profile is pointing at the intended workspace and auth issuer",
                        "Use one auth status for posture and one workspace current for live reachability",
                    ],
                }),
            ));
        }
    };

    let Some(probe) = workspace_probe.as_ref() else {
        // Unreachable: workspace_probe is Some at this point (the Err arm
        // returned early above), but bind defensively rather than .unwrap().
        return Ok(Envelope::ok_with_data(
            "one auth diagnose",
            json!({
                "product": "one",
                "surface": "auth",
                "profile": config.profile_name,
                "workspace_id": workspace_id,
                "oauth_client_id_present": oauth_client_id.is_some(),
                "base_url": one.normalized_base_url(),
                "token_endpoint_url": one.effective_token_endpoint_url_for_workspace(workspace_id),
                "access_token_present": true,
                "refresh_token_present": has_refresh_token,
                "access_token_claims": access_token_claim_summary(access_token),
                "diagnosis": "token present but workspace probe was not executed",
                "workspace_probe": null,
                "recommendations": [
                    "Use one token or auth status for evidence",
                    "Route any failing symptoms into the workflow guidance layer",
                ],
            }),
        ));
    };
    Ok(Envelope::ok_with_data(
        "one auth diagnose",
        json!({
            "product": "one",
            "surface": "auth",
            "profile": config.profile_name,
            "workspace_id": workspace_id,
            "oauth_client_id_present": oauth_client_id.is_some(),
            "base_url": one.normalized_base_url(),
            "token_endpoint_url": one.effective_token_endpoint_url_for_workspace(workspace_id),
            "access_token_present": true,
            "refresh_token_present": has_refresh_token,
            "access_token_claims": access_token_claim_summary(access_token),
            "diagnosis": "token present and workspace probe executed",
            "workspace_probe": sanitize_live_probe_for_user(
                &probe.data,
                access_token.unwrap_or(""),
            ),
            "recommendations": [
                "Use one token or auth status for evidence",
                "Route any failing symptoms into the workflow guidance layer",
            ],
        }),
    ))
}

fn perform_self_update(
    repo_owner: &str,
    repo_name: &str,
    bin_name: &str,
    target_version: Option<&str>,
    skip_confirm: bool,
) -> Result<Envelope> {
    warn_if_update_context_looks_suspicious(bin_name);

    let target = self_update::get_target();
    let mut builder = GitHubUpdate::configure();
    builder
        .repo_owner(repo_owner)
        .repo_name(repo_name)
        .bin_name(bin_name)
        .current_version(env!("CARGO_PKG_VERSION"))
        .target(target);

    if let Some(version) = target_version {
        builder.target_version_tag(version);
    }
    if skip_confirm {
        builder.no_confirm(true);
    }

    let status = builder.build()?.update()?;
    let detail = match status {
        Status::Updated(version) => json!({
            "result": "updated",
            "version": version,
        }),
        Status::UpToDate(version) => json!({
            "result": "up_to_date",
            "version": version,
        }),
    };

    Ok(Envelope::ok_with_data("self-update complete", detail))
}

fn warn_if_update_context_looks_suspicious(bin_name: &str) {
    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!(
                "warning: unable to determine the active {bin_name} binary for self-update: {err}"
            );
            return;
        }
    };

    let exe = current_exe.to_string_lossy();
    let exe_lower = exe.to_lowercase();
    let suspicious = exe_lower.contains("/target/debug/")
        || exe_lower.contains("/target/release/")
        || exe_lower.contains(".cargo/bin/")
        || exe_lower.contains(".local/share/mise/")
        || exe_lower.contains("/node/")
        || exe_lower.contains("/nvm/");

    if suspicious {
        eprintln!(
            "warning: self-update is running from {exe}. If a different {bin_name} appears first on PATH, this update will not affect future invocations."
        );
        eprintln!(
            "warning: prefer a release install such as ~/.local/bin/{bin_name} and confirm with `type -a {bin_name}`."
        );
    }
}

fn main() -> Result<()> {
    // Clap owns --help/-h rendering. Previously a hand-rolled print_help()
    // intercepted bare --help; that drifted from the actual command tree.
    let cli = Cli::parse();
    let output = cli.output.clone();

    // Dispatch runs on the main thread. On Windows, build.rs reserves a 16 MiB
    // main-thread stack (/STACK) so the deep clap parse can't overflow the 1 MiB
    // MSVC default — see ayx-rs/build.rs and issue #59. No worker thread needed.
    let result = execute(cli);

    match result {
        Ok(envelope) => {
            let rendered = format_envelope(&envelope, &output)?;
            if envelope.ok {
                print!("{rendered}");
                println!();
                Ok(())
            } else {
                eprint!("{rendered}");
                eprintln!();
                let _ = io::stdout().lock().flush();
                let _ = io::stderr().lock().flush();
                std::process::exit(exit_code_for_envelope(&envelope));
            }
        }
        Err(err) => {
            let code = classify_anyhow_error(&err);
            let hint = hint_for_error_code(code);
            let mut data = json!({
                "error": err.to_string(),
                "error_code": code.as_str(),
            });
            // Only attach a transport diagnosis for genuine network/upstream
            // failures. Attaching it unconditionally (e.g. to a missing-flag
            // validation error) fabricates a transport problem the user never had.
            if matches!(
                code,
                ayx_core::envelope::ErrorCode::Network | ayx_core::envelope::ErrorCode::Upstream
            ) {
                data["transport"] = serde_json::to_value(transport_error_summary(err.as_ref()))
                    .unwrap_or(Value::Null);
            }
            if let Some(h) = hint {
                data["hint"] = Value::String(h.to_string());
            }
            let err_env = Envelope::err_coded(code, "command failed", data);
            // Errors always go to stderr; the format mirrors the success
            // renderer so JSON consumers see the same envelope shape. Exit
            // non-zero via process::exit (like the ok=false branch) rather than
            // returning Err, which would make the runtime print a second,
            // non-JSON `Error: ...` line and corrupt the stderr envelope.
            eprint!(
                "{}",
                format_envelope(&err_env, &output).unwrap_or_else(|_| err_env.message.clone())
            );
            eprintln!();
            let _ = io::stdout().lock().flush();
            let _ = io::stderr().lock().flush();
            std::process::exit(1);
        }
    }
}

fn exit_code_for_envelope(envelope: &Envelope) -> i32 {
    if envelope.ok { 0 } else { 1 }
}

/// Input accepted by the shared profile loader shims. Runtime callers pass a
/// central profile name while editor/onboarding callers pass an explicit file
/// path.
pub(crate) enum ProfileInput<'a> {
    Runtime(Option<&'a str>),
    Path(&'a Path),
}

impl<'a> From<Option<&'a str>> for ProfileInput<'a> {
    fn from(profile: Option<&'a str>) -> Self {
        Self::Runtime(profile)
    }
}

impl<'a> From<&'a str> for ProfileInput<'a> {
    fn from(profile: &'a str) -> Self {
        Self::Runtime(Some(profile))
    }
}

impl<'a> From<&'a Option<String>> for ProfileInput<'a> {
    fn from(profile: &'a Option<String>) -> Self {
        Self::Runtime(profile.as_deref())
    }
}

impl<'a> From<&'a Path> for ProfileInput<'a> {
    fn from(path: &'a Path) -> Self {
        Self::Path(path)
    }
}

impl<'a> From<&'a PathBuf> for ProfileInput<'a> {
    fn from(path: &'a PathBuf) -> Self {
        Self::Path(path.as_path())
    }
}

/// Canonical profile loader used by runtime-facing commands and the smaller
/// set of explicit path-based editor flows.
pub(crate) fn load_profile_with_env<'a, P>(profile: P, environment: Option<&str>) -> Result<Config>
where
    P: Into<ProfileInput<'a>>,
{
    match profile.into() {
        ProfileInput::Runtime(name) => Ok(Config::load_runtime_profile_with_environment(
            name,
            environment,
        )?),
        ProfileInput::Path(path) => Ok(Config::load_from_path_with_environment(path, environment)?),
    }
}

/// Lenient profile loader for runtime-facing and editor flows that should
/// keep working even when the Server block is present but not fully
/// provisioned.
pub(crate) fn load_profile_with_env_lenient<'a, P>(
    profile: P,
    environment: Option<&str>,
) -> Result<Config>
where
    P: Into<ProfileInput<'a>>,
{
    match profile.into() {
        ProfileInput::Runtime(name) => Ok(Config::load_runtime_profile_with_environment_lenient(
            name,
            environment,
        )?),
        ProfileInput::Path(path) => Ok(Config::load_from_path_with_environment_lenient(
            path,
            environment,
        )?),
    }
}

/// Render an envelope in the requested output format. `output` is constrained
/// by clap (value_parser) to text/json/yaml/table, so the final arm handles
/// text — the default and the explicit `text`.
fn format_envelope(envelope: &Envelope, output: &str) -> Result<String> {
    match output {
        "json" => Ok(serde_json::to_string_pretty(envelope)?),
        "yaml" => Ok(serde_yaml::to_string(envelope)
            .map_err(|e| anyhow!("failed to serialize envelope to yaml: {e}"))?),
        "table" => {
            // Table mode is text-mode rendering but only for list-shaped data.
            // For non-list data we still render text (graceful) rather than
            // erroring — the operator may have piped through `ayx ... --output
            // table` in a script and we don't want to break their pipeline.
            Ok(render::render_text(envelope))
        }
        // Default and explicit text.
        _ => Ok(render::render_text(envelope)),
    }
}

/// Map an `ErrorCode` to a one-line operator hint. `None` means no hint
/// is added; the error message stands on its own.
fn hint_for_error_code(code: ayx_core::envelope::ErrorCode) -> Option<&'static str> {
    use ayx_core::envelope::ErrorCode::*;
    match code {
        ConfigMissing => Some(
            "Run 'ayx onboard' to set up a profile, or 'ayx doctor config' to inspect the current one.",
        ),
        AuthFailed => Some(
            "Run 'ayx doctor auth' to inspect auth posture. Re-run 'ayx one login' if tokens are stale.",
        ),
        PermissionDenied => Some(
            "Check that the active profile's token has the required role/scope for this resource.",
        ),
        NotFound => Some(
            "Verify the id is correct. Use 'ayx <surface> list' to enumerate available resources.",
        ),
        Validation => Some(
            "Inspect the failed flag or input; '--help' on the subcommand documents accepted values.",
        ),
        Conflict => Some(
            "Resource is in a conflicting state. Inspect the current state with the detail command, then retry.",
        ),
        RateLimited => {
            Some("Retry after the suggested delay; consider --max-pages to bound auto-pagination.")
        }
        Network => Some(
            "Run 'ayx doctor network' to diagnose connectivity. Check VPN/proxy if applicable.",
        ),
        Upstream => Some(
            "Upstream returned a 5xx. Retry; if it persists, escalate to the Alteryx One status page.",
        ),
        WorkspaceMismatch => Some(
            "Re-authenticate against the expected workspace, or unset alteryx_one.expected_workspace_id.",
        ),
        Internal => None,
    }
}

/// Best-effort classification of an anyhow error chain into an `ErrorCode`.
///
/// Heuristic: substring-match against the rendered error chain. We prefer
/// being slightly imprecise here over panicking on classification; the
/// rendered `message` field still carries the full text for humans, and
/// future code paths should build typed errors using `ErrorCode::*` directly
/// rather than relying on this fallback.
fn classify_anyhow_error(err: &anyhow::Error) -> ErrorCode {
    let chain = err
        .chain()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    if chain.contains("workspace mismatch") {
        return ErrorCode::WorkspaceMismatch;
    }
    if chain.contains("config missing")
        || chain.contains("missing field")
        || chain.contains("profile")
            && (chain.contains("not found") || chain.contains("does not exist"))
    {
        return ErrorCode::ConfigMissing;
    }
    if chain.contains("unauthorized")
        || chain.contains("401")
        || chain.contains("invalid_grant")
        || chain.contains("token")
            && (chain.contains("expired") || chain.contains("missing") || chain.contains("invalid"))
    {
        return ErrorCode::AuthFailed;
    }
    if chain.contains("forbidden") || chain.contains("403") || chain.contains("permission denied") {
        return ErrorCode::PermissionDenied;
    }
    if chain.contains("not found") || chain.contains("404") {
        return ErrorCode::NotFound;
    }
    if chain.contains("conflict") || chain.contains("409") {
        return ErrorCode::Conflict;
    }
    if chain.contains("rate limit") || chain.contains("429") {
        return ErrorCode::RateLimited;
    }
    if chain.contains("timed out")
        || chain.contains("timeout")
        || chain.contains("connection refused")
        || chain.contains("dns")
        || chain.contains("tls")
        || chain.contains("connect error")
        || chain.contains("network")
    {
        return ErrorCode::Network;
    }
    // Status-code signals win over body-keyword heuristics: a 5xx whose body
    // happens to contain a validation phrase (e.g. "client_id is required") is
    // an upstream fault, not a client-side validation error.
    if chain.contains("500")
        || chain.contains("502")
        || chain.contains("503")
        || chain.contains("504")
    {
        return ErrorCode::Upstream;
    }
    if chain.contains("validation")
        || chain.contains("invalid value")
        || chain.contains("invalid format")
        || chain.contains("cannot be empty")
        || chain.contains("is required")
    {
        return ErrorCode::Validation;
    }
    ErrorCode::Internal
}

#[cfg(feature = "ui")]
pub(crate) fn ui_command_envelope(page: &str, command: &str, data: Value) -> Value {
    json!({
        "page": page,
        "command": command,
        "page_policy": {
            "foreground_tabs": "sticky and user-visible",
            "background_pages": "allowed for read-only refresh and validation",
            "mutations": "serialized per page kind",
        },
        "data": data,
    })
}

pub(crate) fn build_auth_status(
    config: &Config,
    metadata_url: Option<&str>,
    metadata_file: Option<&Path>,
    acs_url: Option<&str>,
    issuer: Option<&str>,
) -> Value {
    json!({
        "server_profile": config.profile_name,
        "server": config.server.as_ref().map(|s| json!({
            "webapi_url": s.webapi_url,
            "verify_tls": s.verify_tls(),
        })),
        "metadata": {
            "url": metadata_url,
            "file": metadata_file.map(|p| p.display().to_string()),
        },
        "expected_endpoints": {
            "acs_url": acs_url,
            "issuer": issuer,
        },
        "log_families": discover_log_inventory(config),
    })
}

pub(crate) fn parse_saml_metadata_source(input: &str) -> Result<Value> {
    let raw = if let Some(url) = input.strip_prefix("metadata_url=") {
        let client = Client::builder()
            .danger_accept_invalid_certs(false)
            .build()
            .context("failed to build metadata client")?;
        let response = client
            .get(url)
            .send()
            .context("failed to fetch SAML metadata url")?
            .error_for_status()
            .context("failed to fetch SAML metadata url")?;
        response
            .text()
            .context("failed to read SAML metadata response")?
    } else {
        let path = Path::new(input);
        if path.exists() {
            fs::read_to_string(path).with_context(|| {
                format!("failed to read SAML metadata file '{}'", path.display())
            })?
        } else {
            input.to_string()
        }
    };

    let doc = Document::parse(&raw).context("failed to parse SAML metadata xml")?;
    let entity = doc
        .descendants()
        .find(|n: &roxmltree::Node<'_, '_>| n.has_tag_name("EntityDescriptor"))
        .or_else(|| {
            doc.descendants()
                .find(|n: &roxmltree::Node<'_, '_>| n.has_tag_name("EntitiesDescriptor"))
        });
    let issuer = entity
        .and_then(|n: roxmltree::Node<'_, '_>| n.attribute("entityID"))
        .map(ToOwned::to_owned);
    let sso_urls: Vec<String> = doc
        .descendants()
        .filter(|n: &roxmltree::Node<'_, '_>| n.has_tag_name("SingleSignOnService"))
        .filter_map(|n: roxmltree::Node<'_, '_>| n.attribute("Location"))
        .map(ToOwned::to_owned)
        .collect();
    let certs: Vec<String> = doc
        .descendants()
        .filter(|n: &roxmltree::Node<'_, '_>| n.has_tag_name("X509Certificate"))
        .filter_map(|n: roxmltree::Node<'_, '_>| n.text())
        .map(|s: &str| s.trim().to_string())
        .filter(|s: &String| !s.is_empty())
        .collect();
    Ok(json!({
        "issuer": issuer,
        "single_sign_on_services": sso_urls,
        "certificate_count": certs.len(),
        "has_certificate": !certs.is_empty(),
    }))
}
