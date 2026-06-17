use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ayx_core::profile::Config;
use chrono::{DateTime, Utc};
use csv::ReaderBuilder;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct LogSource {
    pub kind: &'static str,
    pub name: &'static str,
    pub path: Option<PathBuf>,
    pub pattern: &'static str,
    pub notes: &'static str,
}

pub fn discover_log_sources(config: &Config) -> Vec<LogSource> {
    let runtime = config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.runtime_settings_path.as_ref());
    let runtime_dir = runtime
        .and_then(|p| Path::new(p).parent())
        .map(|p| p.to_path_buf())
        .or_else(|| {
            std::env::var("ProgramData")
                .ok()
                .map(|pd| PathBuf::from(pd).join("Alteryx"))
        });

    vec![
        LogSource {
            kind: "service",
            name: "Alteryx Service",
            path: runtime_dir.as_ref().map(|p| p.join("Service")),
            pattern: "AlteryxServiceLog.log",
            notes: "startup/shutdown and component communication",
        },
        LogSource {
            kind: "gallery",
            name: "Server / Gallery",
            path: runtime_dir.as_ref().map(|p| p.join("Gallery").join("Logs")),
            pattern: "alteryx-YYYY-MM-DD.csv",
            notes: "server processes, schedule migrations, analytic app errors",
        },
        LogSource {
            kind: "engine",
            name: "Engine",
            path: engine_log_dir(config),
            pattern: "Alteryx_Log_*.log",
            notes: "workflow execution and tool timestamps",
        },
        LogSource {
            kind: "ui_error",
            name: "UI Error Logs",
            path: runtime_dir.as_ref().map(|p| p.join("ErrorLogs")),
            pattern: "*.log",
            notes: "stack traces from UI failures",
        },
        LogSource {
            kind: "aas",
            name: "Authentication Service",
            path: runtime_dir.as_ref().map(|p| p.join("Logs")),
            pattern: "aas-log-YYYYMMDD.*",
            notes: "SAML / authentication service activity",
        },
        LogSource {
            kind: "config_changes",
            name: "Configuration Changes",
            path: runtime_dir
                .as_ref()
                .map(|p| p.parent().unwrap_or(p).to_path_buf()),
            pattern: "*log*",
            notes: "system settings change log stored near RuntimeSettings.xml",
        },
    ]
}

pub fn discover_log_inventory(config: &Config) -> Value {
    let sources = discover_log_sources(config);
    let mut arr = Vec::new();
    for src in sources {
        let mut files = Vec::new();
        let exists = src.path.as_ref().map(|p| p.exists()).unwrap_or(false);
        if let Some(path) = &src.path {
            if path.exists() && path.is_dir() {
                if let Ok(read_dir) = fs::read_dir(path) {
                    for entry in read_dir.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            files.push(p.display().to_string());
                        }
                    }
                }
                files.sort();
            } else if path.exists() {
                files.push(path.display().to_string());
            }
        }
        arr.push(json!({
            "kind": src.kind,
            "name": src.name,
            "path": src.path.as_ref().map(|p| p.display().to_string()),
            "exists": exists,
            "pattern": src.pattern,
            "notes": src.notes,
            "files": files,
        }));
    }
    json!({ "sources": arr })
}

pub fn summarize_log_file(path: &Path) -> Result<Value> {
    let (headers, rows) = read_log_rows(path)?;
    let header_lookup = header_index(&headers);
    let mut level_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut logger_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut top_messages = Vec::new();
    let mut first_error = None;
    let mut startup_line = None;

    for (idx, row) in rows.iter().enumerate() {
        let level = field(row, &header_lookup, "LogLevel").unwrap_or_default();
        if !level.is_empty() {
            *level_counts.entry(level.clone()).or_insert(0) += 1;
        }

        let logger = field(row, &header_lookup, "LoggerName").unwrap_or_default();
        if !logger.is_empty() {
            *logger_counts.entry(logger.clone()).or_insert(0) += 1;
        }

        let message = field(row, &header_lookup, "Message").unwrap_or_default();
        if !message.is_empty() && top_messages.len() < 10 {
            top_messages.push(json!({
                "line": idx + 1,
                "level": level,
                "logger": logger,
                "message": message,
            }));
        }

        let message_lower = message.to_ascii_lowercase();
        if first_error.is_none()
            && (level.eq_ignore_ascii_case("error")
                || level.eq_ignore_ascii_case("critical")
                || level.eq_ignore_ascii_case("alert")
                || message_lower.contains("error")
                || message_lower.contains("exception"))
        {
            first_error = Some(idx + 1);
        }

        if startup_line.is_none()
            && (message_lower.contains("starting version")
                || message_lower.contains("service starting")
                || message_lower.contains("alteryxservice starting"))
        {
            startup_line = Some(idx + 1);
        }
    }

    Ok(json!({
        "path": path.display().to_string(),
        "line_count": rows.len(),
        "headers": headers,
        "level_counts": level_counts,
        "logger_counts": logger_counts,
        "first_error_line": first_error,
        "startup_marker_line": startup_line,
        "sample_messages": top_messages,
    }))
}

pub fn parse_service_events(path: &Path) -> Result<Value> {
    let (headers, rows) = read_log_rows(path)?;
    let header_lookup = header_index(&headers);
    let mut events = Vec::new();

    for row in rows {
        let message = field(&row, &header_lookup, "Message").unwrap_or_default();
        let level = field(&row, &header_lookup, "LogLevel").unwrap_or_default();
        let logger = field(&row, &header_lookup, "LoggerName").unwrap_or_default();
        let date = field(&row, &header_lookup, "Date").unwrap_or_default();
        if message.is_empty() {
            continue;
        }

        let mut event = json!({
            "date": date,
            "level": level,
            "logger": logger,
            "message": message,
        });

        if let Some(parsed) =
            parse_setting_root_message(event.get("message").and_then(Value::as_str).unwrap_or(""))
        {
            event["kind"] = json!("config_setting");
            event["setting"] = parsed;
        } else if let Some(parsed) =
            parse_startup_message(event.get("message").and_then(Value::as_str).unwrap_or(""))
        {
            event["kind"] = json!("startup");
            event["startup"] = parsed;
        } else if let Some(parsed) =
            parse_request_message(event.get("message").and_then(Value::as_str).unwrap_or(""))
        {
            event["kind"] = json!("request");
            event["request"] = parsed;
        } else {
            event["kind"] = json!("message");
        }

        events.push(event);
    }

    let startup_events = events
        .iter()
        .filter(|e| e.get("kind").and_then(Value::as_str) == Some("startup"))
        .count();
    let request_events = events
        .iter()
        .filter(|e| e.get("kind").and_then(Value::as_str) == Some("request"))
        .count();
    let setting_events = events
        .iter()
        .filter(|e| e.get("kind").and_then(Value::as_str) == Some("config_setting"))
        .count();

    Ok(json!({
        "path": path.display().to_string(),
        "headers": headers,
        "event_counts": {
            "startup": startup_events,
            "request": request_events,
            "config_setting": setting_events,
            "total": events.len(),
        },
        "events": events.into_iter().take(250).collect::<Vec<_>>(),
    }))
}

pub fn parse_gallery_events(path: &Path) -> Result<Value> {
    let (headers, rows) = read_log_rows(path)?;
    let header_lookup = header_index(&headers);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut requests = Vec::new();

    for row in rows {
        let message = field(&row, &header_lookup, "Message").unwrap_or_default();
        let level = field(&row, &header_lookup, "LogLevel").unwrap_or_default();
        let logger = field(&row, &header_lookup, "LoggerName").unwrap_or_default();
        if message.is_empty() {
            continue;
        }

        let kind = if message.to_ascii_lowercase().contains(" started at http")
            || message
                .to_ascii_lowercase()
                .contains("starting cloud services")
        {
            "startup"
        } else if message.to_ascii_lowercase().contains("unauthenticated")
            || message.to_ascii_lowercase().contains("responsecode")
            || message.to_ascii_lowercase().contains("/gallery/api/")
        {
            "request"
        } else if message.to_ascii_lowercase().contains("exception")
            || message.to_ascii_lowercase().contains("error")
        {
            "error"
        } else {
            "message"
        };

        *counts.entry(kind.to_string()).or_insert(0) += 1;
        if requests.len() < 250 && kind != "message" {
            let mut event = json!({
                "kind": kind,
                "level": level,
                "logger": logger,
                "message": message,
            });
            if kind == "request"
                && let Some(parsed) = parse_gallery_request_message(
                    event.get("message").and_then(Value::as_str).unwrap_or(""),
                )
            {
                event["request"] = parsed;
            }
            requests.push(event);
        }
    }

    Ok(json!({
        "path": path.display().to_string(),
        "headers": headers,
        "event_counts": counts,
        "events": requests,
    }))
}

pub fn extract_context(path: &Path, needle: &str, before: usize, after: usize) -> Result<Value> {
    let text = read_log_text(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let mut matches = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
        {
            let start = idx.saturating_sub(before);
            let end = (idx + after + 1).min(lines.len());
            matches.push(json!({
                "match_line": idx + 1,
                "needle": needle,
                "context": lines[start..end].iter().enumerate().map(|(i, l)| json!({
                    "line": start + i + 1,
                    "text": *l,
                })).collect::<Vec<_>>(),
            }));
        }
    }
    Ok(json!({ "path": path.display().to_string(), "needle": needle, "matches": matches }))
}

pub fn tail_log_file(path: &Path, lines_to_show: usize) -> Result<Value> {
    let text = read_log_text(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(lines_to_show);
    Ok(json!({
        "path": path.display().to_string(),
        "line_count": lines.len(),
        "tail": lines[start..].iter().enumerate().map(|(i, l)| json!({
            "line": start + i + 1,
            "text": *l,
        })).collect::<Vec<_>>()
    }))
}

pub fn parse_gallery_csv(path: &Path) -> Result<Value> {
    let (header, rows) = read_log_rows(path)?;
    let header_lookup = header_index(&header);
    let mut parsed = Vec::new();
    for row in rows.into_iter().take(500) {
        let mut obj = serde_json::Map::new();
        for key in &header {
            if let Some(val) = field(&row, &header_lookup, key) {
                obj.insert(key.clone(), json!(val));
            }
        }
        parsed.push(Value::Object(obj));
    }
    Ok(json!({ "path": path.display().to_string(), "header": header, "rows": parsed }))
}

pub fn recent_log_candidates(config: &Config, days: i64) -> Value {
    let sources = discover_log_sources(config);
    let cutoff = Utc::now() - chrono::Duration::days(days);
    let mut result = Vec::new();
    for src in sources {
        if let Some(path) = src.path
            && path.exists()
            && path.is_dir()
            && let Ok(read_dir) = fs::read_dir(&path)
        {
            for entry in read_dir.flatten() {
                let p = entry.path();
                if let Ok(meta) = entry.metadata()
                    && let Ok(modified) = meta.modified()
                {
                    let modified_dt: DateTime<Utc> = modified.into();
                    if modified_dt >= cutoff {
                        result.push(json!({
                            "kind": src.kind,
                            "path": p.display().to_string(),
                            "modified_utc": modified_dt,
                        }));
                    }
                }
            }
        }
    }
    json!({ "cutoff_utc": cutoff, "matches": result })
}

fn engine_log_dir(config: &Config) -> Option<PathBuf> {
    config
        .mongo
        .embedded
        .as_ref()
        .and_then(|e| e.runtime_settings_path.as_ref())
        .and_then(|p| Path::new(p).parent().map(|dir| dir.join("Engine")))
}

fn read_log_text(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read log file '{}'", path.display()))?;
    Ok(decode_log_bytes(&bytes))
}

fn read_log_rows(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read log file '{}'", path.display()))?;
    let text = decode_log_bytes(&bytes);
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map(|h| h.iter().map(normalize_field).collect::<Vec<_>>())
        .context("failed to read log headers")?;
    let mut rows = Vec::new();
    for record in reader.records().flatten() {
        rows.push(record.iter().map(normalize_field).collect::<Vec<_>>());
    }
    Ok((headers, rows))
}

fn decode_log_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16_le(&bytes[2..]);
    }

    if bytes.contains(&0) {
        return decode_utf16_le(bytes);
    }

    String::from_utf8_lossy(bytes).to_string()
}

fn decode_utf16_le(bytes: &[u8]) -> String {
    let usable = bytes.len() & !1;
    let units: Vec<u16> = bytes[..usable]
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn normalize_field(value: &str) -> String {
    value.trim().trim_matches('\u{feff}').to_string()
}

fn header_index(headers: &[String]) -> BTreeMap<String, usize> {
    headers
        .iter()
        .enumerate()
        .map(|(idx, key)| (key.to_ascii_lowercase(), idx))
        .collect()
}

fn field(row: &[String], headers: &BTreeMap<String, usize>, key: &str) -> Option<String> {
    headers
        .get(&key.to_ascii_lowercase())
        .and_then(|idx| row.get(*idx))
        .cloned()
        .filter(|s| !s.is_empty())
}

fn parse_setting_root_message(message: &str) -> Option<Value> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("setting root <") || !lower.contains("name <") {
        return None;
    }
    Some(json!({
        "raw": message,
        "scope": extract_between(message, "setting root <", ">"),
        "name": extract_between(message, "name <", ">"),
        "value": extract_between(message, "value <", ">"),
    }))
}

fn parse_startup_message(message: &str) -> Option<Value> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("starting version") && !lower.contains("was started at") {
        return None;
    }
    let version = extract_between(message, "version <", ">");
    let command = extract_between(message, "command <", ">");
    let endpoint = message
        .split(" at ")
        .last()
        .map(|s| s.trim().trim_end_matches(',').to_string())
        .filter(|s| !s.is_empty());
    Some(json!({
        "raw": message,
        "version": version,
        "command": command,
        "endpoint": endpoint,
    }))
}

fn parse_request_message(message: &str) -> Option<Value> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("/gallery/api/")
        && !lower.contains("requestcode")
        && !lower.contains("responsecode")
    {
        return None;
    }
    Some(json!({
        "raw": message,
        "method": extract_http_method(message),
        "target": extract_api_target(message),
        "response_code": extract_between(message, "ResponseCode <", ">"),
        "response_time": extract_between(message, "ResponseTime <", ">"),
        "user_id": extract_between(message, "UserId <", ">"),
        "request_id": extract_between(message, "RequestId <", ">"),
        "client_ip": extract_between(message, "ClientIP <", ">"),
    }))
}

fn parse_gallery_request_message(message: &str) -> Option<Value> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("/gallery/api/") {
        return None;
    }
    Some(json!({
        "raw": message,
        "method": extract_http_method(message),
        "target": extract_api_target(message),
        "status": extract_between(message, "ResponseCode <", ">"),
    }))
}

fn extract_http_method(message: &str) -> Option<String> {
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        if message.contains(&format!(" {method} "))
            || message.contains(&format!("Method <{method}>"))
        {
            return Some(method.to_string());
        }
    }
    None
}

fn extract_api_target(message: &str) -> Option<String> {
    if let Some(idx) = message.find("/gallery/api/") {
        return Some(message[idx..].trim().trim_end_matches(',').to_string());
    }
    extract_between(message, "RequestTarget <", ">")
}

fn extract_between(message: &str, start: &str, end: &str) -> Option<String> {
    let idx = message.find(start)? + start.len();
    let rest = &message[idx..];
    let end_idx = rest.find(end)?;
    let val = rest[..end_idx].trim();
    (!val.is_empty()).then(|| val.to_string())
}
