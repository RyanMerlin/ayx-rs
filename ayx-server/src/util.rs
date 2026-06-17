use std::fs;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use ayx_core::audit::write_audit_artifact;
use ayx_core::profile::Config;
use chrono::Utc;
use serde_json::{Value, json};

pub fn capture_system_info() -> Result<Value> {
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_default();
    let ip_address = local_ip_address().unwrap_or_default();
    let disks = disk_stats();
    let environment = parse_systeminfo(systeminfo_text().unwrap_or_default().as_str());
    let total_memory_mb = total_memory_mb();
    let free_memory_mb = free_memory_mb();

    Ok(json!({
        "server_name": hostname.clone(),
        "server_name_1": hostname,
        "server_ip": ip_address,
        "windows_os": std::env::consts::OS,
        "windows_os_1": std::env::var("OS").unwrap_or_default(),
        "manufacturer": null,
        "model": null,
        "ram_gb": round_gb(total_memory_mb / 1024.0),
        "ram_free_gb": round_gb(free_memory_mb / 1024.0),
        "ram_free_pct": if total_memory_mb == 0.0 { 0.0 } else { (free_memory_mb / total_memory_mb) * 100.0 },
        "last_boot": null,
        "disk_stats": disks,
        "timezone": std::env::var("TZ").unwrap_or_default(),
        "environment": environment,
        "processes": process_snapshot()
    }))
}

pub fn runtime_settings_summary(path: &Path) -> Result<Value> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("failed to read runtime settings '{}'", path.display()))?;
    let doc = roxmltree::Document::parse(&xml).context("failed to parse runtime settings xml")?;
    let metadata = fs::metadata(path).ok();
    let auth = first_text(&doc, &["AuthenticationType"]);
    let embedded = first_text(&doc, &["EmbeddedMongoDBEnabled"]);
    let embedded_path = first_text(&doc, &["EmbeddedMongoDBRootPath"]);
    let mongo_connection = first_text(&doc, &["MongoConnectionString", "MongoDBConnectionString"]);
    let mongo_host = first_text(&doc, &["MongoHost", "MongoDBHost"]);
    let mongo_port = first_text(&doc, &["MongoPort", "MongoDBPort"]);
    let mongo_user = first_text(&doc, &["MongoUser", "MongoDBUser"]);
    let mongo_auth_db = first_text(&doc, &["MongoAuthDatabase", "MongoDBAuthDatabase"]);
    let gallery_logging = first_text(&doc, &["LoggingPath"]);
    let engine_logging = first_text(&doc, &["LogFilePath"]);
    let working_path = first_text(&doc, &["WorkingPath"]);

    Ok(json!({
        "path": path.display().to_string(),
        "metadata": {
            "created": metadata.as_ref().and_then(|m| m.created().ok()).map(chrono::DateTime::<Utc>::from),
            "modified": metadata.as_ref().and_then(|m| m.modified().ok()).map(chrono::DateTime::<Utc>::from),
        },
        "system_settings": {
            "gallery": {
                "authentication_type": auth,
                "logging_path": gallery_logging,
            },
            "controller": {
                "embedded_mongo_enabled": embedded,
                "embedded_mongo_path": embedded_path,
                "working_path": working_path,
            },
            "engine": {
                "log_file_path": engine_logging,
            },
            "mongo": {
                "connection_string": mongo_connection,
                "host": mongo_host,
                "port": mongo_port,
                "username": mongo_user,
                "auth_database": mongo_auth_db,
            }
        },
        "root": doc.root_element().tag_name().name(),
    }))
}

pub fn write_runtime_settings_json(path: &Path, output: &Path) -> Result<Value> {
    let summary = runtime_settings_summary(path)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_string_pretty(&summary)?)?;
    Ok(json!({
        "path": path.display().to_string(),
        "output": output.display().to_string(),
    }))
}

pub fn ayx_paths() -> Value {
    let program_data = std::env::var("ProgramData").ok();
    let app_data = std::env::var("APPDATA").ok();
    json!({
        "runtime_settings": program_data.as_ref().map(|p| format!(r"{p}\Alteryx\RuntimeSettings.xml")).unwrap_or_default(),
        "logging_candidates": program_data.as_ref().map(|p| vec![
            format!(r"{p}\Alteryx\Logs"),
            format!(r"{p}\Alteryx\Engine"),
        ]).unwrap_or_default(),
        "connection_files": [
            program_data.as_ref().map(|p| format!(r"{p}\Alteryx\Engine\SystemConnections.xml")),
            app_data.as_ref().map(|p| format!(r"{p}\Alteryx\Engine\UserConnections.xml")),
        ]
    })
}

pub fn server_logs(path: &Path) -> Result<Value> {
    let mut logs = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            logs.push(entry.path().display().to_string());
        }
    }
    logs.sort();
    Ok(json!({ "path": path.display().to_string(), "logs": logs }))
}

pub fn backup_plan(backup_dir: &Path) -> Result<Value> {
    Ok(json!({
        "backup_dir": backup_dir.display().to_string(),
        "settings_files": [
            "RuntimeSettings.xml",
            "SystemConnections.xml",
            "SystemAlias.xml",
            "UserConnections.xml",
            "UserAlias.xml"
        ],
        "commands": [
            "AlteryxService.exe stop",
            "AlteryxService.exe emongodump=<backup_dir>/MongoDB",
            "AlteryxService.exe start"
        ]
    }))
}

pub fn run_server_backup(
    config: &Config,
    backup_dir: &Path,
    apply: bool,
    audit_dir: &Path,
) -> Result<Value> {
    let runtime_settings = resolve_runtime_settings_path(config)?;
    let service_path = resolve_alteryx_service_path(config, &runtime_settings)?;
    let connection_paths = connection_file_paths();

    let execution = if apply {
        let settings_dir = backup_dir.join("settings");
        let mongo_dir = backup_dir.join("MongoDB");
        fs::create_dir_all(&settings_dir).with_context(|| {
            format!(
                "failed to create backup directory '{}'",
                settings_dir.display()
            )
        })?;
        fs::create_dir_all(&mongo_dir).with_context(|| {
            format!(
                "failed to create mongo backup directory '{}'",
                mongo_dir.display()
            )
        })?;

        let mut copied = Vec::new();
        copied.push(copy_file(
            &runtime_settings,
            &settings_dir.join("RuntimeSettings.xml"),
        )?);
        for path in &connection_paths {
            if let Some(name) = path.file_name() {
                copied.push(copy_file(path, &settings_dir.join(name))?);
            }
        }

        let stop = run_command_capture(service_path.as_path(), &["stop"], None)?;
        let dump_arg = format!("emongodump={}", mongo_dir.display());
        let dump = run_command_capture(service_path.as_path(), &[dump_arg.as_str()], None)?;
        let start = run_command_capture(service_path.as_path(), &["start"], None)?;

        Some(json!({
            "copied": copied,
            "commands": [stop, dump, start]
        }))
    } else {
        None
    };

    let audit_payload = json!({
        "command": "server backup",
        "timestamp_utc": Utc::now(),
        "profile": config.profile_name,
        "dry_run": !apply,
        "applied": apply,
        "backup_dir": backup_dir,
    });
    let audit_path = write_audit_artifact(audit_dir, "server-backup", &audit_payload)?;

    Ok(json!({
        "profile": config.profile_name,
        "dry_run": !apply,
        "applied": apply,
        "backup_dir": backup_dir.display().to_string(),
        "runtime_settings_path": runtime_settings.display().to_string(),
        "service_path": service_path.display().to_string(),
        "connection_paths": connection_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "execution": execution,
        "audit_artifact": audit_path,
    }))
}

fn round_gb(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn local_ip_address() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn disk_stats() -> Value {
    let mut out = serde_json::Map::new();
    if let Ok(output) = Command::new("cmd")
        .args([
            "/C",
            "wmic logicaldisk get Caption,FreeSpace,Size /format:csv",
        ])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split(',').collect();
            if cols.len() < 4 {
                continue;
            }
            let caption = cols.get(1).copied().unwrap_or("").trim();
            let free = cols
                .get(2)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0);
            let total = cols
                .get(3)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0);
            if caption.is_empty() || total <= 0.0 {
                continue;
            }
            let used = total - free;
            let total_gb = total / 1024.0 / 1024.0 / 1024.0;
            let free_gb = free / 1024.0 / 1024.0 / 1024.0;
            let used_gb = used / 1024.0 / 1024.0 / 1024.0;
            out.insert(
                caption.to_string(),
                json!({
                    "total_gb": round_gb(total_gb),
                    "used_gb": round_gb(used_gb),
                    "free_gb": round_gb(free_gb),
                    "percent": if total == 0.0 { 0.0 } else { (used / total) * 100.0 }
                }),
            );
        }
    }
    Value::Object(out)
}

fn process_snapshot() -> Value {
    let output = Command::new("cmd")
        .args(["/C", "tasklist /FO CSV"])
        .output()
        .ok();
    let Some(output) = output else {
        return Value::Array(Vec::new());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines().skip(1).take(25) {
        let cols: Vec<&str> = line.trim_matches('"').split("\",\"").collect();
        if cols.len() >= 5 {
            rows.push(json!({
                "name": cols[0],
                "pid": cols[1],
                "session_name": cols[2],
                "session_num": cols[3],
                "mem_usage": cols[4]
            }));
        }
    }
    Value::Array(rows)
}

fn systeminfo_text() -> Option<String> {
    let output = std::process::Command::new("systeminfo").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_systeminfo(stdout: &str) -> Value {
    let mut sysinfo = serde_json::Map::new();
    let mut prev_key = String::new();

    for line in stdout.lines() {
        let (raw_key, raw_value) = match line.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        let value = raw_value.trim();

        if !raw_key.starts_with(' ') {
            let key = raw_key.trim().to_string();
            let parsed_value = match key.as_str() {
                "Original Install Date" | "System Boot Time" => {
                    let parts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
                    if parts.len() == 2 {
                        json!({ "Date": parts[0], "Time": parts[1] })
                    } else {
                        json!(value)
                    }
                }
                "Processor(s)" | "Hotfix(s)" | "Network Card(s)" => json!({}),
                _ => json!(value),
            };
            sysinfo.insert(key.clone(), parsed_value);
            prev_key = key;
        } else if !prev_key.is_empty()
            && let Some(entry) = sysinfo.get_mut(&prev_key)
            && let Some(obj) = entry.as_object_mut()
        {
            let child_key = raw_key.trim().replace(['[', ']'], "");
            obj.insert(child_key, json!(value));
        }
    }

    Value::Object(sysinfo)
}

fn total_memory_mb() -> f64 {
    let output = Command::new("cmd")
        .args(["/C", "wmic computersystem get TotalPhysicalMemory /value"])
        .output()
        .ok();
    let Some(output) = output else {
        return 0.0;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| line.strip_prefix("TotalPhysicalMemory="))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map(|bytes| bytes / 1024.0 / 1024.0)
        .unwrap_or(0.0)
}

fn free_memory_mb() -> f64 {
    let output = Command::new("cmd")
        .args(["/C", "wmic OS get FreePhysicalMemory /value"])
        .output()
        .ok();
    let Some(output) = output else {
        return 0.0;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| line.strip_prefix("FreePhysicalMemory="))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn first_text(doc: &roxmltree::Document<'_>, names: &[&str]) -> Option<String> {
    doc.descendants()
        .find(|n| n.is_element() && names.contains(&n.tag_name().name()))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_runtime_settings_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.runtime_settings_path.as_ref())
    {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
    }

    for candidate in [
        r"C:\ProgramData\Alteryx\RuntimeSettings.xml",
        r"C:\ProgramData\Alteryx\Engine\RuntimeSettings.xml",
        r"C:\ProgramData\Alteryx\Server\RuntimeSettings.xml",
    ] {
        let pb = PathBuf::from(candidate);
        if pb.exists() {
            return Ok(pb);
        }
    }

    Err(anyhow::anyhow!("could not discover RuntimeSettings.xml"))
}

fn resolve_alteryx_service_path(config: &Config, runtime_settings: &Path) -> Result<PathBuf> {
    if let Some(path) = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.alteryx_service_path.as_ref())
    {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
    }

    for candidate in extract_install_path_candidates(runtime_settings)? {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!("could not discover AlteryxService.exe"))
}

fn extract_install_path_candidates(runtime_settings: &Path) -> Result<Vec<PathBuf>> {
    let xml = fs::read_to_string(runtime_settings).with_context(|| {
        format!(
            "failed to read runtime settings '{}'",
            runtime_settings.display()
        )
    })?;
    let doc = roxmltree::Document::parse(&xml).context("failed to parse runtime settings xml")?;
    let mut candidates = Vec::new();
    for key in [
        "LoggingPath",
        "WorkingPath",
        "WebInterfaceStagingPath",
        "SQLitePath",
    ] {
        if let Some(value) = first_text(&doc, &[key]) {
            let pb = PathBuf::from(value);
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

fn connection_file_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        paths.push(PathBuf::from(format!(
            r"{pd}\Alteryx\Engine\SystemConnections.xml"
        )));
        paths.push(PathBuf::from(format!(
            r"{pd}\Alteryx\Engine\SystemAlias.xml"
        )));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        paths.push(PathBuf::from(format!(
            r"{ad}\Alteryx\Engine\UserConnections.xml"
        )));
        paths.push(PathBuf::from(format!(r"{ad}\Alteryx\Engine\UserAlias.xml")));
    }
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn copy_file(source: &Path, destination: &Path) -> Result<Value> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy '{}' to '{}'",
            source.display(),
            destination.display()
        )
    })?;
    Ok(json!({
        "source": source.display().to_string(),
        "destination": destination.display().to_string(),
    }))
}

fn run_command_capture(binary: &Path, args: &[&str], cwd: Option<&Path>) -> Result<Value> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let output = cmd
        .output()
        .with_context(|| format!("failed to execute '{}'", binary.display()))?;

    Ok(json!({
        "binary": binary.display().to_string(),
        "args": args,
        "status": output.status.code(),
        "success": output.status.success(),
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
    }))
}
