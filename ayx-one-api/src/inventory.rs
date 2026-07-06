use crate::ONE_API_BASE_URL;
use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use serde_json::json;

struct EndpointSpec {
    method: &'static str,
    path: &'static str,
    command: &'static str,
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
        command: "one platform workspace current",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{id}/configuration",
        command: "one platform workspace configuration",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{id}/people",
        command: "one platform workspace people",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{workspaceId}/admins",
        command: "one platform workspace admins",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/workspaces/{id}/people/batch",
        command: "one platform workspace invite-users",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/workspaces/{workspaceId}/people/{id}",
        command: "one platform workspace remove-user",
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/workspaces/{id}/people/{personId}/suspended",
        command: "one platform workspace suspend-users",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/workspaces/{id}/people/{personId}/suspended",
        command: "one platform workspace unsuspend-users",
    },
    EndpointSpec {
        method: "POST",
        path: "/iam/v1/workspaces/{id}/people/suspend",
        command: "one platform workspace suspend-users",
    },
    EndpointSpec {
        method: "POST",
        path: "/iam/v1/workspaces/{id}/people/unsuspend",
        command: "one platform workspace unsuspend-users",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/workspaces/{id}/transfer",
        command: "one platform workspace transfer",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/authorization/roles/{id}/people",
        command: "one platform role list-assignments",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/authorization/roles/{id}/people/{subjectId}",
        command: "one platform role assign",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/authorization/roles/{id}/people/{subjectId}",
        command: "one platform role unassign",
    },
];

const PLANS_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans",
        command: "one plans list",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/{id}",
        command: "one plans detail",
    },
    EndpointSpec {
        method: "POST",
        path: "/plans/v1/plans/{id}/run",
        command: "one plans run",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/count",
        command: "one plans count",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/{id}/runParameters",
        command: "one plans run-parameters",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/{id}/schedules",
        command: "one plans schedules",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/{id}/package",
        command: "one plans export",
    },
    EndpointSpec {
        method: "POST",
        path: "/plans/v1/plans/package",
        command: "one plans import",
    },
    EndpointSpec {
        method: "GET",
        path: "/plans/v1/plans/{id}/permissions",
        command: "one plans permissions",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/plans/v1/plans/{id}/permissions/{subjectId}",
        command: "one plans permissions remove",
    },
];

const SCHEDULING_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/scheduling/v1/schedules",
        command: "one scheduling list",
    },
    EndpointSpec {
        method: "GET",
        path: "/scheduling/v1/schedules/{id}",
        command: "one scheduling detail",
    },
    EndpointSpec {
        method: "POST",
        path: "/scheduling/v1/schedules/{id}/enable",
        command: "one scheduling enable",
    },
    EndpointSpec {
        method: "POST",
        path: "/scheduling/v1/schedules/{id}/disable",
        command: "one scheduling disable",
    },
    EndpointSpec {
        method: "GET",
        path: "/scheduling/v1/schedules/count",
        command: "one scheduling count",
    },
];

const BILLING_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/billing/v1/my/billing-accounts/current",
        command: "one billing current-account",
    },
    EndpointSpec {
        method: "GET",
        path: "/billing/v1/usage/export",
        command: "one billing usage-export",
    },
];

const PLAN_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/plans",
        command: "one plans create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans",
        command: "one plans list",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/{id}/run",
        command: "one plans run",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/{id}/permissions",
        command: "one plans share",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/permissions",
        command: "one plans permissions",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/plans/package",
        command: "one plans import",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/count",
        command: "one plans count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/runParameters",
        command: "one plans run-parameters",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/full",
        command: "one plans full",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/schedules",
        command: "one plans schedules",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/plans/{id}/package",
        command: "one plans export",
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/plans/{id}",
        command: "one plans update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/plans/{id}",
        command: "one plans delete",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/plans/{id}/permissions/{subjectId}",
        command: "one plans permissions remove",
    },
];

const FLOW_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/flows",
        command: "one flows create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows",
        command: "one flows list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/count",
        command: "one flows count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flowsLibrary",
        command: "one flows library list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flowsLibrary/count",
        command: "one flows library count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders",
        command: "one flows folders list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/count",
        command: "one flows folders count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}",
        command: "one flows folders detail",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/folders",
        command: "one flows folders create",
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/folders/{id}",
        command: "one flows folders update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/folders/{id}",
        command: "one flows folders delete",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}/flows",
        command: "one flows folders flows list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/folders/{id}/flows/count",
        command: "one flows folders flows count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}",
        command: "one flows detail",
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/flows/{id}",
        command: "one flows update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/flows/{id}",
        command: "one flows delete",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/copy",
        command: "one flows copy",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/run",
        command: "one flows run",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/validate",
        command: "one flows validate",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/recipeParameters",
        command: "one flows parameters",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/inputs",
        command: "one flows inputs",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/outputs",
        command: "one flows outputs",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/permissions",
        command: "one flows permissions",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/{id}/move",
        command: "one flows move",
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/flows/{id}/replaceDataset",
        command: "one flows replace-dataset",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/package",
        command: "one flows import",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/flows/package/dryRun",
        command: "one flows import-dry-run",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/package",
        command: "one flows export",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/flows/{id}/package/dryRun",
        command: "one flows export-dry-run",
    },
];

const DATASET_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/datasetLibrary",
        command: "one datasets list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/datasetLibrary/count",
        command: "one datasets count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets",
        command: "one datasets wrangled list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets/count",
        command: "one datasets wrangled count",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/wrangledDatasets/{id}",
        command: "one datasets wrangled detail",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/importedDatasets/{id}",
        command: "one datasets imported detail",
    },
];

const JOB_GROUP_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/jobLibrary",
        command: "one job-group list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobLibrary/count",
        command: "one job-group count",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/jobGroups",
        command: "one job-group run",
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/jobGroups/{id}/publish",
        command: "one job-group publish",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}",
        command: "one job-group detail",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/jobGroups/{id}/cancel",
        command: "one job-group cancel",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/status",
        command: "one job-group status",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/inputs",
        command: "one job-group inputs",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/pdfResults",
        command: "one job-group pdf-results",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/outputs",
        command: "one job-group outputs",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/jobs",
        command: "one job-group jobs",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/publications",
        command: "one job-group publications",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/profile",
        command: "one job-group profile",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/jobGroups/{id}/profileResults",
        command: "one job-group profile-results",
    },
];

const OUTPUT_OBJECT_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects",
        command: "one output-object list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/count",
        command: "one output-object count",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/outputObjects",
        command: "one output-object create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/{id}",
        command: "one output-object detail",
    },
    EndpointSpec {
        method: "PUT",
        path: "/v4/outputObjects/{id}",
        command: "one output-object update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/outputObjects/{id}",
        command: "one output-object delete",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/outputObjects/{id}/inputs",
        command: "one output-object inputs",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/outputObjects/{id}/wrangleToPython",
        command: "one output-object wrangle-to-python",
    },
];

const WEBHOOK_FLOW_TASK_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "POST",
        path: "/v4/webhookFlowTasks",
        command: "one webhook-flow-task create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/webhookFlowTasks/{id}",
        command: "one webhook-flow-task detail",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/webhookFlowTasks/{id}",
        command: "one webhook-flow-task delete",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/webhooks/test",
        command: "one webhooks test",
    },
];

const WRITE_SETTING_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings",
        command: "one write-setting list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings/count",
        command: "one write-setting count",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/writeSettings",
        command: "one write-setting create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/writeSettings/{id}",
        command: "one write-setting detail",
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/writeSettings/{id}",
        command: "one write-setting update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/writeSettings/{id}",
        command: "one write-setting delete",
    },
];

const CONNECTION_ENDPOINTS: &[EndpointSpec] = &[
    EndpointSpec {
        method: "GET",
        path: "/v4/connections",
        command: "one connections list",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/count",
        command: "one connections count",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections",
        command: "one connections create",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections/dryRun",
        command: "one connections dry-run",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}",
        command: "one connections detail",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}/status",
        command: "one connections status",
    },
    EndpointSpec {
        method: "PATCH",
        path: "/v4/connections/{id}",
        command: "one connections update",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connections/{id}",
        command: "one connections delete",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}/permissions",
        command: "one connections permissions",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connections/{id}/permissions",
        command: "one connections permissions create",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connections/{id}/permissions/{aid}",
        command: "one connections permissions detail",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connections/{id}/permissions/{aid}",
        command: "one connections permissions delete",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/defaults",
        command: "one connections connector-metadata defaults",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}",
        command: "one connections connector-metadata detail",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/publish/info",
        command: "one connections connector-metadata publish-info",
    },
    EndpointSpec {
        method: "GET",
        path: "/v4/connectorMetadata/{connector}/overrides",
        command: "one connections connector-metadata overrides list",
    },
    EndpointSpec {
        method: "POST",
        path: "/v4/connectorMetadata/{connector}/overrides",
        command: "one connections connector-metadata overrides create",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/v4/connectorMetadata/{connector}/overrides",
        command: "one connections connector-metadata overrides delete",
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
                command: "one platform token",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/apiAccessTokens",
                command: "one platform token create",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/apiAccessTokens/{tokenId}",
                command: "one platform token detail",
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/apiAccessTokens/{tokenId}",
                command: "one platform token delete",
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
                command: "one platform user",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people",
                command: "one platform person list",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people/current",
                command: "one platform person current",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people/count",
                command: "one platform person count",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/people/{id}",
                command: "one platform person detail",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/people",
                command: "one platform person create",
            },
            EndpointSpec {
                method: "PUT",
                path: "/v4/people/{id}",
                command: "one platform person update",
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/people/{id}",
                command: "one platform person patch",
            },
            EndpointSpec {
                method: "DELETE",
                path: "/v4/people/{id}",
                command: "one platform person delete",
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/people/current/updatePassword",
                command: "one platform person update-password",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/passwordresetrequest",
                command: "one platform person password-reset-request",
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
                command: "one platform workspace list",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/{id}/configuration",
                command: "one platform workspace configuration-v4",
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/current/transfer",
                command: "one platform workspace transfer-assets",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/current/configuration",
                command: "one platform workspace current-configuration",
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/current/configuration",
                command: "one platform workspace save-current-configuration",
            },
            EndpointSpec {
                method: "PATCH",
                path: "/v4/workspaces/{id}/configuration",
                command: "one platform workspace save-configuration-v4",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/{id}/configuration-schema",
                command: "one platform workspace configuration-schema",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/workspaces/current/configuration-schema",
                command: "one platform workspace current-configuration-schema",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/current/delete-configuration",
                command: "one platform workspace delete-current-configuration",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/workspaces/{id}/delete-configuration",
                command: "one platform workspace delete-configuration",
            },
        ],
        notes: &[
            "Workspace listing, configuration, transfer, and v4 configuration-by-id endpoints are wired; other workspace families remain documented-only.",
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
            command: "one platform api open-api-spec",
        }],
        notes: &["The OpenAPI spec is now exposed through the CLI."],
    },
    SurfaceSpec {
        name: "plan",
        status: "implemented",
        endpoints: PLAN_ENDPOINTS,
        notes: &[
            "Indexed in the official Alteryx One API help pages; the repo now wires the plan surface.",
        ],
    },
    SurfaceSpec {
        name: "plans",
        status: "implemented",
        endpoints: PLANS_ENDPOINTS,
        notes: &["Managed plans surface."],
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
    SurfaceSpec {
        name: "billing",
        status: "implemented",
        endpoints: BILLING_ENDPOINTS,
        notes: &["Managed billing posture and usage export surface."],
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
                        "command": endpoint.command,
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
                        "command": endpoint.command,
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
                        "command": endpoint.command,
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
                        "command": endpoint.command,
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
                account_email: "test@example.com".to_string(),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: Some("client-123".to_string()),
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: Some("https://example.invalid/token".to_string()),
                access_token: Some("token".to_string()),
                access_token_ref: None,
                refresh_token: Some("refresh".to_string()),
                refresh_token_ref: None,
                workspace_credentials: Default::default(),
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
        assert!(surfaces.iter().any(|surface| surface["name"] == "billing"));
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
