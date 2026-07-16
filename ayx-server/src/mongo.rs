use std::collections::{BTreeMap, BTreeSet};
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
use serde_json::{Value, json};

use crate::upgrade::manifest::compute_sha256;

/// A read-only support/diagnostic query template (`knowledge/mongo/queries.yaml`).
///
/// Deliberately has no `update` field and no `read_only` flag: this type
/// cannot express a write, so no code path that only knows about
/// `MongoSupportQueryTemplate` can accidentally treat a support template as
/// a remediation. Writable templates live in `MongoMutationTemplate`
/// (`knowledge/mongo/mutations.yaml`) instead.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MongoSupportQueryTemplate {
    pub name: String,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: Value,
    #[serde(default)]
    pub projection: Option<Value>,
    #[serde(default)]
    pub sort: Option<Value>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub kba_refs: Vec<String>,
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
    queries: Vec<MongoSupportQueryTemplate>,
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
                let mut _password_file: Option<MongoPasswordFile> = None;
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
                        let pwfile = write_password_config(password)?;
                        args.push("--config".to_string());
                        args.push(pwfile.path.display().to_string());
                        _password_file = Some(pwfile);
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
                drop(_password_file);
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

fn attach_connection_args(
    config: &Config,
    args: &mut Vec<String>,
) -> Result<Option<MongoPasswordFile>> {
    let managed = config.mongo.managed.as_ref();
    let mut password_file: Option<MongoPasswordFile> = None;
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
            let pwfile = write_password_config(password)?;
            args.push("--config".to_string());
            args.push(pwfile.path.display().to_string());
            password_file = Some(pwfile);
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
    Ok(password_file)
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

fn mongo_doctor_queries(config: &Config) -> Result<Vec<MongoSupportQueryTemplate>> {
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

pub fn mongo_query_spec_from_name(name: &str) -> Result<MongoQuerySpec> {
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

fn mongo_query_spec_from_template(template: &MongoSupportQueryTemplate) -> Result<MongoQuerySpec> {
    Ok(MongoQuerySpec {
        database: template.database.clone(),
        collection: template.collection.clone(),
        filter: template.filter.clone(),
        projection: template.projection.clone(),
        update: None,
        sort: template.sort.clone(),
        limit: template.limit,
        template_name: Some(template.name.clone()),
    })
}

pub fn mongo_query_templates() -> Result<Vec<MongoSupportQueryTemplate>> {
    let registry: MongoQueryRegistry =
        serde_yaml::from_str(include_str!("../knowledge/mongo/queries.yaml"))?;
    Ok(registry.queries)
}

// ─────────────────────────────────────────────────────────────────────────
// Mutation remediation registry (knowledge/mongo/mutations.yaml)
//
// Support-query templates (`MongoSupportQueryTemplate`, above) can never
// express a write. Named remediation templates live here instead, typed so
// that:
//   - a template's `update` is always exactly one non-empty `$set` document
//     with no `_id` target and no nested/positional/JS-shaped operator;
//   - parameter substitution is structural (a placeholder must occupy an
//     entire JSON string) rather than string interpolation into `mongosh`;
//   - a template stays `preview_only` until an owner deliberately promotes
//     it to `executable`.
//
// `resolve_mutation_template` is the only supported way to turn a named,
// executable template plus caller-supplied parameters into a
// `ResolvedMutation`. Live preview/apply execution (Task 2) and CLI wiring
// (Task 3) build on these types; this module does not call mongosh for
// mutations.
// ─────────────────────────────────────────────────────────────────────────

/// Global hard cap on documents a single mutation may affect, enforced in
/// code regardless of what a template's YAML declares.
const MONGO_MUTATION_GLOBAL_MAX_AFFECTED: u32 = 1000;

/// Rollback strategies the executor (Task 4) knows how to invert. A
/// template that declares anything else fails validation at load time.
const SUPPORTED_ROLLBACK_STRATEGIES: &[&str] = &["guarded_set_inverse"];

/// Update-document keys that indicate server-side JavaScript execution.
/// Banned anywhere in a template's `filter`.
const JS_EXECUTION_OPERATOR_KEYS: &[&str] = &["$where", "$function", "$accumulator"];

/// Whether a named mutation template is live-executable or preview-only.
///
/// Promotion from `PreviewOnly` to `Executable` is a deliberate, reviewed
/// YAML edit by the remediation owner — never inferred from the shape of
/// the filter/update.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationTemplateMode {
    Executable,
    PreviewOnly,
}

impl MutationTemplateMode {
    fn as_str(self) -> &'static str {
        match self {
            MutationTemplateMode::Executable => "executable",
            MutationTemplateMode::PreviewOnly => "preview_only",
        }
    }
}

/// A typed parameter a mutation template accepts.
///
/// `type` intentionally supports only `string` today. `integer`, `boolean`,
/// and `json` are a documented extension point (add a
/// `MutationParameterType` variant plus a `bind_typed_parameter` match arm
/// and validation tests) — do not add the YAML surface for them without
/// tests, per plan Task 1 Step 2.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MutationParameter {
    #[serde(rename = "type")]
    pub type_: MutationParameterType,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationParameterType {
    String,
}

/// A named remediation template (`knowledge/mongo/mutations.yaml`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MongoMutationTemplate {
    pub id: String,
    pub revision: u32,
    pub mode: MutationTemplateMode,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: Value,
    #[serde(default)]
    pub update: Value,
    #[serde(default)]
    pub parameters: BTreeMap<String, MutationParameter>,
    pub max_affected: u32,
    pub max_backup_age_minutes: u32,
    pub purpose: String,
    #[serde(default)]
    pub kba_refs: Vec<String>,
    pub rollback: String,
}

#[derive(Debug, Deserialize)]
struct MongoMutationRegistry {
    #[serde(default)]
    mutations: Vec<MongoMutationTemplate>,
}

/// A named template plus caller-supplied parameters, fully resolved: every
/// `${param}` placeholder in `filter`/`update` has been replaced with its
/// bound typed value. Nothing downstream needs the original template or
/// raw parameter strings again.
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedMutation {
    pub template_id: String,
    pub template_revision: u32,
    /// `sha256:<hex>` over the template's canonical (pre-substitution) shape.
    pub template_source_digest: String,
    pub database: String,
    pub collection: String,
    pub filter: Value,
    /// Always `{"$set": {...}}` — see `validate_mutation_template`.
    pub update: Value,
    pub max_affected: u32,
    pub max_backup_age_minutes: u32,
    pub parameters: BTreeMap<String, Value>,
    /// `sha256:<hex>` over the resolved parameter map.
    pub parameter_digest: String,
    pub purpose: String,
    pub kba_refs: Vec<String>,
    pub rollback: String,
}

/// A snapshot of the documents a mutation preview matched — just enough to
/// bind an approval digest to the exact candidate set at preview time.
///
/// Task 2 parses this from live `mongosh` preview output and is expected to
/// extend it with per-document field-diff data once that program exists;
/// the shape here is deliberately the minimum `canonical_mutation_digest`
/// needs so Task 1 can ship a deterministic digest function without a live
/// Mongo connection.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CandidateSnapshot {
    pub matched_count: u64,
    /// Extended-JSON `_id` values, in the same deterministic order the
    /// preview query used.
    pub candidate_ids: Vec<Value>,
}

/// Parse repeated `--param key=value` values into a `BTreeMap`, rejecting a
/// duplicate key rather than silently letting the last one win.
pub fn parse_mutation_params(items: &[String]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for item in items {
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid --param '{item}', expected key=value"))?;
        if key.is_empty() {
            anyhow::bail!("invalid --param '{item}', expected key=value with a non-empty key");
        }
        if map.insert(key.to_string(), value.to_string()).is_some() {
            anyhow::bail!("duplicate --param key '{key}'");
        }
    }
    Ok(map)
}

/// Resolve a named, `Executable` mutation template against caller-supplied
/// parameters into a `ResolvedMutation`.
///
/// This is the only supported live-preview/apply resolver: it requires
/// `--template` (there is no free-form `--database`/`--collection`/
/// `--filter`/`--update` path here), binds only the template's declared
/// typed parameters, and refuses a `PreviewOnly` template outright.
pub fn resolve_mutation_template(
    name: &str,
    params: &BTreeMap<String, String>,
) -> Result<ResolvedMutation> {
    resolve_mutation_template_from(mongo_mutation_templates()?, name, params)
}

/// Same contract as `resolve_mutation_template`, but takes the candidate
/// template list as a parameter instead of always reading the compiled-in
/// registry. `resolve_mutation_template` is a thin wrapper over this with
/// `mongo_mutation_templates()`; tests use this seam to resolve against a
/// safe, test-only fixture template without adding a fake `executable`
/// entry to the shipped `mutations.yaml`.
fn resolve_mutation_template_from(
    templates: Vec<MongoMutationTemplate>,
    name: &str,
    params: &BTreeMap<String, String>,
) -> Result<ResolvedMutation> {
    let template = templates
        .into_iter()
        .find(|t| t.id == name)
        .ok_or_else(|| anyhow::anyhow!("unknown mongo mutation template '{name}'"))?;

    // Belt-and-suspenders: `mongo_mutation_templates()` already validates
    // every entry it loads, but this function is also reachable with an
    // injected template list, so re-validate here too.
    validate_mutation_template(&template)?;

    if template.mode != MutationTemplateMode::Executable {
        anyhow::bail!(
            "mongo mutation template '{name}' is '{}' and cannot be resolved for live preview/apply",
            template.mode.as_str()
        );
    }

    for key in params.keys() {
        if !template.parameters.contains_key(key) {
            anyhow::bail!("mongo mutation template '{name}' does not declare parameter '{key}'");
        }
    }

    let mut resolved_params: BTreeMap<String, Value> = BTreeMap::new();
    for (param_name, decl) in &template.parameters {
        match params.get(param_name) {
            Some(raw) => {
                resolved_params.insert(param_name.clone(), bind_typed_parameter(decl, raw));
            }
            None if decl.required => {
                anyhow::bail!(
                    "mongo mutation template '{name}' is missing required parameter '{param_name}'"
                );
            }
            None => {}
        }
    }

    // anyhow::Error's Display only shows the outermost message, not the
    // full context chain, and this crate's callers surface errors via
    // `err.to_string()` — so fold the underlying reason into the same
    // top-level message rather than burying it behind `with_context`.
    let resolved_filter = resolve_placeholders_in_tree(&template.filter, &resolved_params)
        .map_err(|e| {
            anyhow::anyhow!("failed to resolve filter for mongo mutation template '{name}': {e}")
        })?;
    let resolved_update = resolve_placeholders_in_tree(&template.update, &resolved_params)
        .map_err(|e| {
            anyhow::anyhow!("failed to resolve update for mongo mutation template '{name}': {e}")
        })?;

    let parameter_digest = digest_with_prefix(&json!(resolved_params));
    let template_source_digest = digest_with_prefix(&serde_json::to_value(&template)?);

    Ok(ResolvedMutation {
        template_id: template.id.clone(),
        template_revision: template.revision,
        template_source_digest,
        database: template.database.clone(),
        collection: template.collection.clone(),
        filter: resolved_filter,
        update: resolved_update,
        max_affected: template.max_affected,
        max_backup_age_minutes: template.max_backup_age_minutes,
        parameters: resolved_params,
        parameter_digest,
        purpose: template.purpose.clone(),
        kba_refs: template.kba_refs.clone(),
        rollback: template.rollback,
    })
}

/// Bind one caller-supplied raw string to its template-declared typed
/// value. Only `string` is implemented; see `MutationParameterType`.
fn bind_typed_parameter(decl: &MutationParameter, raw: &str) -> Value {
    match decl.type_ {
        MutationParameterType::String => Value::String(raw.to_string()),
    }
}

/// A deterministic digest binding a resolved mutation to the exact
/// candidate set a preview matched. The caller's `--approve` at apply time
/// must equal this value exactly (Task 3/4); any change to the template
/// identity, resolved filter/update, parameters, or candidate set changes
/// the digest.
pub fn canonical_mutation_digest(
    mutation: &ResolvedMutation,
    candidates: &CandidateSnapshot,
) -> String {
    let payload = json!({
        "template_id": mutation.template_id,
        "template_revision": mutation.template_revision,
        "template_source_digest": mutation.template_source_digest,
        "database": mutation.database,
        "collection": mutation.collection,
        "filter": mutation.filter,
        "update": mutation.update,
        "max_affected": mutation.max_affected,
        "parameter_digest": mutation.parameter_digest,
        "candidates": {
            "matched_count": candidates.matched_count,
            "candidate_ids": candidates.candidate_ids,
        },
    });
    digest_with_prefix(&payload)
}

fn digest_with_prefix(value: &Value) -> String {
    format!("sha256:{}", compute_sha256(value))
}

/// Load and validate every mutation template in the registry. Validation
/// happens here (not only when a specific template is resolved) so a
/// malformed registry shape is rejected the moment it's read, before any
/// subprocess could be created.
pub fn mongo_mutation_templates() -> Result<Vec<MongoMutationTemplate>> {
    let registry: MongoMutationRegistry =
        serde_yaml::from_str(include_str!("../knowledge/mongo/mutations.yaml"))?;
    let mut seen_ids = BTreeSet::new();
    for template in &registry.mutations {
        validate_mutation_template(template)?;
        if !seen_ids.insert(template.id.clone()) {
            anyhow::bail!("duplicate mongo mutation template id '{}'", template.id);
        }
    }
    Ok(registry.mutations)
}

fn validate_mutation_template(template: &MongoMutationTemplate) -> Result<()> {
    let id = template.id.trim();
    if id.is_empty() {
        anyhow::bail!("mongo mutation template has an empty id");
    }
    if template.database.trim().is_empty() {
        anyhow::bail!("mongo mutation template '{id}' has an empty database");
    }
    if template.collection.trim().is_empty() {
        anyhow::bail!("mongo mutation template '{id}' has an empty collection");
    }
    if template.purpose.trim().is_empty() {
        anyhow::bail!("mongo mutation template '{id}' has an empty purpose");
    }
    if template.revision == 0 {
        anyhow::bail!("mongo mutation template '{id}' must declare a positive revision");
    }
    if template.max_affected == 0 {
        anyhow::bail!("mongo mutation template '{id}' must declare a positive max_affected");
    }
    if template.max_affected > MONGO_MUTATION_GLOBAL_MAX_AFFECTED {
        anyhow::bail!(
            "mongo mutation template '{id}' max_affected {} exceeds the global cap of {}",
            template.max_affected,
            MONGO_MUTATION_GLOBAL_MAX_AFFECTED
        );
    }
    if template.max_backup_age_minutes == 0 {
        anyhow::bail!(
            "mongo mutation template '{id}' must declare a positive max_backup_age_minutes"
        );
    }
    if !matches!(&template.filter, Value::Object(map) if !map.is_empty()) {
        anyhow::bail!("mongo mutation template '{id}' must declare a non-empty object filter");
    }
    if contains_js_execution_operator(&template.filter) {
        anyhow::bail!(
            "mongo mutation template '{id}' filter contains a raw JavaScript-shaped operator"
        );
    }

    let set_doc = validate_update_is_single_set(id, &template.update)?;
    validate_set_document(id, set_doc)?;

    if !SUPPORTED_ROLLBACK_STRATEGIES.contains(&template.rollback.as_str()) {
        anyhow::bail!(
            "mongo mutation template '{id}' declares unsupported rollback strategy '{}'",
            template.rollback
        );
    }

    for name in template.parameters.keys() {
        if name.trim().is_empty() {
            anyhow::bail!("mongo mutation template '{id}' declares a parameter with an empty name");
        }
    }

    validate_placeholders_in_tree(id, &template.filter, &template.parameters)?;
    validate_placeholders_in_tree(id, &template.update, &template.parameters)?;

    Ok(())
}

/// Require `update` to be exactly `{"$set": {<non-empty>}}` — no pipeline
/// (array) updates, no other operator alongside or instead of `$set`.
fn validate_update_is_single_set<'a>(
    id: &str,
    update: &'a Value,
) -> Result<&'a serde_json::Map<String, Value>> {
    let update_obj = update.as_object().ok_or_else(|| {
        anyhow::anyhow!(
            "mongo mutation template '{id}' update must be a JSON object (pipeline-style array updates are not supported)"
        )
    })?;
    if update_obj.len() != 1 {
        anyhow::bail!(
            "mongo mutation template '{id}' update must contain exactly one '$set' operator and nothing else"
        );
    }
    let set_value = update_obj.get("$set").ok_or_else(|| {
        anyhow::anyhow!(
            "mongo mutation template '{id}' update must use '$set'; no other update operator is supported"
        )
    })?;
    set_value
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("mongo mutation template '{id}' '$set' must be a non-empty object")
        })
}

/// Reject a `_id` target and any `$`-bearing key at any depth inside the
/// `$set` document — this single rule covers dotted array-positional paths
/// (`items.$.qty`), nested update operators, and JS-execution operator
/// keys in one pass.
fn validate_set_document(id: &str, set_doc: &serde_json::Map<String, Value>) -> Result<()> {
    for (key, value) in set_doc {
        if key == "_id" || key.starts_with("_id.") {
            anyhow::bail!("mongo mutation template '{id}' update may not target _id");
        }
        if key.contains('$') {
            anyhow::bail!(
                "mongo mutation template '{id}' update path '{key}' uses an unsupported operator or positional syntax"
            );
        }
        reject_dollar_keys_recursive(id, value)?;
    }
    Ok(())
}

fn reject_dollar_keys_recursive(id: &str, value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key.starts_with('$') {
                    anyhow::bail!(
                        "mongo mutation template '{id}' update contains an unsupported nested operator '{key}'"
                    );
                }
                reject_dollar_keys_recursive(id, v)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_dollar_keys_recursive(id, item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn contains_js_execution_operator(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, v)| {
            JS_EXECUTION_OPERATOR_KEYS.contains(&key.as_str()) || contains_js_execution_operator(v)
        }),
        Value::Array(items) => items.iter().any(contains_js_execution_operator),
        _ => false,
    }
}

/// Registry-shape check: every whole-string `${name}` placeholder in the
/// tree must reference a declared parameter, and no placeholder may be
/// embedded inline inside a larger string.
fn validate_placeholders_in_tree(
    id: &str,
    value: &Value,
    declared: &BTreeMap<String, MutationParameter>,
) -> Result<()> {
    match value {
        Value::String(s) => {
            // Fold the underlying reason into the same top-level message
            // (see the note in `resolve_mutation_template_from`) rather
            // than burying it behind `with_context`, since callers surface
            // errors via `err.to_string()`.
            let placeholder = whole_string_placeholder(s)
                .map_err(|e| anyhow::anyhow!("mongo mutation template '{id}': {e}"))?;
            if let Some(name) = placeholder
                && !declared.contains_key(&name)
            {
                anyhow::bail!(
                    "mongo mutation template '{id}' references unknown placeholder parameter '{name}'"
                );
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                validate_placeholders_in_tree(id, item, declared)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for v in map.values() {
                validate_placeholders_in_tree(id, v, declared)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Structural substitution: walk the tree and replace every whole-string
/// `${name}` placeholder with its resolved typed JSON value. A string with
/// no placeholder marker passes through unchanged; a string with
/// placeholder syntax that doesn't occupy the entire value is rejected by
/// `whole_string_placeholder` rather than string-interpolated.
fn resolve_placeholders_in_tree(
    value: &Value,
    resolved_params: &BTreeMap<String, Value>,
) -> Result<Value> {
    match value {
        Value::String(s) => match whole_string_placeholder(s)? {
            Some(name) => resolved_params.get(&name).cloned().ok_or_else(|| {
                anyhow::anyhow!("unresolved template placeholder parameter '{name}'")
            }),
            None => Ok(Value::String(s.clone())),
        },
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .map(|v| resolve_placeholders_in_tree(v, resolved_params))
                .collect::<Result<Vec<_>>>()?,
        )),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), resolve_placeholders_in_tree(v, resolved_params)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// If `s` is exactly one placeholder occupying the whole string (e.g.
/// `"${new_email}"`), returns `Ok(Some(name))`. If `s` has no `${` marker
/// at all, returns `Ok(None)` — it's an ordinary literal. Anything else
/// (inline interpolation like `"prefix-${x}"`, a malformed name, an
/// unterminated marker) is rejected: parameter substitution is structural,
/// not string interpolation.
fn whole_string_placeholder(s: &str) -> Result<Option<String>> {
    if !s.contains("${") {
        return Ok(None);
    }
    if let Some(rest) = s.strip_prefix("${")
        && let Some(name) = rest.strip_suffix('}')
    {
        let looks_valid = !name.is_empty()
            && !name.contains("${")
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if looks_valid {
            return Ok(Some(name.to_string()));
        }
    }
    anyhow::bail!(
        "template placeholder '{s}' is malformed or not a whole-string placeholder; a placeholder must occupy an entire string value, e.g. \"${{param_name}}\""
    );
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
            let mut _restore_password_file: Option<MongoPasswordFile> = None;

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
                    let pwfile = write_password_config(password)?;
                    args.push("--config".to_string());
                    args.push(pwfile.path.display().to_string());
                    _restore_password_file = Some(pwfile);
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

/// Holds a temporary `--config` file for `mongodump`/`mongorestore`/`mongosh`
/// so the password is not visible in argv (`ps`, `/proc/<pid>/cmdline`).
///
/// The file is created with `0o600` permissions on Unix and is deleted when
/// the returned `TempDir` is dropped. Callers must keep the `TempDir` alive
/// until after `run_command_capture` returns.
struct MongoPasswordFile {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn write_password_config(password: &str) -> Result<MongoPasswordFile> {
    let dir = tempfile::Builder::new()
        .prefix("ayx-mongo-")
        .tempdir()
        .context("failed to create temp dir for mongo password file")?;
    let path = dir.path().join("config.yaml");
    // mongodump/mongorestore (mongo-tools) accept a YAML --config file with
    // `password: "..."`. mongosh accepts the same shape via `--config`.
    let body = serde_yaml::to_string(&json!({ "password": password }))?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| {
                format!("failed to open mongo password config '{}'", path.display())
            })?;
        f.write_all(body.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&path, body.as_bytes()).with_context(|| {
            format!("failed to write mongo password config '{}'", path.display())
        })?;
    }
    Ok(MongoPasswordFile { _dir: dir, path })
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

    // ── Mutation registry / resolver tests (plan Task 1, Step 4) ──────────

    /// A safe, test-only executable `$set` template. Deliberately not part
    /// of the shipped `mutations.yaml` — no shipped template is silently
    /// made live just because a test needs an `executable` fixture.
    fn test_executable_template() -> MongoMutationTemplate {
        let mut parameters = BTreeMap::new();
        parameters.insert(
            "new_value".to_string(),
            MutationParameter {
                type_: MutationParameterType::String,
                required: true,
            },
        );
        MongoMutationTemplate {
            id: "test_fixture_set_value".to_string(),
            revision: 1,
            mode: MutationTemplateMode::Executable,
            database: "TestDb".to_string(),
            collection: "widgets".to_string(),
            filter: json!({ "status": "pending" }),
            update: json!({ "$set": { "value": "${new_value}" } }),
            parameters,
            max_affected: 10,
            max_backup_age_minutes: 60,
            purpose: "test-only fixture for resolver unit tests".to_string(),
            kba_refs: vec![],
            rollback: "guarded_set_inverse".to_string(),
        }
    }

    #[test]
    fn fixture_template_passes_validation() {
        validate_mutation_template(&test_executable_template())
            .expect("well-formed fixture template should validate");
    }

    #[test]
    fn resolves_executable_fixture_template_successfully() {
        let mut params = BTreeMap::new();
        params.insert("new_value".to_string(), "42".to_string());

        let resolved = resolve_mutation_template_from(
            vec![test_executable_template()],
            "test_fixture_set_value",
            &params,
        )
        .expect("fixture template should resolve");

        assert_eq!(resolved.template_id, "test_fixture_set_value");
        assert_eq!(resolved.database, "TestDb");
        assert_eq!(resolved.collection, "widgets");
        assert_eq!(resolved.filter, json!({ "status": "pending" }));
        assert_eq!(resolved.update, json!({ "$set": { "value": "42" } }));
        assert!(resolved.parameter_digest.starts_with("sha256:"));
        assert!(resolved.template_source_digest.starts_with("sha256:"));
    }

    #[test]
    fn rejects_unknown_template_name() {
        let params = BTreeMap::new();
        let err = resolve_mutation_template("does_not_exist_template", &params)
            .expect_err("unknown template name should error");
        assert!(err.to_string().contains("unknown mongo mutation template"));
    }

    #[test]
    fn rejects_unknown_parameter() {
        let mut params = BTreeMap::new();
        params.insert("new_value".to_string(), "42".to_string());
        params.insert("bogus".to_string(), "x".to_string());

        let err = resolve_mutation_template_from(
            vec![test_executable_template()],
            "test_fixture_set_value",
            &params,
        )
        .expect_err("unknown caller parameter should be rejected");
        assert!(err.to_string().contains("does not declare parameter"));
    }

    #[test]
    fn rejects_missing_required_parameter() {
        let params = BTreeMap::new();

        let err = resolve_mutation_template_from(
            vec![test_executable_template()],
            "test_fixture_set_value",
            &params,
        )
        .expect_err("missing required parameter should be rejected");
        assert!(err.to_string().contains("missing required parameter"));
    }

    #[test]
    fn parse_mutation_params_rejects_duplicate_key() {
        let items = vec!["new_value=1".to_string(), "new_value=2".to_string()];
        let err =
            parse_mutation_params(&items).expect_err("duplicate --param key should be rejected");
        assert!(err.to_string().contains("duplicate --param key"));
    }

    #[test]
    fn parse_mutation_params_rejects_malformed_entry() {
        let items = vec!["no-equals-sign".to_string()];
        assert!(parse_mutation_params(&items).is_err());
    }

    #[test]
    fn parse_mutation_params_rejects_empty_key() {
        let items = vec!["=value".to_string()];
        assert!(parse_mutation_params(&items).is_err());
    }

    #[test]
    fn parse_mutation_params_accepts_distinct_keys() {
        let items = vec!["a=1".to_string(), "b=2".to_string()];
        let map = parse_mutation_params(&items).expect("distinct keys should parse");
        assert_eq!(map.get("a").map(String::as_str), Some("1"));
        assert_eq!(map.get("b").map(String::as_str), Some("2"));
    }

    #[test]
    fn rejects_inline_placeholder() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "value": "prefix-${new_value}" } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("inline (non-whole-string) placeholder should be rejected");
        assert!(err.to_string().contains("whole-string placeholder"));
    }

    #[test]
    fn rejects_unknown_placeholder() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "value": "${undeclared_param}" } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("placeholder referencing an undeclared parameter should be rejected");
        assert!(err.to_string().contains("unknown placeholder parameter"));
    }

    #[test]
    fn rejects_empty_filter() {
        let mut tmpl = test_executable_template();
        tmpl.filter = json!({});
        let err = validate_mutation_template(&tmpl).expect_err("empty filter should be rejected");
        assert!(err.to_string().contains("non-empty object filter"));
    }

    #[test]
    fn rejects_non_object_filter() {
        let mut tmpl = test_executable_template();
        tmpl.filter = Value::Null;
        assert!(validate_mutation_template(&tmpl).is_err());
    }

    #[test]
    fn rejects_empty_set_document() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": {} });
        let err = validate_mutation_template(&tmpl).expect_err("empty $set should be rejected");
        assert!(err.to_string().contains("non-empty object"));
    }

    #[test]
    fn rejects_pipeline_style_array_update() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!([{ "$set": { "value": 1 } }]);
        let err = validate_mutation_template(&tmpl)
            .expect_err("pipeline-style array update should be rejected");
        assert!(err.to_string().contains("pipeline-style array updates"));
    }

    #[test]
    fn rejects_non_set_update_operator() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$inc": { "value": 1 } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("a non-$set update operator should be rejected");
        assert!(err.to_string().contains("must use '$set'"));
    }

    #[test]
    fn rejects_extra_operator_alongside_set() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "value": 1 }, "$inc": { "count": 1 } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("an operator alongside $set should be rejected");
        assert!(err.to_string().contains("exactly one '$set' operator"));
    }

    #[test]
    fn rejects_id_target() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "_id": "x" } });
        let err = validate_mutation_template(&tmpl).expect_err("_id target should be rejected");
        assert!(err.to_string().contains("may not target _id"));
    }

    #[test]
    fn rejects_dotted_id_target() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "_id.sub": "x" } });
        assert!(validate_mutation_template(&tmpl).is_err());
    }

    #[test]
    fn rejects_positional_operator_path() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "items.$.qty": 1 } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("array positional operator path should be rejected");
        assert!(
            err.to_string()
                .contains("unsupported operator or positional syntax")
        );
    }

    #[test]
    fn rejects_nested_operator_inside_set_value() {
        let mut tmpl = test_executable_template();
        tmpl.update = json!({ "$set": { "profile": { "$rename": "x" } } });
        let err = validate_mutation_template(&tmpl)
            .expect_err("nested operator inside a $set value should be rejected");
        assert!(err.to_string().contains("unsupported nested operator"));
    }

    #[test]
    fn rejects_javascript_shaped_filter_operator() {
        let mut tmpl = test_executable_template();
        tmpl.filter = json!({ "$where": "this.a == this.b" });
        let err =
            validate_mutation_template(&tmpl).expect_err("$where in filter should be rejected");
        assert!(err.to_string().contains("JavaScript-shaped operator"));
    }

    #[test]
    fn rejects_max_affected_above_global_cap() {
        let mut tmpl = test_executable_template();
        tmpl.max_affected = 1001;
        let err = validate_mutation_template(&tmpl)
            .expect_err("max_affected above the global 1000-document cap should be rejected");
        assert!(err.to_string().contains("exceeds the global cap"));
    }

    #[test]
    fn allows_max_affected_at_global_cap_boundary() {
        let mut tmpl = test_executable_template();
        tmpl.max_affected = 1000;
        validate_mutation_template(&tmpl).expect("max_affected == 1000 should be allowed");
    }

    #[test]
    fn rejects_zero_max_affected() {
        let mut tmpl = test_executable_template();
        tmpl.max_affected = 0;
        assert!(validate_mutation_template(&tmpl).is_err());
    }

    #[test]
    fn rejects_zero_revision() {
        let mut tmpl = test_executable_template();
        tmpl.revision = 0;
        assert!(validate_mutation_template(&tmpl).is_err());
    }

    #[test]
    fn rejects_zero_max_backup_age_minutes() {
        let mut tmpl = test_executable_template();
        tmpl.max_backup_age_minutes = 0;
        assert!(validate_mutation_template(&tmpl).is_err());
    }

    #[test]
    fn rejects_unsupported_rollback_strategy() {
        let mut tmpl = test_executable_template();
        tmpl.rollback = "manual_only".to_string();
        let err = validate_mutation_template(&tmpl)
            .expect_err("unsupported rollback strategy should be rejected");
        assert!(err.to_string().contains("unsupported rollback strategy"));
    }

    #[test]
    fn support_query_registry_contains_no_mutation() {
        let templates =
            mongo_query_templates().expect("support query registry should load and parse");
        assert!(
            templates
                .iter()
                .all(|t| t.name != "user_email_domain_migration"),
            "user_email_domain_migration must not remain in the read-only support-query registry"
        );
        assert!(!templates.is_empty());
        // MongoSupportQueryTemplate has no `update` field at all (checked
        // at compile time by the type itself), so no support template can
        // structurally express a write.
    }

    #[test]
    fn mutation_registry_loads_and_validates() {
        let templates =
            mongo_mutation_templates().expect("shipped mutations.yaml should load and validate");
        assert!(!templates.is_empty());
    }

    #[test]
    fn shipped_user_email_domain_migration_is_preview_only_and_not_resolvable() {
        let templates =
            mongo_mutation_templates().expect("mutation registry should load and validate");
        let template = templates
            .iter()
            .find(|t| t.id == "user_email_domain_migration")
            .expect("user_email_domain_migration should exist in the mutation registry");
        assert_eq!(template.mode, MutationTemplateMode::PreviewOnly);

        let mut params = BTreeMap::new();
        params.insert("new_email".to_string(), "admin@companyB.com".to_string());
        let err = resolve_mutation_template("user_email_domain_migration", &params)
            .expect_err("preview_only template must not resolve for live preview/apply");
        assert!(err.to_string().contains("preview_only"));
    }

    #[test]
    fn canonical_mutation_digest_is_deterministic_and_reacts_to_candidate_changes() {
        let mut params = BTreeMap::new();
        params.insert("new_value".to_string(), "42".to_string());
        let resolved = resolve_mutation_template_from(
            vec![test_executable_template()],
            "test_fixture_set_value",
            &params,
        )
        .expect("fixture should resolve");

        let candidates = CandidateSnapshot {
            matched_count: 2,
            candidate_ids: vec![json!("a"), json!("b")],
        };

        let digest_a = canonical_mutation_digest(&resolved, &candidates);
        let digest_b = canonical_mutation_digest(&resolved, &candidates);
        assert_eq!(
            digest_a, digest_b,
            "digest must be deterministic for identical input"
        );
        assert!(digest_a.starts_with("sha256:"));

        let different_candidates = CandidateSnapshot {
            matched_count: 3,
            candidate_ids: vec![json!("a"), json!("b"), json!("c")],
        };
        let digest_c = canonical_mutation_digest(&resolved, &different_candidates);
        assert_ne!(
            digest_a, digest_c,
            "digest must change when the candidate set changes"
        );
    }
}
