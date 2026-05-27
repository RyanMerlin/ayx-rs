use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use thiserror::Error;

/// Shared helper for owner-only local artifacts such as profiles, workspaces,
/// state, audit payloads, and observability logs.
///
/// On Unix, directories are tightened to `0o700` and files to `0o600`.
/// On Windows we rely on the platform's default ACL behavior and keep the
/// contract documented as best-effort unless a future Windows-native ACL layer
/// is introduced.
#[derive(Debug, Error)]
pub enum SensitiveIoError {
    #[error("failed to create sensitive directory '{path}': {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write sensitive file '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to append sensitive file '{path}': {source}")]
    Append {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub fn ensure_sensitive_dir(path: &Path) -> Result<(), SensitiveIoError> {
    fs::create_dir_all(path).map_err(|source| SensitiveIoError::CreateDir {
        path: path.display().to_string(),
        source,
    })?;
    tighten_dir_permissions(path);
    Ok(())
}

pub fn write_sensitive_file(path: &Path, contents: &[u8]) -> Result<(), SensitiveIoError> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        file.write_all(contents)
            .map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            }
        })?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents).map_err(|source| SensitiveIoError::Write {
            path: path.display().to_string(),
            source,
        })
    }
}

pub fn append_sensitive_file(path: &Path, contents: &[u8]) -> Result<(), SensitiveIoError> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| SensitiveIoError::Append {
                path: path.display().to_string(),
                source,
            })?;
        file.write_all(contents)
            .map_err(|source| SensitiveIoError::Append {
                path: path.display().to_string(),
                source,
            })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            SensitiveIoError::Append {
                path: path.display().to_string(),
                source,
            }
        })?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|source| SensitiveIoError::Append {
                path: path.display().to_string(),
                source,
            })?;
        file.write_all(contents)
            .map_err(|source| SensitiveIoError::Append {
                path: path.display().to_string(),
                source,
            })
    }
}

fn tighten_dir_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_append_sensitive_file_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("logs").join("api-events.jsonl");
        write_sensitive_file(&path, b"first\n").expect("write");
        append_sensitive_file(&path, b"second\n").expect("append");
        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text, "first\nsecond\n");
    }
}
