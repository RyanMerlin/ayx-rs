use crate::ONE_API_BASE_URL;
use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use serde_json::json;

struct EndpointSpec {
    method: &'static str,
    path: &'static str,
    /// Every CLI command that dispatches this endpoint. More than one is legitimate:
    /// `one whoami` and `one person current` both hit `GET /v4/people/current`.
    commands: &'static [&'static str],
}

struct SurfaceSpec {
    name: &'static str,
    status: &'static str,
    endpoints: &'static [EndpointSpec],
    notes: &'static [&'static str],
}

const IAM_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/current",
        commands: &["one workspace current"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{workspaceId}",
        commands: &["one workspace detail"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{id}/configuration",
        commands: &[
            "one workspace configuration",
            "one workspace configuration-v4",
        ],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/people",
        commands: &[
            "one person list",
            "one workspace people",
            // `share` resolves --to-person emails to ids before building its body.
            "one workflows share",
        ],
    },
    // The tenant OpenAPI spec declares `workspaceId` as an integer — the
    // numeric workspace id. An earlier probe used the workspace GID, 404'd,
    // and this command was wrongly moved to `/v4/people?role=admin` (which the
    // gateway ignores). See docs/ayx-cli-testing-issues.md Issue 1.
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{workspaceId}/admins",
        commands: &["one workspace admins"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{id}/groups",
        commands: &["one workspace groups"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/groups",
        commands: &["one workspace groups-global"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{id}/invitationLink",
        commands: &["one workspace invitation-link"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{workspaceId}/cloudConfigs",
        commands: &["one workspace cloud-configs"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/workspaces/{id}/people/batch",
        commands: &["one workspace invite-users", "one workspace invite-list"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/workspaces/{workspaceId}/people/{id}",
        commands: &["one workspace remove-user"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/workspaces/{id}/people/suspend",
        commands: &["one workspace suspend-users"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/workspaces/{id}/people/unsuspend",
        commands: &["one workspace unsuspend-users"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/workspaces/{id}/transfer",
        commands: &["one workspace transfer"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/authorization/roles/{id}/people",
        commands: &["one role list-assignments"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/authorization/roles",
        commands: &["one role list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/authorization/roles/{id}",
        commands: &["one role detail"],
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/authorization/roles/{id}/people",
        commands: &["one role assign"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/authorization/roles/{id}/people/{subjectId}",
        commands: &["one role unassign"],
    },
];

const PLANS_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/plans",
        commands: &["one plans list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/full",
        commands: &["one plans detail", "one plans full"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/{id}/run",
        commands: &["one plans run"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/count",
        commands: &["one plans count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/runParameters",
        commands: &["one plans run-parameters"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/schedules",
        commands: &["one plans schedules"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/package",
        commands: &["one plans export"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/package",
        commands: &["one plans import"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/permissions",
        commands: &["one plans permissions"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/plans/{id}/permissions/{subjectId}",
        commands: &["one plans permissions"],
    },
];

const SCHEDULING_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/schedules",
        commands: &["one scheduling create"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/schedules",
        commands: &["one scheduling list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/schedules/{id}",
        commands: &["one scheduling detail"],
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/schedules/{id}",
        commands: &["one scheduling update"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/schedules/{id}/enable",
        commands: &["one scheduling enable"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/schedules/{id}/disable",
        commands: &["one scheduling disable"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/schedules/{id}",
        commands: &["one scheduling delete"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/schedules/count",
        commands: &["one scheduling count"],
    },
];

const PLAN_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/plans",
        commands: &["one plans create"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/{id}/permissions",
        commands: &["one plans share"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/plans/{id}",
        commands: &["one plans update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/plans/{id}",
        commands: &["one plans delete"],
    },
];

/// Alteryx One cloud-native workflows.
///
/// A separate service from the `/v4` gateway: ULID-keyed canvas workflows served by
/// `/svc-workflow/api/vN`. `GET /v4/workflows` is the one listing route the gateway
/// exposes and is absent from the published `/v4/open-api-spec`, so `one api coverage`
/// will report it as stale — correctly, since the spec does not describe it.
/// There is no `GET /v4/workflows/{id}` and no `/v4/workflows/count`.
/// Read and management rows live-verified 2026-07-26 through 2026-09-01;
/// execution and cancellation were verified against the Workflow Service on
/// 2026-09-02, with cancellation capability-blocked on the target workspace.
const WORKFLOW_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/workflows",
        commands: &["one workflows list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workflows?limit=1",
        commands: &["one workflows count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/svc-workflow/api/v1/assets",
        commands: &[
            "one workflows assets",
            "one workflows detail",
            // `copy` resolves the current version from the asset list when
            // --version is omitted.
            "one workflows copy",
        ],
    },
    EndpointSpec {
        method: "GET",
        path: "/svc-workflow/api/v1/assets/{id}/dependencies",
        commands: &["one workflows dependencies"],
    },
    EndpointSpec {
        method: "GET",
        path: "/svc-workflow/api/v0/workflows/{id}/availableEngines",
        commands: &["one workflows engines"],
    },
    EndpointSpec {
        method: "GET",
        path: "/svc-workflow/api/v1/tools",
        commands: &["one workflows tools"],
    },
    EndpointSpec {
        method: "POST",
        path: "/svc-workflow/api/v1/workflows/{id}/run",
        commands: &["one workflows run"],
    },
    EndpointSpec {
        method: "POST",
        path: "/svc-workflow/api/v1/jobs/{id}/cancel",
        commands: &["one workflows cancel"],
    },
    EndpointSpec {
        method: "POST",
        path: "/svc-workflow/api/v2/workflows/{id}/duplicate",
        commands: &["one workflows copy"],
    },
    EndpointSpec {
        method: "POST",
        path: "/svc-workflow/api/v2/workflows/{id}/share",
        commands: &["one workflows share"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/svc-workflow/api/v2/workflows/{id}",
        commands: &["one workflows delete"],
    },
];

const FLOW_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/flows",
        commands: &["one flows create"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows",
        commands: &["one flows list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/count",
        commands: &["one flows count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flowsLibrary",
        commands: &["one flows library list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flowsLibrary/count",
        commands: &["one flows library count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders",
        commands: &["one flows folders list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/count",
        commands: &["one flows folders count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}",
        commands: &["one flows folders detail"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/folders",
        commands: &["one flows folders create"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/folders/{id}",
        commands: &["one flows folders update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/folders/{id}",
        commands: &["one flows folders delete"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}/flows",
        commands: &["one flows folders flows list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}/flows/count",
        commands: &["one flows folders flows count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}",
        commands: &["one flows detail"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/flows/{id}",
        commands: &["one flows update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/flows/{id}",
        commands: &["one flows delete"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/copy",
        commands: &["one flows copy"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/run",
        commands: &["one flows run"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/validate",
        commands: &["one flows validate"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/recipeParameters",
        commands: &["one flows parameters"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/inputs",
        commands: &["one flows inputs"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/outputs",
        commands: &["one flows outputs"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/permissions",
        commands: &["one flows permissions"],
    },
    // Read side of the same path. `one flows permissions-get` has always dispatched
    // this; the inventory only ever recorded the POST.
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/permissions",
        commands: &["one flows permissions-get"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/move",
        commands: &["one flows move"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/flows/{id}/replaceDataset",
        commands: &["one flows replace-dataset"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/package",
        commands: &["one flows import"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/package/dryRun",
        commands: &["one flows import-dry-run"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/package",
        commands: &["one flows export"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/package/dryRun",
        commands: &["one flows export-dry-run"],
    },
];

const DATASET_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/datasetLibrary",
        commands: &["one datasets list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/datasetLibrary/count",
        commands: &["one datasets count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets",
        commands: &["one datasets wrangled list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets/count",
        commands: &["one datasets wrangled count"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets/{id}",
        commands: &["one datasets wrangled detail"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/importedDatasets/{id}",
        commands: &["one datasets imported detail"],
    },
];

const JOB_GROUP_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/jobLibrary",
        commands: &["one job-groups list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobLibrary/count",
        commands: &["one job-groups count"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/jobGroups",
        commands: &["one job-groups run"],
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/jobGroups/{id}/publish",
        commands: &["one job-groups publish"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}",
        commands: &["one job-groups detail"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/jobGroups/{id}/cancel",
        commands: &["one job-groups cancel"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/status",
        commands: &["one job-groups status"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/inputs",
        commands: &["one job-groups inputs"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/pdfResults",
        commands: &["one job-groups pdf-results"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/outputs",
        commands: &["one job-groups outputs"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/jobs",
        commands: &["one job-groups jobs"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/publications",
        commands: &["one job-groups publications"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/profile",
        commands: &["one job-groups profile"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/profileResults",
        commands: &["one job-groups profile-results"],
    },
];

const OUTPUT_OBJECT_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects",
        commands: &["one output-objects list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/count",
        commands: &["one output-objects count"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/outputObjects",
        commands: &["one output-objects create"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/{id}",
        commands: &["one output-objects detail"],
    },
    // PATCH, not PUT: cmd/one_output_objects.rs sends PATCH and the live API is the
    // authority. Same correction previously applied to `flows update`.
    EndpointSpec {
        method: "PATCH",
        path: "/v4/outputObjects/{id}",
        commands: &["one output-objects update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/outputObjects/{id}",
        commands: &["one output-objects delete"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/{id}/inputs",
        commands: &["one output-objects inputs"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/outputObjects/{id}/wrangleToPython",
        commands: &["one output-objects wrangle-to-python"],
    },
];

const WEBHOOK_FLOW_TASK_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/webhookFlowTasks",
        commands: &["one webhook-flow-tasks create"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/webhookFlowTasks/{id}",
        commands: &["one webhook-flow-tasks detail"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/webhookFlowTasks/{id}",
        commands: &["one webhook-flow-tasks delete"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/webhooks/test",
        commands: &["one webhook-flow-tasks test"],
    },
];

const WRITE_SETTING_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings",
        commands: &["one write-settings list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings/count",
        commands: &["one write-settings count"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/writeSettings",
        commands: &["one write-settings create"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings/{id}",
        commands: &["one write-settings detail"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/writeSettings/{id}",
        commands: &["one write-settings update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/writeSettings/{id}",
        commands: &["one write-settings delete"],
    },
];

const CONNECTION_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/connections",
        commands: &["one connections list"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/count",
        commands: &["one connections count"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections",
        commands: &["one connections create"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections/dryRun",
        commands: &["one connections dry-run"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}",
        commands: &["one connections detail"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}/status",
        commands: &["one connections status"],
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/connections/{id}",
        commands: &["one connections update"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connections/{id}",
        commands: &["one connections delete"],
    },
    // Live-verified 2026-07-26. The previous rows pointed at
    // /v4/connections/{id}/permissions[/{aid}], which the API answers with
    // RouteNotFoundException; sharing lives on a shared /v4/connections/share route
    // that carries the connection id in the body (POST) or query (DELETE).
    //
    // `telemetry permissions connections --deep` shipped wired to the same dead
    // /permissions route (no /sharedSubjects suffix) until it was repaired
    // alongside this inventory row's own fix -- see telemetry/permissions.rs. It
    // is not listed in `commands` below: that field is `ayx one ...`-only (see
    // `coverage::tests::every_endpoint_row_names_at_least_one_one_command`), and
    // `telemetry permissions connections` is a different top-level surface that
    // happens to dispatch the same endpoint.
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}/permissions/sharedSubjects",
        commands: &[
            "one connections permissions",
            "one connections permissions detail",
        ],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections/share",
        commands: &["one connections permissions create"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connections/share",
        commands: &["one connections permissions delete"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/defaults",
        commands: &[
            "one connections connector-metadata defaults",
            "one connections connector-metadata template",
        ],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}",
        commands: &["one connections connector-metadata detail"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/publish/info",
        commands: &["one connections connector-metadata publish-info"],
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/overrides",
        commands: &["one connections connector-metadata overrides list"],
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connectorMetadata/{connector}/overrides",
        commands: &["one connections connector-metadata overrides create"],
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connectorMetadata/{connector}/overrides",
        commands: &["one connections connector-metadata overrides delete"],
    },
];

const DOCUMENTED_ONLY_SURFACES: &[SurfaceSpec] = &[];

const PARTIAL_SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec {
        name: "connection",
        status: "partial",
        endpoints: CONNECTION_ENDPOINTS,
        notes: &[
            "Connection lifecycle, dry-run, status, and permissions commands are wired.",
            "Connector metadata defaults, current values, and overrides are wired for JDBC behavior control.",
            "Credential-backend specifics remain encoded in the API payloads rather than a local domain model.",
        ],
    },
    SurfaceSpec {
        name: "dataset",
        status: "partial",
        endpoints: DATASET_ENDPOINTS,
        notes: &[
            "Dataset library list/count plus wrangled and imported dataset detail reads are wired.",
            "Mutating dataset lifecycle operations remain documented-only in this first cut.",
        ],
    },
    SurfaceSpec {
        name: "jobGroup",
        status: "partial",
        endpoints: JOB_GROUP_ENDPOINTS,
        notes: &[
            "Job-group execution, publish, and inspection commands are wired.",
            "PDF/log artifact downloads and other deeper job-library paths remain documented-only.",
        ],
    },
    SurfaceSpec {
        name: "outputObject",
        status: "partial",
        endpoints: OUTPUT_OBJECT_ENDPOINTS,
        notes: &[
            "Output object lifecycle and wrangle-to-python commands are wired.",
            "Additional nested resources stay documented-only until the CLI needs them.",
        ],
    },
    SurfaceSpec {
        name: "webhookFlowTask",
        status: "partial",
        endpoints: WEBHOOK_FLOW_TASK_ENDPOINTS,
        notes: &["Webhook task create/read/delete plus webhook test are wired."],
    },
    SurfaceSpec {
        name: "writeSetting",
        status: "partial",
        endpoints: WRITE_SETTING_ENDPOINTS,
        notes: &["Write-setting CRUD is wired."],
    },
    SurfaceSpec {
        name: "apiAccessTokens",
        status: "partial",
        endpoints: &[
            EndpointSpec {
                method: "GET",
                path: "/v4/apiAccessTokens",
                commands: &[
                    "one auth diagnose",
                    "one auth status",
                    "one doctor auth",
                    "one token",
                ],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/apiAccessTokens",
                commands: &["one token create"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/apiAccessTokens/{tokenId}",
                commands: &["one token detail"],
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/apiAccessTokens/{tokenId}",
                commands: &["one token delete"],
            },
        ],
        notes: &[
            "One API access-token CRUD is wired; additional token administration endpoints remain documented-only.",
        ],
    },
    SurfaceSpec {
        name: "person",
        status: "partial",
        endpoints: &[
            EndpointSpec {
                method: "GET",
                path: "/v4/people/current",
                commands: &["one person current", "one whoami"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people",
                commands: &[
                    "one person list",
                    "one workspace people",
                    // `share` resolves --to-person emails to ids before building its body.
                    "one workflows share",
                ],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people/count",
                commands: &["one person count"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people/{id}",
                commands: &["one person detail"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/people",
                commands: &["one person create"],
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/people/{id}",
                commands: &["one person update"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/people/{id}",
                commands: &["one person patch"],
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/people/{id}",
                commands: &["one person delete"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/people/current/updatePassword",
                commands: &["one person update-password"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/passwordresetrequest",
                commands: &["one person password-reset-request"],
            },
        ],
        notes: &[
            "Current lookup plus person list/count/detail/create/update/patch/delete/password workflows are wired; remaining person families stay documented-only.",
        ],
    },
    SurfaceSpec {
        name: "workspace",
        status: "partial",
        endpoints: &[
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces",
                commands: &["one workspace list"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces",
                commands: &["one workspace create"],
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/workspaces/{id}",
                commands: &["one workspace delete"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{id}/groups",
                commands: &["one workspace create-group"],
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/workspaces/{id}/groups/{groupId}",
                commands: &["one workspace delete-group"],
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/workspaces/{id}/groups/{groupId}",
                commands: &["one workspace update-group"],
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/workspaces/{id}/groups/{groupId}/roles",
                commands: &["one workspace set-group-roles"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{id}/groups/{groupId}/users",
                commands: &["one workspace add-group-users"],
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/workspaces/{id}/groups/{groupId}/users",
                commands: &["one workspace remove-group-users"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/{id}/configuration",
                commands: &[
                    "one workspace configuration",
                    "one workspace configuration-v4",
                ],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/current/transfer",
                commands: &["one workspace transfer-assets"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/current/configuration",
                commands: &["one workspace current-configuration"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/current/configuration",
                commands: &["one workspace save-current-configuration"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/{id}/configuration",
                commands: &["one workspace save-configuration-v4"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/{id}/configuration-schema",
                commands: &["one workspace configuration-schema"],
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/current/configuration-schema",
                commands: &["one workspace current-configuration-schema"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/current/delete-configuration",
                commands: &["one workspace delete-current-configuration"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{id}/delete-configuration",
                commands: &["one workspace delete-configuration"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{id}/people",
                commands: &["one workspace invite"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/{id}/people/batch",
                commands: &["one workspace reinvite-users"],
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/workspaces/{id}/people/{personId}/suspended",
                commands: &["one workspace suspend-user"],
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}",
                commands: &["one workspace create-cloud-config"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}",
                commands: &["one workspace update-cloud-config"],
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/{workspaceId}/people/{id}",
                commands: &["one workspace patch-user"],
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/workspaces/{workspaceId}/people/{id}",
                commands: &["one workspace update-user"],
            },
        ],
        notes: &[
            "Workspace listing, configuration, transfer, group, invitation, cloud-config, role, and workspace-user endpoints are wired; remaining workspace families stay documented-only.",
        ],
    },
];

const DEFERRED_SURFACES: &[SurfaceSpec] = &[];

const SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec {
        name: "platform.iam",
        status: "implemented",
        endpoints: IAM_ENDPOINTS,
        notes: &["Managed IAM / workspace-admin surface."],
    },
    SurfaceSpec {
        name: "misc",
        status: "implemented",
        endpoints: &[EndpointSpec {
            method: "GET",
            path: "/v4/open-api-spec",
            commands: &["one api coverage", "one api open-api-spec"],
        }],
        notes: &["The OpenAPI spec is now exposed through the CLI."],
    },
    SurfaceSpec {
        name: "plan",
        status: "implemented",
        endpoints: PLAN_ENDPOINTS,
        notes: &[
            "Only the /v4 plan endpoints the CLI actually dispatches are listed. Read paths \
             (list/count/run/permissions/package/runParameters/schedules) now use the \
             spec-documented /v4 plan paths instead — see the `plans` surface.",
        ],
    },
    SurfaceSpec {
        name: "plans",
        status: "implemented",
        endpoints: PLANS_ENDPOINTS,
        notes: &["Managed plans surface."],
    },
    SurfaceSpec {
        name: "workflow",
        status: "implemented",
        endpoints: WORKFLOW_ENDPOINTS,
        notes: &[
            "Alteryx One cloud-native (canvas) workflows, ULID-keyed, served by /svc-workflow.",
            "Distinct from the `flow` surface, which is Designer Cloud /v4/flows keyed by integer ids.",
            "detail and count are synthesized client-side; the API exposes no per-id or count route.",
        ],
    },
    SurfaceSpec {
        name: "flow",
        status: "implemented",
        endpoints: FLOW_ENDPOINTS,
        notes: &[
            "Flow lifecycle, package, parameters, library, folder, and permission commands are wired.",
            "The One surface does not expose arbitrary workflow authoring through this family.",
        ],
    },
    SurfaceSpec {
        name: "scheduling",
        status: "implemented",
        endpoints: SCHEDULING_ENDPOINTS,
        notes: &["Managed scheduling surface."],
    },
];

/// Returns every (method, path-template) pair declared in the inventory.
///
/// Used by drift-detection tests to verify that every endpoint string
/// hard-coded into the CLI dispatcher (`main.rs`) has a corresponding entry
/// in the inventory. When the inventory and the wiring diverge, the CLI
/// catalog lies to operators — tests must catch this.
pub fn inventory_endpoints() -> Vec<(&'static str, &'static str)> {
    SURFACES
        .iter()
        .chain(PARTIAL_SURFACES.iter())
        .chain(DOCUMENTED_ONLY_SURFACES.iter())
        .chain(DEFERRED_SURFACES.iter())
        .flat_map(|s| s.endpoints.iter().map(|e| (e.method, e.path)))
        .collect()
}

/// Like [`inventory_endpoints`] but also returns every wired command name.
///
/// One endpoint may be dispatched by several commands (`one whoami` and
/// `one person current` both issue `GET /v4/people/current`), so this yields a slice
/// rather than a single name.
pub fn inventory_endpoints_full() -> Vec<(&'static str, &'static str, &'static [&'static str])> {
    SURFACES
        .iter()
        .chain(PARTIAL_SURFACES.iter())
        .chain(DOCUMENTED_ONLY_SURFACES.iter())
        .chain(DEFERRED_SURFACES.iter())
        .flat_map(|s| s.endpoints.iter().map(|e| (e.method, e.path, e.commands)))
        .collect()
}

pub fn one_surface_inventory_envelope(config: &Config) -> Result<Envelope> {
    let implemented = SURFACES
        .iter()
        .map(|surface| {
            json!({
                "name": surface.name,
                "status": surface.status,
                "notes": surface.notes,
                "endpoints": surface.endpoints.iter().map(|endpoint| {
                    json!({
                        "method": endpoint.method,
                        "path": endpoint.path,
                        "commands": endpoint.commands,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let documented_only = DOCUMENTED_ONLY_SURFACES
        .iter()
        .map(|surface| {
            json!({
                "name": surface.name,
                "status": surface.status,
                "notes": surface.notes,
                "endpoints": surface.endpoints.iter().map(|endpoint| {
                    json!({
                        "method": endpoint.method,
                        "path": endpoint.path,
                        "commands": endpoint.commands,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let partial = PARTIAL_SURFACES
        .iter()
        .map(|surface| {
            json!({
                "name": surface.name,
                "status": surface.status,
                "notes": surface.notes,
                "endpoints": surface.endpoints.iter().map(|endpoint| {
                    json!({
                        "method": endpoint.method,
                        "path": endpoint.path,
                        "commands": endpoint.commands,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let deferred = DEFERRED_SURFACES
        .iter()
        .map(|surface| {
            json!({
                "name": surface.name,
                "status": surface.status,
                "notes": surface.notes,
                "endpoints": surface.endpoints.iter().map(|endpoint| {
                    json!({
                        "method": endpoint.method,
                        "path": endpoint.path,
                        "commands": endpoint.commands,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    Ok(Envelope::ok_with_data(
        "one api surface inventory",
        json!({
            "profile": config.profile_name,
            "base_url": ONE_API_BASE_URL,
            "surfaces": implemented,
            "partial_surfaces": partial,
            "documented_only_surfaces": documented_only,
            "deferred_surfaces": deferred,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile,
    };

    fn config() -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: None,
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(AlteryxOneProfile {
                schema_version: ayx_core::profile::CURRENT_PROFILE_SCHEMA_VERSION,
                account_email: "test@example.com".to_string(),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: Some("client-123".to_string()),
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: Some("https://example.invalid/token".to_string()),
                access_token: Some("token".to_string()),
                access_token_ref: None,
                refresh_token: Some("refresh".to_string()),
                refresh_token_ref: None,
                workspace_password: None,
                workspace_password_ref: None,
                workspace_credentials: Default::default(),
                active_workspace_id: None,
                auth_rollout: None,
                expected_workspace_id: None,
                sp_client_id: None,
                sp_token_endpoint_url: None,
                workspace_gid: None,
                auth_mode: Default::default(),
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
    fn inventory_includes_core_surfaces() {
        let env = one_surface_inventory_envelope(&config()).expect("inventory");
        let surfaces = env.data["surfaces"].as_array().expect("surfaces");
        assert!(
            surfaces
                .iter()
                .any(|surface| surface["name"] == "platform.iam")
        );
        assert!(surfaces.iter().any(|surface| surface["name"] == "plan"));
        assert!(surfaces.iter().any(|surface| surface["name"] == "plans"));
        assert!(surfaces.iter().any(|surface| surface["name"] == "flow"));
        assert!(surfaces.iter().any(|surface| surface["name"] == "misc"));
        assert!(
            surfaces
                .iter()
                .any(|surface| surface["name"] == "scheduling")
        );
        let partial = env.data["partial_surfaces"]
            .as_array()
            .expect("partial_surfaces");
        assert!(
            partial
                .iter()
                .any(|surface| surface["name"] == "connection")
        );
        assert!(partial.iter().any(|surface| surface["name"] == "jobGroup"));
        assert!(
            partial
                .iter()
                .any(|surface| surface["name"] == "outputObject")
        );
        assert!(
            partial
                .iter()
                .any(|surface| surface["name"] == "webhookFlowTask")
        );
        assert!(
            partial
                .iter()
                .any(|surface| surface["name"] == "writeSetting")
        );
        assert!(
            partial
                .iter()
                .any(|surface| surface["name"] == "apiAccessTokens")
        );
        assert!(partial.iter().any(|surface| surface["name"] == "person"));
        assert!(partial.iter().any(|surface| surface["name"] == "workspace"));
        let documented = env.data["documented_only_surfaces"]
            .as_array()
            .expect("documented_only_surfaces");
        assert!(documented.is_empty());
        let deferred = env.data["deferred_surfaces"]
            .as_array()
            .expect("deferred_surfaces");
        assert!(deferred.is_empty());
    }
}
