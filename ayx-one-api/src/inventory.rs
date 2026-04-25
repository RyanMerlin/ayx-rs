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
        path: "/iam/v1/workspaces/current",
        command: "one platform workspace current",
    },
    EndpointSpec {
        method: "GET",
        path: "/iam/v1/workspaces/{id}/configuration",
        command: "one platform workspace configuration",
    },
    EndpointSpec {
        method: "GET",
        path: "/iam/v1/workspaces/{id}/people",
        command: "one platform workspace people",
    },
    EndpointSpec {
        method: "GET",
        path: "/iam/v1/workspaces/{workspaceId}/admins",
        command: "one platform workspace admins",
    },
    EndpointSpec {
        method: "POST",
        path: "/iam/v1/workspaces/{id}/people/batch",
        command: "one platform workspace invite-users",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/iam/v1/workspaces/{id}/people/{personId}",
        command: "one platform workspace remove-user",
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
        path: "/iam/v1/workspaces/{id}/transfer",
        command: "one platform workspace transfer",
    },
    EndpointSpec {
        method: "GET",
        path: "/iam/v1/authorization/roles/{id}/people",
        command: "one platform role list-assignments",
    },
    EndpointSpec {
        method: "POST",
        path: "/iam/v1/authorization/roles/{id}/people/{subjectId}",
        command: "one platform role assign",
    },
    EndpointSpec {
        method: "DELETE",
        path: "/iam/v1/authorization/roles/{id}/people/{subjectId}",
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

const DOCUMENTED_ONLY_SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec {
        name: "plan",
        status: "documented",
        endpoints: &[
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
        ],
        notes: &["Indexed in the official Alteryx One API help pages; the repo currently wires the managed plans subset."],
    },
    SurfaceSpec {
        name: "flow",
        status: "documented",
        endpoints: &[
            EndpointSpec {
                method: "POST",
                path: "/v4/flows/package",
                command: "one flows import",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/flows",
                command: "one flows list",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/flows/{id}/package",
                command: "one flows export",
            },
        ],
        notes: &["Indexed in the official Alteryx One API help pages; not wired in this CLI yet."],
    },
    SurfaceSpec {
        name: "jobGroup",
        status: "documented",
        endpoints: &[
            EndpointSpec {
                method: "POST",
                path: "/v4/jobGroups",
                command: "one job-group run",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/jobGroups/{id}",
                command: "one job-group detail",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/jobGroups/{id}/jobs",
                command: "one job-group jobs",
            },
        ],
        notes: &["Indexed in the official Alteryx One API help pages; not wired in this CLI yet."],
    },
    SurfaceSpec {
        name: "misc",
        status: "documented",
        endpoints: &[EndpointSpec {
            method: "GET",
            path: "/v4/open-api-spec",
            command: "one api open-api-spec",
        }],
        notes: &["The platform exposes an OpenAPI document through the product UI/docs."],
    },
    SurfaceSpec {
        name: "outputObject",
        status: "documented",
        endpoints: &[
            EndpointSpec {
                method: "GET",
                path: "/v4/outputObjects/count",
                command: "one output-object count",
            },
            EndpointSpec {
                method: "GET",
                path: "/v4/outputObjects/{id}",
                command: "one output-object detail",
            },
            EndpointSpec {
                method: "PATCH",
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
        ],
        notes: &["Indexed in the official Alteryx One API help pages; not wired in this CLI yet."],
    },
    SurfaceSpec {
        name: "webhookFlowTask",
        status: "documented",
        endpoints: &[
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
        ],
        notes: &["Indexed in the official Alteryx One API help pages; not wired in this CLI yet."],
    },
    SurfaceSpec {
        name: "writeSetting",
        status: "documented",
        endpoints: &[EndpointSpec {
            method: "DELETE",
            path: "/v4/writeSettings/{id}",
            command: "one write-setting delete",
        }],
        notes: &["Indexed in the official Alteryx One API help pages; not wired in this CLI yet."],
    },
];

const PARTIAL_SURFACES: &[SurfaceSpec] = &[
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
        notes: &["One API access-token CRUD is wired; additional token administration endpoints remain documented-only."],
    },
    SurfaceSpec {
        name: "person",
        status: "partial",
        endpoints: &[EndpointSpec {
            method: "GET",
            path: "/v4/people/current",
            command: "one platform user",
        }],
        notes: &["Current-person lookup is wired; broader person lifecycle endpoints remain documented-only."],
    },
    SurfaceSpec {
        name: "person",
        status: "partial",
        endpoints: &[
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
                path: "/v4/people/{id}",
                command: "one platform person detail",
            },
            EndpointSpec {
                method: "POST",
                path: "/v4/people",
                command: "one platform person create",
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
        notes: &["Person list/current/detail/create/password workflows are wired; remaining person families stay documented-only."],
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
        notes: &["Workspace listing, configuration, and transfer endpoints are wired; other workspace families remain documented-only."],
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
        name: "plans",
        status: "implemented",
        endpoints: PLANS_ENDPOINTS,
        notes: &["Managed plans surface."],
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
                oauth_client_id: Some("client-123".to_string()),
                token_endpoint_url: Some("https://example.invalid/token".to_string()),
                access_token: Some("token".to_string()),
                refresh_token: Some("refresh".to_string()),
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
        assert!(surfaces
            .iter()
            .any(|surface| surface["name"] == "platform.iam"));
        assert!(surfaces.iter().any(|surface| surface["name"] == "plans"));
        assert!(surfaces
            .iter()
            .any(|surface| surface["name"] == "scheduling"));
        assert!(surfaces.iter().any(|surface| surface["name"] == "billing"));
        let partial = env.data["partial_surfaces"]
            .as_array()
            .expect("partial_surfaces");
        assert!(partial
            .iter()
            .any(|surface| surface["name"] == "apiAccessTokens"));
        assert!(partial.iter().any(|surface| surface["name"] == "person"));
        assert!(partial.iter().any(|surface| surface["name"] == "workspace"));
        let documented = env.data["documented_only_surfaces"]
            .as_array()
            .expect("documented_only_surfaces");
        assert!(documented.iter().any(|surface| surface["name"] == "misc"));
        let deferred = env.data["deferred_surfaces"]
            .as_array()
            .expect("deferred_surfaces");
        assert!(deferred.is_empty());
    }
}
