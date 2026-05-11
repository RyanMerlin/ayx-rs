use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("failed to create audit directory '{path}': {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize audit payload: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to write audit artifact '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub fn write_audit_artifact(
    audit_dir: &Path,
    operation_prefix: &str,
    payload: &Value,
) -> Result<PathBuf, AuditError> {
    let resolved = resolve_audit_dir(audit_dir);
    fs::create_dir_all(&resolved).map_err(|source| AuditError::CreateDir {
        path: resolved.display().to_string(),
        source,
    })?;

    // Tighten directory permissions on Unix to 0o700 so audit artifacts
    // (which may contain workspace ids, request bodies, and operation
    // metadata) are owner-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&resolved, fs::Permissions::from_mode(0o700));
    }

    let op_id = format!(
        "{}-{}",
        operation_prefix,
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    );
    let artifact_path = resolved.join(format!("{}.json", op_id));
    let content = serde_json::to_string_pretty(payload)?;

    fs::write(&artifact_path, &content).map_err(|source| AuditError::Write {
        path: artifact_path.display().to_string(),
        source,
    })?;

    // Per-file 0o600.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&artifact_path, fs::Permissions::from_mode(0o600));
    }

    Ok(artifact_path)
}

/// Summary of one `sweep_audit_dir` invocation.
#[derive(Debug, Clone, Default)]
pub struct SweepReport {
    pub examined: usize,
    pub removed: usize,
    pub bytes_freed: u64,
    pub removed_paths: Vec<PathBuf>,
}

/// Delete audit artifacts older than `retain_days` from `audit_dir`.
///
/// Idempotent. Honors the same audit-dir resolution rules as
/// [`write_audit_artifact`] (the default `audits` path redirects under
/// `${AYX_CONFIG_HOME}/audits/`). When `dry_run` is true, no files are
/// removed — the returned report describes what would be deleted.
///
/// Only files (not directories) are considered. The filter is age-based,
/// using mtime; we don't rely on filenames containing a timestamp.
pub fn sweep_audit_dir(
    audit_dir: &Path,
    retain_days: u32,
    dry_run: bool,
) -> Result<SweepReport, AuditError> {
    let resolved = resolve_audit_dir(audit_dir);
    if !resolved.exists() {
        return Ok(SweepReport::default());
    }
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(
            retain_days as u64 * 24 * 60 * 60,
        ))
        .unwrap_or(std::time::UNIX_EPOCH);

    let mut report = SweepReport::default();
    let entries = fs::read_dir(&resolved).map_err(|source| AuditError::CreateDir {
        path: resolved.display().to_string(),
        source,
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        report.examined += 1;
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if mtime < cutoff {
            let size = metadata.len();
            if !dry_run && fs::remove_file(&path).is_err() {
                continue;
            }
            report.removed += 1;
            report.bytes_freed += size;
            report.removed_paths.push(path);
        }
    }
    Ok(report)
}

/// Resolve the effective audit directory.
///
/// If the caller passed the default `audits` (a CWD-relative path that
/// pollutes whichever directory the user invokes the CLI from), redirect to
/// `${AYX_CONFIG_HOME}/audits` so artifacts land in a stable host-local
/// location. Any non-default path is honored verbatim.
pub fn resolve_audit_dir(audit_dir: &Path) -> PathBuf {
    if audit_dir == Path::new("audits") {
        if let Ok(home) = crate::profile::ayx_config_home() {
            return home.join("audits");
        }
    }
    audit_dir.to_path_buf()
}
