use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use roxmltree::Document;
use serde_json::{json, Value};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_core::observability::transport_error_summary;
use ayx_core::profile::{
    ayx_config_home, ayx_profiles_dir, ayx_state_path, ayx_workspaces_dir, list_central_profiles,
    load_ayx_state, profile_resolution_detail, profile_shape_label, profile_storage_path,
    save_ayx_state, AyxState, Config, ServerProfile,
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
use self_update::backends::github::Update as GitHubUpdate;
use self_update::Status;

mod capability;
mod cmd;
mod onboard;
mod render;
mod tui;

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
                  See `ayx <command> --help` for branch-specific help, and `ayx catalog list` \
                  for the machine-readable command registry.",
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(long, default_value = "text")]
    output: String,
    #[arg(long)]
    environment: Option<String>,
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

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Alteryx One platform branch and API surface")]
    One {
        #[command(subcommand)]
        command: Option<OneCommand>,
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
    #[command(about = "SQL Server status, prechecks, connection helpers, and migration planning")]
    Sqlserver {
        #[command(subcommand)]
        command: Option<SqlserverCommand>,
    },
    #[command(about = "Workflow package and XML tooling for .yxmd, .yxmc, .yxzp, and .yxdb")]
    Workflow {
        #[command(subcommand)]
        command: Option<WorkflowCommand>,
    },
    #[command(about = "Cross-environment tools for environments.yaml source/target workflows")]
    Tools {
        #[command(subcommand)]
        command: Option<ToolsCommand>,
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
        about = "Interactive TUI for profile setup, One credentials, and connectivity checks"
    )]
    Tui,
    #[command(about = "Central profile registry and active profile management")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    #[command(about = "Run configuration, auth, network, and product health diagnostics")]
    Doctor {
        #[command(subcommand)]
        command: Option<DoctorCommand>,
        #[arg(long)]
        fix: bool,
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    #[command(about = "Licensing portal branch and API surface")]
    License {
        #[command(subcommand)]
        command: Option<LicenseCommand>,
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
    #[command(about = "Machine-readable command registry")]
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    #[command(about = "Generate shell completion scripts (bash, zsh, fish, powershell, elvish)")]
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    #[command(
        about = "Show active profile, account email, workspace, and environment in one shot."
    )]
    Whoami {
        /// Override profile path. Defaults to the central profile resolver.
        #[arg(long)]
        profile: Option<PathBuf>,
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
        about = "Operational telemetry: running jobs, run history, top workflows/plans, errors, weekly run-counts"
    )]
    Telemetry {
        #[command(subcommand)]
        command: cmd::telemetry::TelemetryCommand,
    },
    #[command(
        about = "Serve a local read-only operational web dashboard (overview, jobs, workflows)"
    )]
    Dashboard(cmd::dashboard::DashboardCommand),
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

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    List,
    Current,
    Show {
        name: Option<String>,
    },
    Use {
        name: String,
    },
    Path,
    Migrate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DoctorCommand {
    Config,
    Auth,
    Network,
    One,
    Server,
    Mongo,
    /// Run every applicable diagnostic in sequence and return one merged envelope.
    /// Surfaces per-check status (ok / warn / fail / skipped); skipped indicates
    /// the active profile doesn't have that surface configured.
    All,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MongoCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Backup {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long, default_value = "backups")]
        output_dir: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
    Restore {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        input_path: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, default_value = "audits")]
        audit_dir: PathBuf,
    },
    Query {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Mutate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Doctor {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerCommand {
    Api {
        #[command(subcommand)]
        command: ServerApiCommand,
    },
    SystemInfo {
        #[arg(long, default_value = "system_info.json")]
        output: PathBuf,
    },
    RuntimeSettings {
        #[arg(long, default_value = DEFAULT_RUNTIME_SETTINGS_PATH)]
        path: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    AyxPaths,
    ServerLogs {
        #[command(subcommand)]
        command: ServerLogsCommand,
    },
    Diagnose {
        #[command(subcommand)]
        command: ServerDiagnoseCommand,
    },
    Auth {
        #[command(subcommand)]
        command: ServerAuthCommand,
    },
    Doctor {
        #[command(subcommand)]
        command: ServerDoctorCommand,
    },
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    BackupPlan {
        #[arg(long)]
        backup_dir: PathBuf,
    },
    Backup {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Precheck {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        collation: Option<String>,
    },
    ValidateStrings {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    ConnectionString {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Migrate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        target_version: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Prepare {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        target_version: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum WorkflowCommand {
    Inspect {
        #[arg(long)]
        input: PathBuf,
    },
    Unpack {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    Validate {
        #[arg(long)]
        input: PathBuf,
    },
    Replace {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        find: String,
        #[arg(long)]
        replace: String,
        #[arg(long)]
        validate: bool,
    },
    Repackage {
        #[arg(long)]
        input_dir: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Recurse {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        rules: Option<PathBuf>,
        #[arg(long = "find")]
        find: Vec<String>,
        #[arg(long = "replace")]
        replace: Vec<String>,
        #[arg(long)]
        validate: bool,
    },
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
    ConvertCloud {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = false)]
        fail_on_unsupported: bool,
    },
    Publish {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Migrate {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
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
pub(crate) enum UiCommand {
    Session {
        #[command(subcommand)]
        command: Option<UiSessionCommand>,
    },
    Workflow {
        #[command(subcommand)]
        command: Option<UiWorkflowCommand>,
    },
    Data {
        #[command(subcommand)]
        command: Option<UiDataCommand>,
    },
    Library {
        #[command(subcommand)]
        command: Option<UiLibraryCommand>,
    },
    Schedules {
        #[command(subcommand)]
        command: Option<UiSchedulesCommand>,
    },
    Jobs {
        #[command(subcommand)]
        command: Option<UiJobsCommand>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiSessionCommand {
    Status,
    Ensure,
    Attach {
        #[arg(long)]
        tab: Option<String>,
    },
    Inventory,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiWorkflowCommand {
    Open {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        foreground: bool,
    },
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        foreground: bool,
    },
    Inventory {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        foreground: bool,
    },
    PaneConfig {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: Option<String>,
    },
    PaneResults {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: Option<String>,
    },
    ToolList {
        #[arg(long)]
        workflow_id: Option<String>,
    },
    ToolSelect {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: String,
    },
    ToolInspect {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        tool_id: String,
    },
    GraphGet {
        #[arg(long)]
        workflow_id: Option<String>,
    },
    GraphPut {
        #[arg(long)]
        workflow_id: Option<String>,
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiDataCommand {
    ListDatasets {
        #[arg(long)]
        foreground: bool,
    },
    DatasetDetail {
        #[arg(long)]
        dataset_id: String,
        #[arg(long)]
        foreground: bool,
    },
    DatasetPreview {
        #[arg(long)]
        dataset_id: String,
        #[arg(long)]
        foreground: bool,
    },
    Upload {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        foreground: bool,
    },
    ListConnections {
        #[arg(long)]
        foreground: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiLibraryCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiSchedulesCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiJobsCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ToolsCommand {
    Workspace {
        #[command(subcommand)]
        command: Option<ToolsWorkspaceCommand>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ToolsWorkspaceCommand {
    Init {
        #[arg(long, default_value = "environments.yaml")]
        output: PathBuf,
        #[arg(long, default_value = "dev")]
        active_environment: String,
        #[arg(long, default_value = "dev")]
        source_environment: String,
        #[arg(long, default_value = "prod")]
        target_environment: String,
    },
    Resolve {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    Compare {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    MigrateWorkflows {
        #[arg(long, default_value = "environments.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
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
    Platform {
        #[command(subcommand)]
        command: Option<OnePlatformCommand>,
    },
    Doctor {
        #[command(subcommand)]
        command: Option<OneDoctorCommand>,
    },
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Plans {
        #[command(subcommand)]
        command: Option<OnePlansCommand>,
    },
    Flows {
        #[command(subcommand)]
        command: Option<OneFlowsCommand>,
    },
    Connections {
        #[command(subcommand)]
        command: Option<OneConnectionsCommand>,
    },
    JobGroups {
        #[command(subcommand)]
        command: Option<OneJobGroupCommand>,
    },
    OutputObjects {
        #[command(subcommand)]
        command: Option<OneOutputObjectCommand>,
    },
    WebhookFlowTasks {
        #[command(subcommand)]
        command: Option<OneWebhookFlowTaskCommand>,
    },
    WriteSettings {
        #[command(subcommand)]
        command: Option<OneWriteSettingCommand>,
    },
    Scheduling {
        #[command(subcommand)]
        command: Option<OneSchedulingCommand>,
    },
    Billing {
        #[command(subcommand)]
        command: Option<OneBillingCommand>,
    },
    #[command(about = "Experimental Alteryx One visual interface surface")]
    Ui {
        #[command(subcommand)]
        command: Option<UiCommand>,
    },
    AutoInsights {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    DesktopExec {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlatformCommand {
    Api {
        #[command(subcommand)]
        command: OnePlatformApiCommand,
    },
    Auth {
        #[command(subcommand)]
        command: OnePlatformAuthCommand,
    },
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Workspace {
        #[command(subcommand)]
        command: OneWorkspaceCommand,
    },
    Role {
        #[command(subcommand)]
        command: OneRoleCommand,
    },
    User,
    Token {
        #[command(subcommand)]
        command: Option<OnePlatformTokenCommand>,
    },
    Person {
        #[command(subcommand)]
        command: Option<OnePlatformPersonCommand>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlatformTokenCommand {
    List,
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        token_id: String,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        token_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlatformPersonCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Current,
    Count,
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        person_id: String,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        person_id: String,
        #[arg(long)]
        body: PathBuf,
    },
    Patch {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        person_id: String,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        person_id: String,
    },
    UpdatePassword {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    PasswordResetRequest {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWorkspaceCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Current,
    CurrentConfiguration,
    ConfigurationV4 {
        #[arg(long)]
        workspace_id: String,
    },
    SaveCurrentConfiguration {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    SaveConfigurationV4 {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        body: PathBuf,
    },
    Configuration {
        #[arg(long)]
        workspace_id: String,
    },
    ConfigurationSchema {
        #[arg(long)]
        workspace_id: String,
    },
    CurrentConfigurationSchema,
    DeleteCurrentConfiguration {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    DeleteConfiguration {
        #[arg(long)]
        workspace_id: String,
    },
    People {
        #[arg(long)]
        workspace_id: String,
    },
    Admins {
        #[arg(long)]
        workspace_id: String,
    },
    InviteUsers {
        #[arg(long)]
        workspace_id: String,
    },
    RemoveUser {
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        person_id: String,
    },
    SuspendUsers {
        #[arg(long)]
        workspace_id: String,
    },
    UnsuspendUsers {
        #[arg(long)]
        workspace_id: String,
    },
    Transfer {
        #[arg(long)]
        workspace_id: String,
    },
    TransferAssets {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneRoleCommand {
    ListAssignments {
        #[arg(long)]
        role_id: String,
    },
    Assign {
        #[arg(long)]
        role_id: String,
        #[arg(long)]
        subject_id: String,
    },
    Unassign {
        #[arg(long)]
        role_id: String,
        #[arg(long)]
        subject_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlatformApiCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Diagnose {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    OpenApiSpec {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlatformAuthCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Diagnose {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OnePlansCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Full {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Run {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    RunParameters {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Schedules {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Export {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
    },
    Share {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Import {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Permissions {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        plan_id: Option<String>,
        #[arg(long)]
        subject_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneFlowsCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
    Copy {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
        #[arg(long)]
        body: Option<PathBuf>,
    },
    Run {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
        #[arg(long)]
        body: Option<PathBuf>,
    },
    Validate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
    Parameters {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
        #[arg(long)]
        output_object_type: Option<String>,
    },
    Inputs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
    Outputs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
    Import {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        folder_id: Option<String>,
        #[arg(long)]
        from_ui: bool,
        #[arg(long)]
        override_js_udfs: bool,
    },
    ImportDryRun {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        folder_id: Option<String>,
        #[arg(long)]
        from_ui: bool,
        #[arg(long)]
        override_js_udfs: bool,
    },
    Export {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
        #[arg(long)]
        output: PathBuf,
    },
    ExportDryRun {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        flow_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectionsCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    DryRun {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
    },
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
    },
    ConnectorMetadata {
        #[command(subcommand)]
        command: Option<OneConnectorMetadataCommand>,
    },
    Permissions {
        #[command(subcommand)]
        command: Option<OneConnectionPermissionCommand>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectorMetadataCommand {
    Defaults {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
    },
    PublishInfo {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
    },
    Overrides {
        #[command(subcommand)]
        command: Option<OneConnectorMetadataOverridesCommand>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectorMetadataOverridesCommand {
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
        #[arg(long)]
        body: PathBuf,
    },
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connector: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneConnectionPermissionCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
        #[arg(long)]
        aid: String,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        connection_id: Option<String>,
        #[arg(long)]
        aid: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneJobGroupCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Run {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Publish {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Cancel {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Inputs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Outputs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Jobs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Publications {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    Profile {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    ProfileResults {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
    PdfResults {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        job_group_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneOutputObjectCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        output_object_id: Option<String>,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        output_object_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        output_object_id: Option<String>,
    },
    Inputs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        output_object_id: Option<String>,
    },
    WrangleToPython {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        output_object_id: Option<String>,
        #[arg(long)]
        body: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWebhookFlowTaskCommand {
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        webhook_flow_task_id: Option<String>,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        webhook_flow_task_id: Option<String>,
    },
    Test {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneWriteSettingCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Create {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        body: PathBuf,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        write_setting_id: Option<String>,
    },
    Update {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        write_setting_id: Option<String>,
        #[arg(long)]
        body: PathBuf,
    },
    Delete {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        write_setting_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneSchedulingCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        max_pages: Option<u32>,
    },
    Detail {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        schedule_id: Option<String>,
    },
    Enable {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        schedule_id: Option<String>,
    },
    Disable {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        schedule_id: Option<String>,
    },
    Count {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneBillingCommand {
    CurrentAccount {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    UsageExport {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum OneDoctorCommand {
    Auth {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Discover {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Platform {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Plans {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Scheduling {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Billing {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum LicenseCommand {
    Api {
        #[command(subcommand)]
        command: LicenseApiCommand,
    },
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum LicenseApiCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Diagnose {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum CatalogCommand {
    List {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "compact")]
        format: String,
    },
    Describe {
        target: Option<String>,
        #[arg(long)]
        command: Option<String>,
    },
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
        prerequisites: &["config.yaml", "mongo.mode", "mongo.databases"],
        notes: &["Use this first to validate embedded or managed Mongo configuration."],
    },
    CommandSpec {
        name: "mongo inventory",
        path: "mongo/inventory",
        summary: "Generate an inventory plan for the Mongo-backed databases.",
        output: "database inventory plan",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "mongo.databases"],
        notes: &["Use this before backup or restore planning."],
    },
    CommandSpec {
        name: "mongo backup",
        path: "mongo/backup",
        summary: "Back up the Gallery and Service Mongo databases.",
        output: "backup plan or execution artifacts",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "mongo.mode"],
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
        prerequisites: &["config.yaml", "restore input path"],
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
        prerequisites: &["config.yaml", "server.webapi_url"],
        notes: &["Use before server api call."],
    },
    CommandSpec {
        name: "server api status",
        path: "server/api/status",
        summary: "Summarize Server API credentials and base URL posture.",
        output: "server api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Useful before diagnostics, import, or call."],
    },
    CommandSpec {
        name: "server api diagnose",
        path: "server/api/diagnose",
        summary: "Validate token acquisition and API reachability for Server.",
        output: "diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Use before server api import-swagger or server api call."],
    },
    CommandSpec {
        name: "server api call",
        path: "server/api/call",
        summary: "Invoke a Server API operation by operationId.",
        output: "call response envelope",
        safety: "mutating-or-read-only",
        mutating: false,
        prerequisites: &["cached Swagger document", "config.yaml"],
        notes: &["Operation behavior depends on the selected endpoint."],
    },
    CommandSpec {
        name: "license status",
        path: "license/status",
        summary: "Summarize the Licensing branch posture.",
        output: "license status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Product branch ready; API subcommands are the primary entry point."],
    },
    CommandSpec {
        name: "license inventory",
        path: "license/inventory",
        summary: "Summarize Licensing branch inventory candidates.",
        output: "license inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
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
        prerequisites: &["config.yaml", "server_api", "workflow package"],
        notes: &[
            "Uses the Server workflow upload API for the actual publish step.",
            "Accepts a ready .yxzp or a directory that can be repackaged first.",
        ],
    },
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
        name: "one platform status",
        path: "one/platform/status",
        summary: "Summarize the Alteryx One platform posture.",
        output: "one platform status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Use this before platform, plans, auto-insights, or desktop-exec workflows.",
            "Managed IAM lives in walter/docs/one/api/managed-iam-v1.yaml.",
        ],
    },
    CommandSpec {
        name: "one platform inventory",
        path: "one/platform/inventory",
        summary: "Summarize the current One API surface registry.",
        output: "one platform inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml"],
        notes: &[
            "Use this as the authoritative One endpoint registry.",
            "Implemented and partial surfaces are listed separately from documented-only gaps.",
        ],
    },
    CommandSpec {
        name: "one platform user",
        path: "one/platform/user",
        summary: "Show the current One user profile.",
        output: "one platform user envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CommandSpec {
        name: "one platform person list",
        path: "one/platform/person/list",
        summary: "List One people.",
        output: "one platform person list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people in the One API docs."],
    },
    CommandSpec {
        name: "one platform person current",
        path: "one/platform/person/current",
        summary: "Inspect the current One person record.",
        output: "one platform person current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CommandSpec {
        name: "one platform person count",
        path: "one/platform/person/count",
        summary: "Count One people.",
        output: "one platform person count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/count in the One API docs."],
    },
    CommandSpec {
        name: "one platform person detail",
        path: "one/platform/person/detail",
        summary: "Inspect a One person record by id.",
        output: "one platform person detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one platform person create",
        path: "one/platform/person/create",
        summary: "Create a One person from JSON payload.",
        output: "one platform person create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to POST /v4/people in the One API docs."],
    },
    CommandSpec {
        name: "one platform person update",
        path: "one/platform/person/update",
        summary: "Replace a One person record from JSON payload.",
        output: "one platform person update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to PUT /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one platform person patch",
        path: "one/platform/person/patch",
        summary: "Patch a One person record from JSON payload.",
        output: "one platform person patch envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to PATCH /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one platform person delete",
        path: "one/platform/person/delete",
        summary: "Delete a One person record.",
        output: "one platform person delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/people/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one platform person update-password",
        path: "one/platform/person/update-password",
        summary: "Update the current One person's password from JSON payload.",
        output: "one platform person update-password envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to PATCH /v4/people/current/updatePassword in the One API docs."],
    },
    CommandSpec {
        name: "one platform person password-reset-request",
        path: "one/platform/person/password-reset-request",
        summary: "Request a One password reset from JSON payload.",
        output: "one platform person password reset request envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to POST /v4/passwordresetrequest in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace current",
        path: "one/platform/workspace/current",
        summary: "Inspect the current One workspace posture.",
        output: "one platform workspace current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/workspaces/current in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace current-configuration",
        path: "one/platform/workspace/current-configuration",
        summary: "Inspect the current One workspace configuration.",
        output: "one platform workspace current configuration envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace configuration-v4",
        path: "one/platform/workspace/configuration-v4",
        summary: "Inspect a One workspace configuration by id.",
        output: "one platform workspace configuration-v4 envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace save-current-configuration",
        path: "one/platform/workspace/save-current-configuration",
        summary: "Update the current One workspace configuration from JSON payload.",
        output: "one platform workspace save-current-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to PATCH /v4/workspaces/current/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace save-configuration-v4",
        path: "one/platform/workspace/save-configuration-v4",
        summary: "Update a One workspace configuration by id from JSON payload.",
        output: "one platform workspace save-configuration-v4 envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to PATCH /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace list",
        path: "one/platform/workspace/list",
        summary: "List accessible One workspaces.",
        output: "one platform workspace list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace configuration-schema",
        path: "one/platform/workspace/configuration-schema",
        summary: "Inspect the workspace configuration schema.",
        output: "one platform workspace configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration-schema in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace current-configuration-schema",
        path: "one/platform/workspace/current-configuration-schema",
        summary: "Inspect the current workspace configuration schema.",
        output: "one platform workspace current configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration-schema in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace delete-current-configuration",
        path: "one/platform/workspace/delete-current-configuration",
        summary: "Reset the current workspace configuration.",
        output: "one platform workspace delete-current-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/current/delete-configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace delete-configuration",
        path: "one/platform/workspace/delete-configuration",
        summary: "Reset a workspace configuration by workspace id.",
        output: "one platform workspace delete-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/delete-configuration in the One API docs."],
    },
    CommandSpec {
        name: "one platform workspace people",
        path: "one/platform/workspace/people",
        summary: "List people in the current One workspace.",
        output: "one platform workspace people envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /iam/v1/workspaces/{id}/people in managed-iam-v1.yaml."],
    },
    CommandSpec {
        name: "one platform workspace admins",
        path: "one/platform/workspace/admins",
        summary: "List workspace admins.",
        output: "one platform workspace admins envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /iam/v1/workspaces/{workspaceId}/admins in managed-iam-v1.yaml."],
    },
    CommandSpec {
        name: "one platform role list-assignments",
        path: "one/platform/role/list-assignments",
        summary: "Inspect role assignments for One managed IAM.",
        output: "one platform role assignments envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /iam/v1/authorization/roles/{id}/people in managed-iam-v1.yaml."],
    },
    CommandSpec {
        name: "one platform auth status",
        path: "one/platform/auth/status",
        summary: "Summarize One API token posture for managed IAM.",
        output: "one platform auth status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Confirms OAuth client ID, token endpoint, access token presence, refresh token presence, and whether a safe workspace endpoint is reachable."],
    },
    CommandSpec {
        name: "one platform token",
        path: "one/platform/token",
        summary: "List One API access tokens.",
        output: "one platform token envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens in the One API docs."],
    },
    CommandSpec {
        name: "one platform token create",
        path: "one/platform/token/create",
        summary: "Create a One API access token from JSON payload.",
        output: "one platform token create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token", "payload json"],
        notes: &["Maps to POST /v4/apiAccessTokens in the One API docs."],
    },
    CommandSpec {
        name: "one platform token detail",
        path: "one/platform/token/detail",
        summary: "Inspect a One API access token by id.",
        output: "one platform token detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CommandSpec {
        name: "one platform token delete",
        path: "one/platform/token/delete",
        summary: "Delete a One API access token by id.",
        output: "one platform token delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CommandSpec {
        name: "one platform auth diagnose",
        path: "one/platform/auth/diagnose",
        summary: "Validate One API token reachability and workspace scope.",
        output: "one platform auth diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Uses the managed IAM current workspace endpoint as the safe validation target."],
    },
    CommandSpec {
        name: "one doctor auth",
        path: "one/doctor/auth",
        summary: "Run the One auth doctor workflow.",
        output: "one auth doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Wraps token posture and workspace probe checks."],
    },
    CommandSpec {
        name: "one doctor discover",
        path: "one/doctor/discover",
        summary: "Run the One discovery doctor workflow.",
        output: "one discovery doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Surfaces workspace, plan, schedule, and billing discovery data."],
    },
    CommandSpec {
        name: "one doctor platform",
        path: "one/doctor/platform",
        summary: "Run the One platform doctor workflow.",
        output: "one platform doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Wraps workspace and role discovery checks."],
    },
    CommandSpec {
        name: "one doctor plans",
        path: "one/doctor/plans",
        summary: "Run the One plans doctor workflow.",
        output: "one plans doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Wraps list, count, and plan lookup checks."],
    },
    CommandSpec {
        name: "one doctor scheduling",
        path: "one/doctor/scheduling",
        summary: "Run the One scheduling doctor workflow.",
        output: "one scheduling doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Wraps schedule list and count checks."],
    },
    CommandSpec {
        name: "one doctor billing",
        path: "one/doctor/billing",
        summary: "Run the One billing doctor workflow.",
        output: "one billing doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "alteryx_one.access_token"],
        notes: &["Wraps billing account and usage export checks."],
    },
    CommandSpec {
        name: "one platform api status",
        path: "one/platform/api/status",
        summary: "Summarize the Alteryx One platform API posture.",
        output: "one platform api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Use this to inspect platform API posture before diagnostics.",
            "Treat this as the One managed IAM posture check.",
        ],
    },
    CommandSpec {
        name: "one platform api diagnose",
        path: "one/platform/api/diagnose",
        summary: "Validate One platform API reachability and auth posture.",
        output: "one platform api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Use before future platform API call-style workflows.",
            "Route workflow guidance through the orchestration layer once the symptom is known.",
        ],
    },
    CommandSpec {
        name: "one platform api open-api-spec",
        path: "one/platform/api/open-api-spec",
        summary: "Fetch the One platform OpenAPI specification.",
        output: "one platform api open-api-spec envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/open-api-spec in the One API docs."],
    },
    CommandSpec {
        name: "one plans status",
        path: "one/plans/status",
        summary: "Summarize the Alteryx One plans posture.",
        output: "one plans status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Reserved for plan lifecycle workflows.",
            "Managed Plans lives in walter/docs/one/api/managed-plans-v1.yaml.",
        ],
    },
    CommandSpec {
        name: "one plans list",
        path: "one/plans/list",
        summary: "List One plans.",
        output: "one plans list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /plans/v1/plans in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans create",
        path: "one/plans/create",
        summary: "Create a One plan.",
        output: "one plans create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans in the One API docs."],
    },
    CommandSpec {
        name: "one plans run",
        path: "one/plans/run",
        summary: "Run a One plan.",
        output: "one plans run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to POST /plans/v1/plans/{id}/run in managed-plans-v1.yaml."],
    },
    CommandSpec {
        name: "one plans full",
        path: "one/plans/full",
        summary: "Inspect a One plan with the full documented payload.",
        output: "one plans full envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/full in the One API docs."],
    },
    CommandSpec {
        name: "one plans update",
        path: "one/plans/update",
        summary: "Update a One plan from JSON payload.",
        output: "one plans update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/plans/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one plans delete",
        path: "one/plans/delete",
        summary: "Delete a One plan.",
        output: "one plans delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/plans/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one plans share",
        path: "one/plans/share",
        summary: "Share a One plan from JSON payload.",
        output: "one plans share envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one flows list",
        path: "one/flows/list",
        summary: "List One flows.",
        output: "one flows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows in the One API docs."],
    },
    CommandSpec {
        name: "one flows count",
        path: "one/flows/count",
        summary: "Count One flows.",
        output: "one flows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/count in the One API docs."],
    },
    CommandSpec {
        name: "one flows detail",
        path: "one/flows/detail",
        summary: "Inspect a One flow by id.",
        output: "one flows detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows create",
        path: "one/flows/create",
        summary: "Create a One flow from JSON payload.",
        output: "one flows create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows in the One API docs."],
    },
    CommandSpec {
        name: "one flows update",
        path: "one/flows/update",
        summary: "Update a One flow from JSON payload.",
        output: "one flows update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PUT /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows delete",
        path: "one/flows/delete",
        summary: "Delete a One flow.",
        output: "one flows delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/flows/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one flows copy",
        path: "one/flows/copy",
        summary: "Copy a One flow using a JSON payload.",
        output: "one flows copy envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/copy in the One API docs."],
    },
    CommandSpec {
        name: "one flows run",
        path: "one/flows/run",
        summary: "Run a One flow using a JSON payload.",
        output: "one flows run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/run in the One API docs."],
    },
    CommandSpec {
        name: "one flows validate",
        path: "one/flows/validate",
        summary: "Validate a One flow.",
        output: "one flows validate envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/validate in the One API docs."],
    },
    CommandSpec {
        name: "one flows parameters",
        path: "one/flows/parameters",
        summary: "Inspect flow-level parameters and overrides.",
        output: "one flows parameters envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/recipeParameters in the One API docs."],
    },
    CommandSpec {
        name: "one flows inputs",
        path: "one/flows/inputs",
        summary: "List inputs for a One flow.",
        output: "one flows inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one flows outputs",
        path: "one/flows/outputs",
        summary: "List outputs for a One flow.",
        output: "one flows outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/outputs in the One API docs."],
    },
    CommandSpec {
        name: "one flows import",
        path: "one/flows/import",
        summary: "Import a flow package.",
        output: "one flows import envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package in the One API docs."],
    },
    CommandSpec {
        name: "one flows import-dry-run",
        path: "one/flows/import-dry-run",
        summary: "Dry-run import of a flow package.",
        output: "one flows import dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one flows export",
        path: "one/flows/export",
        summary: "Export a flow package to disk.",
        output: "one flows export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package in the One API docs."],
    },
    CommandSpec {
        name: "one flows export-dry-run",
        path: "one/flows/export-dry-run",
        summary: "Dry-run export of a flow package.",
        output: "one flows export dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one connections list",
        path: "one/connections/list",
        summary: "List One connections.",
        output: "one connections list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections in the One API docs."],
    },
    CommandSpec {
        name: "one connections count",
        path: "one/connections/count",
        summary: "Count One connections.",
        output: "one connections count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections/count in the One API docs."],
    },
    CommandSpec {
        name: "one connections create",
        path: "one/connections/create",
        summary: "Create a One connection from JSON payload.",
        output: "one connections create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections in the One API docs."],
    },
    CommandSpec {
        name: "one connections dry-run",
        path: "one/connections/dry-run",
        summary: "Dry-run creation of a One connection.",
        output: "one connections dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/dryRun in the One API docs."],
    },
    CommandSpec {
        name: "one connections detail",
        path: "one/connections/detail",
        summary: "Inspect a One connection.",
        output: "one connections detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections status",
        path: "one/connections/status",
        summary: "Inspect connection status.",
        output: "one connections status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/status in the One API docs."],
    },
    CommandSpec {
        name: "one connections update",
        path: "one/connections/update",
        summary: "Update a One connection from JSON payload.",
        output: "one connections update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections delete",
        path: "one/connections/delete",
        summary: "Delete a One connection.",
        output: "one connections delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/connections/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions",
        path: "one/connections/permissions",
        summary: "List permissions for a One connection.",
        output: "one connections permissions envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions create",
        path: "one/connections/permissions/create",
        summary: "Create permissions for a One connection.",
        output: "one connections permissions create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/{id}/permissions in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions detail",
        path: "one/connections/permissions/detail",
        summary: "Inspect a One connection permission by aid.",
        output: "one connections permissions detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/permissions/{aid} in the One API docs."],
    },
    CommandSpec {
        name: "one connections permissions delete",
        path: "one/connections/permissions/delete",
        summary: "Delete a One connection permission by aid.",
        output: "one connections permissions delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/connections/{id}/permissions/{aid} in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata defaults",
        path: "one/connections/connector-metadata/defaults",
        summary: "Inspect connector defaults.",
        output: "one connections connector-metadata defaults envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/defaults in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata detail",
        path: "one/connections/connector-metadata/detail",
        summary: "Inspect current connector metadata.",
        output: "one connections connector-metadata detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector} in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata publish-info",
        path: "one/connections/connector-metadata/publish-info",
        summary: "Inspect connector publish information.",
        output: "one connections connector-metadata publish-info envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/publish/info in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides create",
        path: "one/connections/connector-metadata/overrides/create",
        summary: "Create connector metadata overrides from JSON payload.",
        output: "one connections connector-metadata overrides create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides list",
        path: "one/connections/connector-metadata/overrides/list",
        summary: "Inspect connector metadata overrides.",
        output: "one connections connector-metadata overrides list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one connections connector-metadata overrides delete",
        path: "one/connections/connector-metadata/overrides/delete",
        summary: "Delete connector metadata overrides.",
        output: "one connections connector-metadata overrides delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CommandSpec {
        name: "one job-group list",
        path: "one/job-group/list",
        summary: "List One job groups.",
        output: "one job-group list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary in the One API docs."],
    },
    CommandSpec {
        name: "one job-group count",
        path: "one/job-group/count",
        summary: "Count One job groups.",
        output: "one job-group count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary/count in the One API docs."],
    },
    CommandSpec {
        name: "one job-group pdf-results",
        path: "one/job-group/pdf-results",
        summary: "Inspect PDF results for a One job group.",
        output: "one job-group pdf-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/pdfResults in the One API docs."],
    },
    CommandSpec {
        name: "one job-group run",
        path: "one/job-group/run",
        summary: "Run a One job group.",
        output: "one job-group run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/jobGroups in the One API docs."],
    },
    CommandSpec {
        name: "one job-group publish",
        path: "one/job-group/publish",
        summary: "Publish job-group results to a target.",
        output: "one job-group publish envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PUT /v4/jobGroups/{id}/publish in the One API docs."],
    },
    CommandSpec {
        name: "one job-group detail",
        path: "one/job-group/detail",
        summary: "Inspect a One job group.",
        output: "one job-group detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one job-group cancel",
        path: "one/job-group/cancel",
        summary: "Cancel a One job group.",
        output: "one job-group cancel envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to POST /v4/jobGroups/{id}/cancel in the One API docs."],
    },
    CommandSpec {
        name: "one job-group status",
        path: "one/job-group/status",
        summary: "Inspect a One job group status.",
        output: "one job-group status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/status in the One API docs."],
    },
    CommandSpec {
        name: "one job-group inputs",
        path: "one/job-group/inputs",
        summary: "List One job group inputs.",
        output: "one job-group inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group outputs",
        path: "one/job-group/outputs",
        summary: "List One job group outputs.",
        output: "one job-group outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/outputs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group jobs",
        path: "one/job-group/jobs",
        summary: "List jobs for a One job group.",
        output: "one job-group jobs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/jobs in the One API docs."],
    },
    CommandSpec {
        name: "one job-group publications",
        path: "one/job-group/publications",
        summary: "List publications for a One job group.",
        output: "one job-group publications envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/publications in the One API docs."],
    },
    CommandSpec {
        name: "one job-group profile",
        path: "one/job-group/profile",
        summary: "Inspect profile data for a One job group.",
        output: "one job-group profile envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profile in the One API docs."],
    },
    CommandSpec {
        name: "one job-group profile-results",
        path: "one/job-group/profile-results",
        summary: "Inspect profile results for a One job group.",
        output: "one job-group profile-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profileResults in the One API docs."],
    },
    CommandSpec {
        name: "one output-object list",
        path: "one/output-object/list",
        summary: "List One output objects.",
        output: "one output-object list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/outputObjects in the One API docs."],
    },
    CommandSpec {
        name: "one output-object count",
        path: "one/output-object/count",
        summary: "Count One output objects.",
        output: "one output-object count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/count in the One API docs."],
    },
    CommandSpec {
        name: "one output-object create",
        path: "one/output-object/create",
        summary: "Create a One output object from JSON payload.",
        output: "one output-object create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/outputObjects in the One API docs."],
    },
    CommandSpec {
        name: "one output-object detail",
        path: "one/output-object/detail",
        summary: "Inspect a One output object.",
        output: "one output-object detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object update",
        path: "one/output-object/update",
        summary: "Update a One output object from JSON payload.",
        output: "one output-object update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object delete",
        path: "one/output-object/delete",
        summary: "Delete a One output object.",
        output: "one output-object delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/outputObjects/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one output-object inputs",
        path: "one/output-object/inputs",
        summary: "List inputs for a One output object.",
        output: "one output-object inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id}/inputs in the One API docs."],
    },
    CommandSpec {
        name: "one output-object wrangle-to-python",
        path: "one/output-object/wrangle-to-python",
        summary: "Generate Python from a One output object.",
        output: "one output-object wrangle-to-python envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to POST /v4/outputObjects/{id}/wrangleToPython in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task create",
        path: "one/webhook-flow-task/create",
        summary: "Create a webhook flow task from JSON payload.",
        output: "one webhook-flow-task create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhookFlowTasks in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task detail",
        path: "one/webhook-flow-task/detail",
        summary: "Inspect a webhook flow task.",
        output: "one webhook-flow-task detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one webhook-flow-task delete",
        path: "one/webhook-flow-task/delete",
        summary: "Delete a webhook flow task.",
        output: "one webhook-flow-task delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one webhooks test",
        path: "one/webhooks/test",
        summary: "Test webhook settings from JSON payload.",
        output: "one webhooks test envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhooks/test in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting list",
        path: "one/write-setting/list",
        summary: "List One write settings.",
        output: "one write-setting list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/writeSettings in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting count",
        path: "one/write-setting/count",
        summary: "Count One write settings.",
        output: "one write-setting count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/count in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting create",
        path: "one/write-setting/create",
        summary: "Create a One write setting from JSON payload.",
        output: "one write-setting create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to POST /v4/writeSettings in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting detail",
        path: "one/write-setting/detail",
        summary: "Inspect a One write setting.",
        output: "one write-setting detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting update",
        path: "one/write-setting/update",
        summary: "Update a One write setting from JSON payload.",
        output: "one write-setting update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one write-setting delete",
        path: "one/write-setting/delete",
        summary: "Delete a One write setting.",
        output: "one write-setting delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to DELETE /v4/writeSettings/{id} in the One API docs."],
    },
    CommandSpec {
        name: "one scheduling list",
        path: "one/scheduling/list",
        summary: "List One schedules.",
        output: "one scheduling list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /scheduling/v1/schedules in managed-scheduling-v1.yaml."],
    },
    CommandSpec {
        name: "one billing current-account",
        path: "one/billing/current-account",
        summary: "Inspect the current One billing account.",
        output: "one billing current-account envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Maps to GET /billing/v1/my/billing-accounts/current in managed-billing-v1.yaml."],
    },
    CommandSpec {
        name: "one auto-insights status",
        path: "one/auto-insights/status",
        summary: "Summarize the Alteryx One Auto Insights posture.",
        output: "one auto-insights status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Reserved for Auto Insights workflows.",
            "Scheduling and run semantics may map here or to a later dedicated branch.",
        ],
    },
    CommandSpec {
        name: "one desktop-exec status",
        path: "one/desktop-exec/status",
        summary: "Summarize the Alteryx One desktop execution posture.",
        output: "one desktop-exec status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Reserved for desktop execution workflows.",
            "Keep this branch narrow until the desktop-exec surface is validated.",
        ],
    },
    CommandSpec {
        name: "license api status",
        path: "license/api/status",
        summary: "Summarize the Licensing portal API posture.",
        output: "license api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &["Use to inspect licensing API posture before diagnostics."],
    },
    CommandSpec {
        name: "license api diagnose",
        path: "license/api/diagnose",
        summary: "Validate Licensing API reachability and auth posture.",
        output: "license api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
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
        name: "server diagnose startup",
        path: "server/diagnose/startup",
        summary: "Run a guided startup failure diagnosis.",
        output: "startup diagnosis steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "optional startup error", "optional log file"],
        notes: &["Wraps logs, runtime settings, and recent log candidate checks."],
    },
    CommandSpec {
        name: "server diagnose tls",
        path: "server/diagnose/tls",
        summary: "Inspect TLS, certificate, and proxy-related Server checks.",
        output: "tls diagnosis steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server.webapi_url"],
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
        prerequisites: &["config.yaml", "server install path"],
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
        prerequisites: &["config.yaml"],
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
        prerequisites: &["config.yaml", "server settings"],
        notes: &["Use this before SAML diagnosis or simulation."],
    },
    CommandSpec {
        name: "server auth diagnose saml",
        path: "server/auth/diagnose/saml",
        summary: "Inspect SAML configuration, metadata, and callback alignment.",
        output: "saml diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "metadata url or file when available"],
        notes: &["Focuses on Server-side SAML configuration and common mismatch checks."],
    },
    CommandSpec {
        name: "mongo query",
        path: "mongo/query",
        summary: "Run a read-only Mongo query against a Server collection.",
        output: "mongo query envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "mongosh available on PATH"],
        notes: &["Use for targeted inspection of Gallery and Service collections."],
    },
    CommandSpec {
        name: "mongo doctor",
        path: "mongo/doctor",
        summary: "Run the default support query suite across critical Mongo collections.",
        output: "mongo doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "mongosh available on PATH"],
        notes: &["Targets queue, results, users, and app info collections."],
    },
    CommandSpec {
        name: "server auth diagnose saml-logs",
        path: "server/auth/diagnose/saml-logs",
        summary: "Collect and summarize SAML login logs.",
        output: "saml log diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "SAML login logs"],
        notes: &["Targets alteryx-sso and aas log families."],
    },
    CommandSpec {
        name: "server auth diagnose certificate",
        path: "server/auth/diagnose/certificate",
        summary: "Inspect certificate posture for SAML auth.",
        output: "certificate diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "certificate file when available"],
        notes: &["Focuses on certificate presence, parsing, and likely trust issues."],
    },
    CommandSpec {
        name: "server auth diagnose ad-legacy",
        path: "server/auth/diagnose/ad-legacy",
        summary: "Inspect legacy Active Directory auth support signals.",
        output: "legacy ad diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml"],
        notes: &["Kept intentionally narrow as a legacy troubleshooting path."],
    },
    CommandSpec {
        name: "server auth simulate saml",
        path: "server/auth/simulate/saml",
        summary: "Simulate a SAML auth flow using metadata and expected endpoints.",
        output: "saml simulation envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "metadata url or file"],
        notes: &["Designed as a diagnostic harness, not a full IdP emulator."],
    },
    CommandSpec {
        name: "server doctor startup",
        path: "server/doctor/startup",
        summary: "Run a guided startup doctor workflow.",
        output: "startup doctor steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "optional startup error", "optional log file"],
        notes: &["Prescriptive version of server diagnose startup."],
    },
];

#[derive(Subcommand, Debug)]
pub(crate) enum ServerLogsCommand {
    Discover {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Inventory {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Summary {
        #[arg(long)]
        path: PathBuf,
    },
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
    ParseCsv {
        #[arg(long)]
        path: PathBuf,
    },
    ServiceEvents {
        #[arg(long)]
        path: PathBuf,
    },
    GalleryEvents {
        #[arg(long)]
        path: PathBuf,
    },
    Tail {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    Recent {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerDiagnoseCommand {
    Startup {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        error: Option<String>,
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    Logs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Network {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Tls {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    RuntimeSettings {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Diagnose {
        #[command(subcommand)]
        command: ServerAuthDiagnoseCommand,
    },
    Simulate {
        #[command(subcommand)]
        command: ServerAuthSimulateCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthDiagnoseCommand {
    Saml {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        metadata_url: Option<String>,
        #[arg(long)]
        metadata_file: Option<PathBuf>,
        #[arg(long)]
        acs_url: Option<String>,
        #[arg(long)]
        issuer: Option<String>,
    },
    SamlLogs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    Certificate {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        certificate_file: Option<PathBuf>,
    },
    AdLegacy {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        domain: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerAuthSimulateCommand {
    Saml {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Startup {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        error: Option<String>,
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    Logs {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Network {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    RuntimeSettings {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerApiCommand {
    Status {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    Diagnose {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
    },
    ImportSwagger {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long, default_value = "3")]
        version: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value = ".omni/swagger")]
        cache_dir: PathBuf,
    },
    Call {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
    Path {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "embedded-mongo")]
        deployment: String,
    },
    Precheck {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "upgrade-precheck")]
        out: PathBuf,
        #[arg(long, default_value = "embedded-mongo")]
        deployment: String,
    },
    Backup {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        r#type: String,
        #[arg(long, default_value = "upgrade-backup")]
        out: PathBuf,
    },
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
    Apply {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        yes: bool,
    },
    Postcheck {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = "upgrade-postcheck")]
        out: PathBuf,
    },
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
            cli.apply, cli.environment, cli.no_verify_tls, cli.verbose
        );
    }

    // `load_profile` is intentionally a tiny shim around the environment-aware
    // Config loader. Lifting it from a captured closure to a `let`-bound
    // fn-pointer-shaped closure (still capturing `cli.environment`) keeps
    // every existing call-site `load_profile(&path)` working unchanged, while
    // the parallel free function `load_profile_with_env` (below `execute`)
    // is the canonical entry point for code under `cmd/` modules that
    // doesn't have `cli` in scope.
    let environment = cli.environment.clone();
    let load_profile =
        |path: &Path| -> Result<Config> { load_profile_with_env(path, environment.as_deref()) };
    let envelope = match cli.command {
        Command::Mongo { command } => cmd::mongo::execute(cli.environment.as_deref(), command)?,
        Command::Server { command } => cmd::server::execute(cli.environment.as_deref(), command)?,
        Command::Sqlserver { command } => {
            cmd::sqlserver::execute(cli.environment.as_deref(), command)?
        }
        Command::Workflow { command } => {
            cmd::workflow::execute(cli.environment.as_deref(), command)?
        }
        Command::Tools { command } => cmd::tools::execute(command)?,
        Command::Onboard {
            profile,
            environments,
            non_interactive,
        } => {
            let detail = onboard::run_onboarding(
                &profile,
                cli.environment.as_deref(),
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
        } => doctor_envelope(command.as_ref(), &profile, fix, cli.environment.as_deref())?,
        Command::One { command } => cmd::one::execute(
            cmd::one::Ctx {
                apply: cli.apply,
                yes: cli.yes,
                environment: cli.environment.as_deref(),
            },
            command,
        )?,
        Command::License { command } => match command {
            None => Envelope::ok("license commands available: api, status, inventory"),
            Some(LicenseCommand::Api { command }) => match command {
                LicenseApiCommand::Status { profile } => {
                    let config = load_profile(&profile)?;
                    api_status_envelope(&config, "license")?
                }
                LicenseApiCommand::Diagnose { profile } => {
                    let config = load_profile(&profile)?;
                    api_diagnose_envelope(&config, "license")?
                }
            },
            Some(LicenseCommand::Status { profile }) => {
                let config = load_profile(&profile)?;
                api_status_envelope(&config, "license")?
            }
            Some(LicenseCommand::Inventory { profile }) => {
                let config = load_profile(&profile)?;
                api_inventory_envelope(&config, "license")?
            }
        },
        Command::Catalog { command } => match command {
            CatalogCommand::List { tag, format } => catalog_list_envelope(tag.as_deref(), &format)?,
            CatalogCommand::Describe { target, command } => {
                let target = target.as_deref().or(command.as_deref()).ok_or_else(|| {
                    anyhow!("catalog describe requires a command or capability identifier")
                })?;
                catalog_describe_envelope(target)?
            }
            CatalogCommand::Run {
                capability,
                json_input,
                dry_run,
            } => catalog_run_envelope(&capability, &json_input, dry_run)?,
        },
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
            // for a structured payload or pipe through `ayx one platform
            // workspace current` for the live workspace.
            let path = profile.unwrap_or_else(|| PathBuf::from("config.yaml"));
            let resolved = ayx_core::profile::resolve_profile_path(&path).unwrap_or(path.clone());
            let config = load_profile(&resolved).ok();
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
                    "active_profile": active_profile,
                    "active_workspace": active_workspace,
                    "profile_path": resolved.display().to_string(),
                    "environment": cli.environment.clone(),
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
                                if let Ok(meta) = e.metadata() {
                                    if meta.is_file() {
                                        c += 1;
                                        b += meta.len();
                                    }
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
        Command::Tactics { command } => cmd::registry::execute_tactics(cli.apply, command)?,
        Command::Workflows { command } => cmd::registry::execute_workflows(cli.apply, command)?,
        Command::Telemetry { command } => {
            cmd::telemetry::execute(cli.environment.as_deref(), command)?
        }
        Command::Dashboard(dash) => cmd::dashboard::execute(cli.environment.as_deref(), dash)?,
    };
    Ok(envelope)
}

pub(crate) fn one_doctor_platform_envelope(config: &Config) -> Result<Envelope> {
    let auth = one_platform_auth_status_envelope(config)?;
    let workspace = one_api_live_request(
        config,
        "platform",
        "doctor-workspace-current",
        "GET",
        "/v4/workspaces/current",
        false,
        &[],
    )?;
    Ok(Envelope::ok_with_data(
        "one platform doctor workflow generated",
        json!({
            "profile": config.profile_name,
            "checks": [
                auth.data,
                workspace.data,
            ],
            "recommendations": [
                "Use one platform workspace people/admins to drill into workspace scope",
                "Route deeper symptom handling to the workflow guidance layer",
            ]
        }),
    ))
}

pub(crate) fn one_doctor_discover_envelope(config: &Config) -> Result<Envelope> {
    let workspace = one_api_live_request(
        config,
        "platform",
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
                "Use one platform workspace current to identify the workspace context",
                "Use one plans list/detail/run to resolve plan ids",
                "Use one scheduling list/detail/enable/disable to resolve schedule ids",
                "Use the workflow guidance layer to decide whether a symptom belongs to platform, plans, scheduling, or billing",
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
    let state = load_ayx_state()?;
    let active_name = state
        .active_profile
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let path = profile_storage_path(&active_name)?;
    Ok(Envelope::ok_with_data(
        "current profile resolved",
        json!({
            "active_profile": active_name,
            "path": path.display().to_string(),
            "exists": path.exists(),
            "state_path": ayx_state_path()?.display().to_string(),
        }),
    ))
}

fn profile_show_envelope(name: Option<&str>) -> Result<Envelope> {
    let state = load_ayx_state()?;
    let name = name
        .map(ToOwned::to_owned)
        .or(state.active_profile)
        .unwrap_or_else(|| "default".to_string());
    let path = profile_storage_path(&name)?;
    let config = Config::load_from_path(&path)?;
    let resolution = profile_resolution_detail(&path)?;
    Ok(Envelope::ok_with_data(
        "profile loaded",
        json!({
            "name": name,
            "path": path.display().to_string(),
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
    let secretize = onboard::secretize_config(
        &mut config,
        &target_name,
        onboard::InlineSecretPolicy::Allow,
    )?;
    let body = serde_yaml::to_string(&ayx_core::profile::canonical_profile_value(&config)?)?;
    onboard::write_restricted(&target, body.as_bytes())?;
    let mut state = load_ayx_state()?;
    state.active_profile = Some(target_name.clone());
    save_ayx_state(&state)?;
    Ok(Envelope::ok_with_data(
        "profile migrated",
        json!({
            "source": profile.display().to_string(),
            "target": target.display().to_string(),
            "active_profile": target_name,
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
    profile: &Path,
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

fn doctor_full_envelope(profile: &Path, fix: bool, environment: Option<&str>) -> Result<Envelope> {
    let config = doctor_config_envelope(profile, fix)?;
    let auth = doctor_auth_envelope(profile, environment)?;
    let network = doctor_network_envelope(profile, environment)?;
    let one = doctor_one_envelope(profile, environment)?;
    let server = doctor_server_envelope(profile, environment)?;
    let mongo = doctor_mongo_envelope(profile, environment)?;
    Ok(Envelope::ok_with_data(
        "doctor completed",
        json!({
            "sequence": ["config", "auth", "network", "one", "server", "mongo"],
            "fix_applied": fix,
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

fn doctor_config_envelope(profile: &Path, fix: bool) -> Result<Envelope> {
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
        }),
    ))
}

fn doctor_auth_envelope(profile: &Path, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
    let one = config.alteryx_one.as_ref();
    let server = config.server.as_ref();
    Ok(Envelope::ok_with_data(
        "doctor auth completed",
        json!({
            "profile": config.profile_name,
            "one": {
                "configured": one.is_some(),
                "access_token_present": one.and_then(|v| v.access_token.as_ref()).is_some_and(|v| !v.trim().is_empty()),
                "refresh_token_present": one.and_then(|v| v.refresh_token.as_ref()).is_some_and(|v| !v.trim().is_empty()),
                "oauth_client_id_present": one.and_then(|v| v.oauth_client_id.as_ref()).is_some_and(|v| !v.trim().is_empty()),
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
                "configured": server.is_some(),
                "curator_api_key_present": server.is_some_and(|v| !v.curator_api_key.trim().is_empty()),
                "curator_api_secret_present": server.is_some_and(|v| !v.curator_api_secret.trim().is_empty()),
                "curator_api_secret_source": secret_source(
                    server.and_then(|v| v.curator_api_secret_ref.as_ref()),
                    server.map(|v| v.curator_api_secret.as_str()),
                ),
            }
        }),
    ))
}

fn doctor_network_envelope(profile: &Path, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
    Ok(Envelope::ok_with_data(
        "doctor network completed",
        json!({
            "profile": config.profile_name,
            "targets": {
                "one_base_url": config
                    .alteryx_one
                    .as_ref()
                    .and_then(|v| v.normalized_base_url()),
                "one_token_endpoint": config
                    .alteryx_one
                    .as_ref()
                    .and_then(|v| v.effective_token_endpoint_url()),
                "server_base_url": config.server.as_ref().map(|v| v.webapi_url.clone()),
                "server_api_base_url": config.server_api.as_ref().map(|v| v.base_url.clone()),
            },
            "notes": [
                "Network checks currently validate configured endpoints rather than performing invasive probes",
            ],
        }),
    ))
}

fn doctor_one_envelope(profile: &Path, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
    one_platform_auth_diagnose_envelope(&config)
}

fn doctor_server_envelope(profile: &Path, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
    let server_ready = config.server.is_some() || config.server_api.is_some();
    Ok(Envelope::ok_with_data(
        "doctor server completed",
        json!({
            "profile": config.profile_name,
            "configured": server_ready,
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

fn doctor_mongo_envelope(profile: &Path, environment: Option<&str>) -> Result<Envelope> {
    let config = Config::load_from_path_with_environment(profile, environment)?;
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
            "managed_host_present": config.mongo.managed.as_ref().and_then(|managed| managed.host.as_ref()).is_some_and(|v| !v.trim().is_empty()),
            "managed_url_present": config.mongo.managed.as_ref().and_then(|managed| managed.url.as_ref()).is_some_and(|v| !v.trim().is_empty()),
        }),
    ))
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
    let workspace_probe = if one
        .access_token
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        Some(one_api_live_request(
            config,
            "platform",
            "auth-status",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )?)
    } else {
        None
    };

    Ok(Envelope::ok_with_data(
        "one platform auth status",
        json!({
            "product": "one",
            "surface": "platform",
            "profile": config.profile_name,
            "oauth_client_id_present": one.oauth_client_id.as_ref().is_some_and(|v| !v.trim().is_empty()),
            "base_url": one.normalized_base_url(),
            "token_endpoint_url": one.effective_token_endpoint_url(),
            "access_token_present": one.access_token.as_ref().is_some_and(|v| !v.trim().is_empty()),
            "refresh_token_present": one.refresh_token.as_ref().is_some_and(|v| !v.trim().is_empty()),
            "observability": api_logging,
            "token_source": if one.access_token.as_ref().is_some_and(|v| !v.trim().is_empty()) {
                "config/env"
            } else {
                "missing"
            },
            "validation_target": "/v4/workspaces/current",
            "workspace_probe": workspace_probe.as_ref().map(|probe| probe.data.clone()),
            "message": "One API token posture captured",
        }),
    ))
}

pub(crate) fn one_platform_auth_diagnose_envelope(config: &Config) -> Result<Envelope> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("config missing alteryx_one section"))?;
    let has_token = one
        .access_token
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty());
    let has_refresh_token = one
        .refresh_token
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty());
    let workspace_probe = if has_token {
        one_api_live_request(
            config,
            "platform",
            "auth-diagnose",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )?
    } else {
        Envelope::ok_with_data(
            "one platform auth diagnose",
            json!({
                "product": "one",
                "surface": "platform",
                "profile": config.profile_name,
                "oauth_client_id_present": one.oauth_client_id.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "base_url": one.normalized_base_url(),
                "token_endpoint_url": one.effective_token_endpoint_url(),
                "access_token_present": false,
                "refresh_token_present": has_refresh_token,
                "diagnosis": "alteryx_one.access_token is missing",
                "recommendations": [
                    "Set AYX_ONE_API_ACCESS_TOKEN in .env",
                    "Populate alteryx_one.access_token in config.yaml if you prefer config-based storage"
                ],
            }),
        )
    };

    if has_token {
        Ok(Envelope::ok_with_data(
            "one platform auth diagnose",
            json!({
                "product": "one",
                "surface": "platform",
                "profile": config.profile_name,
                "oauth_client_id_present": one.oauth_client_id.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "base_url": one.normalized_base_url(),
                "token_endpoint_url": one.effective_token_endpoint_url(),
                "access_token_present": true,
                "refresh_token_present": has_refresh_token,
                "diagnosis": "token present and workspace probe executed",
                "workspace_probe": workspace_probe.data,
                "recommendations": [
                    "Use one platform workspace current or people for evidence",
                    "Route any failing symptoms into the workflow guidance layer",
                ],
            }),
        ))
    } else {
        Ok(workspace_probe)
    }
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

    match execute(cli) {
        Ok(envelope) => {
            print!("{}", format_envelope(&envelope, &output)?);
            // Most renderers don't add a trailing newline; add one for shells.
            println!();
            Ok(())
        }
        Err(err) => {
            let code = classify_anyhow_error(&err);
            let hint = hint_for_error_code(code);
            let mut data = json!({
                "error": err.to_string(),
                "transport": transport_error_summary(err.as_ref()),
                "error_code": code.as_str(),
            });
            if let Some(h) = hint {
                data["hint"] = Value::String(h.to_string());
            }
            let err_env = Envelope::err_coded(code, "command failed", data);
            // Errors always go to stderr; the format mirrors the success
            // renderer so JSON consumers see the same envelope shape.
            eprint!(
                "{}",
                format_envelope(&err_env, &output).unwrap_or_else(|_| err_env.message.clone())
            );
            eprintln!();
            if !matches!(output.as_str(), "json" | "yaml") {
                eprintln!("{}", err);
            }
            Err(err)
        }
    }
}

/// Render an envelope according to the requested output format. Returns a
/// `Validation` error envelope-as-string for unknown formats so the
/// failure surfaces uniformly via the outer error path.
/// Canonical profile loader used by both the closure inside `execute()` and
/// future code under `cmd/` modules. Single-source-of-truth for how a
/// `--profile <path>` resolves against `cli.environment`.
pub(crate) fn load_profile_with_env(path: &Path, environment: Option<&str>) -> Result<Config> {
    Ok(Config::load_from_path_with_environment(path, environment)?)
}

/// Lenient profile loader for One/dashboard paths that should keep working
/// even when the Server block is present but not fully provisioned.
pub(crate) fn load_profile_with_env_lenient(
    path: &Path,
    environment: Option<&str>,
) -> Result<Config> {
    Ok(Config::load_from_path_with_environment_lenient(
        path,
        environment,
    )?)
}

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
        ConfigMissing => Some("Run 'ayx onboard' to set up a profile, or 'ayx doctor config' to inspect the current one."),
        AuthFailed => Some("Run 'ayx doctor auth' to inspect auth posture. Re-run 'ayx onboard' if tokens are stale."),
        PermissionDenied => Some("Check that the active profile's token has the required role/scope for this resource."),
        NotFound => Some("Verify the id is correct. Use 'ayx <surface> list' to enumerate available resources."),
        Validation => Some("Inspect the failed flag or input; '--help' on the subcommand documents accepted values."),
        Conflict => Some("Resource is in a conflicting state. Inspect the current state with the detail command, then retry."),
        RateLimited => Some("Retry after the suggested delay; consider --max-pages to bound auto-pagination."),
        Network => Some("Run 'ayx doctor network' to diagnose connectivity. Check VPN/proxy if applicable."),
        Upstream => Some("Upstream returned a 5xx. Retry; if it persists, escalate to the Alteryx One status page."),
        WorkspaceMismatch => Some("Re-authenticate against the expected workspace, or unset alteryx_one.expected_workspace_id."),
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
    if chain.contains("validation")
        || chain.contains("invalid value")
        || chain.contains("invalid format")
        || chain.contains("cannot be empty")
    {
        return ErrorCode::Validation;
    }
    if chain.contains("500")
        || chain.contains("502")
        || chain.contains("503")
        || chain.contains("504")
    {
        return ErrorCode::Upstream;
    }
    ErrorCode::Internal
}

fn catalog_list_envelope(tag: Option<&str>, format: &str) -> Result<Envelope> {
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

fn catalog_describe_envelope(identifier: &str) -> Result<Envelope> {
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

fn catalog_run_envelope(capability_id: &str, json_input: &str, dry_run: bool) -> Result<Envelope> {
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
        assert!(names.contains(&"one platform status"));
        assert!(names.contains(&"one platform inventory"));
        assert!(names.contains(&"one platform user"));
        assert!(names.contains(&"one platform person list"));
        assert!(names.contains(&"one platform person current"));
        assert!(names.contains(&"one platform person count"));
        assert!(names.contains(&"one platform person detail"));
        assert!(names.contains(&"one platform person create"));
        assert!(names.contains(&"one platform person update"));
        assert!(names.contains(&"one platform person patch"));
        assert!(names.contains(&"one platform person delete"));
        assert!(names.contains(&"one platform person update-password"));
        assert!(names.contains(&"one platform person password-reset-request"));
        assert!(names.contains(&"one platform api status"));
        assert!(names.contains(&"one platform auth status"));
        assert!(names.contains(&"one platform workspace current"));
        assert!(names.contains(&"one platform workspace list"));
        assert!(names.contains(&"one platform workspace current-configuration"));
        assert!(names.contains(&"one platform workspace configuration-v4"));
        assert!(names.contains(&"one platform workspace save-current-configuration"));
        assert!(names.contains(&"one platform workspace save-configuration-v4"));
        assert!(names.contains(&"one platform role list-assignments"));
        assert!(names.contains(&"one plans status"));
        assert!(names.contains(&"one plans list"));
        assert!(names.contains(&"one plans create"));
        assert!(names.contains(&"one plans full"));
        assert!(names.contains(&"one plans update"));
        assert!(names.contains(&"one plans delete"));
        assert!(names.contains(&"one plans share"));
        assert!(names.contains(&"one flows list"));
        assert!(names.contains(&"one flows count"));
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
        assert!(names.contains(&"one flows import"));
        assert!(names.contains(&"one flows import-dry-run"));
        assert!(names.contains(&"one flows export"));
        assert!(names.contains(&"one flows export-dry-run"));
        assert!(names.contains(&"one connections list"));
        assert!(names.contains(&"one connections create"));
        assert!(names.contains(&"one connections dry-run"));
        assert!(names.contains(&"one connections permissions"));
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
        assert!(names.contains(&"one platform api open-api-spec"));
        assert!(names.contains(&"one scheduling list"));
        assert!(names.contains(&"one billing current-account"));
        assert!(names.contains(&"one platform token"));
        assert!(names.contains(&"one platform token create"));
        assert!(names.contains(&"one platform token detail"));
        assert!(names.contains(&"one platform token delete"));
        assert!(names.contains(&"one auto-insights status"));
        assert!(names.contains(&"one desktop-exec status"));
        assert!(!names.contains(&"one platform group"));
        assert!(!names.contains(&"one platform sso"));
        assert!(!names.contains(&"one platform audit"));
        assert!(!names.contains(&"one platform session"));
        assert!(!names.contains(&"one platform oauth-client"));
        assert!(!names.contains(&"one platform env-param"));
        assert!(!names.contains(&"one platform pdh"));
        assert!(!names.contains(&"one platform app"));
        assert!(!names.contains(&"one platform health"));
        let capabilities = env.data["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "designer.workflow.context"));
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

        let env =
            catalog_describe_envelope("profile current").expect("catalog describe should work");
        assert_eq!(env.data["path"], "profile/current");

        let env = catalog_describe_envelope("doctor config")
            .expect("catalog describe should work for top-level doctor");
        assert_eq!(env.data["path"], "doctor/config");

        let env =
            catalog_describe_envelope("one platform status").expect("catalog describe should work");
        assert_eq!(env.data["name"], "one platform status");

        let env = catalog_describe_envelope("one platform inventory")
            .expect("catalog describe should work for inventory");
        assert_eq!(env.data["path"], "one/platform/inventory");

        let env =
            catalog_describe_envelope("one platform user").expect("catalog describe should work");
        assert_eq!(env.data["path"], "one/platform/user");

        let env = catalog_describe_envelope("one platform person list")
            .expect("catalog describe should work for person list");
        assert_eq!(env.data["path"], "one/platform/person/list");

        let env = catalog_describe_envelope("one platform person current")
            .expect("catalog describe should work for person current");
        assert_eq!(env.data["path"], "one/platform/person/current");

        let env = catalog_describe_envelope("one platform person count")
            .expect("catalog describe should work for person count");
        assert_eq!(env.data["path"], "one/platform/person/count");

        let env = catalog_describe_envelope("one platform person detail")
            .expect("catalog describe should work for person detail");
        assert_eq!(env.data["path"], "one/platform/person/detail");

        let env = catalog_describe_envelope("one platform person update")
            .expect("catalog describe should work for person update");
        assert_eq!(env.data["path"], "one/platform/person/update");

        let env = catalog_describe_envelope("one platform person patch")
            .expect("catalog describe should work for person patch");
        assert_eq!(env.data["path"], "one/platform/person/patch");

        let env = catalog_describe_envelope("one platform person delete")
            .expect("catalog describe should work for person delete");
        assert_eq!(env.data["path"], "one/platform/person/delete");

        let env = catalog_describe_envelope("one platform workspace list")
            .expect("catalog describe should work for workspace list");
        assert_eq!(env.data["path"], "one/platform/workspace/list");

        let env = catalog_describe_envelope("one platform workspace current-configuration")
            .expect("catalog describe should work for current configuration");
        assert_eq!(
            env.data["path"],
            "one/platform/workspace/current-configuration"
        );

        let env = catalog_describe_envelope("one platform workspace configuration-v4")
            .expect("catalog describe should work for workspace configuration-v4");
        assert_eq!(env.data["path"], "one/platform/workspace/configuration-v4");

        let env = catalog_describe_envelope("one platform workspace save-current-configuration")
            .expect("catalog describe should work for save current configuration");
        assert_eq!(
            env.data["path"],
            "one/platform/workspace/save-current-configuration"
        );

        let env = catalog_describe_envelope("one platform workspace save-configuration-v4")
            .expect("catalog describe should work for save configuration-v4");
        assert_eq!(
            env.data["path"],
            "one/platform/workspace/save-configuration-v4"
        );

        let env =
            catalog_describe_envelope("one platform token").expect("catalog describe should work");
        assert_eq!(env.data["path"], "one/platform/token");

        let env = catalog_describe_envelope("one platform token detail")
            .expect("catalog describe should work for token detail");
        assert_eq!(env.data["path"], "one/platform/token/detail");

        let env = catalog_describe_envelope("one platform api diagnose")
            .expect("catalog describe should work for one platform api");
        assert_eq!(env.data["path"], "one/platform/api/diagnose");

        let env = catalog_describe_envelope("one platform auth diagnose")
            .expect("catalog describe should work for one platform auth");
        assert_eq!(env.data["path"], "one/platform/auth/diagnose");

        let env = catalog_describe_envelope("one auto-insights status")
            .expect("catalog describe should work for auto-insights");
        assert_eq!(env.data["path"], "one/auto-insights/status");

        let env = catalog_describe_envelope("one plans list")
            .expect("catalog describe should work for one plans list");
        assert_eq!(env.data["path"], "one/plans/list");

        let env = catalog_describe_envelope("one plans create")
            .expect("catalog describe should work for one plans create");
        assert_eq!(env.data["path"], "one/plans/create");

        let env = catalog_describe_envelope("one plans full")
            .expect("catalog describe should work for one plans full");
        assert_eq!(env.data["path"], "one/plans/full");

        let env = catalog_describe_envelope("one plans update")
            .expect("catalog describe should work for one plans update");
        assert_eq!(env.data["path"], "one/plans/update");

        let env = catalog_describe_envelope("one plans delete")
            .expect("catalog describe should work for one plans delete");
        assert_eq!(env.data["path"], "one/plans/delete");

        let env = catalog_describe_envelope("one plans share")
            .expect("catalog describe should work for one plans share");
        assert_eq!(env.data["path"], "one/plans/share");

        let env = catalog_describe_envelope("one flows list")
            .expect("catalog describe should work for one flows list");
        assert_eq!(env.data["path"], "one/flows/list");

        let env = catalog_describe_envelope("one flows export")
            .expect("catalog describe should work for one flows export");
        assert_eq!(env.data["path"], "one/flows/export");

        let env = catalog_describe_envelope("one connections list")
            .expect("catalog describe should work for one connections list");
        assert_eq!(env.data["path"], "one/connections/list");

        let env = catalog_describe_envelope("one connections permissions")
            .expect("catalog describe should work for one connections permissions");
        assert_eq!(env.data["path"], "one/connections/permissions");

        let env = catalog_describe_envelope("one connections connector-metadata defaults")
            .expect("catalog describe should work for one connections connector-metadata defaults");
        assert_eq!(
            env.data["path"],
            "one/connections/connector-metadata/defaults"
        );

        let env = catalog_describe_envelope("one connections connector-metadata detail")
            .expect("catalog describe should work for one connections connector-metadata detail");
        assert_eq!(
            env.data["path"],
            "one/connections/connector-metadata/detail"
        );

        let env = catalog_describe_envelope("one connections connector-metadata overrides list")
            .expect(
            "catalog describe should work for one connections connector-metadata overrides list",
        );
        assert_eq!(
            env.data["path"],
            "one/connections/connector-metadata/overrides/list"
        );

        let env = catalog_describe_envelope("one job-group run")
            .expect("catalog describe should work for one job-group run");
        assert_eq!(env.data["path"], "one/job-group/run");

        let env = catalog_describe_envelope("one job-group pdf-results")
            .expect("catalog describe should work for one job-group pdf-results");
        assert_eq!(env.data["path"], "one/job-group/pdf-results");

        let env = catalog_describe_envelope("one job-group publish")
            .expect("catalog describe should work for one job-group publish");
        assert_eq!(env.data["path"], "one/job-group/publish");

        let env = catalog_describe_envelope("one output-object wrangle-to-python")
            .expect("catalog describe should work for one output-object wrangle-to-python");
        assert_eq!(env.data["path"], "one/output-object/wrangle-to-python");

        let env = catalog_describe_envelope("one output-object create")
            .expect("catalog describe should work for one output-object create");
        assert_eq!(env.data["path"], "one/output-object/create");

        let env = catalog_describe_envelope("one webhook-flow-task create")
            .expect("catalog describe should work for one webhook-flow-task create");
        assert_eq!(env.data["path"], "one/webhook-flow-task/create");

        let env = catalog_describe_envelope("one webhook-flow-task delete")
            .expect("catalog describe should work for one webhook-flow-task delete");
        assert_eq!(env.data["path"], "one/webhook-flow-task/delete");

        let env = catalog_describe_envelope("one write-setting create")
            .expect("catalog describe should work for one write-setting create");
        assert_eq!(env.data["path"], "one/write-setting/create");

        let env = catalog_describe_envelope("one write-setting update")
            .expect("catalog describe should work for one write-setting update");
        assert_eq!(env.data["path"], "one/write-setting/update");

        let env = catalog_describe_envelope("one platform api open-api-spec")
            .expect("catalog describe should work for one platform api open-api-spec");
        assert_eq!(env.data["path"], "one/platform/api/open-api-spec");

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

        // Use serde_json to build the JSON literal so backslashes in Windows
        // paths (e.g. `D:\a\...`) are escaped properly. The previous
        // `format!(r#"...{}"#)` interpolation produced invalid JSON on
        // Windows CI runners.
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
