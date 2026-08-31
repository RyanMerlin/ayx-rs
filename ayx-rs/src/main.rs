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
use ayx_core::observability::{redact_text, transport_error_summary};
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
mod output;
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

/// Read automation input without accepting secrets on the process command line.
fn read_secret_stdin() -> Result<String> {
    use std::io::Read;

    let mut value = String::new();
    io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut value)
        .context("failed to read secret from standard input")?;
    if value.len() > 64 * 1024 {
        bail!("secret input exceeds the 64 KiB safety limit");
    }
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        bail!("secret input cannot be empty");
    }
    Ok(value)
}

/// Interactive secret entry deliberately uses the terminal's no-echo facility.
fn read_secret_prompt(slot: &str) -> Result<String> {
    let value = rpassword::prompt_password(format!("Secret for {slot}: "))
        .context("failed to read secret from terminal")?;
    if value.is_empty() {
        bail!("secret input cannot be empty");
    }
    Ok(value)
}

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

fn auth_token_health(access_token: Option<&str>) -> &'static str {
    let expires_at = access_token
        .and_then(decode_token_claims)
        .and_then(|claims| claims.get("exp").and_then(Value::as_i64));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    match ayx_core::auth::credential_health(expires_at, now) {
        ayx_core::auth::CredentialHealth::Fresh => "fresh",
        ayx_core::auth::CredentialHealth::Stale => "stale",
        ayx_core::auth::CredentialHealth::UnknownExpiry => "unknown_expiry",
    }
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
    /// Output format for the result. Put this after the complete command path,
    /// for example: `ayx one flows list --output json`.
    #[arg(
        long,
        default_value_t = output::OutputMode::Text,
        global = true
    )]
    output: output::OutputMode,
    /// Universal One workspace selector: numeric ID, GID, or exact saved name.
    #[arg(long, global = true)]
    workspace: Option<String>,
    /// Refuse all interactive input. Login then requires explicit token-based
    /// credentials and ambiguous workspace selections fail closed.
    #[arg(long, global = true)]
    no_input: bool,
    /// Preferred per-page size for list requests. `--limit` remains supported
    /// on individual commands as a deprecated compatibility alias.
    #[arg(long, global = true)]
    page_size: Option<u32>,
    /// Format command errors independently from successful command output.
    #[arg(long, value_enum, default_value_t = output::ErrorFormat::Text, global = true)]
    error_format: output::ErrorFormat,
    /// Maximum list rows in text and compact JSON output; 0 shows every
    /// projected row. Does not affect `--output json-full`.
    #[arg(long, default_value_t = output::DEFAULT_OUTPUT_LIMIT, global = true)]
    output_limit: usize,
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

/// Family-owned display metadata. The central output module owns rendering;
/// this mapping makes the command family's intended presentation explicit.
fn output_descriptor(command: &Command) -> output::OutputDescriptor {
    use output::{OutputDescriptor, ViewKind};
    match command {
        Command::Discover { .. } => OutputDescriptor::new("discover", ViewKind::Raw)
            .with_fields(&["schema_version", "binary"]),
        Command::Catalog { .. } => OutputDescriptor::new("catalog", ViewKind::Raw),
        Command::Doctor { .. } => OutputDescriptor::new("doctor", ViewKind::Diagnostic),
        Command::Telemetry { .. } => OutputDescriptor::new("telemetry", ViewKind::List),
        Command::Actions { command } => match command {
            ActionsCommand::List { .. } | ActionsCommand::Resolve { .. } => {
                OutputDescriptor::new("actions", ViewKind::List)
            }
            ActionsCommand::Workflows {
                command: WorkflowsCommand::List { .. },
            } => OutputDescriptor::new("actions.workflows", ViewKind::List),
            ActionsCommand::Export { .. } => {
                OutputDescriptor::new("actions.export", ViewKind::Export)
            }
            _ => OutputDescriptor::new("actions", ViewKind::Result),
        },
        Command::Completions { .. } => OutputDescriptor::new("completions", ViewKind::Export)
            .with_fields(&["shell", "bytes", "usage_hint"]),
        Command::Tui => OutputDescriptor::new("tui", ViewKind::Raw),
        Command::One { command } => cmd::one::output_descriptor(command),
        Command::Server { .. } => OutputDescriptor::new("server", ViewKind::Raw),
        Command::Mongo { .. } => OutputDescriptor::new("mongo", ViewKind::Raw),
        Command::Sqlserver { .. } => OutputDescriptor::new("sqlserver", ViewKind::Raw),
        Command::Designer { .. } => OutputDescriptor::new("designer", ViewKind::Raw),
        Command::Tools { .. } => OutputDescriptor::new("tools", ViewKind::Result),
        Command::Profile { .. } => OutputDescriptor::new("profile", ViewKind::Result),
        Command::Secret { .. } => OutputDescriptor::new("secret", ViewKind::Result),
        Command::License { .. } => OutputDescriptor::new("license", ViewKind::Raw),
        Command::Onboard { .. } => OutputDescriptor::new("onboard", ViewKind::Result),
        Command::Audit { .. } => OutputDescriptor::new("audit", ViewKind::Result),
        Command::Whoami { .. } => OutputDescriptor::new("whoami", ViewKind::Detail),
        Command::Update { .. } => OutputDescriptor::new("update", ViewKind::Result),
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
        about = "Alteryx Designer / Server artifact tooling — .yxmd/.yxmc/.yxzp/.yxdb",
        arg_required_else_help = true
    )]
    Designer {
        #[command(subcommand)]
        command: DesignerCommand,
    },
    #[command(
        about = "Server discovery, logs, auth, diagnose, doctor, upgrade, and low-level API calls",
        arg_required_else_help = true
    )]
    Server {
        #[command(subcommand)]
        command: ServerCommand,
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
        #[arg(long)]
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
        about = "Action registry — named playbooks with safety, validation, and rollback notes"
    )]
    Actions {
        #[command(subcommand)]
        command: ActionsCommand,
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
pub(crate) enum ActionsCommand {
    /// List every action, with title, safety classification, and tags.
    /// Compact index only — no input/output schema. Call `describe` on a
    /// candidate id for its full contract before constructing `--param`s.
    List {
        /// Filter by tag (substring match).
        #[arg(long)]
        tag: Option<String>,
        /// Filter by safety classification: read_only | mutating | destructive.
        #[arg(long)]
        safety: Option<String>,
    },
    /// Describe a single action: steps, validations, rollback, plus its
    /// effective `input_schema` (declared or inferred, tagged by
    /// `input_schema_source`) and declared `output_schema`, if any — the
    /// agent-facing source of truth for what this action requires/returns.
    Describe {
        /// Action id, e.g. `mongo.backup-restore`.
        id: String,
    },
    /// Resolve a free-text task description to a ranked list of candidate
    /// actions. Ranking/lookup only — no schema. Call `describe` on the
    /// chosen id for its full contract before constructing `--param`s.
    Resolve {
        /// The task description, e.g. "back up mongo before a migration".
        #[arg(long)]
        task: String,
        /// Cap returned hits.
        #[arg(long, default_value = "5")]
        limit: usize,
    },
    /// Execute an action. Without `--apply`, mutating/destructive actions
    /// emit a structured plan and never invoke a subprocess. Read-only
    /// actions always run.
    Run {
        /// Action id.
        id: String,
        /// Provide a placeholder value, e.g. `--param profile=prod`. Repeat
        /// for each placeholder referenced by the action.
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
        /// On a TTY, prompt interactively for any params that the action
        /// requires but were not provided via --param or --param-file.
        /// Always off on stdin redirection / CI (we detect TTY).
        #[arg(long)]
        prompt_missing: bool,
    },
    /// Cross-check every step in every loaded action against the catalog.
    /// Emits warnings for unknown command paths, capability ids, and
    /// dangling workflow → action references. Read-only.
    Validate,
    /// Workflow registry — higher-order skills composing actions.
    #[command(about = "Workflow registry — higher-order skills composing actions")]
    Workflows {
        #[command(subcommand)]
        command: WorkflowsCommand,
    },
    /// Print an action's full YAML so an operator can fork it into their
    /// config home (`${AYX_CONFIG_HOME}/registry/`) to override the bundled
    /// stdlib version.
    Export {
        /// Action id.
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum WorkflowsCommand {
    /// List every workflow with its title, safety, and action count.
    /// Compact index only — no input/output schema. Call `explain` on a
    /// candidate id for its full contract before constructing `--param`s.
    List {
        #[arg(long)]
        tag: Option<String>,
    },
    /// Explain a workflow: title, safety, ordered action ids with summaries,
    /// resolved/missing action detail, plus its effective `input_schema`
    /// (declared or inferred, tagged by `input_schema_source`) and declared
    /// `output_schema`, if any — the agent-facing source of truth for what
    /// this workflow requires/returns.
    Explain {
        /// Workflow id, e.g. `governance.go-live`.
        id: String,
    },
    /// Execute a workflow as an ordered chain of actions. Honors the same
    /// `--apply` semantics as `actions run`.
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
    fn shared_profile_loader_rejects_mismatched_bound_one_refs_but_reads_legacy_refs() {
        let mut config: Config = serde_yaml::from_str(
            r#"
profile_name: binding-test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
        )
        .expect("minimal config should parse");
        let wrong = ayx_core::auth::CredentialBinding::new(
            "tester@example.com",
            "https://other.example.test/as/token",
            "other",
            "https://other.example.test",
            None,
            None,
        )
        .expect("wrong binding should still be structurally valid");
        let one = ayx_core::profile::AlteryxOneProfile {
            account_email: "tester@example.com".to_string(),
            base_url: Some("https://us1.example.test".to_string()),
            token_endpoint_url: Some("https://us1.example.test/as/token".to_string()),
            access_token: Some("resolved-token".to_string()),
            access_token_ref: Some(format!(
                "keyring:{}",
                wrong.keyring_account("alteryx_one.access_token")
            )),
            ..Default::default()
        };
        config.alteryx_one = Some(one);

        let error = validate_loaded_auth_bindings(&config)
            .expect_err("generic profile loading must fail closed on a mismatched bound ref");
        assert!(error.to_string().contains("credential binding mismatch"));

        config
            .alteryx_one
            .as_mut()
            .expect("One profile")
            .access_token_ref = Some("keyring:binding-test/alteryx_one.access_token".to_string());
        validate_loaded_auth_bindings(&config)
            .expect("legacy profile-scoped refs remain readable for compatibility");
    }

    #[test]
    fn auth_summary_keeps_one_and_server_readiness_independent() {
        let (status, summary) = doctor_auth_status_summary(
            true, false, false, false, // One is configured but incomplete.
            true, true, true, // Server is fully configured.
        );

        assert_eq!(status, "warn");
        assert_eq!(summary, "One incomplete; Server configured");
        assert_eq!(auth_product_status(true, false), "incomplete");
        assert_eq!(auth_product_status(true, true), "configured");
        assert_eq!(auth_product_status(false, false), "not_configured");
    }

    /// `ayx-server-api` embeds the code it already computed as
    /// `error_code=<code>`; the dispatcher must read that rather than scanning
    /// prose. It previously did not: the prose scan looks for `"not found"`
    /// (space) while the token is `not_found` (underscore), so a Server-side
    /// 404 was classified only by accident of the body text. The 410 scream
    /// test now carries its own structured code (`gone`) instead of collapsing
    /// into `not_found`.
    #[test]
    fn classify_reads_the_structured_error_code_from_server_api() {
        let gone = anyhow::anyhow!(
            "api request failed [GET] status=410 code=http_error error_code=gone \
             url=https://example/v3/workflows/1 body={{\"message\":\"resource retired\"}}"
        );
        assert_eq!(classify_anyhow_error(&gone), ErrorCode::Gone);

        // No prose anywhere says "conflict"; only the structured token does.
        let conflict = anyhow::anyhow!(
            "api request failed [POST] status=412 code=http_error error_code=conflict \
             url=https://example/v3/x body={{\"m\":\"precondition\"}}"
        );
        assert_eq!(classify_anyhow_error(&conflict), ErrorCode::Conflict);

        // An unparseable token must not hijack the prose fallback.
        let bogus = anyhow::anyhow!("api request failed error_code=not_a_real_code 404 not found");
        assert_eq!(classify_anyhow_error(&bogus), ErrorCode::NotFound);
    }

    #[test]
    fn datasets_list_defaults_to_all_and_accepts_comma_or_repeat_forms() {
        let defaulted = Cli::try_parse_from(["ayx", "one", "datasets", "list"])
            .expect("datasets list should parse with the default filter");
        let Command::One {
            command:
                OneCommand::Datasets {
                    command:
                        OneDatasetsCommand::List {
                            datasets_filter, ..
                        },
                },
        } = defaulted.command
        else {
            panic!("expected one datasets list");
        };
        assert_eq!(datasets_filter, vec![DatasetFilter::All]);

        let comma = Cli::try_parse_from([
            "ayx",
            "one",
            "datasets",
            "list",
            "--datasets-filter",
            "imported,reference",
        ])
        .expect("comma-separated filters should parse");
        let Command::One {
            command:
                OneCommand::Datasets {
                    command:
                        OneDatasetsCommand::List {
                            datasets_filter, ..
                        },
                },
        } = comma.command
        else {
            panic!("expected one datasets list");
        };
        assert_eq!(
            datasets_filter,
            vec![DatasetFilter::Imported, DatasetFilter::Reference]
        );

        let repeated = Cli::try_parse_from([
            "ayx",
            "one",
            "datasets",
            "list",
            "--datasets-filter",
            "recipe",
            "--datasets-filter",
            "all",
        ])
        .expect("repeated filters should parse");
        let Command::One {
            command:
                OneCommand::Datasets {
                    command:
                        OneDatasetsCommand::List {
                            datasets_filter, ..
                        },
                },
        } = repeated.command
        else {
            panic!("expected one datasets list");
        };
        assert_eq!(
            datasets_filter,
            vec![DatasetFilter::Recipe, DatasetFilter::All]
        );
    }

    #[test]
    fn datasets_list_rejects_unknown_filter_values() {
        let err = Cli::try_parse_from([
            "ayx",
            "one",
            "datasets",
            "list",
            "--datasets-filter",
            "bogus",
        ])
        .expect_err("invalid dataset filters should fail clap parsing");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

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
    fn input_contract_violation_classifies_as_validation() {
        // ayx-registry's ExecutorError::InputContractViolation ("action/workflow
        // '<id>' input contract violation — '/ts': missing required property
        // 'ts'") is caller-supplied bad input, same as a missing clap argument —
        // it must classify as Validation, not Internal, so an agent knows fixing
        // its params (not retrying blindly) is the right response.
        let err = anyhow::anyhow!(
            "action/workflow 'mongo.backup-restore' input contract violation — '/ts': missing required property 'ts'"
        );
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
        for fmt in ["text", "json", "json-full", "yaml", "table"] {
            let parsed = Cli::try_parse_from(["ayx", "--output", fmt, "profile", "current"]);
            assert!(parsed.is_ok(), "clap should accept --output {fmt}");
        }
    }

    #[test]
    fn accepts_global_output_after_subcommand() {
        let parsed =
            Cli::try_parse_from(["ayx", "one", "workspace", "current", "--output", "json"]);
        assert!(
            parsed.is_ok(),
            "global --output should work after the subcommand"
        );
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
    #[command(about = "Show secret source and resolution posture without returning secret values")]
    Status {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Validate configured secret references without making network requests")]
    Validate {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Store a named secret in the OS keyring, or attach an environment reference")]
    Set {
        #[arg(value_name = "SLOT")]
        slot: String,
        #[arg(long)]
        profile: Option<String>,
        /// Read the secret from standard input. Never pass secret values as command arguments.
        #[arg(long, conflicts_with = "from_env")]
        from_stdin: bool,
        /// Persist env:NAME without reading or storing the environment value.
        #[arg(long, value_name = "NAME", conflicts_with = "from_stdin")]
        from_env: Option<String>,
    },
    #[command(about = "Detach a named secret and safely remove its private keyring entry")]
    Unset {
        #[arg(value_name = "SLOT")]
        slot: String,
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Move supported plaintext profile secrets into the OS keyring")]
    Migrate {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Print a non-secret environment-variable template for automation")]
    EnvTemplate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "dotenv", value_parser = ["dotenv", "json"])]
        format: String,
    },
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
    #[command(
        about = "Apply a guarded, template-based Mongo mutation with mandatory preview approval."
    )]
    Mutate {
        #[arg(long)]
        profile: Option<String>,
        /// Named executable mutation template from the remediation registry
        /// (`knowledge/mongo/mutations.yaml`). Free-form filter/update is not
        /// supported for live preview/apply — only a reviewed, capped
        /// template can execute a write.
        #[arg(long)]
        template: Option<String>,
        /// Bind a template parameter, e.g. `--param new_email=a@b.com`. May
        /// be repeated. Unknown or duplicate keys are rejected.
        #[arg(long, value_name = "KEY=VALUE")]
        param: Vec<String>,
        /// Render the resolved mongosh invocation without querying the
        /// database or writing an audit artifact. Mutually exclusive with
        /// the apply flags below.
        #[arg(
            long,
            conflicts_with_all = ["apply", "backup_audit_artifact", "approval_artifact", "approve"]
        )]
        print: bool,
        /// Execute the mutation live. Requires --accept-mutation-risk,
        /// --backup-audit-artifact, --approval-artifact, and --approve
        /// together — missing pieces are reported all at once.
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        accept_mutation_risk: bool,
        /// Path to a current, successful `mongo backup` audit artifact
        /// proving a backup exists before this mutation runs.
        #[arg(long, value_name = "PATH")]
        backup_audit_artifact: Option<PathBuf>,
        /// Path to the audit artifact this command wrote on a prior
        /// (non-apply) preview run — proves a human reviewed the exact
        /// candidate diff before approving it.
        #[arg(long, value_name = "PATH")]
        approval_artifact: Option<PathBuf>,
        /// The `sha256:` approval digest printed by the preview run, copied
        /// verbatim to prove the operator reviewed that specific diff.
        #[arg(long, value_name = "DIGEST")]
        approve: Option<String>,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
    #[command(about = "Run the default support query suite across critical Mongo collections.")]
    Doctor {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(about = "Reverse a prior guarded Mongo mutation from its execution audit artifact.")]
    Undo {
        #[arg(long)]
        profile: Option<String>,
        /// The execution audit artifact written by the `mongo mutate --apply`
        /// run being reversed.
        #[arg(long, value_name = "PATH")]
        mutation_audit_artifact: PathBuf,
        /// Render the resolved undo mongosh invocation without querying the
        /// database or writing an audit artifact. Mutually exclusive with
        /// the apply flags below.
        #[arg(
            long,
            conflicts_with_all = ["apply", "approval_artifact", "approve"]
        )]
        print: bool,
        /// Execute the undo live. Requires --accept-mutation-risk,
        /// --approval-artifact, and --approve together.
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        accept_mutation_risk: bool,
        /// Path to the audit artifact this command wrote on a prior
        /// (non-apply) undo preview run — proves a human reviewed the exact
        /// restore diff before approving it.
        #[arg(long, value_name = "PATH")]
        approval_artifact: Option<PathBuf>,
        /// The `sha256:` approval digest printed by the undo preview run,
        /// mirroring `mongo mutate --approve`.
        #[arg(long, value_name = "DIGEST")]
        approve: Option<String>,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
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

#[derive(Subcommand, Debug)]
pub(crate) enum DesignerCommand {
    /// Workflow package and XML tooling for .yxmd, .yxmc, .yxzp, and .yxdb
    #[command(subcommand)]
    Workflow(WorkflowCommand),
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
    #[command(
        about = "(preview) Resolve and summarize both workspace profiles — comparison not yet implemented"
    )]
    Compare {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    #[command(
        about = "(preview) Resolve and summarize both workspace profiles — workflow migration not yet implemented"
    )]
    MigrateWorkflows {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    #[command(
        about = "(preview) Resolve and summarize both workspace profiles — DCM connection checks not yet implemented"
    )]
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
    /// Default (no flags): Wizard email OTP flow — sends a one-time passcode
    /// to your account email address, then completes the Alteryx One OIDC
    /// workspace handshake via a pure-HTTP reqwest flow (no browser or Python
    /// required). Use --auth-flow legacy for the compatibility rollback lane.
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
        /// Regional Alteryx One API base URL for this login (overrides the profile value).
        #[arg(long)]
        base_url: Option<String>,
        /// Workspace id to bind these credentials to (key in workspace_credentials).
        #[arg(long)]
        workspace_id: Option<String>,
        /// Workspace ULID (gid) — stored as workspace_gid for SP scope.
        #[arg(long)]
        workspace_gid: Option<String>,
        /// Authentication flow for email-OTP login: wizard (default) or legacy.
        #[arg(long, value_name = "FLOW", value_parser = ["wizard", "legacy"])]
        auth_flow: Option<String>,
        /// Save the workspace password without the interactive secure-save prompt.
        #[arg(long)]
        save_workspace_password: bool,
        /// Credential persistence: secure (default) or plaintext (explicit fallback).
        /// Session-only credentials are not usable by this standalone command.
        #[arg(long, value_name = "POLICY", value_parser = ["secure", "plaintext"])]
        secret_policy: Option<String>,
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
        about = "Alteryx One cloud-native workflows — list, inspect, copy, share, and delete",
        long_about = "Alteryx One cloud-native workflows — list, inspect, copy, share, and delete.\n\n\
                      These are the Alteryx One canvas workflows (the \
                      cloud-native/workflows/{id} web path), identified by ULIDs and served \
                      by /svc-workflow. They are NOT `one flows`, which is the Designer \
                      Cloud /v4/flows family keyed by integer ids — a workspace can hold \
                      dozens of cloud-native workflows while `one flows list` returns none.",
        arg_required_else_help = true
    )]
    Workflows {
        #[command(subcommand)]
        command: OneWorkflowsCommand,
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
        about = "Alteryx One schedules — create, inspect, and manage",
        long_about = "Alteryx One schedules — create, inspect, and manage. Note: the Scheduling \
                      API requires an enterprise-tier workspace — returns 404 on some workspace \
                      tiers.",
        arg_required_else_help = true
    )]
    Scheduling {
        #[command(subcommand)]
        command: OneSchedulingCommand,
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
    /// Create a One workspace from a JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a One workspace.
    Delete {
        #[arg(value_name = "ID")]
        id: String,
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
    /// List groups in a One workspace.
    Groups {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: Option<String>,
    },
    /// List groups visible to the current One user.
    GroupsGlobal,
    /// Create a group in a One workspace from a JSON payload.
    CreateGroup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Delete a group from a One workspace.
    DeleteGroup {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "GROUP-ID")]
        group_id: String,
    },
    /// Update a One workspace group from a JSON payload.
    UpdateGroup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "GROUP-ID")]
        group_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Set roles for a One workspace group from a JSON payload.
    SetGroupRoles {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "GROUP-ID")]
        group_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Add users to a One workspace group.
    AddGroupUsers {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "GROUP-ID")]
        group_id: String,
        #[arg(long = "user-id", value_name = "USER-ID", required = true)]
        user_ids: Vec<String>,
    },
    /// Remove users from a One workspace group.
    RemoveGroupUsers {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "GROUP-ID")]
        group_id: String,
        #[arg(long = "user-id", value_name = "USER-ID", required = true)]
        user_ids: Vec<String>,
    },
    /// Get the invitation link for a person in a One workspace.
    InvitationLink {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(long, value_name = "PERSON-ID")]
        person_id: String,
    },
    /// Get workspace cloud configuration records.
    CloudConfigs {
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
    },
    /// Select which authenticated workspace is active for this profile.
    Switch {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "TARGET")]
        id: Option<String>,
    },
    /// Invite users to a One workspace.
    InviteUsers {
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Invite a single user to a One workspace from a JSON payload.
    Invite {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Invite a list of users to a One workspace from a JSON payload.
    InviteList {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Reinvite workspace users from a JSON payload.
    ReinviteUsers {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
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
    /// Suspend one workspace user.
    SuspendUser {
        #[arg(long)]
        workspace_id: Option<String>,
        #[arg(value_name = "PERSON-ID")]
        person_id: String,
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
    /// Create workspace cloud configuration from a JSON payload.
    CreateCloudConfig {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "CLOUD-PROVIDER")]
        cloud_provider: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Update workspace cloud configuration from a JSON payload.
    UpdateCloudConfig {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "CLOUD-PROVIDER")]
        cloud_provider: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Patch a workspace user from a JSON payload.
    PatchUser {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "PERSON-ID")]
        person_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Replace a workspace user from a JSON payload.
    UpdateUser {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "WORKSPACE-ID")]
        workspace_id: String,
        #[arg(value_name = "PERSON-ID")]
        person_id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneRoleCommand {
    /// List managed IAM roles.
    List,
    /// Inspect a managed IAM role.
    Detail {
        #[arg(value_name = "ID")]
        id: String,
    },
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
    /// Diff the live One OpenAPI spec against wired commands (covered / missing / stale).
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
    /// Validate a versioned, secret-free agent authentication request.
    Protocol {
        /// JSON request file, or `-` to read JSON from stdin.
        #[arg(long, value_name = "FILE")]
        request: std::path::PathBuf,
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
    /// List One flows (flat — no folder structure; see `flows library` for a folder-aware view).
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
    /// Count One flows (flat — see `flows library count` for a breakdown that includes folders).
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    #[command(arg_required_else_help = true)]
    /// Browse the One flow library: flows AND their containing folders together, unlike the flat `flows list`/`flows count` (list, count).
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
        #[arg(
            long = "datasets-filter",
            value_enum,
            value_delimiter = ',',
            action = clap::ArgAction::Append,
            default_values_t = [DatasetFilter::All]
        )]
        datasets_filter: Vec<DatasetFilter>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count datasets in the user-facing One dataset library.
    Count {
        #[arg(long)]
        profile: Option<String>,
        #[arg(
            long = "datasets-filter",
            value_enum,
            value_delimiter = ',',
            action = clap::ArgAction::Append
        )]
        datasets_filter: Vec<DatasetFilter>,
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
    /// List the One flow library — a folder-aware view combining flows and folders, unlike the flat `flows list`.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Count the One flow library — returns separate flow/folder/total counts, unlike the flat `flows count`.
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
    /// List the people and groups a One connection is shared with.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Share a One connection with people or groups.
    #[command(alias = "share")]
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        /// Access level to grant. Required unless --body is used.
        #[arg(long, value_enum)]
        policy: Option<ConnectionSharePolicy>,
        /// Person id to share with. Repeatable.
        #[arg(long = "to-person", value_name = "PERSON-ID", action = clap::ArgAction::Append)]
        to_person: Vec<String>,
        /// Group id to share with. Repeatable.
        #[arg(long = "to-group", value_name = "GROUP-ID", action = clap::ArgAction::Append)]
        to_group: Vec<String>,
        #[arg(
            long,
            value_name = "FILE",
            help = "path to JSON body file",
            conflicts_with_all = ["policy", "to_person", "to_group"]
        )]
        body: Option<PathBuf>,
    },
    /// Inspect one subject's access to a One connection.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTION-ID")]
        connection_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
    },
    /// Revoke a subject's access to a One connection.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "CONNECTION-ID")]
        connection_id: String,
        #[arg(value_name = "SUBJECT-ID")]
        subject_id: String,
        /// Whether the subject id names a person or a group.
        #[arg(long, value_enum, default_value_t = ShareSubjectType::Person)]
        subject_type: ShareSubjectType,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWorkflowsCommand {
    /// List Alteryx One cloud-native workflows.
    List {
        #[arg(long)]
        profile: Option<String>,
        /// Cap results per page (server-side limit). Default is the server's own
        /// page size (25 for /v4/workflows).
        #[arg(long)]
        limit: Option<u32>,
        /// Fetch a specific page; pass the `nextPageToken` returned by a previous call.
        #[arg(long)]
        page_token: Option<String>,
        /// Automatically follow `nextPageToken` until all pages are fetched.
        /// Capped by `--max-pages` (default 50).
        #[arg(long)]
        all: bool,
        /// Hard cap on pages when `--all` is set. Prevents runaway loops against
        /// very large tenants.
        #[arg(long)]
        max_pages: Option<u32>,
    },
    /// Count cloud-native workflows in the workspace.
    Count {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Inspect one cloud-native workflow.
    #[command(alias = "describe")]
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        /// Also resolve the workflow's connections, datasets, and macros.
        #[arg(long)]
        include_dependencies: bool,
    },
    /// List the connections, datasets, and macros a workflow depends on.
    #[command(alias = "deps")]
    Dependencies {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List workflow assets with the richer svc-workflow projection.
    Assets {
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
    /// Show which execution engines a workflow can run on.
    Engines {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// List the tools available to cloud-native workflows.
    Tools {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Delete a cloud-native workflow. Irreversible — no known restore/trash endpoint exists.
    Delete {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Duplicate a cloud-native workflow.
    Copy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        /// Name for the copy.
        #[arg(long)]
        name: String,
        /// Source version to copy. Defaults to the workflow's current version.
        #[arg(long)]
        version: Option<u64>,
    },
    /// Share a cloud-native workflow with people or groups.
    Share {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        /// Recipient: an email address (resolved via GET /v4/people) or a
        /// numeric person id. Repeatable.
        #[arg(long = "to-person", value_name = "EMAIL|ID", action = clap::ArgAction::Append)]
        to_person: Vec<String>,
        /// Group id to share with. Repeatable.
        #[arg(long = "to-group", value_name = "GROUP-ID", action = clap::ArgAction::Append)]
        to_group: Vec<String>,
        /// Privilege to grant. Repeatable; required unless --body is used.
        #[arg(long = "privilege", value_enum, action = clap::ArgAction::Append)]
        privilege: Vec<WorkflowPrivilege>,
        /// Also share the workflow's connections and datasets in the same call.
        #[arg(long)]
        include_dependencies: bool,
        /// Notify recipients by email.
        #[arg(long)]
        send_email: bool,
        /// Optional note included with the share notification.
        #[arg(long)]
        message: Option<String>,
        /// Treat every --to-person value as an already-numeric person id and
        /// skip the GET /v4/people email-resolution lookup.
        #[arg(long)]
        no_resolve_emails: bool,
        #[arg(
            long,
            value_name = "FILE",
            help = "path to JSON body file",
            conflicts_with_all = [
                "to_person", "to_group", "privilege", "include_dependencies",
                "send_email", "message", "no_resolve_emails",
            ]
        )]
        body: Option<PathBuf>,
    },
}

/// Access level for a shared connection. Mirrors the `policy` enum of
/// `POST /v4/connections/share`.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionSharePolicy {
    Editor,
    Viewer,
}

impl ConnectionSharePolicy {
    /// The API enum is upper-case; clap renders the variants lower-case.
    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            ConnectionSharePolicy::Editor => "EDITOR",
            ConnectionSharePolicy::Viewer => "VIEWER",
        }
    }
}

/// Whether a share subject id names a person or a group. Required by
/// `DELETE /v4/connections/share`, which cannot infer it from the id alone.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareSubjectType {
    Person,
    Group,
}

impl ShareSubjectType {
    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            ShareSubjectType::Person => "person",
            ShareSubjectType::Group => "group",
        }
    }
}

/// Privilege grantable by `POST /svc-workflow/api/v2/workflows/{id}/share`.
///
/// A real clap `ValueEnum` so a typo (`--privilege raed`) is rejected by the
/// parser before any network call, rather than surfacing as an opaque 400 from
/// the service.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WorkflowPrivilege {
    Create,
    Delete,
    Execute,
    Read,
    Share,
    Update,
}

impl WorkflowPrivilege {
    /// The API's privilege strings are lower-case, matching clap's own
    /// rendering for these single-word variants — kept as an explicit
    /// function rather than relying on that coincidence.
    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            WorkflowPrivilege::Create => "create",
            WorkflowPrivilege::Delete => "delete",
            WorkflowPrivilege::Execute => "execute",
            WorkflowPrivilege::Read => "read",
            WorkflowPrivilege::Share => "share",
            WorkflowPrivilege::Update => "update",
        }
    }
}

/// Dataset-library filter accepted by `GET /v4/datasetLibrary` and
/// `GET /v4/datasetLibrary/count`.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatasetFilter {
    All,
    Imported,
    Reference,
    Recipe,
}

impl DatasetFilter {
    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            DatasetFilter::All => "all",
            DatasetFilter::Imported => "imported",
            DatasetFilter::Reference => "reference",
            DatasetFilter::Recipe => "recipe",
        }
    }
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
    /// Create a One schedule from a JSON payload.
    Create {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
    },
    /// Inspect a One schedule by id.
    Detail {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Update a One schedule from a JSON payload.
    Update {
        #[arg(long)]
        profile: Option<String>,
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "FILE", help = "path to JSON body file")]
        body: PathBuf,
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
    /// Delete a One schedule.
    Delete {
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
pub(crate) enum OneDoctorCommand {
    /// Run the One auth doctor workflow.
    Auth {
        #[arg(long)]
        profile: Option<String>,
        /// Migrate inline authentication secrets into the secure store.
        #[arg(long)]
        migrate: bool,
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

/// Which command records `catalog list`/`catalog describe` draw from.
///
/// `All` (the default) is every visible `ayx` command, derived live from the
/// clap tree — this can never omit a command that exists. `Curated` narrows
/// that same derived set down to records that also carry a
/// `CATALOG_METADATA` row (the legacy-compatible view: output/safety/
/// mutating/prerequisites/notes are all populated, not `unclassified`/null).
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogScope {
    All,
    Curated,
}

impl CatalogScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CatalogScope::All => "all",
            CatalogScope::Curated => "curated",
        }
    }
}

#[derive(Subcommand, Debug)]
enum CatalogCommand {
    #[command(about = "List machine-readable command metadata.")]
    List {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "compact")]
        format: String,
        /// Command records to include: `all` (every visible command, the
        /// default) or `curated` (only commands with semantic metadata).
        #[arg(long, value_enum, default_value_t = CatalogScope::All)]
        scope: CatalogScope,
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
    #[command(about = "(preview) Simulate an upgrade apply — no changes are made")]
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
            "preview": true,
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
            "preview": true,
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

fn execute(cli: Cli, output_mode: output::OutputMode) -> Result<Envelope> {
    // A few older command-family adapters still receive only `--yes`. Make
    // the global noninteractive policy visible to their shared confirmation
    // helper so no mutation can accidentally prompt when stdin is a TTY.
    if cli.no_input {
        // This process is single-threaded during dispatch; the variable is
        // removed by the process boundary and is never persisted.
        unsafe { std::env::set_var("AYX_NO_INPUT", "1") };
    }
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
    let (workspace, workspace_source) = if let Some(value) = cli.workspace.clone() {
        (
            Some(value),
            ayx_core::profile::WorkspaceResolutionSource::Cli,
        )
    } else if let Ok(value) = std::env::var("AYX_WORKSPACE") {
        (
            Some(value),
            ayx_core::profile::WorkspaceResolutionSource::Environment,
        )
    } else {
        (
            None,
            ayx_core::profile::WorkspaceResolutionSource::ActiveProfile,
        )
    };
    let load_profile = |profile: Option<&str>| -> Result<Config> {
        load_profile_with_env(profile, environment.as_deref())
    };
    let envelope = match cli.command {
        Command::Mongo { command } => {
            cmd::mongo::execute(environment.as_deref(), cli.yes, command)?
        }
        Command::Server { command } => cmd::server::execute(environment.as_deref(), command)?,
        Command::Sqlserver { command } => cmd::sqlserver::execute(environment.as_deref(), command)?,
        Command::Designer { command } => match command {
            DesignerCommand::Workflow(command) => {
                cmd::workflow::execute(environment.as_deref(), command)?
            }
        },
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
                workspace: workspace.as_deref(),
                workspace_source,
                no_input: cli.no_input,
                page_size: cli.page_size,
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
            // Completion scripts retain their direct text-mode output for
            // shell redirection. Structured modes return only an envelope.
            if output_mode == output::OutputMode::Text {
                print!("{}", script);
            }
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
            SecretCommand::Status { profile } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let report = secret::inspect_profile(Path::new(&resolution.resolved_profile_path))?;
                Envelope::ok_with_data(
                    "secret status",
                    json!({ "profile": resolution.selected_profile, "slots": report }),
                )
            }
            SecretCommand::Validate { profile } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let report = secret::inspect_profile(Path::new(&resolution.resolved_profile_path))?;
                let failures = report
                    .iter()
                    .filter(|entry| entry.validation == "error")
                    .count();
                let warnings = report
                    .iter()
                    .filter(|entry| entry.validation == "warning")
                    .count();
                if failures > 0 {
                    bail!(
                        "secret validation failed: {failures} unresolved or invalid reference(s); run `ayx secret status` for remediation"
                    );
                }
                let status = if warnings > 0 { "warning" } else { "passed" };
                Envelope::ok_with_data(
                    "secret validation completed",
                    json!({
                        "profile": resolution.selected_profile,
                        "status": status,
                        "failures": failures,
                        "warnings": warnings,
                        "slots": report,
                        "network_checked": false,
                    }),
                )
            }
            SecretCommand::Set {
                slot,
                profile,
                from_stdin,
                from_env,
            } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let input = match (from_stdin, from_env) {
                    (true, _) => secret::SecretInput::Stdin(read_secret_stdin()?),
                    (false, Some(name)) => secret::SecretInput::Environment(name),
                    (false, None) if cli.no_input => {
                        bail!(
                            "`--no-input` requires `--from-stdin` or `--from-env NAME` for `ayx secret set`"
                        );
                    }
                    (false, None) => secret::SecretInput::Prompt(read_secret_prompt(&slot)?),
                };
                let result =
                    secret::set_slot(Path::new(&resolution.resolved_profile_path), &slot, input)?;
                Envelope::ok_with_data(
                    "secret stored",
                    json!({ "profile": resolution.selected_profile, "slot": result.slot, "source": result.source, "reference_changed": true }),
                )
            }
            SecretCommand::Unset { slot, profile } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let result = secret::unset_slot(
                    Path::new(&resolution.resolved_profile_path),
                    &slot,
                    &ayx_profiles_dir()?,
                )?;
                Envelope::ok_with_data(
                    "secret removed",
                    json!({ "profile": resolution.selected_profile, "slot": result.slot, "keyring_entry_deleted": result.keyring_entry_deleted }),
                )
            }
            SecretCommand::Migrate { profile } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let output = secret::migrate_profile(Path::new(&resolution.resolved_profile_path))?;
                let migrated_slots = secret::migrated_slot_names(&output);
                Envelope::ok_with_data(
                    "secret migration completed",
                    json!({
                        "profile": resolution.selected_profile,
                        "migrated_fields": output,
                        // Compatibility alias retained for scripts introduced
                        // with the initial secret lifecycle release.
                        "migrated_slots": migrated_slots,
                    }),
                )
            }
            SecretCommand::EnvTemplate { profile, format } => {
                let resolution = resolve_runtime_profile(profile.as_deref())?;
                let template = secret::env_template(Path::new(&resolution.resolved_profile_path))?;
                if format == "json" {
                    Envelope::ok_with_data(
                        "secret environment template",
                        json!({ "profile": resolution.selected_profile, "variables": template }),
                    )
                } else {
                    Envelope::ok_with_data(
                        "secret environment template",
                        json!({ "profile": resolution.selected_profile, "format": "dotenv", "content": template.iter().map(|name| format!("{name}=")).collect::<Vec<_>>().join("\n") }),
                    )
                }
            }
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
        Command::Actions { command } => cmd::registry::execute_actions(cli.apply, command)?,
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
        "/v4/plans",
        false,
        &[],
    )?;
    let schedules = one_api_live_request(
        config,
        "scheduling",
        "discover-schedules-list",
        "GET",
        "/v4/schedules",
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
            ],
            "recommendations": [
                "Use one workspace current to identify the workspace context",
                "Use one plans list/detail/run to resolve plan ids",
                "Use one scheduling list/detail/enable/disable to resolve schedule ids",
                "Use the workflow guidance layer to decide whether a symptom belongs to identity, plans, or scheduling",
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
        "/v4/plans",
        false,
        &[],
    )?;
    let count = one_api_live_request(
        config,
        "plans",
        "doctor-plans-count",
        "GET",
        "/v4/plans/count",
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
        "/v4/schedules",
        false,
        &[],
    )?;
    let count = one_api_live_request(
        config,
        "scheduling",
        "doctor-schedules-count",
        "GET",
        "/v4/schedules/count",
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
    let config = load_profile_with_env(name, None)?;
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
    let secret_posture = secret::inspect_profile(Path::new(&resolution.resolved_profile_path))
        .map(|slots| json!({ "available": true, "slots": slots }))
        .unwrap_or_else(|_| {
            json!({
                "available": false,
                "status": "unavailable",
                "hint": "run `ayx secret validate` for reference remediation"
            })
        });
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
            "secret_posture": secret_posture,
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
    let secret_posture = secret::inspect_profile(profile)
        .map(|slots| json!({ "available": true, "slots": slots }))
        .unwrap_or_else(|_| {
            json!({
                "available": false,
                "status": "unavailable",
                "hint": "run `ayx secret validate` for reference remediation"
            })
        });
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
            "secret_posture": secret_posture,
            "fix_applied": fix,
            "status": status,
            "summary": summary,
        }),
    ))
}

fn doctor_auth_envelope(profile: Option<&str>, environment: Option<&str>) -> Result<Envelope> {
    let config = load_profile_with_env(profile, environment)?;
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
    let one_status = auth_product_status(
        one_configured,
        one_access_token_present && one_refresh_token_present && one_oauth_client_id_present,
    );
    let server_status = auth_product_status(
        server_configured,
        server_api_key_present && server_api_secret_present,
    );
    Ok(Envelope::ok_with_data(
        "doctor auth completed",
        json!({
            "profile": config.profile_name,
            "status": status,
            "summary": summary,
            "one_status": one_status,
            "server_status": server_status,
            "inline_secret_fields": ayx_core::auth::inline_secret_fields(&config),
            "migration": {
                "available": !ayx_core::auth::inline_secret_fields(&config).is_empty(),
                "hint": "run `ayx one doctor auth --migrate` when secure storage is available"
            },
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
    validate_loaded_auth_bindings(&config)?;
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
    let one_status = auth_product_status(
        one_configured,
        one_access_token_present && one_refresh_token_present && one_oauth_client_id_present,
    );
    let server_status = auth_product_status(
        server_configured,
        server_api_key_present && server_api_secret_present,
    );
    Ok(Envelope::ok_with_data(
        "doctor auth completed",
        json!({
            "profile": config.profile_name,
            "status": status,
            "summary": summary,
            "one_status": one_status,
            "server_status": server_status,
            "inline_secret_fields": ayx_core::auth::inline_secret_fields(&config),
            "migration": {
                "available": !ayx_core::auth::inline_secret_fields(&config).is_empty(),
                "hint": "run `ayx one doctor auth --migrate` when secure storage is available"
            },
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
    let config = load_profile_with_env(profile, environment)?;
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
    let config = load_profile_with_env(profile, environment)?;
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
        let one_label = if !one_configured {
            "One not configured"
        } else if one_ready {
            "One configured"
        } else {
            "One incomplete"
        };
        let server_label = if !server_configured {
            "Server not configured"
        } else if server_ready {
            "Server configured"
        } else {
            "Server incomplete"
        };
        return ("warn", format!("{one_label}; {server_label}"));
    }

    let summary = match (one_configured, server_configured) {
        (true, true) => "One and Server auth configured",
        (true, false) => "One auth configured",
        (false, true) => "Server auth configured",
        (false, false) => "One and Server auth not configured",
    };
    ("ok", summary.to_string())
}

fn auth_product_status(configured: bool, ready: bool) -> &'static str {
    match (configured, ready) {
        (false, _) => "not_configured",
        (true, true) => "configured",
        (true, false) => "incomplete",
    }
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
            "credential_health": auth_token_health(access_token),
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
                    "credential_health": auth_token_health(access_token),
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
                "credential_health": auth_token_health(access_token),
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
            "credential_health": auth_token_health(access_token),
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
    let output = cli.output;
    let error_format = cli.error_format;
    let output_limit = cli.output_limit;
    let descriptor = output_descriptor(&cli.command);

    // Dispatch runs on the main thread. On Windows, build.rs reserves a 16 MiB
    // main-thread stack (/STACK) so the deep clap parse can't overflow the 1 MiB
    // MSVC default — see ayx-rs/build.rs and issue #59. No worker thread needed.
    let result = execute(cli, output);

    match result {
        Ok(envelope) => {
            let rendered = format_envelope(&envelope, output, descriptor, output_limit)?;
            if envelope.ok {
                print!("{rendered}");
                println!();
                let _ = io::stdout().lock().flush();
                let _ = io::stderr().lock().flush();
                std::process::exit(exit_code_for_envelope(&envelope));
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
                // anyhow error strings routinely embed upstream URLs and
                // response bodies; redact before they reach stderr.
                "error": redact_text(&err.to_string()),
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
                format_envelope(
                    &err_env,
                    if error_format == output::ErrorFormat::Json {
                        output::OutputMode::Json
                    } else {
                        output
                    },
                    descriptor,
                    output_limit,
                )
                .unwrap_or_else(|_| err_env.message.clone())
            );
            eprintln!();
            let _ = io::stdout().lock().flush();
            let _ = io::stderr().lock().flush();
            std::process::exit(exit_code_for_envelope(&err_env));
        }
    }
}

fn exit_code_for_envelope(envelope: &Envelope) -> i32 {
    if envelope.ok {
        return 0;
    }
    use ayx_core::envelope::ErrorCode::*;
    match envelope.error_code.unwrap_or(Internal) {
        Validation => 2,
        ConfigMissing | WorkspaceMismatch => 3,
        AuthFailed => 4,
        PermissionDenied => 5,
        NotFound | Gone | Conflict | RateLimited | Network | Upstream => 6,
        Incomplete => 7,
        OutputClassification | Internal => 70,
    }
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
fn validate_loaded_auth_bindings(config: &Config) -> Result<()> {
    if config.alteryx_one.is_none() {
        return Ok(());
    }
    let workspace_id = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.active_workspace_id());
    let binding = onboard::binding_for_auth_config(config, workspace_id)?;
    onboard::validate_auth_credential_bindings(config, &binding)
}

pub(crate) fn load_profile_with_env<'a, P>(profile: P, environment: Option<&str>) -> Result<Config>
where
    P: Into<ProfileInput<'a>>,
{
    let config = match profile.into() {
        ProfileInput::Runtime(name) => {
            Config::load_runtime_profile_with_environment(name, environment)?
        }
        ProfileInput::Path(path) => Config::load_from_path_with_environment(path, environment)?,
    };
    validate_loaded_auth_bindings(&config)?;
    Ok(config)
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
    let config = match profile.into() {
        ProfileInput::Runtime(name) => {
            Config::load_runtime_profile_with_environment_lenient(name, environment)?
        }
        ProfileInput::Path(path) => {
            Config::load_from_path_with_environment_lenient(path, environment)?
        }
    };
    validate_loaded_auth_bindings(&config)?;
    Ok(config)
}

pub(crate) fn load_profile_with_env_lenient_unvalidated<'a, P>(
    profile: P,
    environment: Option<&str>,
) -> Result<Config>
where
    P: Into<ProfileInput<'a>>,
{
    match profile.into() {
        ProfileInput::Runtime(name) => {
            Config::load_runtime_profile_with_environment_lenient(name, environment)
                .map_err(anyhow::Error::from)
        }
        ProfileInput::Path(path) => {
            Config::load_from_path_with_environment_lenient(path, environment)
                .map_err(anyhow::Error::from)
        }
    }
}

/// Render an envelope through the central output contract.
fn format_envelope(
    envelope: &Envelope,
    output: output::OutputMode,
    descriptor: output::OutputDescriptor,
    output_limit: usize,
) -> Result<String> {
    output::render_envelope(envelope, output, descriptor, output_limit)
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
        Gone => Some(
            "The upstream endpoint or resource was removed. Recheck the docs or switch to a list-based workflow if one exists.",
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
        Incomplete => Some(
            "The response is partial. Resume with the returned next_page_token or raise --max-pages.",
        ),
        OutputClassification => {
            Some("Use --output json-full to inspect the sanitized upstream envelope.")
        }
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
    // Prefer a classification that was already computed over re-guessing it
    // from prose. `ayx-server-api` bails with `... error_code=<code> ...`,
    // derived from `ErrorCode::from_http_status`, and its comment says the
    // outer dispatcher picks that up. Nothing did: the scan below looks for
    // `"not found"` with a space while the embedded token is `not_found` with
    // an underscore. A Server-side 404 was classified only when the body
    // prose happened to say "not found"; a 410 scream test now carries `gone`
    // and should stay distinct from `not_found`.
    if let Some(code) = chain
        .split("error_code=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(ErrorCode::parse_code)
    {
        return code;
    }
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
    if chain.contains("gone") || chain.contains("410") {
        return ErrorCode::Gone;
    }
    if chain.contains("gone") || chain.contains("410") {
        return ErrorCode::Gone;
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
        // ayx-registry's `ExecutorError::InputContractViolation` — a declared
        // action/workflow's own `--param` map failed its input_schema (unknown
        // key, missing required key, enum/const mismatch, etc.), caught before
        // any step runs. This is caller-supplied bad input, exactly like the
        // "is required" clap-argument case above — an agent needs Validation
        // here (not Internal) to know retrying with the same params won't help.
        || chain.contains("input contract violation")
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
