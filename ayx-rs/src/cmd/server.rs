//! Dispatch for `ayx server ...`.
//!
//! Wraps the server-api transport, log readers, upgrade engine, system
//! info / runtime settings / backups, auth diagnose+simulate, and the
//! per-doctor surfaces. Mostly load-then-helper; the upgrade arms drive
//! the rules engine in `ayx_server::upgrade`.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};
use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::envelope::Envelope;
use ayx_server::logs::{
    discover_log_inventory, extract_context, parse_gallery_csv, parse_gallery_events,
    parse_service_events, recent_log_candidates, summarize_log_file, tail_log_file,
};
use ayx_server::upgrade::{
    compute_path, run_apply, run_backup, run_bundle, run_plan, run_postcheck, run_precheck,
};
use ayx_server::util::{
    ayx_paths, backup_plan, capture_system_info, run_server_backup, runtime_settings_summary,
    write_runtime_settings_json,
};
use ayx_server::{call_operation, diagnose_api, import_swagger};
use serde_json::json;

use anyhow::Context;

use crate::{
    build_auth_status, load_payload, load_profile_with_env, parse_key_value_params,
    parse_saml_metadata_source, server_profile, ServerApiCommand, ServerAuthCommand,
    ServerAuthDiagnoseCommand, ServerAuthSimulateCommand, ServerCommand, ServerDiagnoseCommand,
    ServerDoctorCommand, ServerLogsCommand, UpgradeCommand,
};

#[allow(clippy::too_many_lines)]
pub fn execute(environment: Option<&str>, command: Option<ServerCommand>) -> Result<Envelope> {
    fn load_profile<'a, P>(p: P, environment: Option<&str>) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        load_profile_with_env(p, environment)
    }
    Ok(match command {
            None => Envelope::ok("server commands: api, system-info, runtime-settings, ayx-paths, server-logs, backup-plan, backup"),
            Some(ServerCommand::Api { command }) => match command {
                ServerApiCommand::Status { profile } => {
                    let config = load_profile(profile.as_deref(), environment)?;
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    let server = server_profile(&config)?;
                    diagnose_api(server, config.observability.as_ref())?
                }
                ServerApiCommand::ImportSwagger {
                    profile,
                    version,
                    url,
                    cache_dir,
                } => {
                    let config = load_profile(profile.as_deref(), environment)?;
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
                    let config = load_profile(profile.as_deref(), environment)?;
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    Envelope::ok_with_data(
                        "log sources discovered",
                        discover_log_inventory(&config),
                    )
                }
                ServerLogsCommand::Inventory { profile } => {
                    let config = load_profile(profile.as_deref(), environment)?;
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    Envelope::ok_with_data(
                        "recent log candidates discovered",
                        recent_log_candidates(&config, days),
                    )
                }
            },
            Some(ServerCommand::Diagnose { command }) => match command {
                ServerDiagnoseCommand::Startup { profile, error, log_file } => {
                    let config = load_profile(profile.as_deref(), environment)?;
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
                            "profile": config.profile_name.clone(),
                            "steps": steps,
                        }),
                    )
                }
                ServerDiagnoseCommand::Logs { profile } => {
                    let config = load_profile(profile.as_deref(), environment)?;
                    let logs = discover_log_inventory(&config);
                    Envelope::ok_with_data(
                        "server log diagnosis generated",
                        json!({
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    let paths = ayx_paths();
                    let detail = json!({
                        "profile": config.profile_name.clone(),
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
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    let detail = json!({
                        "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
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
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    Envelope::ok_with_data(
                        "server auth status generated",
                        json!({
                            "profile": config.profile_name.clone(),
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
                        let config = load_profile(profile.as_deref(), environment)?;
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
                                "profile": config.profile_name.clone(),
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
                        let config = load_profile(profile.as_deref(), environment)?;
                        let logs = recent_log_candidates(&config, days);
                        let detail = json!({
                            "profile": config.profile_name.clone(),
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
                        let config = load_profile(profile.as_deref(), environment)?;
                        let cert_path = certificate_file.as_ref().map(|p| p.display().to_string());
                        let detail = json!({
                            "profile": config.profile_name.clone(),
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
                        let config = load_profile(profile.as_deref(), environment)?;
                        let detail = json!({
                            "profile": config.profile_name.clone(),
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
                        let config = load_profile(profile.as_deref(), environment)?;
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
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
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
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    Envelope::ok_with_data(
                        "server log doctor workflow generated",
                        json!({
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    Envelope::ok_with_data(
                        "server network doctor workflow generated",
                        json!({
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
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
                            "profile": config.profile_name.clone(),
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
                    let config = load_profile(profile.as_deref(), environment)?;
                    let detail = run_precheck(&config, &target, &out, &deployment)?;
                    Envelope::ok_with_data("upgrade precheck completed", detail)
                }
                UpgradeCommand::Backup {
                    profile,
                    r#type,
                    out,
                } => {
                    let config = load_profile(profile.as_deref(), environment)?;
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
                    let config = load_profile(profile.as_deref(), environment)?;
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
                let config = load_profile(profile.as_deref(), environment)?;
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
    })
}
