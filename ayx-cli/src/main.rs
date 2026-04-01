use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use reqwest::StatusCode;
use roxmltree::Document;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::envelope::Envelope;
use ayx_core::observability::{record_api_event, response_shape, ApiEvent};
use ayx_core::profile::{Config, ServerProfile};
use ayx_one::{api_diagnose_envelope, api_inventory_envelope, api_status_envelope};
use ayx_server::logs::{
    discover_log_inventory, extract_context, parse_gallery_csv, parse_gallery_events,
    parse_service_events, recent_log_candidates, summarize_log_file, tail_log_file,
};
use ayx_server::mongo::{
    backup_envelope, doctor_envelope as mongo_doctor_envelope, inventory_envelope,
    query_envelope as mongo_query_envelope, restore_envelope, status_envelope,
};
use ayx_server::upgrade::{
    compute_path, run_apply, run_backup, run_bundle, run_plan, run_postcheck, run_precheck,
};
use ayx_server::util::{
    ayx_paths, backup_plan, capture_system_info, run_server_backup, runtime_settings_summary,
    write_runtime_settings_json,
};
use ayx_server::{call_operation, diagnose_api, import_swagger};
use self_update::backends::github::Update as GitHubUpdate;
use self_update::Status;

#[derive(Parser, Debug)]
#[command(name = "ayx")]
#[command(about = "AYX Rust CLI")]
struct Cli {
    #[arg(long, default_value = "text")]
    output: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Mongo {
        #[command(subcommand)]
        command: MongoCommand,
    },
    Server {
        #[command(subcommand)]
        command: Option<ServerCommand>,
    },
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    Sqlserver {
        #[command(subcommand)]
        command: Option<SqlserverCommand>,
    },
    Workflow {
        #[command(subcommand)]
        command: Option<WorkflowCommand>,
    },
    One {
        #[command(subcommand)]
        command: Option<OneCommand>,
    },
    License {
        #[command(subcommand)]
        command: Option<LicenseCommand>,
    },
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
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
    Status,
    Inventory,
}

#[derive(Subcommand, Debug)]
enum WorkflowCommand {
    Status,
    Inventory,
    Logs,
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
    Scheduling {
        #[command(subcommand)]
        command: Option<OneSchedulingCommand>,
    },
    Billing {
        #[command(subcommand)]
        command: Option<OneBillingCommand>,
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
    Group,
    Sso,
    Audit,
    Session,
    Token,
    OauthClient,
    EnvParam,
    Pdh,
    App,
    Health,
}

#[derive(Subcommand, Debug)]
enum OneWorkspaceCommand {
    Current,
    Configuration {
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
    Detail {
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
    List,
    Describe {
        #[arg(long)]
        command: String,
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
        summary: "Summarize One platform inventory candidates.",
        output: "one platform inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["config.yaml", "server_api"],
        notes: &[
            "Use this as the first pass for platform surface discovery.",
            "Start with workspace, user, role, and group operations before broader platform areas.",
        ],
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
        name: "upgrade plan",
        path: "upgrade/plan",
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

fn load_profile(path: &Path) -> Result<Config> {
    Ok(Config::load_from_path(path)?)
}

fn load_payload(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read payload file '{}'", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON payload from '{}'", path.display()))?;
    Ok(value)
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
        Command::Upgrade { command } => match command {
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
        Command::Sqlserver { command } => match command {
            None => Envelope::ok("sqlserver commands are not yet implemented"),
            Some(SqlserverCommand::Status) => bail!("sqlserver status is not yet implemented"),
            Some(SqlserverCommand::Inventory) => {
                bail!("sqlserver inventory is not yet implemented")
            }
        },
        Command::Workflow { command } => match command {
            None => Envelope::ok("workflow commands are not yet implemented"),
            Some(WorkflowCommand::Status) => bail!("workflow status is not yet implemented"),
            Some(WorkflowCommand::Inventory) => {
                bail!("workflow inventory is not yet implemented")
            }
            Some(WorkflowCommand::Logs) => bail!("workflow logs are not yet implemented"),
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
                },
                Some(OnePlatformCommand::Status { profile }) => {
                    let config = load_profile(&profile)?;
                    api_status_envelope(&config, "one platform")?
                }
                Some(OnePlatformCommand::Inventory { profile }) => {
                    let config = load_profile(&profile)?;
                    api_inventory_envelope(&config, "one platform")?
                }
                Some(OnePlatformCommand::Workspace { command }) => match command {
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
                Some(OnePlatformCommand::User) => Envelope::ok_with_data(
                    "one platform user",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "user workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Group) => Envelope::ok_with_data(
                    "one platform group",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "group workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Sso) => Envelope::ok_with_data(
                    "one platform sso",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "sso workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Audit) => Envelope::ok_with_data(
                    "one platform audit",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "audit workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Session) => Envelope::ok_with_data(
                    "one platform session",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "session workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Token) => Envelope::ok_with_data(
                    "one platform token",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "token workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::OauthClient) => Envelope::ok_with_data(
                    "one platform oauth-client",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "oauth-client workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::EnvParam) => Envelope::ok_with_data(
                    "one platform env-param",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "env-param workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Pdh) => Envelope::ok_with_data(
                    "one platform pdh",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "pdh workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::App) => Envelope::ok_with_data(
                    "one platform app",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "app workflow scaffolded",
                    }),
                ),
                Some(OnePlatformCommand::Health) => Envelope::ok_with_data(
                    "one platform health",
                    json!({
                        "product": "one",
                        "surface": "platform",
                        "message": "health workflow scaffolded",
                    }),
                ),
                None => Envelope::ok("one platform commands available: api, status, inventory, workspace, role, user, group, sso, audit, session, token, oauth-client, env-param, pdh, app, health"),
            },
            Some(OneCommand::Status { profile }) => {
                let config = load_profile(&profile)?;
                api_status_envelope(&config, "one")?
            }
            Some(OneCommand::Inventory { profile }) => {
                let config = load_profile(&profile)?;
                api_inventory_envelope(&config, "one")?
            }
            Some(OneCommand::Plans { command }) => match command {
                None => Envelope::ok(
                    "one plans commands available: list, detail, run, count, run-parameters, schedules, export, import, permissions",
                ),
                Some(OnePlansCommand::List { profile }) => {
                    let config = load_profile(&profile)?;
                    one_api_live_request(&config, "plans", "list", "GET", "/plans/v1/plans", false, &[])?
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
            CatalogCommand::List => catalog_list_envelope()?,
            CatalogCommand::Describe { command } => catalog_describe_envelope(&command)?,
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

fn one_api_live_request(
    config: &Config,
    surface: &str,
    operation: &str,
    method: &str,
    endpoint: &str,
    mutating: bool,
    path_params: &[(&str, &str)],
) -> Result<Envelope> {
    let observability = config.observability.as_ref();
    let base_url = "https://api.us1.alteryxcloud.com";
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build one api client")?;
    let mut access_token = resolve_one_access_token(config, &client)?;

    let mut url = format!("{}{}", base_url, endpoint);
    for (key, value) in path_params {
        url = url.replace(&format!("{{{}}}", key), value);
    }
    let method_name = method.to_string();
    let method = reqwest::Method::from_bytes(method_name.as_bytes())
        .map_err(|_| anyhow!("unsupported one api method '{}'", method))?;
    let mut attempt = 0u32;
    let max_attempts = if mutating { 1 } else { 4 };
    let started = Instant::now();
    let mut last_status: Option<StatusCode> = None;
    let mut retry_after_seconds: Option<u64> = None;
    let mut refreshed_once = false;

    loop {
        attempt += 1;
        let mut request = client
            .request(method.clone(), &url)
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
            .header(reqwest::header::ACCEPT, "application/json");
        if mutating {
            request = request.header(CONTENT_TYPE, "application/json");
        }

        let response = request.send();
        match response {
            Ok(response) => {
                let status = response.status();
                last_status = Some(status);
                retry_after_seconds = parse_retry_after(response.headers().get(RETRY_AFTER));
                if status == StatusCode::UNAUTHORIZED && !refreshed_once {
                    access_token = refresh_one_access_token(config, &client)?;
                    refreshed_once = true;
                    continue;
                }
                if status.is_success()
                    || !should_retry_status(status, mutating)
                    || attempt >= max_attempts
                {
                    let content_type = response
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|val| val.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let request_id = response
                        .headers()
                        .get("x-request-id")
                        .and_then(|val| val.to_str().ok())
                        .map(ToOwned::to_owned);
                    let text = response.text().unwrap_or_else(|_| String::new());
                    let response_body = parse_response_text(&content_type, &text);
                    let envelope = Envelope::ok_with_data(
                        format!(
                            "{} {} {}",
                            surface,
                            operation,
                            if status.is_success() { "ok" } else { "failed" }
                        ),
                        json!({
                            "surface": surface,
                            "operation": operation,
                            "method": method_name,
                            "url": url,
                            "attempts": attempt,
                            "elapsed_ms": started.elapsed().as_millis(),
                            "status_code": status.as_u16(),
                            "ok": status.is_success(),
                            "request_id": request_id,
                            "retry_after_seconds": retry_after_seconds,
                            "response": response_body,
                        }),
                    );
                    let _ = record_api_event(
                        observability,
                        ApiEvent {
                            product: "one",
                            surface,
                            operation,
                            method: &method_name,
                            endpoint_template: endpoint,
                            resolved_url: &url,
                            status_code: Some(status.as_u16()),
                            duration_ms: started.elapsed().as_millis(),
                            attempt,
                            retry_after_seconds,
                            request_id: request_id.as_deref(),
                            ok: status.is_success(),
                            error_class: None,
                            response_shape: Some(response_shape(&response_body)),
                            mutating,
                            dry_run: false,
                        },
                    );
                    return Ok(envelope);
                }
                let delay = retry_delay(attempt, retry_after_seconds);
                std::thread::sleep(delay);
                continue;
            }
            Err(err) => {
                if mutating || attempt >= max_attempts {
                    let envelope = Envelope::ok_with_data(
                        format!("{} {} failed", surface, operation),
                        json!({
                            "surface": surface,
                            "operation": operation,
                            "method": method_name,
                            "url": url,
                            "attempts": attempt,
                            "elapsed_ms": started.elapsed().as_millis(),
                            "ok": false,
                            "status_code": last_status.map(|s| s.as_u16()),
                            "retry_after_seconds": retry_after_seconds,
                            "error": err.to_string(),
                            "response": Value::Null,
                        }),
                    );
                    let _ = record_api_event(
                        observability,
                        ApiEvent {
                            product: "one",
                            surface,
                            operation,
                            method: &method_name,
                            endpoint_template: endpoint,
                            resolved_url: &url,
                            status_code: last_status.map(|s| s.as_u16()),
                            duration_ms: started.elapsed().as_millis(),
                            attempt,
                            retry_after_seconds,
                            request_id: None,
                            ok: false,
                            error_class: Some("transport"),
                            response_shape: Some("null"),
                            mutating,
                            dry_run: false,
                        },
                    );
                    return Ok(envelope);
                }
                let delay = retry_delay(attempt, retry_after_seconds);
                std::thread::sleep(delay);
            }
        }
    }
}

fn parse_response_text(content_type: &str, text: &str) -> Value {
    if content_type.to_lowercase().contains("application/json") {
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    } else if text.trim().is_empty() {
        Value::Null
    } else {
        json!({ "raw": text })
    }
}

fn should_retry_status(status: StatusCode, mutating: bool) -> bool {
    if mutating {
        return false;
    }
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504)
}

fn parse_retry_after(header: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let value = header?.to_str().ok()?.trim();
    value.parse::<u64>().ok()
}

fn resolve_one_access_token(config: &Config, client: &Client) -> Result<String> {
    if let Some(access_token) = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.access_token.as_ref())
        .filter(|token| !token.trim().is_empty())
    {
        return Ok(access_token.clone());
    }

    refresh_one_access_token(config, client)
}

fn refresh_one_access_token(config: &Config, client: &Client) -> Result<String> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("config missing alteryx_one section"))?;
    let client_id = one
        .oauth_client_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("alteryx_one.oauth_client_id is required for refresh_token support")
        })?;
    let refresh_token = one
        .refresh_token
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("alteryx_one.refresh_token is required to refresh access tokens"))?;
    let token_endpoint_url = one
        .token_endpoint_url
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("alteryx_one.token_endpoint_url is required to refresh access tokens")
        })?;

    let response = client
        .post(token_endpoint_url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .send()
        .with_context(|| format!("refresh token request to '{}' failed", token_endpoint_url))?
        .error_for_status()
        .context("refresh token request returned error status")?;

    let token_json: Value = response
        .json()
        .context("failed to parse refresh token response")?;
    let token_type = token_json
        .get("token_type")
        .and_then(Value::as_str)
        .unwrap_or("Bearer");
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("refresh token response missing access_token"))?;

    Ok(format!("{token_type} {access_token}"))
}

fn retry_delay(attempt: u32, retry_after_seconds: Option<u64>) -> Duration {
    if let Some(seconds) = retry_after_seconds {
        return Duration::from_secs(seconds.clamp(1, 60));
    }
    let shift = attempt.saturating_sub(1).min(6);
    let multiplier = 1u64 << shift;
    let base_ms = 250u64.saturating_mul(multiplier);
    Duration::from_millis(base_ms.min(8_000))
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
    "AYX Rust CLI\n\nUSAGE:\n    ayx [OPTIONS] <COMMAND>\n\nOPTIONS:\n    --help       Print this help message\n    --output     Output format: text or json\n\nCOMMANDS:\n    mongo         Mongo inventory, backup, restore, query, and doctor helpers\n    server api    Server API operations\n    server        Server discovery, logs, auth, diagnose, doctor, and low-level API calls\n    upgrade       Upgrade planning and execution helpers\n    catalog       Machine-readable command registry\n    license       Licensing portal branch and API surface\n    one           Alteryx One platform branch and API surface\n    sqlserver     SQL Server command family (stubbed)\n    workflow      Workflow command family (stubbed)\n    update        Self-update from GitHub releases\n"
    );
}

fn wants_help() -> bool {
    std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
}

fn catalog_list_envelope() -> Result<Envelope> {
    let commands: Vec<Value> = COMMAND_SPECS
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "path": spec.path,
                "summary": spec.summary,
                "output": spec.output,
                "safety": spec.safety,
                "mutating": spec.mutating,
            })
        })
        .collect();

    Ok(Envelope::ok_with_data(
        "catalog entries listed",
        json!({
            "count": commands.len(),
            "commands": commands,
        }),
    ))
}

fn catalog_describe_envelope(command: &str) -> Result<Envelope> {
    let spec = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == command || spec.path == command)
        .ok_or_else(|| anyhow!("catalog entry '{}' not found", command))?;

    Ok(Envelope::ok_with_data(
        "catalog entry described",
        json!({
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
            upgrade: None,
        }
    }

    #[test]
    fn catalog_list_includes_core_commands() {
        let env = catalog_list_envelope().expect("catalog list should succeed");
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
        assert!(names.contains(&"one platform api status"));
        assert!(names.contains(&"one platform auth status"));
        assert!(names.contains(&"one platform workspace current"));
        assert!(names.contains(&"one platform role list-assignments"));
        assert!(names.contains(&"one plans status"));
        assert!(names.contains(&"one plans list"));
        assert!(names.contains(&"one scheduling list"));
        assert!(names.contains(&"one billing current-account"));
        assert!(names.contains(&"one auto-insights status"));
        assert!(names.contains(&"one desktop-exec status"));
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
