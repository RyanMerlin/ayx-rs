use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ayx_core::profile::{Config, ServerProfile};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Utc;
use reqwest::blocking::ClientBuilder;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use roxmltree::Document;
use serde_json::{Value, json};
use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::upgrade::{io, manifest, rules};

const DEFAULT_RUNTIME: &str = r"C:\ProgramData\Alteryx\RuntimeSettings.xml";
const DEFAULT_GALLERY_LOGS: &str = r"C:\ProgramData\Alteryx\Gallery\Logs";
const DEFAULT_SERVICE_DIR: &str = r"C:\ProgramData\Alteryx\Service";

fn runtime_path() -> PathBuf {
    PathBuf::from(DEFAULT_RUNTIME)
}

fn service_dir() -> PathBuf {
    PathBuf::from(DEFAULT_SERVICE_DIR)
}

fn gallery_logs() -> PathBuf {
    PathBuf::from(DEFAULT_GALLERY_LOGS)
}

pub fn compute_path(source: &str, target: &str, deployment: &str) -> Value {
    let rules = rules::UpgradeRules::new();
    let route = rules.resolve_path(source, target, deployment);
    if route.is_empty() {
        json!({
            "ok": false,
            "source": source,
            "target": target,
            "deployment": deployment,
            "error": "no supported upgrade route",
            "pit_stops": [],
            "route": [],
        })
    } else {
        let pit_stops = if route.len() > 2 {
            route[1..route.len() - 1].to_vec()
        } else {
            vec![]
        };
        json!({
            "ok": true,
            "source": source,
            "target": target,
            "deployment": deployment,
            "route": route,
            "pit_stops": pit_stops,
            "hop_count": route.len().saturating_sub(1),
        })
    }
}

pub fn run_precheck(
    config: &Config,
    target_version: &str,
    out_dir: &Path,
    deployment: &str,
) -> Result<Value> {
    let mut checks = Vec::new();
    let runtime = runtime_path();
    let service = service_dir();
    let gallery = gallery_logs();
    if runtime.exists() {
        checks.push(check_entry(
            "runtime_settings_exists",
            "pass",
            runtime.display().to_string(),
            "info",
        ));
    } else {
        checks.push(check_entry(
            "runtime_settings_exists",
            "fail",
            runtime.display().to_string(),
            "high",
        ));
    }
    if service.exists() {
        checks.push(check_entry(
            "service_dir_exists",
            "pass",
            service.display().to_string(),
            "info",
        ));
    } else {
        checks.push(check_entry(
            "service_dir_exists",
            "fail",
            service.display().to_string(),
            "high",
        ));
    }
    if gallery.exists() {
        checks.push(check_entry(
            "gallery_logs_exists",
            "pass",
            gallery.display().to_string(),
            "info",
        ));
    } else {
        checks.push(check_entry(
            "gallery_logs_exists",
            "warn",
            gallery.display().to_string(),
            "medium",
        ));
    }

    let migration_logs: Vec<_> = if service.exists() {
        fs::read_dir(&service)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains("migration"))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        Vec::new()
    };
    if migration_logs.is_empty() {
        checks.push(check_entry(
            "migration_log_detected",
            "warn",
            "no migration csv found".into(),
            "medium",
        ));
    } else {
        let details = migration_logs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        checks.push(check_entry(
            "migration_log_detected",
            "pass",
            details,
            "low",
        ));
    }

    let runtime_data = read_runtime(&runtime);
    let server = config.server.as_ref();
    let mut token_status = "skipped";
    let mut token_details = "no server credentials".to_string();
    if let Some(server) = server
        && !server.webapi_url.is_empty()
        && !server.curator_api_key.is_empty()
        && !server.curator_api_secret.is_empty()
    {
        match validate_token_endpoint(server) {
            Ok(status) => {
                token_status = "pass";
                token_details = status;
            }
            Err(err) => {
                token_status = "fail";
                token_details = err.to_string();
            }
        }
    }
    checks.push(check_entry(
        "oauth2_token_endpoint",
        token_status,
        token_details,
        if token_status == "fail" {
            "high"
        } else {
            "info"
        },
    ));

    let rules = rules::UpgradeRules::new();
    let current_version = current_version_from_runtime(&runtime_data).unwrap_or_default();
    let path_eval = compute_path(&current_version, target_version, deployment);
    let path_ok = path_eval
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    checks.push(check_entry(
        "upgrade_path_supported",
        if path_ok { "pass" } else { "fail" },
        serde_json::to_string(&path_eval)?,
        if path_ok { "info" } else { "high" },
    ));

    let issues = if current_version.is_empty() {
        Vec::new()
    } else {
        rules.matching_issues(&current_version, target_version, deployment)
    };
    for issue in &issues {
        checks.push(check_entry(
            "known_issue",
            "warn",
            serde_json::to_string(issue)?,
            issue.severity.as_str(),
        ));
    }

    let blockers = checks
        .iter()
        .filter(|c| c.get("status").map(|s| s == "fail").unwrap_or(false))
        .count();
    let out = io::ensure_dir(out_dir)?;
    let rows_path = io::write_csv(
        &out.join("precheck_results.csv"),
        &checks,
        Some(&["check", "status", "severity", "details"]),
    )?;
    let issues_rows: Vec<_> = issues
        .iter()
        .map(|issue| {
            let mut map = HashMap::new();
            map.insert("id".into(), issue.issue_id.clone());
            map.insert("severity".into(), issue.severity.clone());
            map.insert("message".into(), issue.message.clone());
            map
        })
        .collect();
    let issues_path = io::write_csv(
        &out.join("known_issues.csv"),
        &issues_rows,
        Some(&["id", "severity", "message"]),
    )?;
    let manifest_path = manifest::write_run_manifest(
        &out,
        "upgrade precheck",
        if blockers == 0 { "ok" } else { "failed" },
        &[rows_path.clone(), issues_path.clone()],
        Some(&json!({
            "target_version": target_version,
            "deployment": deployment,
            "runtime": runtime_data,
        })),
    )?;

    Ok(json!({
        "ok": blockers == 0,
        "blocker_count": blockers,
        "checks": checks,
        "issues": issues.iter().map(|issue| {
            json!({
                "id": issue.issue_id,
                "severity": issue.severity,
                "message": issue.message,
            })
        }).collect::<Vec<_>>(),
        "artifacts": [
            rows_path.display().to_string(),
            issues_path.display().to_string(),
            manifest_path.display().to_string(),
        ],
        "runtime": runtime_data,
    }))
}

fn check_entry(
    name: &str,
    status: &str,
    details: String,
    severity: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("check".into(), name.into());
    map.insert("status".into(), status.into());
    map.insert("details".into(), details);
    map.insert("severity".into(), severity.into());
    map
}

fn read_runtime(path: &Path) -> Value {
    if !path.exists() {
        return json!({});
    }
    let content = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return json!({}),
    };
    let document = Document::parse(&content).ok();
    if let Some(doc) = document {
        let version = doc
            .descendants()
            .find(|n| n.has_tag_name("ProductVersion"))
            .or_else(|| doc.descendants().find(|n| n.has_tag_name("Version")))
            .or_else(|| doc.descendants().find(|n| n.has_tag_name("BuildVersion")))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let auth = doc
            .descendants()
            .find(|n| n.has_tag_name("AuthenticationType"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let enabled = doc
            .descendants()
            .find(|n| n.has_tag_name("EmbeddedMongoDBEnabled"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let path = doc
            .descendants()
            .find(|n| n.has_tag_name("EmbeddedMongoDBRootPath"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        return json!({
            "version": version,
            "authentication_type": auth,
            "embedded_mongo_enabled": enabled,
            "embedded_mongo_path": path,
        });
    }
    json!({})
}

fn current_version_from_runtime(runtime_data: &Value) -> Option<String> {
    runtime_data
        .get("version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .filter(|s| !s.trim().is_empty())
}

fn validate_token_endpoint(server: &ServerProfile) -> Result<String> {
    let token_url = format!(
        "{}/webapi/oauth2/token",
        server.webapi_url.trim_end_matches('/')
    );
    let client = ClientBuilder::new()
        .danger_accept_invalid_certs(!server.verify_tls())
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let credentials = format!("{}:{}", server.curator_api_key, server.curator_api_secret);
    let encoded = BASE64.encode(credentials);
    let response = client
        .post(&token_url)
        .header(AUTHORIZATION, format!("Basic {}", encoded))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials&scope=admin")
        .send()?;
    Ok(format!("status={}", response.status()))
}

pub fn run_backup(config: &Config, backup_type: &str, out_dir: &Path) -> Result<Value> {
    let out = io::ensure_dir(out_dir)?;
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_root = io::ensure_dir(&out.join(format!("backup_{}", timestamp)))?;
    let mut actions = Vec::new();
    let runtime = runtime_path();
    let service = service_dir();
    let gallery = gallery_logs();

    if ["runtime", "all"].contains(&backup_type) {
        let (ok, details) = io::copy_if_exists(&runtime, &backup_root.join("RuntimeSettings.xml"))?;
        actions.push(check_entry(
            "runtime_settings_copy",
            if ok { "pass" } else { "warn" },
            details,
            "info",
        ));
    }
    if ["logs", "all"].contains(&backup_type) {
        let (ok, details) = io::copy_if_exists(&service, &backup_root.join("Service"))?;
        actions.push(check_entry(
            "service_logs_copy",
            if ok { "pass" } else { "warn" },
            details,
            "info",
        ));
        let (ok, details) = io::copy_if_exists(&gallery, &backup_root.join("GalleryLogs"))?;
        actions.push(check_entry(
            "gallery_logs_copy",
            if ok { "pass" } else { "warn" },
            details,
            "info",
        ));
    }
    if ["mongo", "all"].contains(&backup_type) {
        let runtime_data = read_runtime(&runtime);
        let mongo_path = runtime_data
            .get("embedded_mongo_path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let instructions = format!(
            "Use embedded Mongo backup tooling. Detected path: {}",
            mongo_path
        );
        let instr_path = backup_root.join("mongo_backup_instructions.txt");
        fs::write(&instr_path, instructions)?;
        actions.push(check_entry(
            "mongo_backup_instruction",
            "pass",
            instr_path.display().to_string(),
            "info",
        ));
    }
    let rows_path = io::write_csv(
        &out.join("backup_results.csv"),
        &actions,
        Some(&["check", "status", "details", "severity"]),
    )?;
    let manifest_path = manifest::write_run_manifest(
        &out,
        "upgrade backup",
        "ok",
        &[rows_path.clone(), backup_root.clone()],
        Some(&json!({
            "backup_type": backup_type,
            "profile": config.profile_name,
        })),
    )?;
    Ok(json!({
        "ok": true,
        "actions": actions,
        "artifacts": [
            rows_path.display().to_string(),
            backup_root.display().to_string(),
            manifest_path.display().to_string(),
        ],
    }))
}

pub fn run_plan(source: &str, target: &str, deployment: &str, out_dir: &Path) -> Result<Value> {
    let out = io::ensure_dir(out_dir)?;
    let path_result = compute_path(source, target, deployment);
    if !path_result
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        manifest::write_run_manifest(&out, "upgrade plan", "failed", &[], Some(&path_result))?;
        return Ok(path_result);
    }
    let route = path_result
        .get("route")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let mut steps = vec![
        json!({"id": "precheck", "action": "ayx upgrade precheck", "mode": "read-only"}),
        json!({"id": "backup", "action": "ayx upgrade backup --type all", "mode": "safe"}),
    ];
    for idx in 0..route.len().saturating_sub(1) {
        steps.push(json!({
            "id": format!("hop_{}", idx + 1),
            "action": format!("Upgrade {} -> {}", route[idx], route[idx + 1]),
            "mode": "hold-point",
            "rollback_note": format!("Rollback snapshot before hop {}", idx + 1),
        }));
    }
    steps.push(json!({"id": "postcheck", "action": "ayx upgrade postcheck", "mode": "read-only"}));
    let plan_payload = json!({
        "source": source,
        "target": target,
        "deployment": deployment,
        "route": route,
        "pit_stops": route.iter().skip(1).take(route.len().saturating_sub(2)).cloned().collect::<Vec<_>>(),
        "steps": steps,
    });
    let plan_path = io::write_json(&out.join("upgrade_plan.json"), &plan_payload)?;
    let manifest_path = manifest::write_plan_manifest(&out, &plan_payload)?;
    let run_manifest = manifest::write_run_manifest(
        &out,
        "upgrade plan",
        "ok",
        &[plan_path.clone(), manifest_path.clone()],
        Some(&json!({"hop_count": route.len().saturating_sub(1)})),
    )?;
    Ok(json!({
        "ok": true,
        "plan": plan_payload,
        "artifacts": [
            plan_path.display().to_string(),
            manifest_path.display().to_string(),
            run_manifest.display().to_string(),
        ],
    }))
}

pub fn run_apply(manifest_path: &Path, apply: bool, yes: bool) -> Result<Value> {
    if !apply || !yes {
        return Ok(json!({"ok": false, "error": "apply requires both --apply and --yes"}));
    }
    let (valid, reason, payload) = manifest::validate_plan_manifest(manifest_path)?;
    if !valid {
        return Ok(json!({"ok": false, "error": reason}));
    }
    let plan = payload.get("plan").cloned().unwrap_or_else(|| json!({}));
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut audit_rows = Vec::new();
    for step in steps {
        let mut row = HashMap::new();
        row.insert("timestamp_utc".into(), Utc::now().to_rfc3339());
        row.insert(
            "step_id".into(),
            step.get("id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        );
        row.insert(
            "action".into(),
            step.get("action")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        row.insert("result".into(), "SIMULATED_APPLY".into());
        audit_rows.push(row);
    }
    let audit_path = io::write_csv(
        &manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("execution_audit.csv"),
        &audit_rows,
        None,
    )?;
    let run_manifest = manifest::write_run_manifest(
        manifest_path.parent().unwrap_or_else(|| Path::new(".")),
        "upgrade apply",
        "ok",
        std::slice::from_ref(&audit_path),
        Some(&json!({"plan_manifest": manifest_path.display().to_string()})),
    )?;
    Ok(json!({
        "ok": true,
        "artifacts": [audit_path.display().to_string(), run_manifest.display().to_string()],
        "step_count": audit_rows.len(),
    }))
}

pub fn run_postcheck(config: &Config, manifest_path: &Path, out_dir: &Path) -> Result<Value> {
    let out = io::ensure_dir(out_dir)?;
    let (valid, reason, payload) = manifest::validate_plan_manifest(manifest_path)?;
    if !valid {
        return Ok(json!({"ok": false, "error": reason}));
    }
    let mut checks = Vec::new();
    let service = service_dir();
    let migration_logs = if service.exists() {
        fs::read_dir(&service)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains("migration"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if migration_logs.is_empty() {
        checks.push(check_entry(
            "migration_log_present_after_upgrade",
            "warn",
            "no migration log detected".into(),
            "medium",
        ));
    } else {
        checks.push(check_entry(
            "migration_log_present_after_upgrade",
            "pass",
            migration_logs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            "medium",
        ));
    }
    checks.push(check_entry(
        "plan_hash_valid",
        "pass",
        "plan manifest hash validated".into(),
        "info",
    ));
    checks.push(check_entry(
        "profile_used",
        "pass",
        config.profile_name.clone(),
        "info",
    ));
    let mismatches: Vec<_> = checks
        .iter()
        .filter(|c| matches!(c.get("status"), Some(status) if status == "fail" || status == "warn"))
        .cloned()
        .collect();
    let post_path = io::write_csv(
        &out.join("postcheck_results.csv"),
        &checks,
        Some(&["check", "status", "severity", "details"]),
    )?;
    let mismatch_path = io::write_csv(
        &out.join("postcheck_mismatches.csv"),
        &mismatches,
        Some(&["check", "status", "severity", "details"]),
    )?;
    let run_manifest = manifest::write_run_manifest(
        &out,
        "upgrade postcheck",
        if checks
            .iter()
            .any(|c| c.get("status").map(|s| s == "fail").unwrap_or(false))
        {
            "failed"
        } else {
            "ok"
        },
        &[post_path.clone(), mismatch_path.clone()],
        Some(&json!({
            "plan_target": payload.get("plan").and_then(|p| p.get("target")).cloned().unwrap_or(Value::Null),
        })),
    )?;
    Ok(json!({
        "ok": true,
        "checks": checks,
        "artifacts": [
            post_path.display().to_string(),
            mismatch_path.display().to_string(),
            run_manifest.display().to_string(),
        ],
    }))
}

pub fn run_bundle(input_dir: &Path, out_zip: &Path) -> Result<Value> {
    let source =
        fs::canonicalize(input_dir).with_context(|| format!("failed to locate {:?}", input_dir))?;
    if let Some(parent) = out_zip.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(out_zip)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(&source) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(&source)?;
        if path.is_file() {
            zip.start_file(rel.to_string_lossy(), options)?;
            let mut f = fs::File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(
        json!({"ok": true, "bundle": out_zip.display().to_string(), "source": source.display().to_string()}),
    )
}
