use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use roxmltree::Document;
use serde_json::{json, Value};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::envelope::Envelope;
use ayx_core::profile::{Config, ServerProfile};
use ayx_one::{
    api_diagnose_envelope, api_inventory_envelope, api_status_envelope,
    one_surface_inventory_envelope,
};
use ayx_one_api::{
    flow_export_package_envelope, flow_import_package_envelope, one_api_live_request,
    one_api_live_request_with_body,
};
use ayx_server::logs::{
    discover_log_inventory, extract_context, parse_gallery_csv, parse_gallery_events,
    parse_service_events, recent_log_candidates, summarize_log_file, tail_log_file,
};
use ayx_server::mongo::{
    backup_envelope, doctor_envelope as mongo_doctor_envelope, inventory_envelope,
    query_envelope as mongo_query_envelope, restore_envelope, status_envelope,
};
use ayx_server::sqlserver::{
    connection_string_envelope, inventory_envelope as sqlserver_inventory_envelope,
    migration_prepare_envelope, precheck_envelope as sqlserver_precheck_envelope,
    status_envelope as sqlserver_status_envelope, validate_connection_strings_envelope,
};
use ayx_server::upgrade::{
    compute_path, run_apply, run_backup, run_bundle, run_plan, run_postcheck, run_precheck,
};
use ayx_server::util::{
    ayx_paths, backup_plan, capture_system_info, run_server_backup, runtime_settings_summary,
    write_runtime_settings_json,
};
use ayx_server::{call_operation, diagnose_api, import_swagger};
use ayx_server_api::workflow_version_upload_envelope;
use ayx_workflow::{
    convert_desktop_to_cloud, inspect as inspect_workflow, load_rules as load_workflow_rules,
    migrate as migrate_workflow, read_yxdb as read_yxdb_workflow, recurse as recurse_workflow,
    repackage_dir as repackage_workflow, replace as replace_workflow, scan as scan_workflow,
    unpack_package as unpack_workflow, validate as validate_workflow, CloudConversionOptions,
    WorkflowReplacement,
};
use self_update::backends::github::Update as GitHubUpdate;
use self_update::Status;

mod capability;
mod onboard;

#[derive(Parser, Debug)]
#[command(name = "ayx")]
#[command(about = "AYX Rust CLI")]
struct Cli {
    #[arg(long, default_value = "text")]
    output: String,
    #[arg(long)]
    environment: Option<String>,

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
    #[command(about = "Cross-environment tools for workspace.yaml source/target workflows")]
    Tools {
        #[command(subcommand)]
        command: Option<ToolsCommand>,
    },
    #[command(
        about = "Interactive first-run setup for config.yaml or workspace.yaml with validation and secret reuse"
    )]
    Onboard {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
        #[arg(long)]
        workspace: bool,
        #[arg(long)]
        non_interactive: bool,
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
        #[arg(long, default_value = "ayx-cli")]
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
}

#[derive(Subcommand, Debug)]
enum MongoCommand {
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
enum ServerCommand {
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
enum SqlserverCommand {
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
enum WorkflowCommand {
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
enum UiCommand {
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
enum UiSessionCommand {
    Status,
    Ensure,
    Attach {
        #[arg(long)]
        tab: Option<String>,
    },
    Inventory,
}

#[derive(Subcommand, Debug)]
enum UiWorkflowCommand {
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
enum UiDataCommand {
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
enum UiLibraryCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
enum UiSchedulesCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
enum UiJobsCommand {
    Inventory,
}

#[derive(Subcommand, Debug)]
enum ToolsCommand {
    Workspace {
        #[command(subcommand)]
        command: Option<ToolsWorkspaceCommand>,
    },
}

#[derive(Subcommand, Debug)]
enum ToolsWorkspaceCommand {
    Init {
        #[arg(long, default_value = "workspace.yaml")]
        output: PathBuf,
        #[arg(long, default_value = "dev")]
        active_environment: String,
        #[arg(long, default_value = "dev")]
        source_environment: String,
        #[arg(long, default_value = "prod")]
        target_environment: String,
    },
    Resolve {
        #[arg(long, default_value = "workspace.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    Compare {
        #[arg(long, default_value = "workspace.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    MigrateWorkflows {
        #[arg(long, default_value = "workspace.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    CheckDcmConnections {
        #[arg(long, default_value = "workspace.yaml")]
        workspace: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
}

#[derive(Subcommand, Debug)]
enum OneCommand {
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
enum OnePlatformCommand {
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
enum OnePlatformTokenCommand {
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
enum OnePlatformPersonCommand {
    List,
    Current,
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
enum OneWorkspaceCommand {
    List,
    Current,
    CurrentConfiguration,
    SaveCurrentConfiguration {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneRoleCommand {
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
enum OnePlatformApiCommand {
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
enum OnePlatformAuthCommand {
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
enum OnePlansCommand {
    List {
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
enum OneFlowsCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneConnectionsCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneConnectorMetadataCommand {
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
enum OneConnectorMetadataOverridesCommand {
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
enum OneConnectionPermissionCommand {
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
enum OneJobGroupCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneOutputObjectCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneWebhookFlowTaskCommand {
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
enum OneWriteSettingCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneSchedulingCommand {
    List {
        #[arg(long, default_value = "config.yaml")]
        profile: PathBuf,
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
enum OneBillingCommand {
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
enum OneDoctorCommand {
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
struct CommandSpec {
    name: &'static str,
    path: &'static str,
    summary: &'static str,
    output: &'static str,
    safety: &'static str,
    mutating: bool,
    prerequisites: &'static [&'static str],
    notes: &'static [&'static str],
}

const COMMAND_SPECS: &[CommandSpec] = &[
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
        notes: &["Maps to GET /iam/v1/workspaces/current in managed-iam-v1.yaml."],
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
            "Route workflow guidance through Walter once the symptom is known.",
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
        notes: &["Maps to PUT /v4/outputObjects/{id} in the One API docs."],
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
enum ServerLogsCommand {
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
enum ServerDiagnoseCommand {
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
enum ServerAuthCommand {
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
enum ServerAuthDiagnoseCommand {
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
enum ServerAuthSimulateCommand {
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
enum ServerDoctorCommand {
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
enum ServerApiCommand {
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
enum UpgradeCommand {
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

fn load_payload(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read payload file '{}'", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON payload from '{}'", path.display()))?;
    Ok(value)
}

fn tools_workspace_init_envelope(
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
        "workspace template written",
        json!({
            "workspace": output.display().to_string(),
            "active_environment": active_environment,
            "environments": [source_environment, target_environment],
            "notes": [
                "workspace.yaml is the canonical multi-environment file",
                "Use --environment to override the active environment for a run",
            ],
        }),
    ))
}

fn tools_workspace_resolve_envelope(
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

fn tools_workspace_compare_envelope(
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

fn tools_workspace_migrate_envelope(
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

fn server_profile(config: &Config) -> Result<&ServerProfile> {
    config.server.as_ref().ok_or_else(|| {
        anyhow!(
            "config missing server section; add server.webapi_url, curator_api_key, and curator_api_secret"
        )
    })
}

fn parse_key_value_params(items: &[String]) -> Result<HashMap<String, String>> {
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
    let load_profile = |path: &Path| -> Result<Config> {
        Ok(Config::load_from_path_with_environment(
            path,
            cli.environment.as_deref(),
        )?)
    };
    let envelope = match cli.command {
        Command::Mongo { command } => match command {
            MongoCommand::Status { profile } => {
                let profile = load_profile(&profile)?;
                status_envelope(&profile)?
            }
            MongoCommand::Inventory { profile } => {
                let profile = load_profile(&profile)?;
                inventory_envelope(&profile)?
            }
            MongoCommand::Backup {
                profile,
                output_dir,
                apply,
                audit_dir,
            } => {
                let profile = load_profile(&profile)?;
                backup_envelope(&profile, &output_dir, apply, &audit_dir)?
            }
            MongoCommand::Restore {
                profile,
                input_path,
                apply,
                audit_dir,
            } => {
                let profile = load_profile(&profile)?;
                restore_envelope(&profile, &input_path, apply, &audit_dir)?
            }
            MongoCommand::Query {
                profile,
                database,
                collection,
                filter,
                projection,
                sort,
                limit,
                print,
                apply,
                template,
            } => {
                let profile = load_profile(&profile)?;
                let spec = ayx_server::mongo::resolve_query_spec(
                    &profile,
                    database.as_deref(),
                    collection.as_deref(),
                    filter.as_deref(),
                    projection.as_deref(),
                    sort.as_deref(),
                    limit,
                    template.as_deref(),
                )?;
                mongo_query_envelope(&profile, &spec, print, apply)?
            }
            MongoCommand::Doctor { profile } => {
                let profile = load_profile(&profile)?;
                mongo_doctor_envelope(&profile)?
            }
            MongoCommand::Mutate {
                profile,
                database,
                collection,
                filter,
                update,
                template,
                print,
                apply,
                accept_mutation_risk,
            } => {
                let profile = load_profile(&profile)?;
                ayx_server::mongo::mutate_envelope(
                    &profile,
                    database.as_deref(),
                    collection.as_deref(),
                    filter.as_deref(),
                    update.as_deref(),
                    template.as_deref(),
                    print,
                    apply,
                    accept_mutation_risk,
                )?
            }
        },
        Command::Server { command } => match command {
            None => Envelope::ok("server commands: api, system-info, runtime-settings, ayx-paths, server-logs, backup-plan, backup"),
            Some(ServerCommand::Api { command }) => match command {
                ServerApiCommand::Status { profile } => {
                    let config = load_profile(&profile)?;
                    let server = server_profile(&config)?;
                    let api_logging = config.observability.as_ref().and_then(|obs| {
                        obs.api_logging.as_ref().map(|logging| json!({
                            "enabled": logging.enabled,
                            "path": logging.path,
                            "redact_bodies": logging.redact_bodies,
                            "log_requests": logging.log_requests,
                            "log_responses": logging.log_responses,
                        }))
                    });
                    Envelope::ok_with_data(
                        "server api status",
                        json!({
                            "profile": config.profile_name,
                            "base_url": server.webapi_url,
                            "verify_tls": server.verify_tls(),
                            "observability": api_logging,
                            "has_credentials": {
                                "curator_api_key": !server.curator_api_key.is_empty(),
                                "curator_api_secret": !server.curator_api_secret.is_empty()
                            }
                        }),
                    )
                }
                ServerApiCommand::Diagnose { profile } => {
                    let config = load_profile(&profile)?;
                    let server = server_profile(&config)?;
                    diagnose_api(server, config.observability.as_ref())?
                }
                ServerApiCommand::ImportSwagger {
                    profile,
                    version,
                    url,
                    cache_dir,
                } => {
                    let config = load_profile(&profile)?;
                    let server = server_profile(&config)?;
                    let cache_name = format!("{}_swagger_v{}.json", config.profile_name, version);
                    import_swagger(server, config.observability.as_ref(), &url, &cache_dir, &cache_name)?
                }
                ServerApiCommand::Call {
                    profile,
                    operation_id,
                    version,
                    cache_dir,
                    swagger,
                    body,
                    param,
                } => {
                    let config = load_profile(&profile)?;
                    let server = server_profile(&config)?;
                    let cache_name = format!("{}_swagger_v{}.json", config.profile_name, version);
                    let swagger_path = swagger
                        .clone()
                        .unwrap_or_else(|| cache_dir.join(&cache_name));
                    if !swagger_path.exists() {
                        bail!(
                            "swagger '{}' not found; run server api import-swagger first",
                            swagger_path.display()
                        );
                    }
                    let params = parse_key_value_params(&param)?;
                    let payload = match body {
                        Some(path) => Some(load_payload(&path)?),
                        None => None,
                    };
                    call_operation(server, config.observability.as_ref(), &operation_id, &params, payload, &swagger_path)?
                }
            },
            Some(ServerCommand::SystemInfo { output }) => {
                let system_info = capture_system_info()?;
                fs::write(&output, serde_json::to_string_pretty(&system_info)?)
                    .with_context(|| format!("failed to write '{}'", output.display()))?;
                Envelope::ok_with_data(
                    "system info captured",
                    json!({ "output": output.display().to_string(), "data": system_info }),
                )
            }
            Some(ServerCommand::RuntimeSettings { path, output }) => {
                let summary = runtime_settings_summary(&path)?;
                if let Some(ref output_path) = output {
                    write_runtime_settings_json(&path, output_path)?;
                }
                Envelope::ok_with_data(
                    "runtime settings summarized",
                    json!({
                        "path": path.display().to_string(),
                        "output": output.as_ref().map(|p| p.display().to_string()),
                        "data": summary
                    }),
                )
            }
            Some(ServerCommand::AyxPaths) => {
                let paths = ayx_paths();
                Envelope::ok_with_data("ayx paths resolved", paths)
            }
            Some(ServerCommand::ServerLogs { command }) => match command {
                ServerLogsCommand::Discover { profile } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "log sources discovered",
                        discover_log_inventory(&config),
                    )
                }
                ServerLogsCommand::Inventory { profile } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "log inventory discovered",
                        discover_log_inventory(&config),
                    )
                }
                ServerLogsCommand::Summary { path } => {
                    let summary = summarize_log_file(&path)?;
                    Envelope::ok_with_data("log summary generated", summary)
                }
                ServerLogsCommand::Context {
                    path,
                    query,
                    before,
                    after,
                } => {
                    let context = extract_context(&path, &query, before, after)?;
                    Envelope::ok_with_data("log context extracted", context)
                }
                ServerLogsCommand::ParseCsv { path } => {
                    let parsed = parse_gallery_csv(&path)?;
                    Envelope::ok_with_data("gallery csv parsed", parsed)
                }
                ServerLogsCommand::ServiceEvents { path } => {
                    let parsed = parse_service_events(&path)?;
                    Envelope::ok_with_data("service log events parsed", parsed)
                }
                ServerLogsCommand::GalleryEvents { path } => {
                    let parsed = parse_gallery_events(&path)?;
                    Envelope::ok_with_data("gallery log events parsed", parsed)
                }
                ServerLogsCommand::Tail { path, lines } => {
                    let tail = tail_log_file(&path, lines)?;
                    Envelope::ok_with_data("log tail generated", tail)
                }
                ServerLogsCommand::Recent { profile, days } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "recent log candidates discovered",
                        recent_log_candidates(&config, days),
                    )
                }
            },
            Some(ServerCommand::Diagnose { command }) => match command {
                ServerDiagnoseCommand::Startup { profile, error, log_file } => {
                    let config = load_profile(&profile)?;
                    let mut steps = vec![
                        json!({
                            "step": "collect_log_sources",
                            "action": "discover available Server log sources",
                            "status": "done",
                            "evidence": discover_log_inventory(&config),
                        }),
                        json!({
                            "step": "inspect_runtime_settings",
                            "action": "summarize RuntimeSettings.xml and embedded Mongo settings",
                            "status": "done",
                            "evidence": runtime_settings_summary(
                                &config
                                    .mongo
                                    .embedded
                                    .as_ref()
                                    .and_then(|e| e.runtime_settings_path.as_ref())
                                    .map(PathBuf::from)
                                    .unwrap_or_else(|| PathBuf::from(DEFAULT_RUNTIME_SETTINGS_PATH))
                            )?,
                        }),
                    ];
                    if let Some(path) = log_file {
                        let mut evidence = json!({
                            "log_file": path.display().to_string(),
                            "log_summary": summarize_log_file(&path)?,
                        });
                        if let Some(error_text) = error.as_ref() {
                            evidence["error_context"] = json!(extract_context(&path, error_text, 25, 25)?);
                        }
                        steps.push(json!({
                            "step": "inspect_supplied_log",
                            "action": "summarize the supplied startup log and extract error context",
                            "status": "done",
                            "evidence": evidence,
                        }));
                    } else {
                        let evidence = json!({
                            "error": error,
                            "recent_candidates": recent_log_candidates(&config, 7),
                        });
                        steps.push(json!({
                            "step": "find_recent_candidates",
                            "action": "identify likely startup-related logs to inspect next",
                            "status": "done",
                            "evidence": evidence,
                        }));
                    }
                    Envelope::ok_with_data(
                        "server startup diagnosis generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": steps,
                        }),
                    )
                }
                ServerDiagnoseCommand::Logs { profile } => {
                    let config = load_profile(&profile)?;
                    let logs = discover_log_inventory(&config);
                    Envelope::ok_with_data(
                        "server log diagnosis generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "discover_log_sources",
                                    "action": "identify Service, Gallery, Engine, SSO, and config-change logs",
                                    "status": "done",
                                    "evidence": logs,
                                }
                            ]
                        }),
                    )
                }
                ServerDiagnoseCommand::Network { profile } => {
                    let config = load_profile(&profile)?;
                    let paths = ayx_paths();
                    let detail = json!({
                        "profile": profile.display().to_string(),
                        "server": config.server.as_ref().map(|s| json!({
                            "webapi_url": s.webapi_url,
                            "verify_tls": s.verify_tls(),
                        })),
                        "paths": paths,
                        "checks": [
                            "Use Test-NetConnection against controller port 80/443/27018",
                            "Use netsh winhttp show proxy for proxy state",
                            "Use netstat -aon and tasklist for port ownership",
                            "Use nltest /dsgetdc and /dclist for domain controller lookup",
                        ]
                    });
                    Envelope::ok_with_data(
                        "server network diagnosis generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "check_local_paths",
                                    "action": "resolve Server-related filesystem paths",
                                    "status": "done",
                                    "evidence": paths,
                                },
                                {
                                    "step": "review_network_checks",
                                    "action": "follow the standard port, proxy, and domain controller checks",
                                    "status": "done",
                                    "evidence": detail,
                                }
                            ]
                        }),
                    )
                }
                ServerDiagnoseCommand::Tls { profile } => {
                    let config = load_profile(&profile)?;
                    let detail = json!({
                        "profile": profile.display().to_string(),
                        "server": config.server.as_ref().map(|s| json!({
                            "webapi_url": s.webapi_url,
                            "verify_tls": s.verify_tls(),
                        })),
                        "checks": [
                            {
                                "name": "https_endpoint",
                                "action": "verify the Server web API URL is https and reachable",
                                "evidence": config.server.as_ref().map(|s| s.webapi_url.clone()),
                            },
                            {
                                "name": "certificate_binding",
                                "action": "confirm the HTTPS port has a valid certificate binding",
                                "evidence": "Use netsh http show sslcert and compare the certificate subject and thumbprint",
                            },
                            {
                                "name": "proxy_configuration",
                                "action": "inspect WinHTTP proxy configuration and browser proxy dependencies",
                                "evidence": "Use netsh winhttp show proxy and validate any required proxy exceptions",
                            },
                            {
                                "name": "port_binding",
                                "action": "check whether 443 is already owned by another process or service",
                                "evidence": "Use netstat -aon and tasklist to map port 443 to a PID and process name",
                            },
                            {
                                "name": "controller_worker_tls",
                                "action": "verify TLS between nodes when worker/controller communication depends on HTTPS",
                                "evidence": "Confirm the controller certificate is trusted by workers and that the configured port matches the TLS setup",
                            }
                        ],
                        "related_commands": [
                            "ayx server diagnose network",
                            "ayx server doctor network",
                            "ayx server logs context --query \"SSL\"",
                        ],
                    });
                    Envelope::ok_with_data("server tls diagnosis generated", detail)
                }
                ServerDiagnoseCommand::RuntimeSettings { profile } => {
                    let config = load_profile(&profile)?;
                    let path = config
                        .mongo
                        .embedded
                        .as_ref()
                        .and_then(|e| e.runtime_settings_path.as_ref())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(DEFAULT_RUNTIME_SETTINGS_PATH));
                    let summary = runtime_settings_summary(&path)?;
                    Envelope::ok_with_data(
                        "server runtime settings diagnosis generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "load_runtime_settings",
                                    "action": "read and summarize RuntimeSettings.xml",
                                    "status": "done",
                                    "evidence": {
                                        "path": path.display().to_string(),
                                        "data": summary,
                                    }
                                }
                            ]
                        }),
                    )
                }
            },
            Some(ServerCommand::Auth { command }) => match command {
                ServerAuthCommand::Status { profile } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "server auth status generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "status": build_auth_status(&config, None, None, None, None),
                        }),
                    )
                }
                ServerAuthCommand::Diagnose { command } => match command {
                    ServerAuthDiagnoseCommand::Saml {
                        profile,
                        metadata_url,
                        metadata_file,
                        acs_url,
                        issuer,
                    } => {
                        let config = load_profile(&profile)?;
                        let status = build_auth_status(
                            &config,
                            metadata_url.as_deref(),
                            metadata_file.as_deref(),
                            acs_url.as_deref(),
                            issuer.as_deref(),
                        );
                        Envelope::ok_with_data(
                            "server saml diagnosis generated",
                            json!({
                                "profile": profile.display().to_string(),
                                "status": status,
                                "checks": [
                                    "Confirm the auth type is SAML",
                                    "Verify metadata URL or file availability",
                                    "Compare issuer / entity ID / ACS URL expectations",
                                    "Confirm TLS certificate trust and signing posture",
                                    "Review recent SSO/AAS logs for the exact failure",
                                ]
                            }),
                        )
                    }
                    ServerAuthDiagnoseCommand::SamlLogs { profile, days } => {
                        let config = load_profile(&profile)?;
                        let logs = recent_log_candidates(&config, days);
                        let detail = json!({
                            "profile": profile.display().to_string(),
                            "log_families": discover_log_inventory(&config),
                            "recent_candidates": logs,
                            "targets": [
                                "alteryx-sso-YYYYMMDD.log",
                                "aas-log-YYYYMMDD.log",
                            ],
                            "checks": [
                                "Look for login failures and redirect/callback errors",
                                "Correlate successful and unsuccessful login attempts",
                                "Check for SAML assertion or signature failures",
                            ],
                        });
                        Envelope::ok_with_data("server saml log diagnosis generated", detail)
                    }
                    ServerAuthDiagnoseCommand::Certificate {
                        profile,
                        certificate_file,
                    } => {
                        let config = load_profile(&profile)?;
                        let cert_path = certificate_file.as_ref().map(|p| p.display().to_string());
                        let detail = json!({
                            "profile": profile.display().to_string(),
                            "server": config.server.as_ref().map(|s| json!({
                                "webapi_url": s.webapi_url,
                                "verify_tls": s.verify_tls(),
                            })),
                            "certificate_file": cert_path,
                            "checks": [
                                "Confirm the certificate file or certificate store reference is available",
                                "Confirm the certificate subject matches the expected Server hostname",
                                "Confirm the certificate chain is trusted on the server and worker nodes",
                                "Confirm the certificate is valid for the configured HTTPS binding",
                            ],
                        });
                        Envelope::ok_with_data("server certificate diagnosis generated", detail)
                    }
                    ServerAuthDiagnoseCommand::AdLegacy {
                        profile,
                        user,
                        domain,
                    } => {
                        let config = load_profile(&profile)?;
                        let detail = json!({
                            "profile": profile.display().to_string(),
                            "legacy_auth": {
                                "user": user,
                                "domain": domain,
                            },
                            "checks": [
                                "Confirm domain membership and controller reachability",
                                "Confirm the legacy Windows auth user context is valid",
                                "Confirm any expected AD group membership or sync path",
                            ],
                            "reference_only": true,
                            "server": config.server.as_ref().map(|s| json!({
                                "webapi_url": s.webapi_url,
                                "verify_tls": s.verify_tls(),
                            })),
                        });
                        Envelope::ok_with_data("server legacy ad diagnosis generated", detail)
                    }
                },
                ServerAuthCommand::Simulate { command } => match command {
                    ServerAuthSimulateCommand::Saml {
                        profile,
                        metadata_url,
                        metadata_file,
                        acs_url,
                        issuer,
                        entity_id,
                        certificate_file,
                        prompt,
                    } => {
                        let config = load_profile(&profile)?;
                        let status = build_auth_status(
                            &config,
                            metadata_url.as_deref(),
                            metadata_file.as_deref(),
                            acs_url.as_deref(),
                            issuer.as_deref(),
                        );
                        let parsed_metadata = metadata_url
                            .as_deref()
                            .map(|url| parse_saml_metadata_source(&format!("metadata_url={url}")))
                            .transpose()?
                            .or_else(|| {
                                metadata_file
                                    .as_ref()
                                    .map(|path| {
                                        parse_saml_metadata_source(
                                            &path.display().to_string(),
                                        )
                                    })
                                    .transpose()
                                    .ok()
                                    .flatten()
                            });
                        let detail = json!({
                            "profile": profile.display().to_string(),
                            "prompt_mode": prompt,
                            "inputs": {
                                "metadata_url": metadata_url,
                                "metadata_file": metadata_file.as_ref().map(|p| p.display().to_string()),
                                "acs_url": acs_url,
                                "issuer": issuer,
                                "entity_id": entity_id,
                                "certificate_file": certificate_file.as_ref().map(|p| p.display().to_string()),
                            },
                            "simulation": {
                                "auth": status,
                                "parsed_metadata": parsed_metadata,
                                "outcomes": [
                                    "metadata fetch / parse",
                                    "issuer alignment",
                                    "acs / callback alignment",
                                    "certificate trust validation",
                                    "clock skew / validity window check",
                                ],
                            },
                            "next_steps": [
                                "Use server auth diagnose saml for exact mismatch analysis",
                                "Use server auth diagnose saml-logs for login trace review",
                            ]
                        });
                        Envelope::ok_with_data("server saml simulation generated", detail)
                    }
                },
            },
            Some(ServerCommand::Doctor { command }) => match command {
                ServerDoctorCommand::Startup { profile, error, log_file } => {
                    let config = load_profile(&profile)?;
                    let runtime_path = config
                        .mongo
                        .embedded
                        .as_ref()
                        .and_then(|e| e.runtime_settings_path.as_ref())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(DEFAULT_RUNTIME_SETTINGS_PATH));
                    let mut steps = vec![
                        json!({
                            "step": "verify_runtime_settings",
                            "action": "confirm runtime settings and embedded Mongo configuration",
                            "status": "done",
                            "evidence": runtime_settings_summary(&runtime_path)?,
                        }),
                        json!({
                            "step": "discover_recent_logs",
                            "action": "identify likely startup-related logs",
                            "status": "done",
                            "evidence": recent_log_candidates(&config, 7),
                        }),
                    ];
                    if let Some(path) = log_file {
                        let mut evidence = json!({
                            "log_file": path.display().to_string(),
                            "summary": summarize_log_file(&path)?,
                        });
                        if let Some(error_text) = error.as_ref() {
                            evidence["error_context"] = json!(extract_context(&path, error_text, 25, 25)?);
                        }
                        steps.push(json!({
                            "step": "pinpoint_error",
                            "action": "extract the exact failure context from the supplied log",
                            "status": "done",
                            "evidence": evidence,
                        }));
                    }
                    Envelope::ok_with_data(
                        "server startup doctor workflow generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": steps,
                            "recommendations": [
                                "Use server diagnose startup to inspect a specific failure",
                                "Use server logs summary or context for raw log follow-up",
                                "If the issue is network-related, proceed to server doctor network",
                            ]
                        }),
                    )
                }
                ServerDoctorCommand::Logs { profile } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "server log doctor workflow generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "discover_log_sources",
                                    "action": "enumerate Server log families and file locations",
                                    "status": "done",
                                    "evidence": discover_log_inventory(&config),
                                },
                                {
                                    "step": "select_log_family",
                                    "action": "choose the relevant log family by symptom",
                                    "status": "done",
                                    "evidence": {
                                        "families": [
                                            "service",
                                            "gallery",
                                            "engine",
                                            "aas",
                                            "config_changes",
                                        ]
                                    }
                                }
                            ],
                            "recommendations": [
                                "Use server logs summary on the selected file",
                                "Use server logs context with a symptom-specific query",
                                "Use server diagnose startup when the service will not start",
                            ]
                        }),
                    )
                }
                ServerDoctorCommand::Network { profile } => {
                    let config = load_profile(&profile)?;
                    Envelope::ok_with_data(
                        "server network doctor workflow generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "resolve_paths",
                                    "action": "identify the Server filesystem paths and runtime settings location",
                                    "status": "done",
                                    "evidence": ayx_paths(),
                                },
                                {
                                    "step": "inspect_server_config",
                                    "action": "confirm web API URL and TLS behavior",
                                    "status": "done",
                                    "evidence": config.server.as_ref().map(|s| json!({
                                        "webapi_url": s.webapi_url,
                                        "verify_tls": s.verify_tls(),
                                    })),
                                },
                                {
                                    "step": "follow_standard_network_checks",
                                    "action": "run port, proxy, domain controller, and DNS checks",
                                    "status": "done",
                                    "evidence": [
                                        "Test-NetConnection on 80, 443, and 27018",
                                        "netsh winhttp show proxy",
                                        "netstat -aon plus tasklist to identify port owners",
                                        "nltest /dsgetdc and /dclist",
                                        "nslookup and ping for name resolution",
                                    ]
                                }
                            ],
                            "recommendations": [
                                "Run ayx server diagnose tls for TLS and certificate validation",
                                "If SSL binding is the problem, inspect the 443 reservation and cert binding",
                                "If workers are missing, validate controller-to-worker connectivity on the configured port",
                            ]
                        }),
                    )
                }
                ServerDoctorCommand::RuntimeSettings { profile } => {
                    let config = load_profile(&profile)?;
                    let path = config
                        .mongo
                        .embedded
                        .as_ref()
                        .and_then(|e| e.runtime_settings_path.as_ref())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(DEFAULT_RUNTIME_SETTINGS_PATH));
                    let summary = runtime_settings_summary(&path)?;
                    Envelope::ok_with_data(
                        "server runtime settings doctor workflow generated",
                        json!({
                            "profile": profile.display().to_string(),
                            "steps": [
                                {
                                    "step": "read_runtime_settings",
                                    "action": "summarize the effective Server runtime settings",
                                    "status": "done",
                                    "evidence": {
                                        "path": path.display().to_string(),
                                        "data": summary,
                                    }
                                },
                                {
                                    "step": "derive_action_items",
                                    "action": "translate the settings into validation checkpoints",
                                    "status": "done",
                                    "evidence": [
                                        "Confirm embedded Mongo root path",
                                        "Confirm gallery logging path",
                                        "Confirm engine log file path",
                                        "Confirm auth type and Mongo host/port"
                                    ]
                                }
                            ]
                        }),
                    )
                }
            },
            Some(ServerCommand::Upgrade { command }) => match command {
                UpgradeCommand::Path {
                    from,
                    to,
                    deployment,
                } => {
                    let detail = compute_path(&from, &to, &deployment);
                    Envelope::ok_with_data("upgrade path computed", detail)
                }
                UpgradeCommand::Precheck {
                    profile,
                    target,
                    out,
                    deployment,
                } => {
                    let config = load_profile(&profile)?;
                    let detail = run_precheck(&config, &target, &out, &deployment)?;
                    Envelope::ok_with_data("upgrade precheck completed", detail)
                }
                UpgradeCommand::Backup {
                    profile,
                    r#type,
                    out,
                } => {
                    let config = load_profile(&profile)?;
                    let detail = run_backup(&config, &r#type, &out)?;
                    Envelope::ok_with_data("upgrade backup completed", detail)
                }
                UpgradeCommand::Plan {
                    from,
                    to,
                    out,
                    deployment,
                } => {
                    let detail = run_plan(&from, &to, &deployment, &out)?;
                    Envelope::ok_with_data("upgrade plan generated", detail)
                }
                UpgradeCommand::Apply {
                    manifest,
                    apply,
                    yes,
                } => {
                    let detail = run_apply(&manifest, apply, yes)?;
                    Envelope::ok_with_data("upgrade apply simulated", detail)
                }
                UpgradeCommand::Postcheck {
                    profile,
                    manifest,
                    out,
                } => {
                    let config = load_profile(&profile)?;
                    let detail = run_postcheck(&config, &manifest, &out)?;
                    Envelope::ok_with_data("upgrade postcheck completed", detail)
                }
                UpgradeCommand::Bundle { input, out } => {
                    let detail = run_bundle(&input, &out)?;
                    Envelope::ok_with_data("upgrade bundle created", detail)
                }
            },
            Some(ServerCommand::BackupPlan { backup_dir }) => {
                let plan = backup_plan(&backup_dir)?;
                Envelope::ok_with_data("backup plan generated", plan)
            }
            Some(ServerCommand::Backup {
                profile,
                backup_dir,
                apply,
                audit_dir,
            }) => {
                let config = load_profile(&profile)?;
                let data = run_server_backup(&config, &backup_dir, apply, &audit_dir)?;
                Envelope::ok_with_data(
                    if apply {
                        "server backup executed"
                    } else {
                        "dry-run only: pass --apply to execute server backup"
                    },
                    data,
                )
            }
        },
        Command::Sqlserver { command } => match command {
            None => Envelope::ok(
                "sqlserver commands available: status, inventory, precheck, connection-string, migrate",
            ),
            Some(SqlserverCommand::Status { profile }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver status summarized",
                    sqlserver_status_envelope(&config)?,
                )
            }
            Some(SqlserverCommand::Inventory { profile }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver inventory summarized",
                    sqlserver_inventory_envelope(&config)?,
                )
            }
            Some(SqlserverCommand::Precheck { profile, collation }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver precheck summarized",
                    sqlserver_precheck_envelope(&config, collation.as_deref())?,
                )
            }
            Some(SqlserverCommand::ValidateStrings { profile }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver connection strings validated",
                    validate_connection_strings_envelope(&config)?,
                )
            }
            Some(SqlserverCommand::ConnectionString {
                profile,
                scope,
                auth,
                server,
                database,
                port,
                encrypt,
                trust_server_certificate,
                multi_subnet_failover,
            }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver connection string generated",
                    connection_string_envelope(
                        &config,
                        &scope,
                        &auth,
                        server.as_deref(),
                        database.as_deref(),
                        port,
                        encrypt,
                        trust_server_certificate,
                        multi_subnet_failover,
                    )?,
                )
            }
            Some(SqlserverCommand::Migrate {
                profile,
                target_version,
                dry_run,
            }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver migration plan generated",
                    migration_prepare_envelope(&config, target_version.as_deref(), dry_run)?,
                )
            }
            Some(SqlserverCommand::Prepare {
                profile,
                target_version,
                dry_run,
            }) => {
                let config = load_profile(&profile)?;
                Envelope::ok_with_data(
                    "sqlserver migration preparation generated",
                    migration_prepare_envelope(&config, target_version.as_deref(), dry_run)?,
                )
            }
        },
        Command::Workflow { command } => match command {
            None => Envelope::ok(
                "workflow commands available: inspect, unpack, validate, replace, repackage, recurse, scan, convert-cloud, publish, migrate, yxdb",
            ),
            Some(WorkflowCommand::Inspect { input }) => {
                let detail = inspect_workflow(&input)?;
                Envelope::ok_with_data(
                    "workflow inspection completed",
                    json!({
                        "input": input.display().to_string(),
                        "data": detail,
                    }),
                )
            }
            Some(WorkflowCommand::Unpack { input, output_dir }) => {
                let detail = unpack_workflow(&input, &output_dir)?;
                Envelope::ok_with_data(
                    "workflow package unpacked",
                    json!({
                        "input": input.display().to_string(),
                        "output_dir": output_dir.display().to_string(),
                        "data": detail,
                    }),
                )
            }
            Some(WorkflowCommand::Validate { input }) => {
                let detail = validate_workflow(&input)?;
                Envelope::ok_with_data(
                    "workflow validation completed",
                    json!({
                        "input": input.display().to_string(),
                        "data": detail,
                    }),
                )
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
                Envelope::ok_with_data(
                    "workflow replacement completed",
                    json!({
                        "input": input.display().to_string(),
                        "output": output.display().to_string(),
                        "data": detail,
                    }),
                )
            }
            Some(WorkflowCommand::Repackage { input_dir, output }) => {
                let detail = repackage_workflow(&input_dir, &output)?;
                Envelope::ok_with_data(
                    "workflow package rebuilt",
                    json!({
                        "input_dir": input_dir.display().to_string(),
                        "output": output.display().to_string(),
                        "data": detail,
                    }),
                )
            }
            Some(WorkflowCommand::Recurse {
                input,
                output,
                rules,
                find,
                replace,
                validate,
            }) => {
                let replacements = if let Some(rules) = rules.as_ref() {
                    let rules = load_workflow_rules(rules)?;
                    rules.replacements
                } else {
                    if find.len() != replace.len() {
                        bail!(
                            "workflow recurse requires the same number of --find and --replace values"
                        );
                    }
                    find.into_iter()
                        .zip(replace)
                        .map(|(find, replace)| WorkflowReplacement { find, replace })
                        .collect()
                };
                let detail = recurse_workflow(&input, &output, &replacements, validate)?;
                Envelope::ok_with_data(
                    "workflow recursion completed",
                    json!({
                        "input": input.display().to_string(),
                        "output": output.display().to_string(),
                        "data": detail,
                    }),
                )
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
                Envelope::ok_with_data(
                    "workflow migration completed",
                    json!({
                        "input": input.display().to_string(),
                        "output": output.display().to_string(),
                    "data": detail,
                }),
                )
            }
            Some(WorkflowCommand::Yxdb { input, csv }) => {
                let detail = read_yxdb_workflow(&input, csv.as_deref())?;
                Envelope::ok_with_data(
                    "workflow yxdb read completed",
                    json!({
                        "input": input.display().to_string(),
                        "csv": csv.as_ref().map(|path| path.display().to_string()),
                        "data": detail,
                    }),
                )
            }
            Some(WorkflowCommand::Scan {
                input,
                rules,
                find,
                replace,
            }) => {
                let replacements = if let Some(rules) = rules.as_ref() {
                    let rules = load_workflow_rules(rules)?;
                    rules.replacements
                } else {
                    if find.len() != replace.len() {
                        bail!(
                            "workflow scan requires the same number of --find and --replace values"
                        );
                    }
                    find.into_iter()
                        .zip(replace)
                        .map(|(find, replace)| WorkflowReplacement { find, replace })
                        .collect()
                };
                let detail = scan_workflow(&input, &replacements)?;
                Envelope::ok_with_data(
                    "workflow scan completed",
                    json!({
                        "input": input.display().to_string(),
                        "data": detail,
                    }),
                )
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
                Envelope::ok_with_data(
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
                )
            }
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
                let config = load_profile(&profile)?;
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
                Envelope::ok_with_data(
                    "workflow publish requested",
                    json!({
                        "input": input.display().to_string(),
                        "package_path": package_path.display().to_string(),
                        "data": detail,
                    }),
                )
            }
        },
        Command::Tools { command } => match command {
            None => Envelope::ok("tools workspace commands available: init, resolve, compare, migrate-workflows, check-dcm-connections"),
            Some(ToolsCommand::Workspace { command }) => match command {
                None => Envelope::ok("tools workspace commands available: init, resolve, compare, migrate-workflows, check-dcm-connections"),
                Some(ToolsWorkspaceCommand::Init {
                    output,
                    active_environment,
                    source_environment,
                    target_environment,
                }) => tools_workspace_init_envelope(
                    &output,
                    &active_environment,
                    &source_environment,
                    &target_environment,
                )?,
                Some(ToolsWorkspaceCommand::Resolve {
                    workspace,
                    source,
                    target,
                }) => tools_workspace_resolve_envelope(&workspace, &source, &target)?,
                Some(ToolsWorkspaceCommand::Compare {
                    workspace,
                    source,
                    target,
                }) => tools_workspace_compare_envelope(&workspace, &source, &target)?,
                Some(ToolsWorkspaceCommand::MigrateWorkflows {
                    workspace,
                    source,
                    target,
                }) => tools_workspace_migrate_envelope(&workspace, &source, &target, "workflows")?,
                Some(ToolsWorkspaceCommand::CheckDcmConnections {
                    workspace,
                    source,
                    target,
                }) => tools_workspace_migrate_envelope(&workspace, &source, &target, "dcm-connections")?,
            },
        },
        Command::Onboard {
            profile,
            workspace,
            non_interactive,
        } => {
            let detail = onboard::run_onboarding(
                &profile,
                cli.environment.as_deref(),
                non_interactive,
                workspace,
            )?;
            Envelope::ok_with_data("onboarding completed", detail)
        },
        Command::One { command } => match command {
            None => Envelope::ok(
                "one commands available: platform, plans, scheduling, billing, auto-insights, desktop-exec",
            ),
            Some(OneCommand::Doctor { command }) => match command {
                Some(OneDoctorCommand::Auth { profile }) => {
                    let config = load_profile(&profile)?;
                    one_platform_auth_diagnose_envelope(&config)?
                }
                Some(OneDoctorCommand::Discover { profile }) => {
                    let config = load_profile(&profile)?;
                    one_doctor_discover_envelope(&config)?
                }
                Some(OneDoctorCommand::Platform { profile }) => {
                    let config = load_profile(&profile)?;
                    one_doctor_platform_envelope(&config)?
                }
                Some(OneDoctorCommand::Plans { profile }) => {
                    let config = load_profile(&profile)?;
                    one_doctor_plans_envelope(&config)?
                }
                Some(OneDoctorCommand::Scheduling { profile }) => {
                    let config = load_profile(&profile)?;
                    one_doctor_scheduling_envelope(&config)?
                }
                Some(OneDoctorCommand::Billing { profile }) => {
                    let config = load_profile(&profile)?;
                    one_doctor_billing_envelope(&config)?
                }
                None => Envelope::ok("one doctor commands available: auth, discover, platform, plans, scheduling, billing"),
            },
            Some(OneCommand::Platform { command }) => match command {
                Some(OnePlatformCommand::Api { command }) => match command {
                    OnePlatformApiCommand::Status { profile } => {
                        let config = load_profile(&profile)?;
                        api_status_envelope(&config, "one platform")?
                    }
                    OnePlatformApiCommand::Diagnose { profile } => {
                        let config = load_profile(&profile)?;
                        api_diagnose_envelope(&config, "one platform")?
                    }
                    OnePlatformApiCommand::OpenApiSpec { profile } => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "open-api-spec",
                            "GET",
                            "/v4/open-api-spec",
                            false,
                            &[],
                        )?
                    }
                },
                Some(OnePlatformCommand::Status { profile }) => {
                    let config = load_profile(&profile)?;
                    api_status_envelope(&config, "one platform")?
                }
                Some(OnePlatformCommand::Inventory { profile }) => {
                    let config = load_profile(&profile)?;
                    one_surface_inventory_envelope(&config)?
                }
                Some(OnePlatformCommand::Workspace { command }) => match command {
                    OneWorkspaceCommand::List => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-list",
                            "GET",
                            "/v4/workspaces",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::CurrentConfiguration => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current-configuration",
                            "GET",
                            "/v4/workspaces/current/configuration",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::SaveCurrentConfiguration { profile, body } => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "workspace-save-current-configuration",
                            "PATCH",
                            "/v4/workspaces/current/configuration",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    OneWorkspaceCommand::Current => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current",
                            "GET",
                            "/iam/v1/workspaces/current",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::ConfigurationSchema { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-configuration-schema",
                            "GET",
                            "/v4/workspaces/{id}/configuration-schema",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::CurrentConfigurationSchema => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current-configuration-schema",
                            "GET",
                            "/v4/workspaces/current/configuration-schema",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::DeleteCurrentConfiguration { profile } => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-delete-current-configuration",
                            "POST",
                            "/v4/workspaces/current/delete-configuration",
                            true,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::DeleteConfiguration { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-delete-configuration",
                            "POST",
                            "/v4/workspaces/{id}/delete-configuration",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Configuration { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-configuration",
                            "GET",
                            "/iam/v1/workspaces/{id}/configuration",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::People { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-people",
                            "GET",
                            "/iam/v1/workspaces/{id}/people",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Admins { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-admins",
                            "GET",
                            "/iam/v1/workspaces/{workspaceId}/admins",
                            false,
                            &[("workspaceId", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::InviteUsers { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-invite-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/batch",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::RemoveUser {
                        workspace_id,
                        person_id,
                    } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-remove-user",
                            "DELETE",
                            "/iam/v1/workspaces/{id}/people/{personId}",
                            true,
                            &[("id", &workspace_id), ("personId", &person_id)],
                        )?
                    }
                    OneWorkspaceCommand::SuspendUsers { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-suspend-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/suspend",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::UnsuspendUsers { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-unsuspend-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/unsuspend",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Transfer { workspace_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-transfer",
                            "POST",
                            "/iam/v1/workspaces/{id}/transfer",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::TransferAssets { profile, body } => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "workspace-transfer-assets",
                            "PATCH",
                            "/v4/workspaces/current/transfer",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                },
                Some(OnePlatformCommand::Role { command }) => match command {
                    OneRoleCommand::ListAssignments { role_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-list-assignments",
                            "GET",
                            "/iam/v1/authorization/roles/{id}/people",
                            false,
                            &[("id", &role_id)],
                        )?
                    }
                    OneRoleCommand::Assign { role_id, subject_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-assign",
                            "POST",
                            "/iam/v1/authorization/roles/{id}/people/{subjectId}",
                            true,
                            &[("id", &role_id), ("subjectId", &subject_id)],
                        )?
                    }
                    OneRoleCommand::Unassign { role_id, subject_id } => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-unassign",
                            "DELETE",
                            "/iam/v1/authorization/roles/{id}/people/{subjectId}",
                            true,
                            &[("id", &role_id), ("subjectId", &subject_id)],
                        )?
                    }
                },
                Some(OnePlatformCommand::Auth { command }) => match command {
                    OnePlatformAuthCommand::Status { profile } => {
                        let config = load_profile(&profile)?;
                        one_platform_auth_status_envelope(&config)?
                    }
                    OnePlatformAuthCommand::Diagnose { profile } => {
                        let config = load_profile(&profile)?;
                        one_platform_auth_diagnose_envelope(&config)?
                    }
                },
                Some(OnePlatformCommand::User) => {
                    let config = load_profile(&PathBuf::from("config.yaml"))?;
                    one_api_live_request(
                        &config,
                        "platform",
                        "user-current",
                        "GET",
                        "/v4/people/current",
                        false,
                        &[],
                    )?
                }
                Some(OnePlatformCommand::Person { command }) => match command {
                    None | Some(OnePlatformPersonCommand::List) => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-list",
                            "GET",
                            "/v4/people",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Current) => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-current",
                            "GET",
                            "/v4/people/current",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Detail { profile, person_id }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-detail",
                            "GET",
                            "/v4/people/{id}",
                            false,
                            &[("id", &person_id)],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Create { profile, body }) => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-create",
                            "POST",
                            "/v4/people",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::UpdatePassword { profile, body }) => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-update-password",
                            "PATCH",
                            "/v4/people/current/updatePassword",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::PasswordResetRequest { profile, body }) => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-password-reset-request",
                            "POST",
                            "/v4/passwordresetrequest",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                },
                Some(OnePlatformCommand::Token { command }) => match command {
                    None | Some(OnePlatformTokenCommand::List) => {
                        let config = load_profile(&PathBuf::from("config.yaml"))?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-list",
                            "GET",
                            "/v4/apiAccessTokens",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformTokenCommand::Create { profile, body }) => {
                        let config = load_profile(&profile)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "api-access-tokens-create",
                            "POST",
                            "/v4/apiAccessTokens",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformTokenCommand::Detail { profile, token_id }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-detail",
                            "GET",
                            "/v4/apiAccessTokens/{tokenId}",
                            false,
                            &[("tokenId", &token_id)],
                        )?
                    }
                    Some(OnePlatformTokenCommand::Delete { profile, token_id }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-delete",
                            "DELETE",
                            "/v4/apiAccessTokens/{tokenId}",
                            true,
                            &[("tokenId", &token_id)],
                        )?
                    }
                },
                None => Envelope::ok("one platform commands available: api, auth, status, inventory, workspace, role, user, token, person"),
            },
            Some(OneCommand::JobGroups { command }) => match command {
                None => Envelope::ok(
                    "one job-group commands available: list, count, pdf-results, run, detail, cancel, status, inputs, outputs, jobs, publications, profile, profile-results",
                ),
                Some(OneJobGroupCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "jobGroup", "list", "GET", "/v4/jobLibrary", false, &[])?
                }
                Some(OneJobGroupCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "count",
                        "GET",
                        "/v4/jobLibrary/count",
                        false,
                        &[],
                    )?
                }
                Some(OneJobGroupCommand::Run { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "jobGroup",
                        "run",
                        "POST",
                        "/v4/jobGroups",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneJobGroupCommand::PdfResults { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id =
                        job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "pdf-results",
                        "GET",
                        "/v4/jobGroups/{id}/pdfResults",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Detail { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "detail",
                        "GET",
                        "/v4/jobGroups/{id}",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Cancel { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "cancel",
                        "POST",
                        "/v4/jobGroups/{id}/cancel",
                        true,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Status { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "status",
                        "GET",
                        "/v4/jobGroups/{id}/status",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Inputs { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "inputs",
                        "GET",
                        "/v4/jobGroups/{id}/inputs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Outputs { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "outputs",
                        "GET",
                        "/v4/jobGroups/{id}/outputs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Jobs { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "jobs",
                        "GET",
                        "/v4/jobGroups/{id}/jobs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Publications { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "publications",
                        "GET",
                        "/v4/jobGroups/{id}/publications",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Profile { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "profile",
                        "GET",
                        "/v4/jobGroups/{id}/profile",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::ProfileResults { profile, job_group_id }) => {
                    let config = load_profile(&profile)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "profile-results",
                        "GET",
                        "/v4/jobGroups/{id}/profileResults",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
            },
            Some(OneCommand::OutputObjects { command }) => match command {
                None => Envelope::ok(
                    "one output-object commands available: list, count, create, detail, update, delete, inputs, wrangle-to-python",
                ),
                Some(OneOutputObjectCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "list",
                        "GET",
                        "/v4/outputObjects",
                        false,
                        &[],
                    )?
                }
                Some(OneOutputObjectCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "count",
                        "GET",
                        "/v4/outputObjects/count",
                        false,
                        &[],
                    )?
                }
                Some(OneOutputObjectCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "outputObject",
                        "create",
                        "POST",
                        "/v4/outputObjects",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneOutputObjectCommand::Detail { profile, output_object_id }) => {
                    let config = load_profile(&profile)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "detail",
                        "GET",
                        "/v4/outputObjects/{id}",
                        false,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::Update {
                    profile,
                    output_object_id,
                    body,
                }) => {
                    let config = load_profile(&profile)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "outputObject",
                        "update",
                        "PATCH",
                        "/v4/outputObjects/{id}",
                        true,
                        &[("id", output_object_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneOutputObjectCommand::Delete { profile, output_object_id }) => {
                    let config = load_profile(&profile)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "delete",
                        "DELETE",
                        "/v4/outputObjects/{id}",
                        true,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::Inputs { profile, output_object_id }) => {
                    let config = load_profile(&profile)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "inputs",
                        "GET",
                        "/v4/outputObjects/{id}/inputs",
                        false,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::WrangleToPython {
                    profile,
                    output_object_id,
                    body,
                }) => {
                    let config = load_profile(&profile)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    match body {
                        Some(body) => {
                            let payload = load_payload(&body)?;
                            one_api_live_request_with_body(
                                &config,
                                "outputObject",
                                "wrangle-to-python",
                                "POST",
                                "/v4/outputObjects/{id}/wrangleToPython",
                                true,
                                &[("id", output_object_id.as_str())],
                                Some(payload),
                            )?
                        }
                        None => one_api_live_request(
                            &config,
                            "outputObject",
                            "wrangle-to-python",
                            "POST",
                            "/v4/outputObjects/{id}/wrangleToPython",
                            false,
                            &[("id", output_object_id.as_str())],
                        )?,
                    }
                }
            },
            Some(OneCommand::WebhookFlowTasks { command }) => match command {
                None => Envelope::ok("one webhook-flow-task commands available: create, detail, delete, test"),
                Some(OneWebhookFlowTaskCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "create",
                        "POST",
                        "/v4/webhookFlowTasks",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Detail {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile(&profile)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "detail",
                        "GET",
                        "/v4/webhookFlowTasks/{id}",
                        false,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Delete {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile(&profile)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "delete",
                        "DELETE",
                        "/v4/webhookFlowTasks/{id}",
                        true,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Test { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "test",
                        "POST",
                        "/v4/webhooks/test",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
            },
            Some(OneCommand::WriteSettings { command }) => match command {
                None => Envelope::ok("one write-setting commands available: list, count, create, detail, update, delete"),
                Some(OneWriteSettingCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "list",
                        "GET",
                        "/v4/writeSettings",
                        false,
                        &[],
                    )?
                }
                Some(OneWriteSettingCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "count",
                        "GET",
                        "/v4/writeSettings/count",
                        false,
                        &[],
                    )?
                }
                Some(OneWriteSettingCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "create",
                        "POST",
                        "/v4/writeSettings",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Detail {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile(&profile)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "detail",
                        "GET",
                        "/v4/writeSettings/{id}",
                        false,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
                Some(OneWriteSettingCommand::Update {
                    profile,
                    write_setting_id,
                    body,
                }) => {
                    let config = load_profile(&profile)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "update",
                        "PATCH",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Delete {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile(&profile)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "delete",
                        "DELETE",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
            },
            Some(OneCommand::Status { profile }) => {
                let config = load_profile(&profile)?;
                api_status_envelope(&config, "one")?
            }
            Some(OneCommand::Inventory { profile }) => {
                let config = load_profile(&profile)?;
                api_inventory_envelope(&config, "one")?
            }
            Some(OneCommand::Connections { command }) => match command {
                None => Envelope::ok(
                    "one connections commands available: list, count, create, dry-run, detail, status, update, delete, permissions, connector-metadata",
                ),
                Some(OneConnectionsCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "list",
                        "GET",
                        "/v4/connections",
                        false,
                        &[],
                    )?
                }
                Some(OneConnectionsCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "count",
                        "GET",
                        "/v4/connections/count",
                        false,
                        &[],
                    )?
                }
                Some(OneConnectionsCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "connection",
                        "create",
                        "POST",
                        "/v4/connections",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneConnectionsCommand::DryRun { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "connection",
                        "dry-run",
                        "POST",
                        "/v4/connections/dryRun",
                        false,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneConnectionsCommand::Detail { profile, connection_id }) => {
                    let config = load_profile(&profile)?;
                    let connection_id =
                        connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "detail",
                        "GET",
                        "/v4/connections/{id}",
                        false,
                        &[("id", connection_id.as_str())],
                    )?
                }
                Some(OneConnectionsCommand::Status { profile, connection_id }) => {
                    let config = load_profile(&profile)?;
                    let connection_id =
                        connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "status",
                        "GET",
                        "/v4/connections/{id}/status",
                        false,
                        &[("id", connection_id.as_str())],
                    )?
                }
                Some(OneConnectionsCommand::Update {
                    profile,
                    connection_id,
                    body,
                }) => {
                    let config = load_profile(&profile)?;
                    let connection_id =
                        connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "connection",
                        "update",
                        "PATCH",
                        "/v4/connections/{id}",
                        true,
                        &[("id", connection_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneConnectionsCommand::Delete { profile, connection_id }) => {
                    let config = load_profile(&profile)?;
                    let connection_id =
                        connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "delete",
                        "DELETE",
                        "/v4/connections/{id}",
                        true,
                        &[("id", connection_id.as_str())],
                    )?
                }
                Some(OneConnectionsCommand::ConnectorMetadata { command }) => match command {
                    None => Envelope::ok(
                        "one connections connector-metadata commands available: defaults, detail, publish-info, overrides",
                    ),
                    Some(OneConnectorMetadataCommand::Defaults { profile, connector }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "connector-metadata-defaults",
                            "GET",
                            "/v4/connectorMetadata/{connector}/defaults",
                            false,
                            &[("connector", connector.as_str())],
                        )?
                    }
                    Some(OneConnectorMetadataCommand::Detail { profile, connector }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "connector-metadata-detail",
                            "GET",
                            "/v4/connectorMetadata/{connector}",
                            false,
                            &[("connector", connector.as_str())],
                        )?
                    }
                    Some(OneConnectorMetadataCommand::PublishInfo { profile, connector }) => {
                        let config = load_profile(&profile)?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "connector-metadata-publish-info",
                            "GET",
                            "/v4/connectorMetadata/{connector}/publish/info",
                            false,
                            &[("connector", connector.as_str())],
                        )?
                    }
                    Some(OneConnectorMetadataCommand::Overrides { command }) => match command {
                        None => Envelope::ok(
                            "one connections connector-metadata overrides commands available: list, create, delete",
                        ),
                        Some(OneConnectorMetadataOverridesCommand::List { profile, connector }) => {
                            let config = load_profile(&profile)?;
                            one_api_live_request(
                                &config,
                                "connection",
                                "connector-metadata-overrides-list",
                                "GET",
                                "/v4/connectorMetadata/{connector}/overrides",
                                false,
                                &[("connector", connector.as_str())],
                            )?
                        }
                        Some(OneConnectorMetadataOverridesCommand::Create {
                            profile,
                            connector,
                            body,
                        }) => {
                            let config = load_profile(&profile)?;
                            let payload = load_payload(&body)?;
                            one_api_live_request_with_body(
                                &config,
                                "connection",
                                "connector-metadata-overrides-create",
                                "POST",
                                "/v4/connectorMetadata/{connector}/overrides",
                                true,
                                &[("connector", connector.as_str())],
                                Some(payload),
                            )?
                        }
                        Some(OneConnectorMetadataOverridesCommand::Delete { profile, connector }) => {
                            let config = load_profile(&profile)?;
                            one_api_live_request(
                                &config,
                                "connection",
                                "connector-metadata-overrides-delete",
                                "DELETE",
                                "/v4/connectorMetadata/{connector}/overrides",
                                true,
                                &[("connector", connector.as_str())],
                            )?
                        }
                    },
                },
                Some(OneConnectionsCommand::Permissions { command }) => match command {
                    None => Envelope::ok(
                        "one connection permissions commands available: list, create, detail, delete",
                    ),
                    Some(OneConnectionPermissionCommand::List { profile, connection_id }) => {
                        let config = load_profile(&profile)?;
                        let connection_id =
                            connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "permissions",
                            "GET",
                            "/v4/connections/{id}/permissions",
                            false,
                            &[("id", connection_id.as_str())],
                        )?
                    }
                    Some(OneConnectionPermissionCommand::Create {
                        profile,
                        connection_id,
                        body,
                    }) => {
                        let config = load_profile(&profile)?;
                        let connection_id =
                            connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "connection",
                            "permissions-create",
                            "POST",
                            "/v4/connections/{id}/permissions",
                            true,
                            &[("id", connection_id.as_str())],
                            Some(payload),
                        )?
                    }
                    Some(OneConnectionPermissionCommand::Detail {
                        profile,
                        connection_id,
                        aid,
                    }) => {
                        let config = load_profile(&profile)?;
                        let connection_id =
                            connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "permissions-detail",
                            "GET",
                            "/v4/connections/{id}/permissions/{aid}",
                            false,
                            &[("id", connection_id.as_str()), ("aid", aid.as_str())],
                        )?
                    }
                    Some(OneConnectionPermissionCommand::Delete {
                        profile,
                        connection_id,
                        aid,
                    }) => {
                        let config = load_profile(&profile)?;
                        let connection_id =
                            connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                        one_api_live_request(
                            &config,
                            "connection",
                            "permissions-delete",
                            "DELETE",
                            "/v4/connections/{id}/permissions/{aid}",
                            true,
                            &[("id", connection_id.as_str()), ("aid", aid.as_str())],
                        )?
                    }
                },
            },
            Some(OneCommand::Flows { command }) => match command {
                None => Envelope::ok(
                    "one flows commands available: list, count, detail, create, update, delete, copy, run, validate, parameters, inputs, outputs, import, import-dry-run, export, export-dry-run",
                ),
                Some(OneFlowsCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "flow", "list", "GET", "/v4/flows", false, &[])?
                }
                Some(OneFlowsCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "flow", "count", "GET", "/v4/flows/count", false, &[])?
                }
                Some(OneFlowsCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "flow",
                        "create",
                        "POST",
                        "/v4/flows",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneFlowsCommand::Detail { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "detail",
                        "GET",
                        "/v4/flows/{id}",
                        false,
                        &[("id", flow_id.as_str())],
                    )?
                }
                Some(OneFlowsCommand::Update { profile, flow_id, body }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "flow",
                        "update",
                        "PUT",
                        "/v4/flows/{id}",
                        true,
                        &[("id", flow_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneFlowsCommand::Delete { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "delete",
                        "DELETE",
                        "/v4/flows/{id}",
                        true,
                        &[("id", flow_id.as_str())],
                    )?
                }
                Some(OneFlowsCommand::Copy { profile, flow_id, body }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    let payload = body.map(|path| load_payload(&path)).transpose()?;
                    match payload {
                        Some(payload) => one_api_live_request_with_body(
                            &config,
                            "flow",
                            "copy",
                            "POST",
                            "/v4/flows/{id}/copy",
                            true,
                            &[("id", flow_id.as_str())],
                            Some(payload),
                        )?,
                        None => one_api_live_request(
                            &config,
                            "flow",
                            "copy",
                            "POST",
                            "/v4/flows/{id}/copy",
                            true,
                            &[("id", flow_id.as_str())],
                        )?,
                    }
                }
                Some(OneFlowsCommand::Run { profile, flow_id, body }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    let payload = body.map(|path| load_payload(&path)).transpose()?;
                    match payload {
                        Some(payload) => one_api_live_request_with_body(
                            &config,
                            "flow",
                            "run",
                            "POST",
                            "/v4/flows/{id}/run",
                            true,
                            &[("id", flow_id.as_str())],
                            Some(payload),
                        )?,
                        None => one_api_live_request(
                            &config,
                            "flow",
                            "run",
                            "POST",
                            "/v4/flows/{id}/run",
                            true,
                            &[("id", flow_id.as_str())],
                        )?,
                    }
                }
                Some(OneFlowsCommand::Validate { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "validate",
                        "GET",
                        "/v4/flows/{id}/validate",
                        false,
                        &[("id", flow_id.as_str())],
                    )?
                }
                Some(OneFlowsCommand::Parameters {
                    profile,
                    flow_id,
                    output_object_type,
                }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    let endpoint = if let Some(value) = output_object_type.as_deref() {
                        format!("/v4/flows/{}/recipeParameters?outputObjectType={}", flow_id, value)
                    } else {
                        format!("/v4/flows/{}/recipeParameters", flow_id)
                    };
                    one_api_live_request(&config, "flow", "parameters", "GET", &endpoint, false, &[])?
                }
                Some(OneFlowsCommand::Inputs { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "inputs",
                        "GET",
                        "/v4/flows/{id}/inputs",
                        false,
                        &[("id", flow_id.as_str())],
                    )?
                }
                Some(OneFlowsCommand::Outputs { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "outputs",
                        "GET",
                        "/v4/flows/{id}/outputs",
                        false,
                        &[("id", flow_id.as_str())],
                    )?
                }
                Some(OneFlowsCommand::Import {
                    profile,
                    input,
                    folder_id,
                    from_ui,
                    override_js_udfs,
                }) => {
                    let config = load_profile(&profile)?;
                    flow_import_package_envelope(
                        &config,
                        &input,
                        folder_id.as_deref(),
                        from_ui,
                        override_js_udfs,
                        false,
                    )?
                }
                Some(OneFlowsCommand::ImportDryRun {
                    profile,
                    input,
                    folder_id,
                    from_ui,
                    override_js_udfs,
                }) => {
                    let config = load_profile(&profile)?;
                    flow_import_package_envelope(
                        &config,
                        &input,
                        folder_id.as_deref(),
                        from_ui,
                        override_js_udfs,
                        true,
                    )?
                }
                Some(OneFlowsCommand::Export {
                    profile,
                    flow_id,
                    output,
                }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    flow_export_package_envelope(&config, &flow_id, &output, false)?
                }
                Some(OneFlowsCommand::ExportDryRun { profile, flow_id }) => {
                    let config = load_profile(&profile)?;
                    let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
                    flow_export_package_envelope(&config, &flow_id, Path::new("unused"), true)?
                }
            },
            Some(OneCommand::Plans { command }) => match command {
                None => Envelope::ok(
                    "one plans commands available: list, create, detail, full, run, count, run-parameters, schedules, export, update, delete, share, import, permissions",
                ),
                Some(OnePlansCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "plans", "list", "GET", "/plans/v1/plans", false, &[])?
                }
                Some(OnePlansCommand::Create { profile, body }) => {
                    let config = load_profile(&profile)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "plans",
                        "create",
                        "POST",
                        "/v4/plans",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OnePlansCommand::Detail { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "detail",
                        "GET",
                        "/plans/v1/plans/{id}",
                        false,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Full { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "full",
                        "GET",
                        "/v4/plans/{id}/full",
                        false,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Run { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "run",
                        "POST",
                        "/plans/v1/plans/{id}/run",
                        true,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "plans", "count", "GET", "/plans/v1/plans/count", false, &[])?
                }
                Some(OnePlansCommand::RunParameters { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "run-parameters",
                        "GET",
                        "/plans/v1/plans/{id}/runParameters",
                        false,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Schedules { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "schedules",
                        "GET",
                        "/plans/v1/plans/{id}/schedules",
                        false,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Export { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "export",
                        "GET",
                        "/plans/v1/plans/{id}/package",
                        false,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Update { profile, plan_id, body }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "plans",
                        "update",
                        "PATCH",
                        "/v4/plans/{id}",
                        true,
                        &[("id", plan_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OnePlansCommand::Delete { profile, plan_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    one_api_live_request(
                        &config,
                        "plans",
                        "delete",
                        "DELETE",
                        "/v4/plans/{id}",
                        true,
                        &[("id", plan_id.as_str())],
                    )?
                }
                Some(OnePlansCommand::Share { profile, plan_id, body }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "plans",
                        "share",
                        "POST",
                        "/v4/plans/{id}/permissions",
                        true,
                        &[("id", plan_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OnePlansCommand::Import { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "plans", "import", "POST", "/plans/v1/plans/package", true, &[])?
                }
                Some(OnePlansCommand::Permissions { profile, plan_id, subject_id }) => {
                    let config = load_profile(&profile)?;
                    let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
                    let subject_id = subject_id.unwrap_or_default();
                    if subject_id.is_empty() {
                        one_api_live_request(
                            &config,
                            "plans",
                            "permissions",
                            "GET",
                            "/plans/v1/plans/{id}/permissions",
                            false,
                            &[("id", plan_id.as_str())],
                        )?
                    } else {
                        one_api_live_request(
                            &config,
                            "plans",
                            "permissions",
                            "DELETE",
                            "/plans/v1/plans/{id}/permissions/{subjectId}",
                            true,
                            &[("id", plan_id.as_str()), ("subjectId", subject_id.as_str())],
                        )?
                    }
                }
            },
            Some(OneCommand::Scheduling { command }) => match command {
                None => Envelope::ok(
                    "one scheduling commands available: list, detail, enable, disable, count",
                ),
                Some(OneSchedulingCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "scheduling", "list", "GET", "/scheduling/v1/schedules", false, &[])?
                }
                Some(OneSchedulingCommand::Detail { profile, schedule_id }) => {
                    let config = load_profile(&profile)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "detail",
                        "GET",
                        "/scheduling/v1/schedules/{id}",
                        false,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Enable { profile, schedule_id }) => {
                    let config = load_profile(&profile)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "enable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/enable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Disable { profile, schedule_id }) => {
                    let config = load_profile(&profile)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "disable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/disable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Count { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "scheduling", "count", "GET", "/scheduling/v1/schedules/count", false, &[])?
                }
            },
            Some(OneCommand::Billing { command }) => match command {
                None => Envelope::ok("one billing commands available: current-account, usage-export"),
                Some(OneBillingCommand::CurrentAccount { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "billing", "current-account", "GET", "/billing/v1/my/billing-accounts/current", false, &[])?
                }
                Some(OneBillingCommand::UsageExport { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "billing", "usage-export", "GET", "/billing/v1/usage/export", false, &[])?
                }
            },
            Some(OneCommand::Ui { command }) => match command {
                None => Envelope::ok("one ui commands available: session, workflow, data, library, schedules, jobs (experimental)"),
                Some(UiCommand::Session { command }) => match command {
                    None => Envelope::ok("one ui session commands available: status, ensure, attach, inventory (experimental)"),
                    Some(UiSessionCommand::Status) => Envelope::ok_with_data(
                        "one ui session status scaffolded",
                        ui_command_envelope("session", "status", json!({
                            "browser": "managed by ayx-rs",
                            "mode": "experimental hybrid pinned visible tabs plus background read-only pages",
                        })),
                    ),
                    Some(UiSessionCommand::Ensure) => Envelope::ok_with_data(
                        "one ui session ensure scaffolded",
                        ui_command_envelope("session", "ensure", json!({ "result": "scaffolded" })),
                    ),
                    Some(UiSessionCommand::Attach { tab }) => Envelope::ok_with_data(
                        "one ui session attach scaffolded",
                        ui_command_envelope("session", "attach", json!({ "tab": tab })),
                    ),
                    Some(UiSessionCommand::Inventory) => Envelope::ok_with_data(
                        "one ui session inventory scaffolded",
                        ui_command_envelope("session", "inventory", json!({
                            "tabs": ["workflow", "data"],
                            "policy": "foreground tabs are reusable; read-only tasks may use background pages",
                        })),
                    ),
                },
                Some(UiCommand::Workflow { command }) => match command {
                    None => Envelope::ok("one ui workflow commands available: open, create, inventory, pane-config, pane-results, tool-list, tool-select, tool-inspect, graph-get, graph-put (experimental)"),
                    Some(UiWorkflowCommand::Open { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow open scaffolded",
                        ui_command_envelope("workflow", "open", json!({ "workflow_id": workflow_id, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Create { name, foreground }) => Envelope::ok_with_data(
                        "one ui workflow create scaffolded",
                        ui_command_envelope("workflow", "create", json!({ "name": name, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Inventory { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow inventory scaffolded",
                        ui_command_envelope("workflow", "inventory", json!({
                            "workflow_id": workflow_id,
                            "foreground": foreground,
                            "captures": ["canvas", "config-pane", "results-pane"],
                        })),
                    ),
                    Some(UiWorkflowCommand::PaneConfig { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-config scaffolded",
                        ui_command_envelope("workflow", "pane-config", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::PaneResults { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-results scaffolded",
                        ui_command_envelope("workflow", "pane-results", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolList { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-list scaffolded",
                        ui_command_envelope("workflow", "tool-list", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::ToolSelect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-select scaffolded",
                        ui_command_envelope("workflow", "tool-select", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolInspect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-inspect scaffolded",
                        ui_command_envelope("workflow", "tool-inspect", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::GraphGet { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow graph-get scaffolded",
                        ui_command_envelope("workflow", "graph-get", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::GraphPut { workflow_id, input }) => Envelope::ok_with_data(
                        "one ui workflow graph-put scaffolded",
                        ui_command_envelope("workflow", "graph-put", json!({ "workflow_id": workflow_id, "input": input.display().to_string() })),
                    ),
                },
                Some(UiCommand::Data { command }) => match command {
                    None => Envelope::ok("one ui data commands available: list-datasets, dataset-detail, dataset-preview, upload, list-connections (experimental)"),
                    Some(UiDataCommand::ListDatasets { foreground }) => Envelope::ok_with_data(
                        "one ui data list-datasets scaffolded",
                        ui_command_envelope("data", "list-datasets", json!({
                            "foreground": foreground,
                            "tab_policy": "use pinned tab when warm; background page for read-only refresh is allowed",
                        })),
                    ),
                    Some(UiDataCommand::DatasetDetail { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-detail scaffolded",
                        ui_command_envelope("data", "dataset-detail", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::DatasetPreview { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-preview scaffolded",
                        ui_command_envelope("data", "dataset-preview", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::Upload { input, foreground }) => Envelope::ok_with_data(
                        "one ui data upload scaffolded",
                        ui_command_envelope("data", "upload", json!({ "input": input.display().to_string(), "foreground": foreground })),
                    ),
                    Some(UiDataCommand::ListConnections { foreground }) => Envelope::ok_with_data(
                        "one ui data list-connections scaffolded",
                        ui_command_envelope("data", "list-connections", json!({ "foreground": foreground })),
                    ),
                },
                Some(UiCommand::Library { command }) => match command {
                    None => Envelope::ok("one ui library commands available: inventory (experimental)"),
                    Some(UiLibraryCommand::Inventory) => Envelope::ok_with_data(
                        "one ui library inventory scaffolded",
                        ui_command_envelope("library", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Schedules { command }) => match command {
                    None => Envelope::ok("one ui schedules commands available: inventory (experimental)"),
                    Some(UiSchedulesCommand::Inventory) => Envelope::ok_with_data(
                        "one ui schedules inventory scaffolded",
                        ui_command_envelope("schedules", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Jobs { command }) => match command {
                    None => Envelope::ok("one ui jobs commands available: inventory (experimental)"),
                    Some(UiJobsCommand::Inventory) => Envelope::ok_with_data(
                        "one ui jobs inventory scaffolded",
                        ui_command_envelope("jobs", "inventory", json!({})),
                    ),
                },
            },
            Some(OneCommand::AutoInsights { profile }) => {
                let config = load_profile(&profile)?;
                api_diagnose_envelope(&config, "one auto-insights")?
            }
            Some(OneCommand::DesktopExec { profile }) => {
                let config = load_profile(&profile)?;
                api_status_envelope(&config, "one desktop-exec")?
            }
        },
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
                let target = target
                    .as_deref()
                    .or(command.as_deref())
                    .ok_or_else(|| anyhow!("catalog describe requires a command or capability identifier"))?;
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
    };
    Ok(envelope)
}

fn one_doctor_platform_envelope(config: &Config) -> Result<Envelope> {
    let auth = one_platform_auth_status_envelope(config)?;
    let workspace = one_api_live_request(
        config,
        "platform",
        "doctor-workspace-current",
        "GET",
        "/iam/v1/workspaces/current",
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
                "Route deeper symptom handling to Walter playbooks",
            ]
        }),
    ))
}

fn one_doctor_discover_envelope(config: &Config) -> Result<Envelope> {
    let workspace = one_api_live_request(
        config,
        "platform",
        "discover-workspace-current",
        "GET",
        "/iam/v1/workspaces/current",
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
                "Use Walter to decide whether a symptom belongs to platform, plans, scheduling, or billing",
            ]
        }),
    ))
}

fn one_doctor_plans_envelope(config: &Config) -> Result<Envelope> {
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
                "Use Walter for support-case sequencing and operator guidance",
            ]
        }),
    ))
}

fn one_doctor_scheduling_envelope(config: &Config) -> Result<Envelope> {
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
                "Route operator selection and escalation guidance through Walter",
            ]
        }),
    ))
}

fn one_doctor_billing_envelope(config: &Config) -> Result<Envelope> {
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
                "Use Walter to decide whether billing belongs in CLI or documentation only",
            ]
        }),
    ))
}

fn one_platform_auth_status_envelope(config: &Config) -> Result<Envelope> {
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
            "/iam/v1/workspaces/current",
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
            "token_endpoint_url": one.token_endpoint_url.clone(),
            "access_token_present": one.access_token.as_ref().is_some_and(|v| !v.trim().is_empty()),
            "refresh_token_present": one.refresh_token.as_ref().is_some_and(|v| !v.trim().is_empty()),
            "observability": api_logging,
            "token_source": if one.access_token.as_ref().is_some_and(|v| !v.trim().is_empty()) {
                "config/env"
            } else {
                "missing"
            },
            "validation_target": "/iam/v1/workspaces/current",
            "workspace_probe": workspace_probe.as_ref().map(|probe| probe.data.clone()),
            "message": "One API token posture captured",
        }),
    ))
}

fn one_platform_auth_diagnose_envelope(config: &Config) -> Result<Envelope> {
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
            "/iam/v1/workspaces/current",
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
                "token_endpoint_url": one.token_endpoint_url.clone(),
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
                "token_endpoint_url": one.token_endpoint_url.clone(),
                "access_token_present": true,
                "refresh_token_present": has_refresh_token,
                "diagnosis": "token present and workspace probe executed",
                "workspace_probe": workspace_probe.data,
                "recommendations": [
                    "Use one platform workspace current or people for evidence",
                    "Route any failing symptoms into Walter playbooks",
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

fn main() -> Result<()> {
    if wants_help() {
        print_help();
        return Ok(());
    }
    let cli = Cli::parse();
    let output_json = cli.output == "json";

    match execute(cli) {
        Ok(envelope) => {
            if output_json {
                println!("{}", serde_json::to_string_pretty(&envelope)?);
            } else {
                println!("{}", envelope.message);
            }
            Ok(())
        }
        Err(err) => {
            let err_env = Envelope::err_with_data(
                "command failed",
                json!({
                    "error": err.to_string()
                }),
            );
            if output_json {
                println!("{}", serde_json::to_string_pretty(&err_env)?);
            } else {
                eprintln!("{}", err_env.message);
                eprintln!("{}", err);
            }
            Err(err)
        }
    }
}

fn print_help() {
    println!(
        "AYX Rust CLI\n\nUSAGE:\n    ayx [OPTIONS] <COMMAND>\n\nOPTIONS:\n    --help           Print this help message\n    --output         Output format: text or json\n    --environment    Active environment name when loading a workspace file\n\nCOMMANDS:\n    one            Alteryx One platform branch and API surface\n    server         Server discovery, logs, auth, diagnose, doctor, upgrade, and low-level API calls\n    mongo          Mongo inventory, backup, restore, query, and doctor helpers\n    sqlserver      SQL Server status, prechecks, connection helpers, and migration planning\n    workflow       Workflow package and XML tooling for .yxmd, .yxmc, .yxzp, .yxdb, and cloud conversion\n    tools          Cross-environment tools for workspace.yaml source/target workflows\n    license        Licensing portal branch and API surface\n    onboard        Interactive first-run setup for config.yaml or workspace.yaml\n    update         Self-update from GitHub releases\n    catalog        Machine-readable command registry\n"
    );
}

fn wants_help() -> bool {
    let mut args = std::env::args().skip(1);
    matches!(
        (args.next(), args.next()),
        (Some(flag), None) if flag == "--help" || flag == "-h"
    )
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

fn ui_command_envelope(page: &str, command: &str, data: Value) -> Value {
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

fn build_auth_status(
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

fn parse_saml_metadata_source(input: &str) -> Result<Value> {
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
    use ayx_one_api::refresh_one_access_token;
    use reqwest::blocking::Client;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn sample_config_for_refresh(server_url: String, access_token: Option<String>) -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: ayx_core::profile::MongoProfile {
                mode: ayx_core::profile::MongoMode::Embedded,
                databases: ayx_core::profile::MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(ayx_core::profile::MongoEmbedded {
                    runtime_settings_path: None,
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(ayx_core::profile::AlteryxOneProfile {
                account_email: "test@example.com".to_string(),
                oauth_client_id: Some("client-123".to_string()),
                token_endpoint_url: Some(server_url),
                access_token,
                refresh_token: Some("refresh-abc".to_string()),
            }),
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn catalog_list_includes_core_commands() {
        let env = catalog_list_envelope(None, "compact").expect("catalog list should succeed");
        let commands = env.data["commands"].as_array().expect("commands array");
        let names: Vec<&str> = commands
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"mongo status"));
        assert!(names.contains(&"catalog list"));
        assert!(names.contains(&"license api status"));
        assert!(names.contains(&"license status"));
        assert!(names.contains(&"one platform status"));
        assert!(names.contains(&"one platform inventory"));
        assert!(names.contains(&"one platform user"));
        assert!(names.contains(&"one platform person list"));
        assert!(names.contains(&"one platform person current"));
        assert!(names.contains(&"one platform person detail"));
        assert!(names.contains(&"one platform person create"));
        assert!(names.contains(&"one platform person update-password"));
        assert!(names.contains(&"one platform person password-reset-request"));
        assert!(names.contains(&"one platform api status"));
        assert!(names.contains(&"one platform auth status"));
        assert!(names.contains(&"one platform workspace current"));
        assert!(names.contains(&"one platform workspace list"));
        assert!(names.contains(&"one platform workspace current-configuration"));
        assert!(names.contains(&"one platform workspace save-current-configuration"));
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

        let env = catalog_describe_envelope("one platform person detail")
            .expect("catalog describe should work for person detail");
        assert_eq!(env.data["path"], "one/platform/person/detail");

        let env = catalog_describe_envelope("one platform workspace list")
            .expect("catalog describe should work for workspace list");
        assert_eq!(env.data["path"], "one/platform/workspace/list");

        let env = catalog_describe_envelope("one platform workspace current-configuration")
            .expect("catalog describe should work for current configuration");
        assert_eq!(
            env.data["path"],
            "one/platform/workspace/current-configuration"
        );

        let env = catalog_describe_envelope("one platform workspace save-current-configuration")
            .expect("catalog describe should work for save current configuration");
        assert_eq!(
            env.data["path"],
            "one/platform/workspace/save-current-configuration"
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

        let env = catalog_run_envelope(
            "designer.workflow.context",
            &format!(r#"{{"workflow_path":"{}"}}"#, input.display()),
            false,
        )
        .expect("catalog run should succeed");
        assert_eq!(env.data["capability"]["id"], "designer.workflow.context");
        assert_eq!(env.data["result"]["workflow"]["tool_count"], 1);
    }

    #[test]
    fn one_refresh_token_path_resolves_access_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("connection should arrive");
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf).expect("request should be readable");
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: application/json\r\n",
                "Connection: close\r\n",
                "\r\n",
                r#"{"token_type":"Bearer","access_token":"fresh-token"}"#
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should write");
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client should build");
        let config = sample_config_for_refresh(
            format!("http://{}/token", addr),
            Some("existing-token".to_string()),
        );
        let token = refresh_one_access_token(&config, &client).expect("refresh should succeed");
        assert_eq!(token, "Bearer fresh-token");
        server.join().expect("server should join");
    }
}
