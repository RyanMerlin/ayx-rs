use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Shared helper for owner-only local artifacts such as profiles, workspaces,
/// state, audit payloads, and observability logs.
///
/// On Unix, directories are tightened to `0o700` and files to `0o600`. On
/// Windows, the target and its containing sensitive directory receive a
/// protected DACL granting access only to the current user.
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
    let existed = path.exists();
    fs::create_dir_all(path).map_err(|source| SensitiveIoError::CreateDir {
        path: path.display().to_string(),
        source,
    })?;
    // Unix permissions are idempotent and must be re-applied even when the
    // caller supplied an already-existing temporary/config directory. Windows
    // uses a protected DACL; replacing an inherited ACL on a caller-owned
    // existing parent can make unrelated cleanup/fixture paths inaccessible,
    // so the Windows path tightens only directories created for this artifact.
    #[cfg(unix)]
    let should_tighten = true;
    #[cfg(not(unix))]
    let should_tighten = !existed;
    if should_tighten && let Err(source) = tighten_dir_permissions(path) {
        // A caller may intentionally place a sensitive file under a shared
        // existing directory such as `/tmp`. The file itself is still
        // created owner-only; failure to chmod a directory we do not own must
        // not make profile reads fail. Newly-created directories still fail
        // closed if tightening cannot complete.
        if !existed || source.kind() != std::io::ErrorKind::PermissionDenied {
            return Err(SensitiveIoError::CreateDir {
                path: path.display().to_string(),
                source,
            });
        }
    }
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
    let lock = SensitiveFileLock::acquire(path)?;
    lock.write(contents)
}

fn write_atomic_contents(path: &Path, contents: &[u8]) -> Result<(), SensitiveIoError> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent)?;
    }

    let tmp_path = sibling_with_suffix(path, ".tmp");
    // A crash can leave a stale temp file behind. Remove it while the stable
    // target lock is held, then use `create_new` below so a concurrent or
    // malicious replacement cannot turn the write into a symlink-following
    // overwrite of another file.
    match fs::remove_file(&tmp_path) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            });
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut tmp_file = OpenOptions::new()
            .create_new(true)
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
        drop(tmp_file);
        tighten_file_permissions(&tmp_path)
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
        #[cfg(windows)]
        let mut tmp_file =
            open_sensitive_temp_file(&tmp_path).map_err(|source| SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            })?;
        #[cfg(not(windows))]
        let mut tmp_file = OpenOptions::new()
            .create_new(true)
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
        // Windows keeps rename/delete sharing decisions on the open handle;
        // close the creation-time protected handle before applying the final
        // ACL and promoting the temp file.
        drop(tmp_file);
        tighten_file_permissions(&tmp_path)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
        fs::rename(&tmp_path, path)
            .map_err(|source| write_err_and_cleanup_tmp(&tmp_path, path, source))?;
    }

    Ok(())
}

/// Recover the readable state after a process crash during an atomic write.
///
/// A completed write is already visible at `path`; an unfinished sibling
/// `path.tmp` is never promoted because it has not passed the rename boundary.
/// Removing that stale temporary file makes recovery deterministic and avoids
/// leaving credential bytes at a predictable path. The next write recreates a
/// fresh temp file under the normal lock.
pub fn recover_sensitive_file(path: &Path) -> Result<(), SensitiveIoError> {
    let _lock = SensitiveFileLock::acquire(path)?;
    let tmp_path = sibling_with_suffix(path, ".tmp");
    match fs::remove_file(&tmp_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SensitiveIoError::Write {
            path: tmp_path.display().to_string(),
            source,
        }),
    }
}

/// A stable lock held across an entire sensitive-artifact transaction.
/// Atomic replacement changes the target inode, so the lock intentionally
/// lives beside the target and is never renamed.
pub struct SensitiveFileLock {
    path: PathBuf,
    lock_file: std::fs::File,
}

impl SensitiveFileLock {
    pub fn acquire(path: &Path) -> Result<Self, SensitiveIoError> {
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
        lock_file.lock().map_err(|source| SensitiveIoError::Lock {
            path: lock_path.display().to_string(),
            source,
        })?;
        tighten_file_permissions(&lock_path).map_err(|source| SensitiveIoError::Lock {
            path: lock_path.display().to_string(),
            source,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            lock_file,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, contents: &[u8]) -> Result<(), SensitiveIoError> {
        let _lock_guard = &self.lock_file;
        write_atomic_contents(&self.path, contents)
    }

    pub fn write_sibling(&self, suffix: &str, contents: &[u8]) -> Result<(), SensitiveIoError> {
        write_atomic_contents(&sibling_with_suffix(&self.path, suffix), contents)
    }

    pub fn read_sibling(&self, suffix: &str) -> Result<Option<Vec<u8>>, SensitiveIoError> {
        let path = sibling_with_suffix(&self.path, suffix);
        match fs::read(&path) {
            Ok(contents) => Ok(Some(contents)),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    pub fn remove_sibling(&self, suffix: &str) -> Result<(), SensitiveIoError> {
        let path = sibling_with_suffix(&self.path, suffix);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(SensitiveIoError::Write {
                path: path.display().to_string(),
                source,
            }),
        }
    }
}

impl std::fmt::Debug for SensitiveFileLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SensitiveFileLock")
            .field("path", &self.path)
            .finish_non_exhaustive()
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
            })?;
        tighten_file_permissions(path).map_err(|source| SensitiveIoError::Append {
            path: path.display().to_string(),
            source,
        })
    }
}

fn tighten_dir_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        tighten_windows_acl(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

fn tighten_file_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(windows)]
    {
        tighten_windows_acl(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(windows)]
fn open_sensitive_temp_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        BuildTrusteeWithSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SetEntriesInAclW, TRUSTEE_IS_SID,
        TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, InitializeSecurityDescriptor, NO_INHERITANCE, SECURITY_ATTRIBUTES,
        SECURITY_DESCRIPTOR, SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE,
    };
    use windows_sys::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = null_mut();
    let mut acl = null_mut();
    let result = unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            let mut size = 0u32;
            let _ = GetTokenInformation(token, TokenUser, null_mut(), 0, &mut size);
            if size == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                let mut buffer = vec![0u8; size as usize];
                if GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut size,
                ) == 0
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    let token_user = buffer.as_ptr().cast::<TOKEN_USER>();
                    let mut trustee = TRUSTEE_W::default();
                    BuildTrusteeWithSidW(&mut trustee, (*token_user).User.Sid);
                    trustee.TrusteeForm = TRUSTEE_IS_SID;
                    trustee.TrusteeType = TRUSTEE_IS_USER;
                    let access = EXPLICIT_ACCESS_W {
                        grfAccessPermissions: GENERIC_WRITE,
                        grfAccessMode: GRANT_ACCESS,
                        grfInheritance: NO_INHERITANCE,
                        Trustee: trustee,
                    };
                    let acl_status = SetEntriesInAclW(1, &access, null(), &mut acl);
                    if acl_status != 0 {
                        Err(std::io::Error::from_raw_os_error(acl_status as i32))
                    } else {
                        let mut descriptor = SECURITY_DESCRIPTOR::default();
                        let descriptor_initialized = InitializeSecurityDescriptor(
                            std::ptr::from_mut(&mut descriptor).cast(),
                            SECURITY_DESCRIPTOR_REVISION,
                        ) != 0;
                        let descriptor_ready = descriptor_initialized
                            && SetSecurityDescriptorDacl(
                                std::ptr::from_mut(&mut descriptor).cast(),
                                1,
                                acl,
                                0,
                            ) != 0;
                        if !descriptor_ready {
                            Err(std::io::Error::last_os_error())
                        } else {
                            let security_attributes = SECURITY_ATTRIBUTES {
                                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                                lpSecurityDescriptor: std::ptr::from_mut(&mut descriptor).cast(),
                                bInheritHandle: 0,
                            };
                            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
                            wide.push(0);
                            let handle = CreateFileW(
                                wide.as_ptr(),
                                GENERIC_WRITE,
                                FILE_SHARE_NONE,
                                &security_attributes,
                                CREATE_NEW,
                                FILE_ATTRIBUTE_NORMAL,
                                null_mut(),
                            );
                            if handle == INVALID_HANDLE_VALUE {
                                Err(std::io::Error::last_os_error())
                            } else {
                                // SAFETY: CreateFileW returned an owned handle;
                                // File takes responsibility for closing it.
                                Ok(std::fs::File::from_raw_handle(handle))
                            }
                        }
                    }
                }
            }
        }
    };
    if !acl.is_null() {
        unsafe {
            let _ = LocalFree(acl.cast());
        }
    }
    if !token.is_null() {
        unsafe {
            let _ = CloseHandle(token);
        }
    }
    result
}

#[cfg(windows)]
fn tighten_windows_acl(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        BuildTrusteeWithSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT, SetEntriesInAclW,
        SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS as FILE_ALL_ACCESS_FS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = null_mut();
    let result = unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            let mut size = 0u32;
            let _ = GetTokenInformation(token, TokenUser, null_mut(), 0, &mut size);
            if size == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                let mut buffer = vec![0u8; size as usize];
                if GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut size,
                ) == 0
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    let token_user = buffer.as_ptr().cast::<TOKEN_USER>();
                    let mut trustee = TRUSTEE_W::default();
                    BuildTrusteeWithSidW(&mut trustee, (*token_user).User.Sid);
                    trustee.TrusteeForm = TRUSTEE_IS_SID;
                    trustee.TrusteeType = TRUSTEE_IS_USER;
                    let access = EXPLICIT_ACCESS_W {
                        grfAccessPermissions: FILE_ALL_ACCESS_FS,
                        grfAccessMode: GRANT_ACCESS,
                        grfInheritance: NO_INHERITANCE,
                        Trustee: trustee,
                    };
                    let mut acl = null_mut();
                    let acl_status = SetEntriesInAclW(1, &access, null(), &mut acl);
                    if acl_status != 0 {
                        Err(std::io::Error::from_raw_os_error(acl_status as i32))
                    } else {
                        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
                        wide.push(0);
                        let status = SetNamedSecurityInfoW(
                            wide.as_ptr(),
                            SE_FILE_OBJECT,
                            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                            null_mut(),
                            null_mut(),
                            acl,
                            null_mut(),
                        );
                        let _ = LocalFree(acl.cast());
                        if status != 0 {
                            Err(std::io::Error::from_raw_os_error(status as i32))
                        } else {
                            Ok(())
                        }
                    }
                }
            }
        }
    };
    if !token.is_null() {
        unsafe {
            let _ = CloseHandle(token);
        }
    }
    result
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

    #[test]
    fn recovery_removes_crash_left_tmp_without_touching_committed_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        write_sensitive_file(&path, b"committed").expect("write");
        let stale_tmp = dir.path().join("profile.yaml.tmp");
        fs::write(&stale_tmp, b"partial-secret-write").expect("seed stale tmp");

        recover_sensitive_file(&path).expect("recover");

        assert_eq!(fs::read_to_string(&path).expect("read target"), "committed");
        assert!(!stale_tmp.exists());
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
