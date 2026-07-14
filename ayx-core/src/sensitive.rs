use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

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
    #[error("failed to lock sensitive file '{path}': {source}")]
    Lock {
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

/// Appends `suffix` to the target's full file name (not its extension), so
/// `profile.yaml` yields a sibling at `profile.yaml.lock` / `profile.yaml.tmp`
/// rather than replacing the extension.
fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut file_name = path.file_name().unwrap_or_default().to_os_string();
    file_name.push(suffix);
    path.with_file_name(file_name)
}

/// Maps an I/O error from writing, syncing, or renaming the temp file into
/// `SensitiveIoError::Write`, and best-effort removes the temp file first --
/// so a write/sync/rename failure partway through doesn't leave partial
/// credential bytes sitting at the predictable `<target>.tmp` path until the
/// next successful write happens to overwrite it. Not used for the initial
/// `open()` of the temp file: if that fails, no temp file was created.
fn write_err_and_cleanup_tmp(
    tmp_path: &Path,
    path: &Path,
    source: std::io::Error,
) -> SensitiveIoError {
    let _ = fs::remove_file(tmp_path);
    SensitiveIoError::Write {
        path: path.display().to_string(),
        source,
    }
}

/// Writes `contents` to `path` atomically.
///
/// The write lands in a temp file in the same directory, which is fsync'd
/// and then renamed over the target -- a crash mid-write can therefore never
/// leave `path` truncated or partially written (R1). An advisory exclusive
/// lock, held on a stable sibling path (`<path>.lock`) for the duration of
/// the write, serializes concurrent writers so two racing calls can't tear
/// the file (R2).
///
/// The lock is deliberately taken on `<path>.lock`, never on `path` itself:
/// once this function renames the temp file over `path`, a lock held on the
/// old inode would no longer protect the new one (locks don't follow
/// renames), reopening the exact race this function exists to close. A
/// dedicated, never-renamed lock path avoids that hazard entirely -- the
/// same pattern git, cargo, and rustup use for their own lock files.
pub fn write_sensitive_file(path: &Path, contents: &[u8]) -> Result<(), SensitiveIoError> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent)?;
    }

    let lock_path = sibling_with_suffix(path, ".lock");
    #[cfg(unix)]
    let lock_file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .create(true)
            .write(true)
            // The lock file's contents are never read or written -- only its
            // existence and lockability matter -- so an existing lock file
            // is reused as-is rather than truncated on every call.
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|source| SensitiveIoError::Lock {
                path: lock_path.display().to_string(),
                source,
            })?
    };
    #[cfg(not(unix))]
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| SensitiveIoError::Lock {
            path: lock_path.display().to_string(),
            source,
        })?;
    // `std::fs::File::lock()` (stable since Rust 1.89; this workspace has no
    // MSRV pin holding it below that) is a blocking, cross-platform
    // exclusive advisory lock -- flock(2) on Unix, LockFileEx on Windows.
    lock_file.lock().map_err(|source| SensitiveIoError::Lock {
        path: lock_path.display().to_string(),
        source,
    })?;

    let tmp_path = sibling_with_suffix(path, ".tmp");

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut tmp_file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&tmp_path)
            .map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        tmp_file
            .write_all(contents)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        // Durability before visibility: the new content must hit disk
        // before the rename makes it visible at `path`.
        tmp_file
            .sync_all()
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        fs::rename(&tmp_path, path)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        // Linux requires an explicit fsync of the parent directory for a
        // rename to be crash-durable: the directory-entry update has its own
        // dirty state, independent of the renamed file's own fsync above.
        if let Some(parent) = path.parent() {
            let dir = std::fs::File::open(parent).map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
            dir.sync_all().map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    #[cfg(not(unix))]
    {
        let mut tmp_file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp_path)
            .map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        tmp_file
            .write_all(contents)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        tmp_file
            .sync_all()
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        fs::rename(&tmp_path, path)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
    }

    // Release the lock only after the rename (and, on Unix, the
    // parent-directory fsync) above has completed -- dropping the handle
    // closes the fd, which releases the OS-level advisory lock.
    drop(lock_file);
    Ok(())
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

    #[test]
    fn write_sensitive_file_leaves_only_target_and_lock_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");

        write_sensitive_file(&path, b"contents").expect("write");

        let mut names: Vec<String> = fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();

        // The target file and its stable `.lock` sibling are expected to
        // remain; no `.tmp` file should survive a successful write.
        assert_eq!(names, vec!["profile.yaml", "profile.yaml.lock"]);
    }

    #[test]
    fn write_sensitive_file_overwrites_a_stale_tmp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        let stale_tmp = dir.path().join("profile.yaml.tmp");

        ensure_sensitive_dir(dir.path()).expect("ensure dir");
        fs::write(&stale_tmp, b"garbage-left-by-a-crashed-run").expect("seed stale tmp");

        write_sensitive_file(&path, b"fresh-content").expect("write");

        let text = fs::read_to_string(&path).expect("read target");
        assert_eq!(text, "fresh-content");
    }

    /// Builds a self-describing record whose total length varies by
    /// `(thread_id, iter)`, ranging from 20,000 to 100,000 bytes. Varying
    /// the length (rather than using one fixed length for every record) is
    /// what makes a torn write detectable: against the old in-place
    /// `truncate` + `write` code, a *shorter* write landing after a
    /// *longer* one only overwrites the leading bytes of the file, leaving
    /// the longer write's trailing bytes behind as garbage -- exactly the
    /// failure this test must catch. Two same-length writes racing would
    /// just silently overwrite each other, which is a lost update but not
    /// a *torn* file, so it would not exercise this failure mode.
    ///
    /// The size (tens of KB, not bytes) is also load-bearing: it was tuned
    /// empirically against a standalone copy of the old implementation --
    /// short (sub-1KB) fixed-ish-size records essentially never raced
    /// visibly on this filesystem (every `write(2)` completed as a single
    /// atomic syscall), while records in this range measurably raise the
    /// chance that concurrent writers' open+truncate+write sequences
    /// interleave and reproduce R2's torn file (see
    /// `concurrent_writes_never_tear_the_file`'s doc comment for the actual
    /// measured hit rate -- it is real but modest, not "every time").
    fn make_record(thread_id: usize, iter: usize) -> Vec<u8> {
        let header = format!("T{thread_id:02}-I{iter:04}-");
        let len = record_len(thread_id, iter);
        let fill = fill_byte(thread_id);
        let mut record = header.into_bytes();
        record.resize(len, fill);
        record
    }

    fn record_len(thread_id: usize, iter: usize) -> usize {
        20_000 + (thread_id * 3_701 + iter * 727) % 80_000
    }

    fn fill_byte(thread_id: usize) -> u8 {
        b'a' + (thread_id as u8 % 26)
    }

    /// Returns `Some((thread_id, iter))` iff `bytes` is *exactly* one
    /// well-formed record as produced by `make_record`: correct header,
    /// correct length for that header's `(thread_id, iter)`, and every
    /// trailing byte equal to that thread's fill byte. Anything torn,
    /// truncated, or mixed with another record's bytes returns `None`.
    fn parse_record(bytes: &[u8]) -> Option<(usize, usize)> {
        const HEADER_LEN: usize = 10; // "T{:02}-I{:04}-"
        if bytes.len() < HEADER_LEN {
            return None;
        }
        let text = std::str::from_utf8(&bytes[..HEADER_LEN]).ok()?;
        let mut chars = text.chars();
        if chars.next()? != 'T' {
            return None;
        }
        let thread_id: usize = text.get(1..3)?.parse().ok()?;
        if text.get(3..5)? != "-I" {
            return None;
        }
        let iter: usize = text.get(5..9)?.parse().ok()?;
        if text.get(9..10)? != "-" {
            return None;
        }

        if bytes.len() != record_len(thread_id, iter) {
            return None;
        }
        let fill = fill_byte(thread_id);
        if bytes[HEADER_LEN..].iter().all(|&b| b == fill) {
            Some((thread_id, iter))
        } else {
            None
        }
    }

    #[test]
    fn parse_record_round_trips_through_make_record() {
        for thread_id in 0..8 {
            for iter in [0usize, 1, 49] {
                let record = make_record(thread_id, iter);
                assert_eq!(parse_record(&record), Some((thread_id, iter)));
            }
        }
    }

    #[test]
    fn parse_record_rejects_a_torn_mix_of_two_records() {
        let a = make_record(0, 0);
        let b = make_record(1, 0);
        let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        assert!(
            short.len() < long.len(),
            "test setup expects distinct lengths"
        );
        // Simulate exactly the torn output this test suite is guarding
        // against: a shorter write's bytes followed by a longer write's
        // leftover tail.
        let mut torn = short.clone();
        torn.extend_from_slice(&long[short.len()..]);
        assert_eq!(parse_record(&torn), None);
    }

    /// Runs one round of the concurrency scenario: 8 threads each call
    /// `write_sensitive_file` on the *same*, fresh path 50 times with
    /// distinct, varying-length payloads. Returns the final file's bytes
    /// once every thread has finished.
    fn run_concurrent_write_round() -> Vec<u8> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = std::sync::Arc::new(dir.path().join("profile.yaml"));

        let handles: Vec<_> = (0..8usize)
            .map(|thread_id| {
                let path = std::sync::Arc::clone(&path);
                std::thread::spawn(move || {
                    for iter in 0..50usize {
                        let record = make_record(thread_id, iter);
                        write_sensitive_file(&path, &record).expect("write");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("writer thread panicked");
        }

        fs::read(&*path).expect("read final file")
    }

    /// Direct test of R2. Against the old in-place `truncate` + `write`
    /// implementation, any single round of `run_concurrent_write_round` has
    /// only a modest chance of reproducing a torn file -- see
    /// `make_record`'s doc comment for why varying record lengths are what
    /// exposes the tear at all -- because it depends on incidental OS
    /// thread scheduling that a debug build running under `cargo nextest`
    /// only occasionally hits. Empirically (repeated full runs of this exact
    /// test against the pre-fix code, via `cargo nextest run -p ayx-core
    /// sensitive::tests::concurrent_writes_never_tear_the_file`), the full
    /// 60-round test caught a genuine tear in roughly 1 in ~19 invocations
    /// (2 catches across 38 runs), e.g. "round 11: final file is not
    /// exactly one well-formed record (81530 bytes, ...)" and "round 44:
    /// ... (70427 bytes, ...)". Running 60 rounds per test (rather than one)
    /// meaningfully improves on a single round's near-zero per-round hit
    /// rate while keeping this test under ~4s; it is not a guarantee of
    /// catching a reintroduced regression on every run. Against the
    /// lock-protected atomic-rename implementation every call is fully
    /// serialized, so every one of the 60 rounds always produces exactly
    /// one complete, well-formed record -- there is no raciness left to
    /// miss, so this is not a source of flakiness post-fix (confirmed
    /// deterministic across 10+ repeated full-suite runs during
    /// development).
    #[test]
    fn concurrent_writes_never_tear_the_file() {
        for round in 0..60 {
            let bytes = run_concurrent_write_round();
            assert!(
                parse_record(&bytes).is_some(),
                "round {round}: final file is not exactly one well-formed record \
                 ({} bytes, first 32: {:?})",
                bytes.len(),
                String::from_utf8_lossy(&bytes[..bytes.len().min(32)]),
            );
        }
    }
}
