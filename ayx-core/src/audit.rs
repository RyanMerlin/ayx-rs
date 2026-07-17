use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::Value;
use thiserror::Error;

use crate::sensitive::write_sensitive_file;

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

/// Maps any `SensitiveIoError` (dir-creation, lock, write, or append) onto
/// the two-variant `AuditError` surface. Shared by every write path in this
/// module so `write_audit_artifact`, `create_sensitive_audit_artifact`, and
/// `update_sensitive_audit_artifact` all report I/O failures identically.
fn map_sensitive_err(err: crate::sensitive::SensitiveIoError) -> AuditError {
    match err {
        crate::sensitive::SensitiveIoError::CreateDir { path, source } => {
            AuditError::CreateDir { path, source }
        }
        crate::sensitive::SensitiveIoError::Lock { path, source }
        | crate::sensitive::SensitiveIoError::Write { path, source }
        | crate::sensitive::SensitiveIoError::Append { path, source } => {
            AuditError::Write { path, source }
        }
    }
}

/// Shared low-level primitive: ensure `dir` exists as an owner-only sensitive
/// directory, serialize `payload` as pretty JSON, and write it atomically to
/// `path`. `dir` and `path` are passed separately because a bare-path update
/// (`update_sensitive_audit_artifact`, called from a different process than
/// the one that created the artifact) only has `path` to work from and must
/// re-derive its parent directory rather than assume the resolved audit dir
/// is still on hand.
fn write_artifact_json(dir: &Path, path: &Path, payload: &Value) -> Result<(), AuditError> {
    crate::sensitive::ensure_sensitive_dir(dir).map_err(map_sensitive_err)?;
    let content = serde_json::to_string_pretty(payload)?;
    write_sensitive_file(path, content.as_bytes()).map_err(map_sensitive_err)?;
    Ok(())
}

pub fn write_audit_artifact(
    audit_dir: &Path,
    operation_prefix: &str,
    payload: &Value,
) -> Result<PathBuf, AuditError> {
    let resolved = resolve_audit_dir(audit_dir);
    let op_id = format!(
        "{}-{}",
        operation_prefix,
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    );
    let artifact_path = resolved.join(format!("{}.json", op_id));
    write_artifact_json(&resolved, &artifact_path, payload)?;
    Ok(artifact_path)
}

/// Handle to a lifecycle-managed audit artifact: its generated operation id
/// (stable for the artifact's whole prepared → terminal lifetime) and the
/// single on-disk path both `create_sensitive_audit_artifact` and every
/// subsequent `update_sensitive_audit_artifact` call write to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditArtifactHandle {
    pub operation_id: String,
    pub path: PathBuf,
}

/// Four random bytes (8 hex chars) of OS entropy. Not cryptographic secrecy
/// -- just enough uniqueness that two concurrent `create_sensitive_audit_artifact`
/// calls in the same millisecond (same `operation_prefix`, same directory)
/// can never collide on the same generated path.
fn random_suffix() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)
        .expect("OS entropy source unavailable — cannot generate audit operation id");
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
}

fn new_operation_id(operation_prefix: &str) -> String {
    format!(
        "{}-{}-{}",
        operation_prefix,
        Utc::now().format("%Y%m%dT%H%M%S%.6fZ"),
        random_suffix()
    )
}

/// Create one uniquely-named lifecycle audit artifact and return the handle
/// future `update_sensitive_audit_artifact` calls must reuse to overwrite
/// the *same* path (Task 4 Step 1). Unlike `write_audit_artifact` (one-shot,
/// backup/restore), this is the first write in a create → update → update
/// (…) → terminal sequence: e.g. a mutation execution artifact is created
/// with `status: "prepared"` before `mongosh` starts, then updated in place
/// to its terminal status afterward.
pub fn create_sensitive_audit_artifact(
    audit_dir: &Path,
    operation_prefix: &str,
    payload: &Value,
) -> Result<AuditArtifactHandle, AuditError> {
    let resolved = resolve_audit_dir(audit_dir);
    let operation_id = new_operation_id(operation_prefix);
    let path = resolved.join(format!("{operation_id}.json"));
    write_artifact_json(&resolved, &path, payload)?;
    Ok(AuditArtifactHandle { operation_id, path })
}

/// Atomically overwrite an existing lifecycle audit artifact at `path` with
/// a new payload (Task 4 Step 1) -- e.g. replacing `status: "prepared"` with
/// a terminal `applied`/`aborted`/`failed`/`failed_or_unknown` status.
///
/// Takes a bare `&Path` rather than an `&AuditArtifactHandle` so a *later*,
/// independent invocation (guarded `mongo undo` backlinking the undo
/// artifact's path into the original mutation's already-finalized artifact)
/// can update it too -- that caller only ever has the recorded path, never
/// the original process's in-memory handle.
pub fn update_sensitive_audit_artifact(path: &Path, payload: &Value) -> Result<(), AuditError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    write_artifact_json(dir, path, payload)
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
    if audit_dir == Path::new("audits")
        && let Ok(home) = crate::profile::ayx_config_home()
    {
        return home.join("audits");
    }
    audit_dir.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Lifecycle helper: create_sensitive_audit_artifact /
    // update_sensitive_audit_artifact (Task 4 Step 1) ──────────────────────

    #[test]
    fn create_sensitive_audit_artifact_writes_owner_only_and_returns_handle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let payload = json!({"status": "prepared"});
        let handle = create_sensitive_audit_artifact(dir.path(), "mongo-mutate-execute", &payload)
            .expect("create should succeed");

        assert!(handle.path.exists());
        assert!(
            handle.operation_id.starts_with("mongo-mutate-execute-"),
            "operation_id should be prefixed: {}",
            handle.operation_id
        );
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&handle.path).expect("read")).expect("json");
        assert_eq!(on_disk, payload);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&handle.path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "artifact file must be owner-only");
            let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "audit dir must be owner-only");
        }
    }

    #[test]
    fn concurrent_create_calls_never_collide_on_the_same_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut paths = std::collections::HashSet::new();
        for _ in 0..25 {
            let handle =
                create_sensitive_audit_artifact(dir.path(), "mongo-mutate-execute", &json!({}))
                    .expect("create should succeed");
            assert!(
                paths.insert(handle.path.clone()),
                "duplicate path generated: {}",
                handle.path.display()
            );
        }
    }

    #[test]
    fn update_sensitive_audit_artifact_replaces_the_same_path_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let handle = create_sensitive_audit_artifact(
            dir.path(),
            "mongo-mutate-execute",
            &json!({"status": "prepared"}),
        )
        .expect("create should succeed");

        update_sensitive_audit_artifact(&handle.path, &json!({"status": "applied"}))
            .expect("update should succeed");

        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "update must replace the same file, not create a second one"
        );

        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&handle.path).expect("read")).expect("json");
        assert_eq!(on_disk, json!({"status": "applied"}));
    }

    #[test]
    fn update_sensitive_audit_artifact_works_by_bare_path_across_handles() {
        // The undo backlink case: a later process only has the recorded
        // artifact *path*, not the original `AuditArtifactHandle`.
        let dir = tempfile::tempdir().expect("tempdir");
        let handle = create_sensitive_audit_artifact(
            dir.path(),
            "mongo-mutate-execute",
            &json!({"undo_artifact": null}),
        )
        .expect("create should succeed");
        let path_only = handle.path.clone();
        drop(handle);

        update_sensitive_audit_artifact(&path_only, &json!({"undo_artifact": "/tmp/undo.json"}))
            .expect("update by bare path should succeed");
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&path_only).expect("read")).expect("json");
        assert_eq!(on_disk["undo_artifact"], "/tmp/undo.json");
    }

    #[test]
    fn write_audit_artifact_behavior_is_unchanged_for_backup_restore_callers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_audit_artifact(dir.path(), "mongo-backup", &json!({"applied": true}))
            .expect("write should succeed");
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mongo-backup-"),
            "backup/restore filename prefix must be unchanged"
        );
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(on_disk, json!({"applied": true}));
    }
}
