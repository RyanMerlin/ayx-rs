use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "".to_string()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|key| format!("\"{}\":{}", key, canonical_json(&map[*key])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

pub fn compute_sha256(value: &Value) -> String {
    let json = canonical_json(value);
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

pub fn write_run_manifest(
    out_dir: &Path,
    command: &str,
    status: &str,
    artifacts: &[PathBuf],
    details: Option<&Value>,
) -> Result<PathBuf> {
    let payload = json!({
        "timestamp_utc": Utc::now().to_rfc3339(),
        "command": command,
        "status": status,
        "artifacts": artifacts.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "details": details.cloned().unwrap_or_else(|| json!({})),
    });
    let manifest_path = out_dir.join("run_manifest.json");
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&manifest_path, serde_json::to_string_pretty(&payload)?)?;
    Ok(manifest_path)
}

pub fn write_plan_manifest(out_dir: &Path, plan_payload: &Value) -> Result<PathBuf> {
    if let Some(parent) = out_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let plan_hash = compute_sha256(plan_payload);
    let payload = json!({
        "plan_hash": plan_hash,
        "plan": plan_payload,
        "created_utc": Utc::now().to_rfc3339(),
    });
    let path = out_dir.join("plan_manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&payload)?)?;
    Ok(path)
}

pub fn validate_plan_manifest(path: &Path) -> Result<(bool, String, Value)> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let payload: Value = serde_json::from_str(&text)?;
    let plan = payload.get("plan").cloned();
    if plan.is_none() || payload.get("plan_hash").is_none() {
        return Ok((false, "manifest missing required keys".to_string(), payload));
    }
    let plan = plan.unwrap();
    let expected = payload
        .get("plan_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let actual = compute_sha256(&plan);
    if actual != expected {
        return Ok((false, "manifest hash mismatch".to_string(), payload));
    }
    Ok((true, "ok".to_string(), payload))
}
