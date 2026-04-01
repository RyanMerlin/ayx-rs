use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use ayx_core::audit::write_audit_artifact;
use ayx_core::envelope::Envelope;
use ayx_core::profile::{Config, MongoMode};
use chrono::Utc;
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MongoQueryTemplate {
    pub name: String,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: Value,
    #[serde(default)]
    pub projection: Option<Value>,
    #[serde(default)]
    pub update: Option<Value>,
    #[serde(default)]
    pub sort: Option<Value>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub kba_refs: Vec<String>,
    #[serde(default = "default_true")]
    pub read_only: bool,
}

#[derive(Clone, Debug)]
pub struct MongoQuerySpec {
    pub database: String,
    pub collection: String,
    pub filter: serde_json::Value,
    pub projection: Option<serde_json::Value>,
    pub update: Option<serde_json::Value>,
    pub sort: Option<serde_json::Value>,
    pub limit: Option<u32>,
    pub template_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MongoQueryPlan {
    pub mongosh: String,
    pub database: String,
    pub collection: String,
    pub filter: serde_json::Value,
    pub projection: Option<serde_json::Value>,
    pub update: Option<serde_json::Value>,
    pub sort: Option<serde_json::Value>,
    pub limit: Option<u32>,
    pub template_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MongoQueryRegistry {
    #[serde(default)]
    queries: Vec<MongoQueryTemplate>,
}

pub fn status_envelope(config: &Config) -> Result<Envelope> {
    let mode = match config.mongo.mode {
        MongoMode::Embedded => "embedded",
        MongoMode::Managed => "managed",
    };

    let detail = resolve_connection_detail(config)?;

    Ok(Envelope::ok_with_data(
        format!(
            "mongo status resolved for profile '{}' in {} mode",
            config.profile_name, mode
        ),
        json!({
            "profile": config.profile_name,
            "mode": mode,
            "detail": detail,
            "databases": {
                "gallery": config.mongo.databases.gallery_name,
                "service": config.mongo.databases.service_name
            }
        }),
    ))
}

pub fn query_envelope(
    config: &Config,
    spec: &MongoQuerySpec,
    print_query: bool,
    apply: bool,
) -> Result<Envelope> {
    let plan = build_query_plan(config, spec)?;
    if print_query {
        return Ok(Envelope::ok_with_data(
            "mongo query plan generated",
            json!({
                "profile": config.profile_name,
                "connection": resolve_connection_detail(config)?,
                "query": {
                    "database": plan.database,
                    "collection": plan.collection,
                    "filter": plan.filter,
                    "projection": plan.projection,
                    "sort": plan.sort,
                    "limit": plan.limit,
                    "template": plan.template_name,
                },
                "mongosh": plan.mongosh,
                "copy_paste": plan.mongosh,
            }),
        ));
    }

    if apply {
        anyhow::bail!("mongo query is read-only; use dedicated mutation workflows for writes");
    }

    let detail = resolve_connection_detail(config)?;
    let execution = execute_query_spec(config, spec)?;
    Ok(Envelope::ok_with_data(
        format!(
            "mongo query executed against {}.{}",
            plan.database, plan.collection
        ),
        json!({
            "profile": config.profile_name,
            "connection": detail,
            "query": {
                "database": plan.database,
                "collection": plan.collection,
                "filter": plan.filter,
                "projection": plan.projection,
                "sort": plan.sort,
                "limit": plan.limit,
                "template": plan.template_name,
            },
            "execution": execution,
        }),
    ))
}

pub fn doctor_envelope(config: &Config) -> Result<Envelope> {
    let queries = mongo_doctor_queries(config)?;
    let mut results = Vec::new();
    for query in &queries {
        let spec = mongo_query_spec_from_template(query)?;
        match execute_query_spec(config, &spec) {
            Ok(value) => results.push(json!({
                "name": spec.template_name,
                "ok": true,
                "result": value,
            })),
            Err(err) => results.push(json!({
                "name": spec.template_name,
                "ok": false,
                "error": err.to_string(),
            })),
        }
    }
    Ok(Envelope::ok_with_data(
        "mongo doctor plan generated",
        json!({
            "profile": config.profile_name,
            "connection": resolve_connection_detail(config)?,
            "queries": queries,
            "results": results,
            "notes": [
                "All queries are read-only and designed for support diagnostics",
                "Use query outputs to validate queue integrity, results integrity, app bindings, and user records",
                "For bulk updates, create a dedicated apply workflow with explicit audit and confirmation gates",
            ],
        }),
    ))
}

pub fn mutate_envelope(
    config: &Config,
    database: Option<&str>,
    collection: Option<&str>,
    filter: Option<&str>,
    update: Option<&str>,
    template: Option<&str>,
    print_query: bool,
    apply: bool,
    accept_mutation_risk: bool,
) -> Result<Envelope> {
    let spec = resolve_mutation_spec(config, database, collection, filter, update, template)?;
    let plan = build_query_plan(config, &spec)?;
    let safety_gate = json!({
        "apply": apply,
        "accept_mutation_risk": accept_mutation_risk,
        "read_only": false,
    });

    if print_query || !apply {
        return Ok(Envelope::ok_with_data(
            "mongo mutation plan generated",
            json!({
                "profile": config.profile_name,
                "connection": resolve_connection_detail(config)?,
                "mutation": {
                    "database": plan.database,
                    "collection": plan.collection,
                    "filter": plan.filter,
                    "update": plan.update,
                    "template": plan.template_name,
                },
                "mongosh": build_mongosh_mutation_eval(config, &spec)?,
                "copy_paste": build_mongosh_mutation_eval(config, &spec)?,
                "safety_gate": safety_gate,
                "notes": [
                    "This command is preview-first and requires explicit confirmation for execution",
                    "Use named mutation templates for repeated bulk updates such as email-domain changes",
                ],
            }),
        ));
    }

    if !accept_mutation_risk {
        anyhow::bail!("mongo mutate requires --accept-mutation-risk when --apply is set");
    }

    anyhow::bail!("mongo mutate execution is not yet enabled; preview only");
}

pub fn resolve_query_spec(
    config: &Config,
    database: Option<&str>,
    collection: Option<&str>,
    filter: Option<&str>,
    projection: Option<&str>,
    sort: Option<&str>,
    limit: Option<u32>,
    template: Option<&str>,
) -> Result<MongoQuerySpec> {
    let mut spec = if let Some(template_name) = template {
        mongo_query_spec_from_name(template_name)?
    } else {
        MongoQuerySpec {
            database: String::new(),
            collection: String::new(),
            filter: json!({}),
            projection: None,
            update: None,
            sort: None,
            limit: None,
            template_name: None,
        }
    };

    if let Some(value) = database {
        spec.database = value.to_string();
    }
    if let Some(value) = collection {
        spec.collection = value.to_string();
    }
    if let Some(value) = filter {
        spec.filter = serde_json::from_str(value)
            .with_context(|| format!("invalid JSON passed to --filter: {value}"))?;
    }
    if let Some(value) = projection {
        spec.projection = Some(
            serde_json::from_str(value)
                .with_context(|| format!("invalid JSON passed to --projection: {value}"))?,
        );
    }
    if let Some(value) = sort {
        spec.sort = Some(
            serde_json::from_str(value)
                .with_context(|| format!("invalid JSON passed to --sort: {value}"))?,
        );
    }
    if let Some(value) = limit {
        spec.limit = Some(value);
    }

    if spec.database.trim().is_empty() || spec.collection.trim().is_empty() {
        anyhow::bail!("mongo query requires either --template or both --database and --collection");
    }

    let _ = config;
    Ok(spec)
}

pub fn inventory_envelope(config: &Config) -> Result<Envelope> {
    let detail = resolve_connection_detail(config)?;
    let dbs = json!([
        config.mongo.databases.gallery_name,
        config.mongo.databases.service_name
    ]);

    Ok(Envelope::ok_with_data(
        "mongo inventory plan generated",
        json!({
            "profile": config.profile_name,
            "connection": detail,
            "databases": dbs,
            "operations": [
                "list collections",
                "collection stats",
                "sample document counts"
            ]
        }),
    ))
}

pub fn backup_envelope(
    config: &Config,
    output_dir: &Path,
    apply: bool,
    audit_dir: &Path,
) -> Result<Envelope> {
    let applied = apply;
    let connection = resolve_connection_detail(config)?;

    let execution = if applied {
        fs::create_dir_all(output_dir).with_context(|| {
            format!(
                "failed to create backup output directory '{}'",
                output_dir.display()
            )
        })?;
        Some(execute_backup(config, output_dir)?)
    } else {
        None
    };

    let audit_payload = json!({
        "command": "mongo backup",
        "timestamp_utc": Utc::now(),
        "profile": config.profile_name,
        "dry_run": !applied,
        "applied": applied,
        "output_dir": output_dir,
        "connection": connection,
        "execution": execution,
        "safety_gate": { "apply": apply },
    });
    let audit_path = write_audit_artifact(audit_dir, "mongo-backup", &audit_payload)?;

    Ok(Envelope::ok_with_data(
        if applied {
            "mongo backup executed"
        } else {
            "dry-run only: pass --apply to execute backup"
        },
        json!({
            "profile": config.profile_name,
            "dry_run": !applied,
            "applied": applied,
            "output_dir": output_dir,
            "execution": execution,
            "audit_artifact": audit_path,
            "safety_gate": { "apply": apply },
        }),
    ))
}

pub fn restore_envelope(
    config: &Config,
    input_path: &Path,
    apply: bool,
    audit_dir: &Path,
) -> Result<Envelope> {
    let applied = apply;

    if !input_path.exists() {
        anyhow::bail!("restore input '{}' does not exist", input_path.display());
    }

    let execution = if applied {
        Some(execute_restore(config, input_path)?)
    } else {
        None
    };

    let audit_payload = json!({
        "command": "mongo restore",
        "timestamp_utc": Utc::now(),
        "profile": config.profile_name,
        "dry_run": !applied,
        "applied": applied,
        "input_path": input_path,
        "execution": execution,
        "safety_gate": { "apply": apply }
    });
    let audit_path = write_audit_artifact(audit_dir, "mongo-restore", &audit_payload)?;

    Ok(Envelope::ok_with_data(
        if applied {
            "mongo restore executed"
        } else {
            "dry-run only: pass --apply to execute restore"
        },
        json!({
            "profile": config.profile_name,
            "dry_run": !applied,
            "applied": applied,
            "input_path": input_path,
            "execution": execution,
            "audit_artifact": audit_path,
            "safety_gate": { "apply": apply },
        }),
    ))
}

fn execute_backup(config: &Config, output_dir: &Path) -> Result<serde_json::Value> {
    match config.mongo.mode {
        MongoMode::Embedded => {
            let service_exe = resolve_alteryx_service_path(config)?;
            let arg = format!("emongodump={}", output_dir.display());
            run_command_capture(service_exe.as_path(), &[arg.as_str()], None)
        }
        MongoMode::Managed => {
            ensure_tool_available("mongodump")?;

            let managed = config
                .mongo
                .managed
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("mongo.managed config missing"))?;

            let mut runs = Vec::new();
            for db in [
                &config.mongo.databases.gallery_name,
                &config.mongo.databases.service_name,
            ] {
                let db_out = output_dir.join(db);
                fs::create_dir_all(&db_out)?;

                let mut args: Vec<String> = Vec::new();
                if let Some(url) = managed.url.as_ref() {
                    args.push("--uri".to_string());
                    args.push(url.to_string());
                } else {
                    if let Some(host) = managed.host.as_ref() {
                        args.push("--host".to_string());
                        args.push(host.to_string());
                    }
                    args.push("--port".to_string());
                    args.push(managed.port.to_string());
                    if let Some(username) = managed.username.as_ref() {
                        args.push("--username".to_string());
                        args.push(username.to_string());
                    }
                    if let Some(password) = managed.password.as_ref() {
                        args.push("--password".to_string());
                        args.push(password.to_string());
                    }
                    if let Some(auth_db) = managed.auth_database.as_ref() {
                        args.push("--authenticationDatabase".to_string());
                        args.push(auth_db.to_string());
                    }
                }

                args.push("--db".to_string());
                args.push(db.to_string());
                args.push("--out".to_string());
                args.push(db_out.display().to_string());

                if managed.tls.enabled {
                    args.push("--tls".to_string());
                    if let Some(ca) = managed.tls.ca_path.as_ref() {
                        args.push("--tlsCAFile".to_string());
                        args.push(ca.to_string());
                    }
                    if managed.tls.cert_path.is_some() || managed.tls.key_path.is_some() {
                        let cert_key = tls_cert_key_file_arg(&managed.tls)?;
                        args.push("--tlsCertificateKeyFile".to_string());
                        args.push(cert_key);
                    }
                    if managed.tls.allow_invalid_hostnames.unwrap_or(false) {
                        args.push("--tlsAllowInvalidHostnames".to_string());
                    }
                }

                let arg_refs: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
                runs.push(run_command_capture(
                    Path::new("mongodump"),
                    &arg_refs,
                    None,
                )?);
            }

            Ok(json!({ "mode": "managed", "runs": runs }))
        }
    }
}

fn execute_query_spec(config: &Config, spec: &MongoQuerySpec) -> Result<serde_json::Value> {
    let plan = build_query_plan(config, spec)?;
    execute_query_plan(&plan)
}

fn build_query_plan(config: &Config, spec: &MongoQuerySpec) -> Result<MongoQueryPlan> {
    Ok(MongoQueryPlan {
        mongosh: build_mongosh_eval(config, spec)?,
        database: spec.database.clone(),
        collection: spec.collection.clone(),
        filter: spec.filter.clone(),
        projection: spec.projection.clone(),
        update: spec.update.clone(),
        sort: spec.sort.clone(),
        limit: spec.limit,
        template_name: spec.template_name.clone(),
    })
}

fn build_mongosh_eval(config: &Config, spec: &MongoQuerySpec) -> Result<String> {
    let database = &spec.database;
    let collection = &spec.collection;
    let filter = serde_json::to_string(&spec.filter)?;
    let projection = match &spec.projection {
        Some(v) => format!(", {}", serde_json::to_string(v)?),
        None => String::new(),
    };
    let sort = spec.sort.as_ref().map(serde_json::to_string).transpose()?;
    let limit = spec.limit.unwrap_or(25);
    let sort_js = sort
        .as_ref()
        .map(|s| format!(".sort({s})"))
        .unwrap_or_default();
    let mut js = String::new();
    js.push_str("const dbName = ");
    js.push_str(&serde_json::to_string(database)?);
    js.push_str("; const collName = ");
    js.push_str(&serde_json::to_string(collection)?);
    js.push_str("; const filter = ");
    js.push_str(&filter);
    js.push_str("; const result = db.getSiblingDB(dbName).getCollection(collName).find(filter");
    js.push_str(&projection);
    js.push_str(&sort_js);
    js.push_str(&format!(
        ".limit({limit}).toArray(); print(JSON.stringify(result));"
    ));
    let mut args: Vec<String> = vec!["--quiet".to_string(), "--eval".to_string(), js];
    attach_connection_args(config, &mut args)?;
    let cmd = if cfg!(target_os = "windows") {
        "mongosh.exe"
    } else {
        "mongosh"
    };
    let quoted = args
        .into_iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a
            }
        })
        .collect::<Vec<String>>()
        .join(" ");
    Ok(format!("{cmd} {quoted}"))
}

fn attach_connection_args(config: &Config, args: &mut Vec<String>) -> Result<()> {
    let managed = config.mongo.managed.as_ref();
    if let Some(url) = managed.and_then(|m| m.url.as_ref()) {
        args.push("--uri".to_string());
        args.push(url.to_string());
    } else if let Some(m) = managed {
        if let Some(host) = m.host.as_ref() {
            args.push("--host".to_string());
            args.push(host.to_string());
        }
        args.push("--port".to_string());
        args.push(m.port.to_string());
        if let Some(username) = m.username.as_ref() {
            args.push("--username".to_string());
            args.push(username.to_string());
        }
        if let Some(password) = m.password.as_ref() {
            args.push("--password".to_string());
            args.push(password.to_string());
        }
        if let Some(auth_db) = m.auth_database.as_ref() {
            args.push("--authenticationDatabase".to_string());
            args.push(auth_db.to_string());
        }
        if m.tls.enabled {
            args.push("--tls".to_string());
            if let Some(ca) = m.tls.ca_path.as_ref() {
                args.push("--tlsCAFile".to_string());
                args.push(ca.to_string());
            }
            if m.tls.cert_path.is_some() || m.tls.key_path.is_some() {
                args.push("--tlsCertificateKeyFile".to_string());
                args.push(tls_cert_key_file_arg(&m.tls)?);
            }
            if m.tls.allow_invalid_hostnames.unwrap_or(false) {
                args.push("--tlsAllowInvalidHostnames".to_string());
            }
        }
    }
    Ok(())
}

fn execute_query_plan(plan: &MongoQueryPlan) -> Result<serde_json::Value> {
    let args: Vec<String> = vec![
        "--quiet".to_string(),
        "--eval".to_string(),
        plan.mongosh.clone(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
    let mongo_cmd = if cfg!(target_os = "windows") {
        "mongosh.exe"
    } else {
        "mongosh"
    };
    ensure_tool_available(mongo_cmd)?;
    let execution = run_command_capture(Path::new(mongo_cmd), &arg_refs, None)?;
    let parsed = execution
        .get("stdout")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or(serde_json::Value::Null);
    Ok(json!({
        "command": mongo_cmd,
        "execution": execution,
        "parsed": parsed,
    }))
}

fn mongo_doctor_queries(config: &Config) -> Result<Vec<MongoQueryTemplate>> {
    let templates = mongo_query_templates()?;
    let mut queries = Vec::new();
    for name in [
        "queue_health",
        "results_health",
        "gallery_users",
        "appinfos",
    ] {
        let template = templates
            .iter()
            .find(|q| q.name == name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing mongo query template: {name}"))?;
        let mut spec = mongo_query_spec_from_template(&template)?;
        spec.database = match name {
            "queue_health" | "results_health" => config.mongo.databases.service_name.clone(),
            _ => config.mongo.databases.gallery_name.clone(),
        };
        let mut adjusted = template.clone();
        adjusted.database = spec.database;
        queries.push(adjusted);
    }
    Ok(queries)
}

fn mongo_query_spec_from_name(name: &str) -> Result<MongoQuerySpec> {
    let template = mongo_query_templates()?
        .into_iter()
        .find(|query| query.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown mongo query template '{}'", name))?;
    mongo_query_spec_from_template(&template)
}

fn resolve_mutation_spec(
    config: &Config,
    database: Option<&str>,
    collection: Option<&str>,
    filter: Option<&str>,
    update: Option<&str>,
    template: Option<&str>,
) -> Result<MongoQuerySpec> {
    let mut spec = resolve_query_spec(
        config, database, collection, filter, None, None, None, template,
    )?;
    if let Some(value) = update {
        spec.update = Some(
            serde_json::from_str(value)
                .with_context(|| format!("invalid JSON passed to --update: {value}"))?,
        );
    }
    Ok(spec)
}

fn build_mongosh_mutation_eval(config: &Config, spec: &MongoQuerySpec) -> Result<String> {
    let database = &spec.database;
    let collection = &spec.collection;
    let filter = serde_json::to_string(&spec.filter)?;
    let update = serde_json::to_string(spec.update.as_ref().unwrap_or(&json!({})))?;
    let mut js = String::new();
    js.push_str("const dbName = ");
    js.push_str(&serde_json::to_string(database)?);
    js.push_str("; const collName = ");
    js.push_str(&serde_json::to_string(collection)?);
    js.push_str("; const filter = ");
    js.push_str(&filter);
    js.push_str("; const update = ");
    js.push_str(&update);
    js.push_str("; const result = db.getSiblingDB(dbName).getCollection(collName).updateMany(filter, update); print(JSON.stringify(result));");
    let mut args: Vec<String> = vec!["--quiet".to_string(), "--eval".to_string(), js];
    attach_connection_args(config, &mut args)?;
    let cmd = if cfg!(target_os = "windows") {
        "mongosh.exe"
    } else {
        "mongosh"
    };
    let quoted = args
        .into_iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a
            }
        })
        .collect::<Vec<String>>()
        .join(" ");
    Ok(format!("{cmd} {quoted}"))
}

fn mongo_query_spec_from_template(template: &MongoQueryTemplate) -> Result<MongoQuerySpec> {
    Ok(MongoQuerySpec {
        database: template.database.clone(),
        collection: template.collection.clone(),
        filter: template.filter.clone(),
        projection: template.projection.clone(),
        update: template.update.clone(),
        sort: template.sort.clone(),
        limit: template.limit,
        template_name: Some(template.name.clone()),
    })
}

fn mongo_query_templates() -> Result<Vec<MongoQueryTemplate>> {
    let registry: MongoQueryRegistry =
        serde_yaml::from_str(include_str!("../knowledge/mongo/queries.yaml"))?;
    Ok(registry.queries)
}

fn default_true() -> bool {
    true
}

fn execute_restore(config: &Config, input_path: &Path) -> Result<serde_json::Value> {
    match config.mongo.mode {
        MongoMode::Embedded => {
            let service_exe = resolve_alteryx_service_path(config)?;
            let target_path = resolve_embedded_restore_target_path(config)?;
            let arg = format!(
                "emongorestore={},{}",
                input_path.display(),
                target_path.display()
            );
            run_command_capture(service_exe.as_path(), &[arg.as_str()], None)
        }
        MongoMode::Managed => {
            ensure_tool_available("mongorestore")?;

            let managed = config
                .mongo
                .managed
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("mongo.managed config missing"))?;

            let mut args: Vec<String> = Vec::new();
            if let Some(url) = managed.url.as_ref() {
                args.push("--uri".to_string());
                args.push(url.to_string());
            } else {
                if let Some(host) = managed.host.as_ref() {
                    args.push("--host".to_string());
                    args.push(host.to_string());
                }
                args.push("--port".to_string());
                args.push(managed.port.to_string());
                if let Some(username) = managed.username.as_ref() {
                    args.push("--username".to_string());
                    args.push(username.to_string());
                }
                if let Some(password) = managed.password.as_ref() {
                    args.push("--password".to_string());
                    args.push(password.to_string());
                }
                if let Some(auth_db) = managed.auth_database.as_ref() {
                    args.push("--authenticationDatabase".to_string());
                    args.push(auth_db.to_string());
                }
            }

            args.push("--drop".to_string());
            args.push(input_path.display().to_string());

            if managed.tls.enabled {
                args.push("--tls".to_string());
                if let Some(ca) = managed.tls.ca_path.as_ref() {
                    args.push("--tlsCAFile".to_string());
                    args.push(ca.to_string());
                }
                if managed.tls.cert_path.is_some() || managed.tls.key_path.is_some() {
                    let cert_key = tls_cert_key_file_arg(&managed.tls)?;
                    args.push("--tlsCertificateKeyFile".to_string());
                    args.push(cert_key);
                }
                if managed.tls.allow_invalid_hostnames.unwrap_or(false) {
                    args.push("--tlsAllowInvalidHostnames".to_string());
                }
            }

            let arg_refs: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
            run_command_capture(Path::new("mongorestore"), &arg_refs, None)
        }
    }
}

fn tls_cert_key_file_arg(tls: &ayx_core::profile::TlsConfig) -> Result<String> {
    match (&tls.cert_path, &tls.key_path) {
        (Some(cert), Some(_key)) => Ok(cert.clone()),
        (Some(cert), None) => Ok(cert.clone()),
        (None, Some(key)) => Ok(key.clone()),
        (None, None) => {
            anyhow::bail!("tls cert/key requested but both cert_path and key_path are empty")
        }
    }
}

fn ensure_tool_available(tool: &str) -> Result<()> {
    let check = if cfg!(target_os = "windows") {
        Command::new("where").arg(tool).output()
    } else {
        Command::new("which").arg(tool).output()
    }
    .with_context(|| format!("failed to check tool '{}' availability", tool))?;

    if check.status.success() {
        Ok(())
    } else {
        anyhow::bail!("required tool '{}' not found on PATH", tool)
    }
}

fn run_command_capture(
    binary: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<serde_json::Value> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let output = cmd
        .output()
        .with_context(|| format!("failed to execute '{}'", binary.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let sanitized_args = sanitize_args(args);

    if !output.status.success() {
        anyhow::bail!(
            "command failed: binary={} status={:?} args={:?} stderr={}",
            binary.display(),
            output.status.code(),
            sanitized_args,
            stderr
        );
    }

    Ok(json!({
        "binary": binary.display().to_string(),
        "args": sanitized_args,
        "status": output.status.code(),
        "success": output.status.success(),
        "stdout": stdout,
        "stderr": stderr,
    }))
}

fn sanitize_args(args: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut mask_next = false;

    for a in args {
        if mask_next {
            out.push("***".to_string());
            mask_next = false;
            continue;
        }

        let lower = a.to_ascii_lowercase();
        if lower == "--password" {
            out.push(a.to_string());
            mask_next = true;
            continue;
        }

        if lower == "--uri" {
            out.push(a.to_string());
            mask_next = true;
            continue;
        }

        out.push(a.to_string());
    }

    // Redact URI value if present in masked slot.
    for i in 0..out.len() {
        if out[i].eq_ignore_ascii_case("--uri") && i + 1 < out.len() {
            out[i + 1] = redact_mongo_uri(&out[i + 1]);
        }
    }

    out
}

fn resolve_connection_detail(config: &Config) -> Result<serde_json::Value> {
    let detail = match config.mongo.mode {
        MongoMode::Embedded => {
            let runtime_settings_path = resolve_runtime_settings_path(config)?;
            let discovered = discover_from_runtime_settings(&runtime_settings_path)?;
            let mongo_path = extract_mongo_path_from_runtime_settings_file(&runtime_settings_path)?;
            json!({
                "runtime_settings_path": runtime_settings_path.display().to_string(),
                "runtime_path_mongo_path": mongo_path.map(|p| p.display().to_string()),
                "discovery": discovered,
            })
        }
        MongoMode::Managed => {
            let managed = config.mongo.managed.as_ref();
            json!({
                "url": managed.and_then(|m| m.url.as_ref().map(|u| redact_mongo_uri(u))),
                "host": managed.and_then(|m| m.host.clone()),
                "port": managed.map(|m| m.port),
                "auth_database": managed.and_then(|m| m.auth_database.clone()),
                "username": managed.and_then(|m| m.username.clone()),
                "tls": managed.map(|m| json!({
                    "enabled": m.tls.enabled,
                    "ca_path": m.tls.ca_path,
                    "cert_path": m.tls.cert_path,
                    "key_path": m.tls.key_path,
                    "allow_invalid_hostnames": m.tls.allow_invalid_hostnames
                })),
                "timeout_ms": managed.and_then(|m| m.timeout_ms),
                "retry_count": managed.and_then(|m| m.retry_count),
                "max_pool_size": managed.and_then(|m| m.max_pool_size),
            })
        }
    };

    Ok(detail)
}

fn resolve_runtime_settings_path(config: &Config) -> Result<PathBuf> {
    let configured = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.runtime_settings_path.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    if let Some(path) = configured {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        anyhow::bail!(
            "configured runtime_settings_path '{}' does not exist",
            pb.display()
        );
    }

    discover_runtime_settings_path().ok_or_else(|| {
        anyhow::anyhow!(
            "could not auto-discover RuntimeSettings.xml; set mongo.embedded.runtime_settings_path in config.yaml"
        )
    })
}

fn resolve_alteryx_service_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.alteryx_service_path.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        anyhow::bail!(
            "configured alteryx_service_path '{}' does not exist",
            pb.display()
        );
    }

    let service_candidates = resolve_service_candidates_from_runtime_settings(config)?;
    if let Some(path) = service_candidates.into_iter().find(|p| p.exists()) {
        return Ok(path);
    }

    discover_alteryx_service_path().ok_or_else(|| {
        anyhow::anyhow!(
            "could not auto-discover AlteryxService.exe; set mongo.embedded.alteryx_service_path in config.yaml"
        )
    })
}

fn resolve_embedded_restore_target_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.restore_target_path.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok(PathBuf::from(path));
    }

    let runtime_settings = resolve_runtime_settings_path(config)?;
    if let Some(p) = extract_mongo_path_from_runtime_settings_file(&runtime_settings)? {
        return Ok(p);
    }

    Ok(PathBuf::from(
        r"C:\ProgramData\Alteryx\Service\Persistence\MongoDB",
    ))
}

fn extract_mongo_path_from_runtime_settings_file(path: &Path) -> Result<Option<PathBuf>> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("failed to read RuntimeSettings.xml at '{}'", path.display()))?;
    let doc = Document::parse(&xml).context("failed to parse RuntimeSettings.xml")?;
    Ok(extract_mongo_path_from_runtime_settings_doc(&doc).map(PathBuf::from))
}

fn extract_mongo_path_from_runtime_settings_doc(doc: &Document<'_>) -> Option<String> {
    if let Some(v) = first_text(
        doc,
        &[
            "EmbeddedMongoDBRootPath",
            "MongoPath",
            "MongoDataPath",
            "PersistencePath",
        ],
    ) {
        return Some(v);
    }

    for node in doc.descendants().filter(|n| n.is_element()) {
        let name_attr = node.attribute("name").or_else(|| node.attribute("Name"));
        let is_mongo_path = name_attr.is_some_and(|v| {
            let lower = v.to_ascii_lowercase();
            lower == "mongopath" || lower == "mongodbpath" || lower == "persistencypath"
        });
        if !is_mongo_path {
            continue;
        }

        if let Some(value_attr) = node.attribute("value").or_else(|| node.attribute("Value")) {
            let trimmed = value_attr.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(text) = node.text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

fn discover_runtime_settings_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(PathBuf::from(r"C:\ProgramData\Alteryx\RuntimeSettings.xml"));

    if let Ok(program_data) = std::env::var("ProgramData") {
        candidates.push(PathBuf::from(&program_data).join("Alteryx/RuntimeSettings.xml"));
        candidates.push(PathBuf::from(&program_data).join("Alteryx/Engine/RuntimeSettings.xml"));
        candidates.push(PathBuf::from(&program_data).join("Alteryx/Server/RuntimeSettings.xml"));
    }

    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(&program_files).join("Alteryx/RuntimeSettings.xml"));
        candidates.push(PathBuf::from(&program_files).join("Alteryx/Engine/RuntimeSettings.xml"));
        candidates.push(PathBuf::from(&program_files).join("Alteryx/Server/RuntimeSettings.xml"));
    }

    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(&program_files_x86).join("Alteryx/RuntimeSettings.xml"));
        candidates
            .push(PathBuf::from(&program_files_x86).join("Alteryx/Engine/RuntimeSettings.xml"));
        candidates
            .push(PathBuf::from(&program_files_x86).join("Alteryx/Server/RuntimeSettings.xml"));
    }

    for letter in 'C'..='Z' {
        let root = PathBuf::from(format!("{}:\\", letter));
        if !root.exists() {
            continue;
        }
        candidates.push(root.join("ProgramData/Alteryx/RuntimeSettings.xml"));
        candidates.push(root.join("Alteryx/RuntimeSettings.xml"));
        candidates.push(root.join("AlteryxData/RuntimeSettings.xml"));
    }

    candidates.into_iter().find(|p| p.exists())
}

fn discover_alteryx_service_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(&program_files).join("Alteryx/bin/AlteryxService.exe"));
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(&program_files_x86).join("Alteryx/bin/AlteryxService.exe"));
    }

    for letter in 'C'..='Z' {
        let root = PathBuf::from(format!("{}:\\", letter));
        if !root.exists() {
            continue;
        }
        candidates.push(root.join("Program Files/Alteryx/bin/AlteryxService.exe"));
        candidates.push(root.join("Alteryx/bin/AlteryxService.exe"));
    }

    candidates.into_iter().find(|p| p.exists())
}

fn resolve_service_candidates_from_runtime_settings(config: &Config) -> Result<Vec<PathBuf>> {
    let runtime_settings = resolve_runtime_settings_path(config)?;
    extract_install_path_candidates(&runtime_settings)
}

fn extract_install_path_candidates(runtime_settings: &Path) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    let xml = fs::read_to_string(runtime_settings).with_context(|| {
        format!(
            "failed to read RuntimeSettings.xml at '{}'",
            runtime_settings.display()
        )
    })?;
    let doc = Document::parse(&xml).context("failed to parse RuntimeSettings.xml")?;

    let keys = [
        "LoggingPath",
        "WorkingPath",
        "WebInterfaceStagingPath",
        "SQLitePath",
    ];

    for key in keys {
        if let Some(value) = first_text(&doc, &[key]) {
            let pb = PathBuf::from(value.trim());
            if let Some(parent) = pb.parent() {
                candidates.push(parent.join("AlteryxService.exe"));
                if let Some(grand) = parent.parent() {
                    candidates.push(grand.join("bin/AlteryxService.exe"));
                }
            }
            candidates.push(pb.join("bin/AlteryxService.exe"));
        }
    }

    Ok(candidates)
}

fn discover_from_runtime_settings(path: &Path) -> Result<serde_json::Value> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("failed to read RuntimeSettings.xml at '{}'", path.display()))?;
    let doc = Document::parse(&xml).context("failed to parse RuntimeSettings.xml")?;

    extract_runtime_settings(&doc)
}

fn extract_runtime_settings(doc: &Document<'_>) -> Result<serde_json::Value> {
    let connection_string = first_text(
        doc,
        &[
            "MongoConnectionString",
            "MongoDBConnectionString",
            "MongoDbConnectionString",
        ],
    );
    let host = first_text(doc, &["MongoHost", "MongoDBHost", "MongoDbHost"]);
    let port = first_text(doc, &["MongoPort", "MongoDBPort", "MongoDbPort"]);
    let user = first_text(doc, &["MongoUser", "MongoDBUser", "MongoDbUser"]);
    let auth_db = first_text(
        doc,
        &[
            "MongoAuthDatabase",
            "MongoDBAuthDatabase",
            "MongoDbAuthDatabase",
        ],
    );
    let gallery_db = first_text(
        doc,
        &[
            "MongoGalleryDatabase",
            "AlteryxGalleryDatabase",
            "GalleryMongoDatabase",
        ],
    )
    .unwrap_or_else(|| "AlteryxGallery".to_string());
    let service_db = first_text(
        doc,
        &[
            "MongoServiceDatabase",
            "AlteryxServiceDatabase",
            "ServiceMongoDatabase",
        ],
    )
    .unwrap_or_else(|| "AlteryxService".to_string());

    Ok(json!({
        "connection_string": connection_string,
        "host": host,
        "port": port,
        "username": user,
        "auth_database": auth_db,
        "databases": {
            "gallery": gallery_db,
            "service": service_db
        }
    }))
}

fn first_text(doc: &Document<'_>, names: &[&str]) -> Option<String> {
    doc.descendants()
        .find(|n| n.is_element() && names.contains(&n.tag_name().name()))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn redact_mongo_uri(uri: &str) -> String {
    if let Some(scheme_end) = uri.find("://") {
        let after_scheme = &uri[(scheme_end + 3)..];
        if let Some(at_pos) = after_scheme.find('@') {
            let host_part = &after_scheme[(at_pos + 1)..];
            return format!("{}://***:***@{}", &uri[..scheme_end], host_part);
        }
    }
    uri.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_mongo_uri_credentials() {
        let input = "mongodb://user:secret@localhost:27017/admin";
        let redacted = redact_mongo_uri(input);
        assert_eq!(redacted, "mongodb://***:***@localhost:27017/admin");
    }
}
