//! Dispatch for `ayx catalog ...`.
//!
//! The catalog surface is the machine-readable registry view: a stable index
//! of commands and capabilities that complements `ayx discover` rather than
//! replacing the live CLI tree.
//!
//! Command identity -- `name`, `path`, and `summary` -- is derived live from
//! the clap tree via `cmd::command_surface`; it is never hand-maintained
//! here. `CATALOG_METADATA` below is a thin *semantic overlay*: for a subset
//! of commands it supplies catalog-only classification (`output`, `safety`,
//! `mutating`, `prerequisites`, `notes`) that clap has no notion of. Every
//! row is validated against the live tree before use (see
//! `validate_metadata`), so a renamed or removed command can never produce
//! stale catalog documentation -- and a newly added command can never be
//! silently missing from `catalog list --scope all`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use ayx_core::envelope::Envelope;
use serde_json::Value;
use serde_json::json;

use crate::capability;
use crate::cmd::command_surface;
use crate::{CatalogCommand, CatalogScope};

pub fn execute(command: CatalogCommand) -> Result<Envelope> {
    Ok(match command {
        CatalogCommand::List { tag, format, scope } => {
            catalog_list_envelope(tag.as_deref(), &format, scope)?
        }
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
    })
}

/// One command's semantic classification, keyed by canonical slash `path` --
/// the join key against `command_surface::visible_commands()`. Deliberately
/// carries no `name`/`summary`/independent existence flag: those are
/// clap-derived and would drift if duplicated here.
#[derive(Debug)]
struct CatalogMetadata {
    path: &'static str,
    output: &'static str,
    safety: &'static str,
    mutating: bool,
    prerequisites: &'static [&'static str],
    notes: &'static [&'static str],
}

// Migrated from the former `COMMAND_SPECS` static array (previously in
// main.rs). Order and `#[cfg(feature = "ui")]` guards preserved from the
// original; `output`/`safety`/`mutating`/`prerequisites`/`notes` copied
// byte-for-byte -- this is data relocation, not reclassification.
//
// Five path prefixes were renamed to match the live clap tree at migration
// time (the clap command was pluralized / renamed at some point after the
// original entry was written, and nothing enforced the two stayed in sync --
// exactly the drift this overlay's validation now prevents): `one/job-group`
// -> `one/job-groups`, `one/output-object` -> `one/output-objects`,
// `one/webhook-flow-task` -> `one/webhook-flow-tasks`, `one/write-setting`
// -> `one/write-settings`, and top-level `server-logs` -> nested
// `server/server-logs`. Only the `path` key changed for those rows; every
// other field is untouched. One entry, `one/plans/status`, had no live
// command at all (`OnePlansCommand` has no `Status` variant) and was dropped
// rather than migrated -- see the Task 2 report for the full accounting.

const CATALOG_METADATA: &[CatalogMetadata] = &[
    CatalogMetadata {
        path: "profile/list",
        output: "profile registry envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["ayx config home"],
        notes: &["Use this to discover centrally managed profiles."],
    },
    CatalogMetadata {
        path: "profile/current",
        output: "active profile envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["ayx config home"],
        notes: &["Use this to see which profile ayx will use by default."],
    },
    CatalogMetadata {
        path: "profile/use",
        output: "state update envelope",
        safety: "mutating-local",
        mutating: true,
        prerequisites: &["existing central profile"],
        notes: &["Updates ayx state only; no remote systems are changed."],
    },
    CatalogMetadata {
        path: "doctor",
        output: "doctor aggregate envelope",
        safety: "read-only-or-safe-local-fix",
        mutating: false,
        prerequisites: &["active or explicit profile"],
        notes: &["Use --fix for safe local remediation such as creating the central config home."],
    },
    CatalogMetadata {
        path: "doctor/config",
        output: "config doctor envelope",
        safety: "read-only-or-safe-local-fix",
        mutating: false,
        prerequisites: &["ayx config home or legacy config"],
        notes: &["Use this first when profile resolution or local state is unclear."],
    },
    CatalogMetadata {
        path: "mongo/status",
        output: "connection detail and database metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongo.mode", "mongo.databases"],
        notes: &["Use this first to validate embedded or managed Mongo configuration."],
    },
    CatalogMetadata {
        path: "mongo/inventory",
        output: "database inventory plan",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongo.databases"],
        notes: &["Use this before backup or restore planning."],
    },
    CatalogMetadata {
        path: "mongo/backup",
        output: "backup plan or execution artifacts",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "mongo.mode"],
        notes: &[
            "Requires --apply for a live backup.",
            "Writes audit artifacts.",
        ],
    },
    CatalogMetadata {
        path: "mongo/restore",
        output: "restore execution artifacts",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "restore input path"],
        notes: &[
            "Requires --apply for a live restore.",
            "Writes audit artifacts.",
        ],
    },
    CatalogMetadata {
        path: "mongo/mutate",
        output: "mutation preview envelope by default, or the terminal applied/aborted/failed_or_unknown execution result envelope with --apply",
        safety: "destructive",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "mongosh available on PATH",
            "a current successful mongo backup audit artifact",
            "an approved, non-expired mongo mutate preview artifact",
        ],
        notes: &[
            "Requires --template; free-form filter/update is not supported.",
            "Requires --apply plus --accept-mutation-risk, --backup-audit-artifact, --approval-artifact, and --approve together for live execution.",
            "Writes an audit artifact for every preview attempt.",
        ],
    },
    CatalogMetadata {
        path: "mongo/undo",
        output: "undo execution result envelope",
        safety: "destructive",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "mongosh available on PATH",
            "the source mutation's execution audit artifact",
        ],
        notes: &[
            "Preview-first: run without --apply to derive the guarded inverse from the source mutation's own recorded prior values, re-verify every candidate is still fresh, and write an approval artifact.",
            "Requires --apply plus --accept-mutation-risk, --approval-artifact, and --approve together for live execution — the same gate tuple as mongo mutate, minus a backup artifact.",
            "Refuses to run against a mutation that was not applied, was already undone, or used an unsupported rollback strategy, and aborts the whole batch if any candidate document is stale (missing, or no longer holding its recorded post-mutation value).",
        ],
    },
    CatalogMetadata {
        path: "server/api/import-swagger",
        output: "cached swagger metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server.webapi_url"],
        notes: &["Use before server api call."],
    },
    CatalogMetadata {
        path: "server/api/status",
        output: "server api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Useful before diagnostics, import, or call."],
    },
    CatalogMetadata {
        path: "server/api/diagnose",
        output: "diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use before server api import-swagger or server api call."],
    },
    CatalogMetadata {
        path: "server/api/call",
        output: "call response envelope",
        safety: "mutating-or-read-only",
        mutating: false,
        prerequisites: &["cached Swagger document", "central runtime profile"],
        notes: &["Operation behavior depends on the selected endpoint."],
    },
    CatalogMetadata {
        path: "license/status",
        output: "license status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Product branch ready; API subcommands are the primary entry point."],
    },
    CatalogMetadata {
        path: "license/inventory",
        output: "license inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Product branch ready; API subcommands are the primary entry point."],
    },
    CatalogMetadata {
        path: "designer/workflow/inspect",
        output: "workflow inspection envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["workflow artifact path"],
        notes: &[
            "Use this to inspect .yxmd, .yxmc, .yxzp, or .yxdb files and directories.",
            "Recursive directory inspection is supported.",
        ],
    },
    CatalogMetadata {
        path: "designer/workflow/unpack",
        output: "workflow unpack envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["input .yxzp package", "output directory"],
        notes: &["Preserves the archive contents in a directory tree for XML-level edits."],
    },
    CatalogMetadata {
        path: "designer/workflow/validate",
        output: "workflow validation envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["workflow artifact path"],
        notes: &["Validates .yxmd, .yxmc, .yxzp, or directories of workflow artifacts."],
    },
    CatalogMetadata {
        path: "designer/workflow/replace",
        output: "workflow replacement envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input artifact", "output path", "find/replace values"],
        notes: &[
            "Use --validate to check the rewritten XML after replacement.",
            "Package inputs are unpacked, rewritten, and re-packed.",
        ],
    },
    CatalogMetadata {
        path: "designer/workflow/repackage",
        output: "workflow repackage envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input directory", "output package path"],
        notes: &["Useful after XML-level edits to workflow package contents."],
    },
    CatalogMetadata {
        path: "designer/workflow/migrate",
        output: "workflow migration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["input artifact", "output path", "find/replace values"],
        notes: &[
            "Combines inspect, replace, validate, and repackaging into one flow.",
            "Use this for NFS-style migration and other recursive XML updates.",
        ],
    },
    CatalogMetadata {
        path: "designer/workflow/recurse",
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
    CatalogMetadata {
        path: "designer/workflow/scan",
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
    CatalogMetadata {
        path: "designer/workflow/publish",
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
    CatalogMetadata {
        path: "one/ui/session/status",
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
    CatalogMetadata {
        path: "one/ui/workflow/inventory",
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
    CatalogMetadata {
        path: "one/ui/data/list-datasets",
        output: "one ui data inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["authenticated Cloud data page"],
        notes: &[
            "May use a pinned Data tab or a background page depending on the caller's policy.",
            "Useful as the first step before preview, detail, upload, or validation fan-out.",
        ],
    },
    CatalogMetadata {
        path: "one/login",
        output: "one login envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one"],
        notes: &[
            "Default flow uses email OTP; browser, device, refresh-token, and access-token paths are also supported.",
            "Stores credentials in the active profile using the existing inline-secret policy.",
        ],
    },
    CatalogMetadata {
        path: "one/logout",
        output: "one logout envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one"],
        notes: &[
            "Clears top-level and workspace-scoped One access/refresh credential fields and refs.",
            "Does not revoke remote tokens or delete external secret-store entries.",
        ],
    },
    CatalogMetadata {
        path: "one/inventory",
        output: "one inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &[
            "Use this as the authoritative One endpoint registry.",
            "Implemented and partial surfaces are listed separately from documented-only gaps.",
        ],
    },
    CatalogMetadata {
        path: "one/whoami",
        output: "one whoami envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/list",
        output: "one person list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/current",
        output: "one person current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/current in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/count",
        output: "one person count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/detail",
        output: "one person detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/people/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/create",
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
    CatalogMetadata {
        path: "one/person/update",
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
    CatalogMetadata {
        path: "one/person/patch",
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
    CatalogMetadata {
        path: "one/person/delete",
        output: "one person delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/people/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/person/update-password",
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
    CatalogMetadata {
        path: "one/person/password-reset-request",
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
    CatalogMetadata {
        path: "one/workspace/current",
        output: "one workspace current envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/workspaces/current in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/current-configuration",
        output: "one workspace current configuration envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/configuration-v4",
        output: "one workspace configuration-v4 envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/configuration",
        output: "one workspace configuration envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/save-current-configuration",
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
    CatalogMetadata {
        path: "one/workspace/save-configuration-v4",
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
    CatalogMetadata {
        path: "one/workspace/list",
        output: "one workspace list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/create",
        output: "one workspace create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/workspaces in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/delete",
        output: "one workspace delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/workspaces/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/create-group",
        output: "one workspace create-group envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/workspaces/{id}/groups in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/delete-group",
        output: "one workspace delete-group envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/workspaces/{id}/groups/{groupId} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/update-group",
        output: "one workspace update-group envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PUT /v4/workspaces/{id}/groups/{groupId} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/set-group-roles",
        output: "one workspace set-group-roles envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PUT /v4/workspaces/{id}/groups/{groupId}/roles in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/add-group-users",
        output: "one workspace add-group-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "user ids",
        ],
        notes: &[
            "Maps to POST /v4/workspaces/{id}/groups/{groupId}/users with repeated userIds query parameters.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/remove-group-users",
        output: "one workspace remove-group-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "user ids",
        ],
        notes: &[
            "Maps to DELETE /v4/workspaces/{id}/groups/{groupId}/users with repeated userIds query parameters.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/configuration-schema",
        output: "one workspace configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/configuration-schema in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/current-configuration-schema",
        output: "one workspace current configuration schema envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/current/configuration-schema in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/delete-current-configuration",
        output: "one workspace delete-current-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/current/delete-configuration in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/delete-configuration",
        output: "one workspace delete-configuration envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/delete-configuration in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/people",
        output: "one workspace people envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to GET /v4/people in the One API docs. Workspace context comes from the x-alteryx-workspace-gid header; /v4/workspaces/{id}/people returns 404.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/admins",
        output: "one workspace admins envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to GET /v4/workspaces/{workspaceId}/admins. workspaceId is the NUMERIC workspace id (resolved by a /v4/workspaces/current preflight), not the workspace GID — probing this route with the GID is what previously made it look like a 404. GET /v4/people?role=admin is not a substitute: the gateway ignores role=admin and only decorates the caller's own record with isAdmin.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/groups",
        output: "one workspace groups envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{id}/groups in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/groups-global",
        output: "one workspace groups-global envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/groups in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/invitation-link",
        output: "one workspace invitation-link envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to GET /v4/workspaces/{id}/invitationLink?personId={personId} in the One API docs.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/cloud-configs",
        output: "one workspace cloud-configs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/workspaces/{workspaceId}/cloudConfigs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/switch",
        output: "one workspace switch envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "stored workspace credentials"],
        notes: &[
            "Updates `alteryx_one.expected_workspace_id` in the selected profile.",
            "Does not call a One API endpoint.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/invite-users",
        output: "one workspace invite-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/people/batch in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/invite",
        output: "one workspace invite envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/workspaces/{id}/people in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/invite-list",
        output: "one workspace invite-list envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to POST /v4/workspaces/{id}/people/batch in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/reinvite-users",
        output: "one workspace reinvite-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/workspaces/{id}/people/batch in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/remove-user",
        output: "one workspace remove-user envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/workspaces/{workspaceId}/people/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/suspend-users",
        output: "one workspace suspend-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/people/suspend in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/unsuspend-users",
        output: "one workspace unsuspend-users envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to POST /v4/workspaces/{id}/people/unsuspend in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/suspend-user",
        output: "one workspace suspend-user envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to PUT /v4/workspaces/{id}/people/{personId}/suspended in the One API docs.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/transfer",
        output: "one workspace transfer envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to PATCH /v4/workspaces/{id}/transfer in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/transfer-assets",
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
    CatalogMetadata {
        path: "one/workspace/create-cloud-config",
        output: "one workspace create-cloud-config envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &[
            "Maps to POST /v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider} in the One API docs.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/update-cloud-config",
        output: "one workspace update-cloud-config envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &[
            "Maps to PATCH /v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider} in the One API docs.",
        ],
    },
    CatalogMetadata {
        path: "one/workspace/patch-user",
        output: "one workspace patch-user envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PATCH /v4/workspaces/{workspaceId}/people/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workspace/update-user",
        output: "one workspace update-user envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "alteryx_one.access_token",
            "payload json",
        ],
        notes: &["Maps to PUT /v4/workspaces/{workspaceId}/people/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/role/list-assignments",
        output: "one role assignments envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/authorization/roles/{id}/people in the One API docs."],
    },
    CatalogMetadata {
        path: "one/role/list",
        output: "one role list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/authorization/roles in the One API docs."],
    },
    CatalogMetadata {
        path: "one/role/detail",
        output: "one role detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/authorization/roles/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/role/assign",
        output: "one role assign envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to PUT /v4/authorization/roles/{id}/people with a bare JSON array containing the subject id.",
        ],
    },
    CatalogMetadata {
        path: "one/role/unassign",
        output: "one role unassign envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Maps to DELETE /v4/authorization/roles/{id}/people/{subjectId} in the One API docs.",
        ],
    },
    CatalogMetadata {
        path: "one/auth/status",
        output: "one auth status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Confirms OAuth client ID, token endpoint, access token presence, refresh token presence, and whether the token can reach the token inventory surface.",
        ],
    },
    CatalogMetadata {
        path: "one/token/list",
        output: "one token list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens in the One API docs."],
    },
    CatalogMetadata {
        path: "one/token/create",
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
    CatalogMetadata {
        path: "one/token/detail",
        output: "one token detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to GET /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/token/delete",
        output: "one token delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Maps to DELETE /v4/apiAccessTokens/{tokenId} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/auth/diagnose",
        output: "one auth diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &[
            "Uses the token inventory endpoint as the safe validation target, while mutating operations still preflight workspace identity separately.",
        ],
    },
    CatalogMetadata {
        path: "one/doctor/auth",
        output: "one auth doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps token posture and workspace probe checks."],
    },
    CatalogMetadata {
        path: "one/doctor/discover",
        output: "one discovery doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Surfaces workspace, plan, and schedule discovery data."],
    },
    CatalogMetadata {
        path: "one/doctor/identity",
        output: "one identity doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps workspace and role discovery checks."],
    },
    CatalogMetadata {
        path: "one/doctor/plans",
        output: "one plans doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps list, count, and plan lookup checks."],
    },
    CatalogMetadata {
        path: "one/doctor/scheduling",
        output: "one scheduling doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "alteryx_one.access_token"],
        notes: &["Wraps schedule list and count checks."],
    },
    CatalogMetadata {
        path: "one/api/status",
        output: "one api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Use this to inspect One API posture before diagnostics.",
            "Treat this as the One managed IAM posture check.",
        ],
    },
    CatalogMetadata {
        path: "one/api/diagnose",
        output: "one api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Use before future One API call-style workflows.",
            "Route workflow guidance through the orchestration layer once the symptom is known.",
        ],
    },
    CatalogMetadata {
        path: "one/api/open-api-spec",
        output: "one api open-api-spec envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/open-api-spec in the One API docs."],
    },
    CatalogMetadata {
        path: "one/api/coverage",
        output: "one api coverage envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Fetches GET /v4/open-api-spec (or --spec <file>) and diffs it against the ayx-one-api inventory.",
            "--check exits non-zero when endpoints are missing.",
        ],
    },
    CatalogMetadata {
        path: "one/plans/list",
        output: "one plans list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/create",
        output: "one plans create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/detail",
        output: "one plans detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/full in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/run",
        output: "one plans run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /v4/plans/{id}/run in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/count",
        output: "one plans count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/full",
        output: "one plans full envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/full in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/run-parameters",
        output: "one plans run-parameters envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/runParameters in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/schedules",
        output: "one plans schedules envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/schedules in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/export",
        output: "one plans export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/plans/{id}/package in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/update",
        output: "one plans update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/plans/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/delete",
        output: "one plans delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/plans/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/share",
        output: "one plans share envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/plans/{id}/permissions in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/import",
        output: "one plans import envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /v4/plans/package in the One API docs."],
    },
    CatalogMetadata {
        path: "one/plans/permissions",
        output: "one plans permissions envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to GET /v4/plans/{id}/permissions in the One API docs.",
            "When `--subject-id` is set, maps to DELETE /v4/plans/{id}/permissions/{subjectId}.",
        ],
    },
    CatalogMetadata {
        path: "one/flows/list",
        output: "one flows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/count",
        output: "one flows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/library/list",
        output: "one flows library list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flowsLibrary in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/library/count",
        output: "one flows library count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flowsLibrary/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/list",
        output: "one flows folders list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/count",
        output: "one flows folders count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/detail",
        output: "one flows folders detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/create",
        output: "one flows folders create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/folders in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/update",
        output: "one flows folders update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/folders/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/delete",
        output: "one flows folders delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/folders/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/flows/list",
        output: "one flows folders flows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id}/flows in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/folders/flows/count",
        output: "one flows folders flows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/folders/{id}/flows/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/detail",
        output: "one flows detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/create",
        output: "one flows create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/update",
        output: "one flows update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/flows/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/delete",
        output: "one flows delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/flows/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/copy",
        output: "one flows copy envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/copy in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/run",
        output: "one flows run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/run in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/validate",
        output: "one flows validate envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/validate in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/parameters",
        output: "one flows parameters envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/recipeParameters in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/inputs",
        output: "one flows inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/inputs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/outputs",
        output: "one flows outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/outputs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/permissions-get",
        output: "one flows permissions-get envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/permissions in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/permissions",
        output: "one flows permissions envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/permissions in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/move",
        output: "one flows move envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/flows/{id}/move in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/replace-dataset",
        output: "one flows replace-dataset envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/flows/{id}/replaceDataset in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/import",
        output: "one flows import envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/import-dry-run",
        output: "one flows import dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api", "flow package"],
        notes: &["Maps to POST /v4/flows/package/dryRun in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/export",
        output: "one flows export envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package in the One API docs."],
    },
    CatalogMetadata {
        path: "one/flows/export-dry-run",
        output: "one flows export dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/flows/{id}/package/dryRun in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/list",
        output: "one datasets list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/datasetLibrary in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/count",
        output: "one datasets count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/datasetLibrary/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/wrangled/list",
        output: "one datasets wrangled list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/wrangled/count",
        output: "one datasets wrangled count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/wrangled/detail",
        output: "one datasets wrangled detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/wrangledDatasets/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/datasets/imported/detail",
        output: "one datasets imported detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/importedDatasets/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/list",
        output: "one connections list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/count",
        output: "one connections count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/create",
        output: "one connections create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/dry-run",
        output: "one connections dry-run envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/dryRun in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/detail",
        output: "one connections detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/status",
        output: "one connections status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/status in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/update",
        output: "one connections update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/connections/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/delete",
        output: "one connections delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connections/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/permissions/list",
        output: "one connections permissions list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connections/{id}/permissions/sharedSubjects."],
    },
    CatalogMetadata {
        path: "one/connections/permissions/create",
        output: "one connections permissions create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connections/share; the connection id travels in the body."],
    },
    CatalogMetadata {
        path: "one/connections/permissions/detail",
        output: "one connections permissions detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Synthesized by filtering GET /v4/connections/{id}/permissions/sharedSubjects; no per-subject route exists.",
        ],
    },
    CatalogMetadata {
        path: "one/connections/permissions/delete",
        output: "one connections permissions delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connections/share?connectionId=&subjectId=&subjectType=."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/defaults",
        output: "one connections connector-metadata defaults envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/defaults in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/detail",
        output: "one connections connector-metadata detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/publish-info",
        output: "one connections connector-metadata publish-info envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/publish/info in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/overrides/create",
        output: "one connections connector-metadata overrides create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/overrides/list",
        output: "one connections connector-metadata overrides list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CatalogMetadata {
        path: "one/connections/connector-metadata/overrides/delete",
        output: "one connections connector-metadata overrides delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/connectorMetadata/{connector}/overrides in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/list",
        output: "one job-group list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/count",
        output: "one job-group count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobLibrary/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/pdf-results",
        output: "one job-group pdf-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/pdfResults in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/run",
        output: "one job-group run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/jobGroups in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/publish",
        output: "one job-group publish envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PUT /v4/jobGroups/{id}/publish in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/detail",
        output: "one job-group detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/cancel",
        output: "one job-group cancel envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to POST /v4/jobGroups/{id}/cancel in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/status",
        output: "one job-group status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/status in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/inputs",
        output: "one job-group inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/inputs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/outputs",
        output: "one job-group outputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/outputs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/jobs",
        output: "one job-group jobs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/jobs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/publications",
        output: "one job-group publications envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/publications in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/profile",
        output: "one job-group profile envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profile in the One API docs."],
    },
    CatalogMetadata {
        path: "one/job-groups/profile-results",
        output: "one job-group profile-results envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/jobGroups/{id}/profileResults in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/list",
        output: "one output-object list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/count",
        output: "one output-object count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/create",
        output: "one output-object create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/outputObjects in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/detail",
        output: "one output-object detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/update",
        output: "one output-object update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/outputObjects/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/delete",
        output: "one output-object delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/outputObjects/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/inputs",
        output: "one output-object inputs envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/outputObjects/{id}/inputs in the One API docs."],
    },
    CatalogMetadata {
        path: "one/output-objects/wrangle-to-python",
        output: "one output-object wrangle-to-python envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to POST /v4/outputObjects/{id}/wrangleToPython in the One API docs.",
            "Requires --apply.",
        ],
    },
    CatalogMetadata {
        path: "one/webhook-flow-tasks/create",
        output: "one webhook-flow-task create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhookFlowTasks in the One API docs."],
    },
    CatalogMetadata {
        path: "one/webhook-flow-tasks/detail",
        output: "one webhook-flow-task detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/webhook-flow-tasks/delete",
        output: "one webhook-flow-task delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/webhookFlowTasks/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/webhook-flow-tasks/test",
        output: "one webhook-flow-tasks test envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/webhooks/test in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/list",
        output: "one write-setting list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/count",
        output: "one write-setting count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/create",
        output: "one write-setting create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to POST /v4/writeSettings in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/detail",
        output: "one write-setting detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/writeSettings/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/update",
        output: "one write-setting update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "payload json"],
        notes: &["Maps to PATCH /v4/writeSettings/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/write-settings/delete",
        output: "one write-setting delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to DELETE /v4/writeSettings/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/scheduling/create",
        output: "one scheduling create envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "server_api",
            "payload json",
            "TTY confirmation",
        ],
        notes: &[
            "Maps to POST /v4/schedules in the One API docs; applied requests require confirmation.",
        ],
    },
    CatalogMetadata {
        path: "one/scheduling/list",
        output: "one scheduling list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/schedules in the One API docs."],
    },
    CatalogMetadata {
        path: "one/scheduling/detail",
        output: "one scheduling detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/schedules/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/scheduling/update",
        output: "one scheduling update envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "server_api",
            "payload json",
            "TTY confirmation",
        ],
        notes: &[
            "Maps to PUT /v4/schedules/{id} in the One API docs; applied requests require confirmation.",
        ],
    },
    CatalogMetadata {
        path: "one/scheduling/enable",
        output: "one scheduling enable envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "TTY confirmation"],
        notes: &[
            "Maps to POST /v4/schedules/{id}/enable in the One API docs; applied requests require confirmation.",
        ],
    },
    CatalogMetadata {
        path: "one/scheduling/disable",
        output: "one scheduling disable envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "TTY confirmation"],
        notes: &[
            "Maps to POST /v4/schedules/{id}/disable in the One API docs; applied requests require confirmation.",
        ],
    },
    CatalogMetadata {
        path: "one/scheduling/delete",
        output: "one scheduling delete envelope",
        safety: "destructive",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api", "TTY confirmation"],
        notes: &["Maps to DELETE /v4/schedules/{id} in the One API docs."],
    },
    CatalogMetadata {
        path: "one/scheduling/count",
        output: "one scheduling count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/schedules/count in the One API docs."],
    },
    CatalogMetadata {
        path: "one/workflows",
        output: "one workflows help",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &[
            "Alteryx One cloud-native (canvas) workflows, ULID-keyed, served by /svc-workflow.",
            "Distinct from `one flows`, which is Designer Cloud /v4/flows keyed by integer ids.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/list",
        output: "one workflows list envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/workflows. Absent from the published /v4 OpenAPI spec."],
    },
    CatalogMetadata {
        path: "one/workflows/count",
        output: "one workflows count envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Synthesized from the GET /v4/workflows envelope total; no /v4/workflows/count route exists.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/detail",
        output: "one workflows detail envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Synthesized by filtering GET /svc-workflow/api/v1/assets; no GET /v4/workflows/{id} route exists.",
            "Emits detail_source so callers can distinguish client-side assembly from a server lookup.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/dependencies",
        output: "one workflows dependencies envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to GET /svc-workflow/api/v1/assets/{id}/dependencies; returns connections, datasets, macros.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/assets",
        output: "one workflows assets envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /svc-workflow/api/v1/assets; a superset of `one workflows list`."],
    },
    CatalogMetadata {
        path: "one/workflows/engines",
        output: "one workflows engines envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /svc-workflow/api/v0/workflows/{id}/availableEngines."],
    },
    CatalogMetadata {
        path: "one/workflows/tools",
        output: "one workflows tools envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /svc-workflow/api/v1/tools; workspace-scoped, not per-workflow."],
    },
    CatalogMetadata {
        path: "one/workflows/run",
        output: "one workflows run envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "server_api",
            "workflow execute permission",
        ],
        notes: &[
            "Maps to POST /svc-workflow/api/v1/workflows/{id}/run. Requires --apply and confirmation.",
            "The applied response returns the provider jobId; use that id with `one workflows cancel`.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/cancel",
        output: "one workflows cancel envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &[
            "central runtime profile",
            "server_api",
            "workflow run/job id",
        ],
        notes: &[
            "Maps to POST /svc-workflow/api/v1/jobs/{id}/cancel. Requires --apply and confirmation.",
            "Some workspaces may return WFS Jobs is not enabled in this environment.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/copy",
        output: "one workflows copy envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to POST /svc-workflow/api/v2/workflows/{id}/duplicate. Requires --apply.",
            "--version defaults to the workflow's current version, resolved via the assets listing.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/share",
        output: "one workflows share envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to POST /svc-workflow/api/v2/workflows/{id}/share. Requires --apply.",
            "--to-person accepts an email (resolved via one GET /v4/people call) or a numeric id; \
             resolution runs before the --apply gate, so a dry run's would_send is byte-identical \
             to what --apply sends.",
            "--include-dependencies on a dry run also fetches /dependencies and attaches a \
             dependency_preview so an unauthorized blast radius is visible before commit.",
        ],
    },
    CatalogMetadata {
        path: "one/workflows/delete",
        output: "one workflows delete envelope",
        safety: "mutating",
        mutating: true,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Maps to DELETE /svc-workflow/api/v2/workflows/{id}. Requires --apply.",
            "Resolves the workflow's name from the assets listing before prompting and before \
             the live call, both to name the target in the confirmation and to reject an \
             unknown id before any mutating request is sent.",
            "No known restore/trash endpoint exists for this resource; treat as irreversible.",
        ],
    },
    CatalogMetadata {
        path: "license/api/status",
        output: "license api status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use to inspect licensing API posture before diagnostics."],
    },
    CatalogMetadata {
        path: "license/api/diagnose",
        output: "license api diagnostic envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Use before future license api call-style workflows."],
    },
    CatalogMetadata {
        path: "server/upgrade/plan",
        output: "upgrade plan manifest",
        safety: "read-only",
        mutating: false,
        prerequisites: &["source version", "target version"],
        notes: &["Use this to map supported upgrade hops."],
    },
    CatalogMetadata {
        path: "catalog/list",
        output: "command catalog entries",
        safety: "read-only",
        mutating: false,
        prerequisites: &["none"],
        notes: &["Use this when another tool needs to discover available commands."],
    },
    CatalogMetadata {
        path: "catalog/describe",
        output: "single command metadata",
        safety: "read-only",
        mutating: false,
        prerequisites: &["catalog entry name or path"],
        notes: &["Accepts either a name or a path-like catalog key."],
    },
    CatalogMetadata {
        path: "discover",
        output: "live cli discovery tree",
        safety: "read-only",
        mutating: false,
        prerequisites: &["none"],
        notes: &[
            "Top-level progressive disclosure entry point for agent harnesses.",
            "Use --deep to expand the full subtree or pass a path to drill down.",
        ],
    },
    CatalogMetadata {
        path: "server/diagnose/startup",
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
    CatalogMetadata {
        path: "server/diagnose/tls",
        output: "tls diagnosis steps and evidence",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server.webapi_url"],
        notes: &[
            "Focuses on SSL/TLS, port binding, and proxy configuration.",
            "Use this for gallery binding, controller cert, and HTTPS setup issues.",
        ],
    },
    CatalogMetadata {
        path: "server/server-logs/discover",
        output: "log inventory envelope (paths, sizes, mtimes)",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server install path"],
        notes: &[
            "First step in any log triage. Surfaces canonical paths so context queries can target them.",
        ],
    },
    CatalogMetadata {
        path: "server/server-logs/context",
        output: "context envelope (matches with before/after windows)",
        safety: "read-only",
        mutating: false,
        prerequisites: &["log file path", "query string"],
        notes: &[
            "Use --before / --after to widen the window.",
            "Pair with `server-logs discover` to enumerate log paths first.",
        ],
    },
    CatalogMetadata {
        path: "server/server-logs/inventory",
        output: "inventory envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &["Coarser than `discover`; intended for at-a-glance posture."],
    },
    CatalogMetadata {
        path: "server/server-logs/summary",
        output: "summary envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["log file path"],
        notes: &["Quick triage before drilling in with `context`."],
    },
    CatalogMetadata {
        path: "server/auth/status",
        output: "auth status envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server settings"],
        notes: &["Use this before SAML diagnosis or simulation."],
    },
    CatalogMetadata {
        path: "server/auth/diagnose/saml",
        output: "saml diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &[
            "central runtime profile",
            "metadata url or file when available",
        ],
        notes: &["Focuses on Server-side SAML configuration and common mismatch checks."],
    },
    CatalogMetadata {
        path: "mongo/query",
        output: "mongo query envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongosh available on PATH"],
        notes: &["Use for targeted inspection of Gallery and Service collections."],
    },
    CatalogMetadata {
        path: "mongo/doctor",
        output: "mongo doctor envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "mongosh available on PATH"],
        notes: &["Targets queue, results, users, and app info collections."],
    },
    CatalogMetadata {
        path: "server/auth/diagnose/saml-logs",
        output: "saml log diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "SAML login logs"],
        notes: &["Targets alteryx-sso and aas log families."],
    },
    CatalogMetadata {
        path: "server/auth/diagnose/certificate",
        output: "certificate diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "certificate file when available"],
        notes: &["Focuses on certificate presence, parsing, and likely trust issues."],
    },
    CatalogMetadata {
        path: "server/auth/diagnose/ad-legacy",
        output: "legacy ad diagnosis envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile"],
        notes: &["Kept intentionally narrow as a legacy troubleshooting path."],
    },
    CatalogMetadata {
        path: "server/auth/simulate/saml",
        output: "saml simulation envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "metadata url or file"],
        notes: &["Designed as a diagnostic harness, not a full IdP emulator."],
    },
    CatalogMetadata {
        path: "server/doctor/startup",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataStatus {
    Curated,
    Unclassified,
}

impl MetadataStatus {
    fn as_str(self) -> &'static str {
        match self {
            MetadataStatus::Curated => "curated",
            MetadataStatus::Unclassified => "unclassified",
        }
    }
}

/// A derived, owned catalog record for one live command. Semantic fields are
/// optional so an all-scope, unannotated command can be represented
/// honestly instead of borrowing a curated command's classification.
struct CatalogCommandRecord {
    name: String,
    path: String,
    summary: String,
    metadata_status: MetadataStatus,
    output: Option<&'static str>,
    safety: Option<&'static str>,
    mutating: Option<bool>,
    prerequisites: &'static [&'static str],
    notes: &'static [&'static str],
}

impl CatalogCommandRecord {
    fn to_json(&self, full: bool) -> Value {
        let mut entry = json!({
            "kind": "command",
            "name": self.name,
            "path": self.path,
            "summary": self.summary,
            "metadata_status": self.metadata_status.as_str(),
            "output": self.output,
            "safety": self.safety,
            "mutating": self.mutating,
        });
        if full {
            entry["prerequisites"] = json!(self.prerequisites);
            entry["notes"] = json!(self.notes);
        }
        entry
    }
}

/// Validate `metadata` against the live command tree and return a lookup
/// keyed by canonical path. `visible_paths` is every non-hidden live command
/// path (`command_surface::visible_command_paths()` in production);
/// `all_paths` is every live command path *including* hidden ones, used only
/// to tell "unknown" (not a command at all) apart from "hidden" (a real
/// command, but not catalog-eligible) in the error message. Exposed as a
/// standalone helper (rather than inlined into `catalog_command_records`) so
/// tests can exercise duplicate/unknown/hidden failures against a supplied
/// metadata slice and a supplied live-path pair without touching the real
/// `CATALOG_METADATA` static or the real `ayx` clap tree.
fn validate_metadata<'a>(
    metadata: &'a [CatalogMetadata],
    visible_paths: &BTreeSet<String>,
    all_paths: &BTreeSet<String>,
) -> Result<BTreeMap<&'a str, &'a CatalogMetadata>> {
    let mut map: BTreeMap<&'a str, &'a CatalogMetadata> = BTreeMap::new();
    for row in metadata {
        if map.contains_key(row.path) {
            bail!(
                "catalog metadata error: duplicate path '{}' appears more than once in CATALOG_METADATA",
                row.path
            );
        }
        if !all_paths.contains(row.path) {
            bail!(
                "catalog metadata error: unknown path '{}' does not match any command in the live clap tree (renamed or removed command?)",
                row.path
            );
        }
        if !visible_paths.contains(row.path) {
            bail!(
                "catalog metadata error: path '{}' resolves to a hidden command and cannot carry catalog metadata",
                row.path
            );
        }
        map.insert(row.path, row);
    }
    Ok(map)
}

/// Every command path in the live tree, hidden or not -- unlike
/// `command_surface::visible_commands()`, this does not stop descending
/// into a hidden node's children. Used only by `validate_metadata` to
/// distinguish "unknown" from "hidden" in error messages.
fn all_command_paths(root: &clap::Command) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut tokens = Vec::new();
    collect_all_paths(root, &mut tokens, &mut out);
    out
}

fn collect_all_paths(
    command: &clap::Command,
    tokens: &mut Vec<String>,
    out: &mut BTreeSet<String>,
) {
    for child in command.get_subcommands() {
        tokens.push(child.get_name().to_string());
        out.insert(tokens.join("/"));
        collect_all_paths(child, tokens, out);
        tokens.pop();
    }
}

/// Join the live command tree with `CATALOG_METADATA`. All-scope always
/// starts from `command_surface::visible_commands()`, so a newly added
/// visible command can never be silently absent -- it just arrives
/// `unclassified` until a metadata row is added for it. `Curated` filters
/// that same derived vector down to metadata-bearing records; it never
/// starts from `CATALOG_METADATA` directly.
fn catalog_command_records(scope: CatalogScope) -> Result<Vec<CatalogCommandRecord>> {
    let live = command_surface::visible_commands();
    let visible_paths: BTreeSet<String> = live.iter().map(|cmd| cmd.path.clone()).collect();
    let all_paths = all_command_paths(&command_surface::root_command());
    let metadata_map = validate_metadata(CATALOG_METADATA, &visible_paths, &all_paths)?;

    let records: Vec<CatalogCommandRecord> = live
        .into_iter()
        .map(|cmd| match metadata_map.get(cmd.path.as_str()) {
            Some(meta) => CatalogCommandRecord {
                name: cmd.name,
                path: cmd.path,
                summary: cmd.summary,
                metadata_status: MetadataStatus::Curated,
                output: Some(meta.output),
                safety: Some(meta.safety),
                mutating: Some(meta.mutating),
                prerequisites: meta.prerequisites,
                notes: meta.notes,
            },
            None => CatalogCommandRecord {
                name: cmd.name,
                path: cmd.path,
                summary: cmd.summary,
                metadata_status: MetadataStatus::Unclassified,
                output: None,
                safety: Some("unclassified"),
                mutating: None,
                prerequisites: &[],
                notes: &[],
            },
        })
        .collect();

    Ok(match scope {
        CatalogScope::All => records,
        CatalogScope::Curated => records
            .into_iter()
            .filter(|record| record.metadata_status == MetadataStatus::Curated)
            .collect(),
    })
}

pub(crate) fn catalog_list_envelope(
    tag: Option<&str>,
    format: &str,
    scope: CatalogScope,
) -> Result<Envelope> {
    let full = match format {
        "compact" => false,
        "full" => true,
        other => bail!(
            "unsupported catalog format '{}'; use compact or full",
            other
        ),
    };
    let records = catalog_command_records(scope)?;
    let commands: Vec<Value> = records.iter().map(|record| record.to_json(full)).collect();
    let capabilities = capability::list_capabilities(tag, full)?;

    Ok(Envelope::ok_with_data(
        "catalog entries listed",
        json!({
            "command_schema_version": 2,
            "format": format,
            "scope": scope.as_str(),
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

    // No scope restriction here: search the all-scope derived command map so
    // `catalog describe` can resolve any visible command, curated or not.
    let records = catalog_command_records(CatalogScope::All)?;
    let record = records
        .iter()
        .find(|record| record.name == identifier || record.path == identifier)
        .ok_or_else(|| anyhow!("catalog entry '{}' not found", identifier))?;

    Ok(Envelope::ok_with_data(
        "catalog entry described",
        record.to_json(true),
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

    // ─── all-scope: derived directly from the live tree ───────────────────

    #[test]
    fn all_scope_paths_exactly_equal_visible_command_paths() {
        let records = catalog_command_records(CatalogScope::All).expect("all-scope records");
        let record_paths: BTreeSet<String> = records.iter().map(|r| r.path.clone()).collect();
        assert_eq!(
            record_paths,
            command_surface::visible_command_paths(),
            "catalog_command_records(All) must exactly equal the live command surface -- \
             a new visible command must never be silently absent"
        );
    }

    #[test]
    fn every_all_scope_record_has_canonical_identity_and_honest_classification() {
        let records = catalog_command_records(CatalogScope::All).expect("all-scope records");
        assert!(!records.is_empty(), "expected a non-empty command tree");
        for record in &records {
            assert_eq!(
                record.name.replace(' ', "/"),
                record.path,
                "name/path identity mismatch for {}",
                record.path
            );
            assert!(
                !record.summary.trim().is_empty(),
                "{} has a blank clap summary",
                record.path
            );
            match record.metadata_status {
                MetadataStatus::Curated => {
                    assert!(
                        record.output.is_some(),
                        "{} curated but output is None",
                        record.path
                    );
                    assert!(
                        record.safety.is_some(),
                        "{} curated but safety is None",
                        record.path
                    );
                    assert!(
                        record.mutating.is_some(),
                        "{} curated but mutating is None",
                        record.path
                    );
                }
                MetadataStatus::Unclassified => {
                    assert!(
                        record.output.is_none(),
                        "{} unclassified but output is Some",
                        record.path
                    );
                    assert_eq!(
                        record.safety,
                        Some("unclassified"),
                        "{} unclassified safety must be the literal string, not null",
                        record.path
                    );
                    assert!(
                        record.mutating.is_none(),
                        "{} unclassified mutation must be null, never false",
                        record.path
                    );
                    assert!(
                        record.prerequisites.is_empty(),
                        "{} expected empty prerequisites",
                        record.path
                    );
                    assert!(
                        record.notes.is_empty(),
                        "{} expected empty notes",
                        record.path
                    );
                }
            }
        }
    }

    // ─── curated scope: exactly the metadata-key path set ─────────────────

    #[test]
    fn curated_scope_matches_metadata_key_set_and_preserves_legacy_values() {
        let records = catalog_command_records(CatalogScope::Curated).expect("curated records");
        let record_paths: BTreeSet<&str> = records.iter().map(|r| r.path.as_str()).collect();
        let metadata_paths: BTreeSet<&str> = CATALOG_METADATA.iter().map(|m| m.path).collect();
        assert_eq!(
            record_paths, metadata_paths,
            "curated scope must be exactly the CATALOG_METADATA key set, no more, no less"
        );
        assert_eq!(
            records.len(),
            CATALOG_METADATA.len(),
            "no duplicate/dropped curated records"
        );
        assert!(
            records
                .iter()
                .all(|r| r.metadata_status == MetadataStatus::Curated)
        );

        let by_path: BTreeMap<&str, &CatalogCommandRecord> =
            records.iter().map(|r| (r.path.as_str(), r)).collect();

        // Read-only representative.
        let profile_list = by_path["profile/list"];
        assert_eq!(profile_list.safety, Some("read-only"));
        assert_eq!(profile_list.mutating, Some(false));
        assert_eq!(profile_list.output, Some("profile registry envelope"));

        // Mutating Mongo representative.
        let mongo_backup = by_path["mongo/backup"];
        assert_eq!(mongo_backup.mutating, Some(true));
        assert_eq!(mongo_backup.safety, Some("mutating"));
        assert_eq!(
            mongo_backup.notes,
            &[
                "Requires --apply for a live backup.",
                "Writes audit artifacts."
            ]
        );

        // Mutating One representative.
        let one_login = by_path["one/login"];
        assert_eq!(one_login.mutating, Some(true));
        assert_eq!(one_login.safety, Some("mutating"));

        // `catalog list` itself is curated.
        assert!(by_path.contains_key("catalog/list"));

        // `ui`-gated rows are curated only when the feature is compiled in --
        // the clap commands themselves (and thus the metadata rows) don't
        // exist in the live tree otherwise.
        if cfg!(feature = "ui") {
            assert!(by_path.contains_key("one/ui/session/status"));
            assert!(by_path.contains_key("one/ui/workflow/inventory"));
            assert!(by_path.contains_key("one/ui/data/list-datasets"));
        } else {
            assert!(!by_path.contains_key("one/ui/session/status"));
            assert!(!by_path.contains_key("one/ui/workflow/inventory"));
            assert!(!by_path.contains_key("one/ui/data/list-datasets"));
        }
    }

    // ─── catalog list envelope ──────────────────────────────────────────

    #[test]
    fn catalog_list_default_scope_is_all_and_carries_schema_fields() {
        let env = catalog_list_envelope(None, "compact", CatalogScope::All)
            .expect("catalog list should succeed");
        assert_eq!(env.data["command_schema_version"], 2);
        assert_eq!(env.data["scope"], "all");
        let commands = env.data["commands"].as_array().expect("commands array");
        assert_eq!(
            commands.len() as u64,
            env.data["command_count"].as_u64().unwrap()
        );
        assert_eq!(
            commands.len(),
            command_surface::visible_command_paths().len()
        );
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
    fn catalog_list_curated_scope_matches_metadata_count() {
        let env = catalog_list_envelope(None, "compact", CatalogScope::Curated)
            .expect("catalog list curated should succeed");
        assert_eq!(env.data["scope"], "curated");
        let commands = env.data["commands"].as_array().expect("commands array");
        assert_eq!(commands.len(), CATALOG_METADATA.len());
    }

    // ─── catalog describe ────────────────────────────────────────────────

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
    fn catalog_describe_finds_a_visible_all_scope_command_that_is_not_curated() {
        // `server/server-logs/tail` is a real, visible clap command with no
        // CATALOG_METADATA row -- `catalog describe` must still resolve it
        // (unlike the old COMMAND_SPECS-only lookup, which could not).
        assert!(
            !CATALOG_METADATA
                .iter()
                .any(|m| m.path == "server/server-logs/tail"),
            "test assumption broken: server/server-logs/tail unexpectedly has metadata"
        );
        let env = catalog_describe_envelope("server/server-logs/tail")
            .expect("catalog describe should resolve an unclassified live command");
        assert_eq!(env.data["path"], "server/server-logs/tail");
        assert_eq!(env.data["metadata_status"], "unclassified");
        assert_eq!(env.data["output"], Value::Null);
        assert_eq!(env.data["safety"], "unclassified");
        assert_eq!(env.data["mutating"], Value::Null);
    }

    #[test]
    fn catalog_describe_mongo_undo_notes_do_not_claim_unimplemented() {
        // Regression guard (final whole-branch review, mongo-mutation-execution
        // branch): `mongo undo --apply` shipped real, transaction-gated,
        // audited execution. The catalog is shipped, machine-readable public
        // metadata — a stale "not yet implemented" note must never silently
        // rot back in once the feature is real.
        let env = catalog_describe_envelope("mongo undo").expect("catalog describe should work");
        let notes = env.data["notes"].as_array().expect("notes array");
        let joined: String = notes
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !joined.to_lowercase().contains("not yet implemented"),
            "mongo undo catalog notes regressed to claiming unimplemented: {joined}"
        );
        assert!(
            !joined.to_lowercase().contains("follow-up task"),
            "mongo undo catalog notes regressed to claiming unimplemented: {joined}"
        );
    }

    #[test]
    fn catalog_describe_rejects_unknown_identifier() {
        let err = catalog_describe_envelope("not/a/real/command").unwrap_err();
        assert!(err.to_string().contains("not/a/real/command"));
    }

    // ─── validation: duplicate / unknown / hidden metadata keys ────────────

    fn sample_metadata(path: &'static str) -> CatalogMetadata {
        CatalogMetadata {
            path,
            output: "sample output",
            safety: "read-only",
            mutating: false,
            prerequisites: &[],
            notes: &[],
        }
    }

    #[test]
    fn validate_metadata_rejects_duplicate_path() {
        let rows = vec![sample_metadata("a/b"), sample_metadata("a/b")];
        let visible: BTreeSet<String> = ["a/b".to_string()].into_iter().collect();
        let all = visible.clone();
        let err = validate_metadata(&rows, &visible, &all).expect_err("duplicate must fail");
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "message was: {msg}");
        assert!(msg.contains("a/b"), "message was: {msg}");
    }

    #[test]
    fn validate_metadata_rejects_unknown_path() {
        let rows = vec![sample_metadata("no/such/command")];
        let visible: BTreeSet<String> = BTreeSet::new();
        let all: BTreeSet<String> = BTreeSet::new();
        let err = validate_metadata(&rows, &visible, &all).expect_err("unknown must fail");
        let msg = err.to_string();
        assert!(msg.contains("unknown"), "message was: {msg}");
        assert!(msg.contains("no/such/command"), "message was: {msg}");
    }

    #[test]
    fn validate_metadata_rejects_hidden_path() {
        let rows = vec![sample_metadata("hidden/cmd")];
        let visible: BTreeSet<String> = BTreeSet::new();
        let all: BTreeSet<String> = ["hidden/cmd".to_string()].into_iter().collect();
        let err = validate_metadata(&rows, &visible, &all).expect_err("hidden must fail");
        let msg = err.to_string();
        assert!(msg.contains("hidden"), "message was: {msg}");
        assert!(msg.contains("hidden/cmd"), "message was: {msg}");
    }

    #[test]
    fn validate_metadata_accepts_clean_overlay() {
        let rows = vec![sample_metadata("a/b")];
        let visible: BTreeSet<String> = ["a/b".to_string()].into_iter().collect();
        let all = visible.clone();
        let map = validate_metadata(&rows, &visible, &all).expect("clean overlay must validate");
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("a/b"));
    }

    // ─── capability listing/filtering/run (unchanged behavior) ────────────

    #[test]
    fn catalog_list_filters_capabilities_by_tag() {
        let env = catalog_list_envelope(Some("cloud"), "compact", CatalogScope::All)
            .expect("catalog list should work");
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
        assert_eq!(token, "fresh-token");
    }
}
