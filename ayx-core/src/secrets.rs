#[cfg(feature = "test-inline-forcing")]
use std::cell::Cell;
use std::cell::RefCell;
use std::env;
use std::fs;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(any(test, feature = "test-inline-forcing"))]
use std::sync::Once;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use keyring_core::{Entry, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::auth::{CredentialBinding, SecretStoreAvailability, SecureStorage};
use crate::profile::ProfileError;
use crate::sensitive::SensitiveFileLock;

const SECRET_SERVICE: &str = "ayx";
const KEYRING_TRANSACTION_VERSION: u16 = 2;
const KEYRING_TRANSACTION_SUFFIX: &str = ".auth-txn";
const KEYRING_TRANSACTION_TMP_SUFFIX: &str = ".auth-txn.tmp";

#[cfg(feature = "test-inline-forcing")]
thread_local! {
    static FORCE_KEYRING_UNAVAILABLE_FOR_CURRENT_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Forces keyring writes to fail on the current thread until the returned guard
/// is dropped. This test-only seam avoids mutating process-global environment
/// variables while Rust unit tests run concurrently.
#[cfg(feature = "test-inline-forcing")]
pub struct ForcedKeyringUnavailable {
    previous: bool,
}

#[cfg(feature = "test-inline-forcing")]
impl ForcedKeyringUnavailable {
    pub fn for_current_thread() -> Self {
        let previous = FORCE_KEYRING_UNAVAILABLE_FOR_CURRENT_THREAD.with(|forced| {
            let previous = forced.get();
            forced.set(true);
            previous
        });
        Self { previous }
    }
}

#[cfg(feature = "test-inline-forcing")]
impl Drop for ForcedKeyringUnavailable {
    fn drop(&mut self) {
        FORCE_KEYRING_UNAVAILABLE_FOR_CURRENT_THREAD.with(|forced| forced.set(self.previous));
    }
}

#[cfg(feature = "test-inline-forcing")]
fn keyring_unavailable_is_forced() -> bool {
    FORCE_KEYRING_UNAVAILABLE_FOR_CURRENT_THREAD.with(Cell::get)
        || env_truthy("AYX_FORCE_INLINE_SECRETS")
}

#[derive(Clone, Serialize, Deserialize)]
struct KeyringTransactionChange {
    account: String,
    backup_account: Option<String>,
    backup_ready: bool,
}

#[derive(Serialize, Deserialize)]
struct KeyringTransactionJournal {
    version: u16,
    transaction_id: String,
    target_digest: Option<String>,
    #[serde(default)]
    rollback_restored: bool,
    changes: Vec<KeyringTransactionChange>,
}

#[derive(Deserialize)]
struct LegacyKeyringTransactionChange {
    account: String,
    previous: Option<String>,
}

#[derive(Deserialize)]
struct LegacyKeyringTransactionJournal {
    target_digest: Option<String>,
    changes: Vec<LegacyKeyringTransactionChange>,
}

/// Coordinates keyring mutations with the profile's atomic file replacement.
/// The journal is written before the first keyring mutation. If the process
/// dies before the profile reaches its target digest, startup restores the
/// recorded pre-images from temporary keyring entries; if the digest is
/// present, the profile commit won and the journal/backup entries are cleared.
pub struct KeyringTransaction<'a> {
    lock: &'a SensitiveFileLock,
    journal: RefCell<KeyringTransactionJournal>,
}

impl<'a> KeyringTransaction<'a> {
    pub fn begin(lock: &'a SensitiveFileLock) -> Self {
        Self {
            lock,
            journal: RefCell::new(KeyringTransactionJournal {
                version: KEYRING_TRANSACTION_VERSION,
                transaction_id: transaction_id(lock.path()),
                target_digest: None,
                rollback_restored: false,
                changes: Vec::new(),
            }),
        }
    }

    pub fn record_change(
        &self,
        account: &str,
        previous: Option<String>,
    ) -> Result<(), ProfileError> {
        let mut journal = self.journal.borrow_mut();
        if journal
            .changes
            .iter()
            .any(|change| change.account == account)
        {
            return Ok(());
        }
        let backup_account = previous
            .as_deref()
            .map(|_| backup_account(&journal.transaction_id, account));
        journal.changes.push(KeyringTransactionChange {
            account: account.to_string(),
            backup_account: backup_account.clone(),
            backup_ready: backup_account.is_none(),
        });
        persist_transaction_journal(self.lock, &journal)?;
        drop(journal);

        if let (Some(secret), Some(backup_account)) = (previous, backup_account) {
            // The old value is copied only into the OS keyring. The on-disk
            // journal contains the opaque backup account name, never the
            // credential itself.
            store_keyring_secret(&backup_account, &secret)?;
            let mut journal = self.journal.borrow_mut();
            if let Some(change) = journal
                .changes
                .iter_mut()
                .find(|change| change.account == account)
            {
                change.backup_ready = true;
            }
            persist_transaction_journal(self.lock, &journal)?;
        }
        Ok(())
    }

    pub fn set_target_digest(&self, contents: &[u8]) -> Result<(), ProfileError> {
        let mut journal = self.journal.borrow_mut();
        if journal.changes.is_empty() {
            return Ok(());
        }
        journal.target_digest = Some(content_digest(contents));
        persist_transaction_journal(self.lock, &journal)
    }

    pub fn commit(&self) -> Result<(), ProfileError> {
        let changes = self.journal.borrow().changes.clone();
        for change in changes {
            if let Some(backup_account) = change.backup_account {
                delete_keyring_secret(&backup_account)?;
            }
        }
        self.lock
            .remove_sibling(KEYRING_TRANSACTION_SUFFIX)
            .map_err(|err| {
                ProfileError::Invalid(format!("failed to finalize auth transaction: {err}"))
            })
    }

    pub fn abort(&self) -> Result<(), ProfileError> {
        self.commit()
    }

    /// Restore every keyring pre-image recorded by this transaction and clear
    /// the journal. If restoration fails, the journal is deliberately left in
    /// place so the next profile read can retry recovery.
    pub fn rollback_and_abort(&self) -> Result<(), ProfileError> {
        let changes = self.journal.borrow().changes.clone();
        restore_keyring_changes(&changes)?;
        {
            let mut journal = self.journal.borrow_mut();
            journal.rollback_restored = true;
            persist_transaction_journal(self.lock, &journal)?;
        }
        cleanup_backup_accounts(&changes)?;
        self.lock
            .remove_sibling(KEYRING_TRANSACTION_SUFFIX)
            .map_err(|err| {
                ProfileError::Invalid(format!("failed to finalize auth transaction: {err}"))
            })
    }
}

fn transaction_id(path: &std::path::Path) -> String {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let material = format!(
        "{}:{}:{}:{}",
        path.display(),
        std::process::id(),
        now,
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    content_digest(material.as_bytes())[..32].to_string()
}

fn backup_account(transaction_id: &str, account: &str) -> String {
    format!(
        "__ayx_auth_txn/{transaction_id}/{}",
        content_digest(account.as_bytes())
    )
}

fn persist_transaction_journal(
    lock: &SensitiveFileLock,
    journal: &KeyringTransactionJournal,
) -> Result<(), ProfileError> {
    let bytes = serde_json::to_vec(journal).map_err(|source| {
        ProfileError::Invalid(format!("failed to encode auth transaction: {source}"))
    })?;
    lock.write_sibling(KEYRING_TRANSACTION_SUFFIX, &bytes)
        .map_err(|err| ProfileError::Invalid(format!("failed to journal auth transaction: {err}")))
}

fn content_digest(contents: &[u8]) -> String {
    Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn restore_keyring_changes(changes: &[KeyringTransactionChange]) -> Result<(), ProfileError> {
    // Restore every pre-image before changing the journal or deleting any
    // backup. If a later restore fails, the next startup can retry the complete
    // operation from the still-present backups.
    for change in changes.iter().rev() {
        if let Some(backup_account) = change.backup_account.as_deref() {
            match resolve_secret_ref(&format!("keyring:{backup_account}"))? {
                Some(previous) => {
                    store_keyring_secret(&change.account, &previous)?;
                }
                None if change.backup_ready => {
                    return Err(ProfileError::Invalid(format!(
                        "transaction backup for keyring account '{}' is missing",
                        change.account
                    )));
                }
                None => {}
            }
        } else {
            delete_keyring_secret(&change.account)?;
        }
    }
    Ok(())
}

fn cleanup_backup_accounts(changes: &[KeyringTransactionChange]) -> Result<(), ProfileError> {
    for change in changes {
        if let Some(backup_account) = change.backup_account.as_deref() {
            delete_keyring_secret(backup_account)?;
        }
    }
    Ok(())
}

/// Recover an interrupted keyring/profile transaction before a profile is
/// consumed. Recovery holds the same stable lock used by writers, so it cannot
/// remove a journal or temporary file underneath an active writer.
pub fn recover_keyring_transaction(path: &std::path::Path) -> Result<(), ProfileError> {
    let lock = SensitiveFileLock::acquire(path).map_err(|err| {
        ProfileError::Invalid(format!(
            "failed to lock auth transaction '{}': {err}",
            path.display()
        ))
    })?;
    recover_keyring_transaction_locked(path, &lock)
}

/// Recover an interrupted transaction while the caller already holds the
/// profile's stable lock. Writers use this form so recovery cannot deadlock
/// by attempting to acquire the same lock a second time.
pub fn recover_keyring_transaction_locked(
    path: &std::path::Path,
    lock: &SensitiveFileLock,
) -> Result<(), ProfileError> {
    lock.remove_sibling(KEYRING_TRANSACTION_TMP_SUFFIX)
        .map_err(|err| {
            ProfileError::Invalid(format!("failed to recover auth transaction: {err}"))
        })?;
    let Some(bytes) = lock
        .read_sibling(KEYRING_TRANSACTION_SUFFIX)
        .map_err(|err| ProfileError::Invalid(format!("failed to read auth transaction: {err}")))?
    else {
        return Ok(());
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| {
        ProfileError::Invalid(format!(
            "invalid authentication persistence journal for '{}': {err}",
            path.display()
        ))
    })?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProfileError::Invalid("authentication journal has no version".into()))?
        as u16;

    if version == 1 {
        let legacy: LegacyKeyringTransactionJournal =
            serde_json::from_value(value).map_err(|err| {
                ProfileError::Invalid(format!(
                    "invalid legacy authentication persistence journal for '{}': {err}",
                    path.display()
                ))
            })?;
        return recover_legacy_transaction(path, lock, legacy);
    }
    if version != KEYRING_TRANSACTION_VERSION {
        return Err(ProfileError::Invalid(format!(
            "unsupported authentication persistence journal version {version}"
        )));
    }
    let journal: KeyringTransactionJournal = serde_json::from_value(value).map_err(|err| {
        ProfileError::Invalid(format!(
            "invalid authentication persistence journal for '{}': {err}",
            path.display()
        ))
    })?;

    let committed = journal.target_digest.as_deref().is_some_and(|digest| {
        fs::read(path)
            .ok()
            .is_some_and(|contents| content_digest(&contents) == digest)
    });
    if !committed {
        if !journal.rollback_restored {
            restore_keyring_changes(&journal.changes)?;
            let mut restored_journal = journal;
            restored_journal.rollback_restored = true;
            persist_transaction_journal(lock, &restored_journal)?;
            cleanup_backup_accounts(&restored_journal.changes)?;
        } else {
            // The pre-images were restored and that fact was journaled before
            // cleanup began. Missing backups are therefore already-cleaned
            // entries, so cleanup can safely be retried after a partial error.
            cleanup_backup_accounts(&journal.changes)?;
        }
    } else {
        for change in &journal.changes {
            if let Some(backup_account) = change.backup_account.as_deref() {
                delete_keyring_secret(backup_account)?;
            }
        }
    }
    lock.remove_sibling(KEYRING_TRANSACTION_SUFFIX)
        .map_err(|err| ProfileError::Invalid(format!("failed to clear auth transaction: {err}")))
}

fn recover_legacy_transaction(
    path: &std::path::Path,
    lock: &SensitiveFileLock,
    journal: LegacyKeyringTransactionJournal,
) -> Result<(), ProfileError> {
    let committed = journal.target_digest.as_deref().is_some_and(|digest| {
        fs::read(path)
            .ok()
            .is_some_and(|contents| content_digest(&contents) == digest)
    });
    if !committed {
        for change in journal.changes.iter().rev() {
            match change.previous.as_deref() {
                Some(secret) => {
                    store_keyring_secret(&change.account, secret)?;
                }
                None => {
                    delete_keyring_secret(&change.account)?;
                }
            }
        }
    }
    lock.remove_sibling(KEYRING_TRANSACTION_SUFFIX)
        .map_err(|err| ProfileError::Invalid(format!("failed to clear auth transaction: {err}")))
}

/// Native-store adapter behind the platform-neutral authentication interface.
/// The selected backend is registered by `ensure_keyring_store`: Windows
/// Credential Manager, macOS Keychain, or Linux/FreeBSD Secret Service.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringSecureStorage;

impl SecureStorage for KeyringSecureStorage {
    type Error = ProfileError;

    fn availability(&self) -> SecretStoreAvailability {
        ensure_keyring_store();
        if keyring_core::get_default_store().is_some() {
            SecretStoreAvailability::Available
        } else {
            SecretStoreAvailability::Unavailable
        }
    }

    fn get(&mut self, account: &str) -> Result<Option<String>, Self::Error> {
        resolve_secret_ref(&format!("keyring:{account}"))
    }

    fn set(&mut self, account: &str, secret: &str) -> Result<(), Self::Error> {
        store_keyring_secret(account, secret).map(|_| ())
    }

    fn delete(&mut self, account: &str) -> Result<(), Self::Error> {
        delete_keyring_secret(account)
    }
}

/// Returns `true` when the named environment variable is set to a truthy value
/// (`1`, `true`, `yes`, `TRUE`, `YES`). Treats an unset or empty variable as
/// falsy. Used to gate `AYX_ALLOW_INLINE_SECRETS` and, when the
/// `test-inline-forcing` feature is active, `AYX_FORCE_INLINE_SECRETS`.
fn env_truthy(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("TRUE") | Some("YES")
    )
}

/// Register the platform credential store as keyring-core's default, exactly once.
///
/// keyring 4.x (keyring-core) selects the credential store at runtime rather than
/// via a compile-time feature, so a store must be registered before any `Entry`
/// is created. If the store cannot be created — e.g. no Secret Service / D-Bus
/// session on a headless host — we leave the default unset; subsequent `Entry`
/// operations then return `NoDefaultStore`, which callers already treat as
/// "keyring unavailable" (inline fallback where permitted).
pub fn ensure_keyring_store() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if keyring_core::get_default_store().is_some() || REGISTERED.get().is_some() {
        return;
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if let Ok(store) = zbus_secret_service_keyring_store::Store::new() {
        keyring_core::set_default_store(store);
    }
    #[cfg(target_os = "macos")]
    if let Ok(store) = apple_native_keyring_store::keychain::Store::new() {
        keyring_core::set_default_store(store);
    }
    #[cfg(target_os = "windows")]
    if let Ok(store) = windows_native_keyring_store::Store::new() {
        keyring_core::set_default_store(store);
    }
    if keyring_core::get_default_store().is_some() {
        let _ = REGISTERED.set(());
    }
}

/// Install a process-local in-memory keyring store for tests.
///
/// The mock store is process-global inside keyring-core, so tests must call this
/// before any keyring operation. The installation is idempotent and safe to race.
#[cfg(any(test, feature = "test-inline-forcing"))]
pub fn install_test_keyring_store() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        keyring_core::set_default_store(
            keyring_core::mock::Store::new().expect("mock keyring store should initialize"),
        );
        // SAFETY: Tests that call this helper explicitly want the process-local
        // mock keyring to win over AYX_FORCE_INLINE_SECRETS. nextest runs each
        // test in its own process, and this mutation happens once at startup.
        unsafe {
            env::remove_var("AYX_FORCE_INLINE_SECRETS");
        }
    });
}

pub fn keyring_secret_ref(scope: &str) -> String {
    format!("keyring:{scope}")
}

pub fn keyring_account(profile_name: &str, field: &str) -> String {
    let mut profile = String::new();
    for ch in profile_name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            profile.push(ch);
        } else {
            profile.push('_');
        }
    }
    format!("{profile}/{field}")
}

/// Return a binding-derived keyring account for new authentication writes.
///
/// `keyring_account` remains unchanged for legacy profile compatibility. New
/// authentication flows should use this helper so a credential is scoped by
/// account, issuer, region, base URL, and workspace identity rather than by a
/// mutable profile name alone.
pub fn bound_keyring_account(binding: &CredentialBinding, field: &str) -> String {
    binding.keyring_account(field)
}

pub fn bound_keyring_account_in_namespace(
    binding: &CredentialBinding,
    namespace: Option<&str>,
    field: &str,
) -> String {
    binding.keyring_account_in_namespace(namespace, field)
}

pub fn env_secret_ref(name: &str) -> String {
    format!("env:{name}")
}

pub fn resolve_secret_ref(reference: &str) -> Result<Option<String>, ProfileError> {
    if let Some(value) = reference.strip_prefix("inline:") {
        return Ok(Some(value.to_string()));
    }
    if let Some(name) = reference.strip_prefix("env:") {
        return Ok(env::var(name).ok());
    }
    if let Some(account) = reference.strip_prefix("keyring:") {
        ensure_keyring_store();
        let entry = match Entry::new(SECRET_SERVICE, account) {
            Ok(e) => e,
            // No store was registered (headless host, no D-Bus / Secret Service).
            // Treat as "keyring unavailable" per the documented contract.
            Err(Error::NoDefaultStore) => return Ok(None),
            Err(source) => {
                return Err(ProfileError::Invalid(format!(
                    "unable to open keyring entry '{}': {}",
                    account, source
                )));
            }
        };
        return match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoEntry) => Ok(None),
            Err(err) => Err(ProfileError::Invalid(format!(
                "unable to load keyring secret '{}': {}",
                account, err
            ))),
        };
    }
    Err(ProfileError::Invalid(
        "unsupported secret reference format; use the plaintext field for a literal value, or keyring:<account>, env:<variable>, or inline:<value>"
            .to_string(),
    ))
}

/// Store a secret in the OS keyring.
///
/// On failure (no keyring backend, denied access, etc.) returns an error so
/// callers must decide explicitly whether to fall back. Use
/// [`store_secret_with_fallback`] when an inline fallback is acceptable.
///
/// # Test lever (feature-gated; compiled out of release binaries)
///
/// When the `test-inline-forcing` Cargo feature is enabled, a
/// [`ForcedKeyringUnavailable`] guard (or the legacy
/// `AYX_FORCE_INLINE_SECRETS=1` test lever) makes this function behave as if
/// the OS keyring were unavailable. This lets tests deterministically exercise
/// the inline-fallback path without requiring a live Secret Service on the test
/// machine.
///
/// The feature is **not enabled by default** and is absent from all production
/// dependency edges. It is intended to be enabled only via
/// `[dev-dependencies]` in crates that need headless-CI test coverage of the
/// inline-fallback path. When the feature is off (i.e. in any release binary),
/// `AYX_FORCE_INLINE_SECRETS` has **no effect whatsoever** — this block is not
/// compiled in and the function goes straight to the real keyring.
pub fn store_keyring_secret(account: &str, secret: &str) -> Result<String, ProfileError> {
    // Deterministic inline-fallback lever for tests. Compiled out of release
    // binaries (requires feature "test-inline-forcing"). The scoped guard is
    // thread-local so parallel tests cannot affect one another.
    #[cfg(feature = "test-inline-forcing")]
    if keyring_unavailable_is_forced() {
        return Err(ProfileError::Invalid(format!(
            "unable to open keyring entry '{}': keyring unavailable (forced by \
             AYX_FORCE_INLINE_SECRETS). Set AYX_ALLOW_INLINE_SECRETS=1 to store in YAML instead.",
            account
        )));
    }
    ensure_keyring_store();
    let entry = Entry::new(SECRET_SERVICE, account).map_err(|source| {
        ProfileError::Invalid(format!(
            "unable to open keyring entry '{}': {}. Set AYX_ALLOW_INLINE_SECRETS=1 to store in YAML instead, or configure a keyring backend.",
            account, source
        ))
    })?;
    entry.set_password(secret).map_err(|source| {
        ProfileError::Invalid(format!(
            "unable to write keyring secret '{}': {}. Set AYX_ALLOW_INLINE_SECRETS=1 to store in YAML instead, or configure a keyring backend.",
            account, source
        ))
    })?;
    Ok(keyring_secret_ref(account))
}

/// Store a secret, preferring the OS keyring and falling back to an inline
/// reference only when the caller has opted in (`allow_inline = true` or the
/// `AYX_ALLOW_INLINE_SECRETS` env var is truthy). Returns the reference plus a
/// flag indicating whether inline fallback was used so the caller can warn.
pub fn store_secret_with_fallback(
    account: &str,
    secret: &str,
    allow_inline: bool,
) -> Result<(String, bool), ProfileError> {
    match store_keyring_secret(account, secret) {
        Ok(reference) => Ok((reference, false)),
        Err(err) => {
            let env_opt_in = env_truthy("AYX_ALLOW_INLINE_SECRETS");
            if allow_inline || env_opt_in {
                Ok((format!("inline:{secret}"), true))
            } else {
                Err(err)
            }
        }
    }
}

/// Store a newly-created credential using a binding-derived account. The
/// fallback policy is explicit: callers must pass `allow_inline = true` after
/// interactive consent, while automation can fail closed or choose session
/// storage before it reaches this function.
pub fn store_bound_secret_with_fallback(
    binding: &CredentialBinding,
    field: &str,
    secret: &str,
    allow_inline: bool,
) -> Result<(String, bool), ProfileError> {
    let account = bound_keyring_account(binding, field);
    store_secret_with_fallback(&account, secret, allow_inline)
}

/// Delete a keyring entry as part of a failed profile transaction. Missing
/// entries are treated as success so rollback is idempotent.
pub fn delete_keyring_secret(account: &str) -> Result<(), ProfileError> {
    #[cfg(test)]
    {
        let mut failure = DELETE_FAILURE_ACCOUNT
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("delete failure hook mutex");
        if failure.as_deref() == Some(account) {
            let failed_account = failure.take().expect("failure account should be present");
            return Err(ProfileError::Invalid(format!(
                "injected keyring delete failure for '{failed_account}'"
            )));
        }
    }
    ensure_keyring_store();
    let entry = match Entry::new(SECRET_SERVICE, account) {
        Ok(entry) => entry,
        // If no backend exists, there cannot be a persisted entry to delete.
        // Treat this as idempotent success so crash recovery can finish a
        // journal created immediately before an unavailable-store fallback.
        Err(Error::NoDefaultStore) => return Ok(()),
        Err(source) => {
            return Err(ProfileError::Invalid(format!(
                "unable to open keyring entry '{}': {}",
                account, source
            )));
        }
    };
    match entry.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(source) => Err(ProfileError::Invalid(format!(
            "unable to delete keyring entry '{}': {}",
            account, source
        ))),
    }
}

#[cfg(test)]
static DELETE_FAILURE_ACCOUNT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(test)]
fn inject_delete_failure(account: &str) {
    *DELETE_FAILURE_ACCOUNT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("delete failure hook mutex") = Some(account.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensitive::write_sensitive_file;

    #[test]
    fn resolves_inline_secret_refs() {
        assert_eq!(
            resolve_secret_ref("inline:fresh-token").expect("inline ref should resolve"),
            Some("fresh-token".to_string())
        );
    }

    #[test]
    fn bare_secret_ref_is_rejected_with_plaintext_remediation() {
        let error = resolve_secret_ref("literal-secret")
            .expect_err("a literal must not be silently treated as a missing reference");
        let message = error.to_string();
        assert!(message.contains("use the plaintext field"));
        assert!(message.contains("keyring:<account>"));
    }

    #[test]
    fn missing_keyring_ref_resolves_none_with_test_store() {
        install_test_keyring_store();
        assert_eq!(
            resolve_secret_ref("keyring:ayx-core-missing-test-account")
                .expect("missing mock-store entry should not error"),
            None
        );
    }

    #[test]
    fn keyring_secret_round_trips_through_test_store() {
        install_test_keyring_store();
        let account = "ayx-core-keyring-roundtrip-test-account";
        let reference = store_keyring_secret(account, "roundtrip-secret")
            .expect("mock store should accept the secret");
        assert_eq!(reference, format!("keyring:{account}"));
        assert_eq!(
            resolve_secret_ref(&reference).expect("mock-store ref should resolve"),
            Some("roundtrip-secret".to_string())
        );
    }

    #[test]
    fn interrupted_keyring_transaction_restores_preimage() {
        install_test_keyring_store();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        write_sensitive_file(&path, b"old-profile").expect("seed profile");
        let account = "transaction-rollback-account";
        store_keyring_secret(account, "old-secret").expect("seed old secret");

        {
            let lock = SensitiveFileLock::acquire(&path).expect("lock");
            let transaction = KeyringTransaction::begin(&lock);
            transaction
                .record_change(account, Some("old-secret".to_string()))
                .expect("journal");
            let journal = fs::read(path.with_file_name("profile.yaml.auth-txn"))
                .expect("journal should be durable before mutation");
            assert!(
                !journal
                    .windows(b"old-secret".len())
                    .any(|window| window == b"old-secret")
            );
            store_keyring_secret(account, "new-secret").expect("store");
            transaction
                .set_target_digest(b"new-profile")
                .expect("target digest");
        }

        recover_keyring_transaction(&path).expect("recover");
        assert_eq!(
            resolve_secret_ref(&format!("keyring:{account}")).unwrap(),
            Some("old-secret".to_string())
        );
        assert!(!path.with_file_name("profile.yaml.auth-txn").exists());
    }

    #[test]
    fn committed_keyring_transaction_keeps_postimage() {
        install_test_keyring_store();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        write_sensitive_file(&path, b"old-profile").expect("seed profile");
        let account = "transaction-commit-account";

        {
            let lock = SensitiveFileLock::acquire(&path).expect("lock");
            let transaction = KeyringTransaction::begin(&lock);
            transaction.record_change(account, None).expect("journal");
            store_keyring_secret(account, "new-secret").expect("store");
            transaction
                .set_target_digest(b"new-profile")
                .expect("target digest");
            lock.write(b"new-profile").expect("profile commit");
        }

        recover_keyring_transaction(&path).expect("recover");
        assert_eq!(
            resolve_secret_ref(&format!("keyring:{account}")).unwrap(),
            Some("new-secret".to_string())
        );
        assert!(!path.with_file_name("profile.yaml.auth-txn").exists());
    }

    #[test]
    fn rollback_and_abort_clears_journal_after_restoring_keyring() {
        install_test_keyring_store();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        let account = "transaction-explicit-abort-account";

        let lock = SensitiveFileLock::acquire(&path).expect("lock");
        let transaction = KeyringTransaction::begin(&lock);
        transaction.record_change(account, None).expect("journal");
        store_keyring_secret(account, "uncommitted-secret").expect("store");
        transaction.rollback_and_abort().expect("rollback");

        assert_eq!(
            resolve_secret_ref(&format!("keyring:{account}")).unwrap(),
            None
        );
        assert!(!path.with_file_name("profile.yaml.auth-txn").exists());
    }

    #[test]
    fn rollback_cleanup_retries_after_partial_backup_deletion() {
        install_test_keyring_store();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.yaml");
        let first_account = "transaction-partial-cleanup-first";
        let second_account = "transaction-partial-cleanup-second";
        store_keyring_secret(first_account, "first-old-secret").expect("seed first secret");
        store_keyring_secret(second_account, "second-old-secret").expect("seed second secret");

        {
            let lock = SensitiveFileLock::acquire(&path).expect("lock");
            let transaction = KeyringTransaction::begin(&lock);
            transaction
                .record_change(first_account, Some("first-old-secret".to_string()))
                .expect("first journal entry");
            transaction
                .record_change(second_account, Some("second-old-secret".to_string()))
                .expect("second journal entry");
            store_keyring_secret(first_account, "first-new-secret").expect("store first");
            store_keyring_secret(second_account, "second-new-secret").expect("store second");
        }

        let journal_path = path.with_file_name("profile.yaml.auth-txn");
        let journal: KeyringTransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("decode");
        let second_backup = journal.changes[1]
            .backup_account
            .as_deref()
            .expect("second backup account");
        inject_delete_failure(second_backup);

        assert!(recover_keyring_transaction(&path).is_err());
        assert!(journal_path.exists());
        assert_eq!(
            resolve_secret_ref(&format!(
                "keyring:{}",
                journal.changes[0]
                    .backup_account
                    .as_deref()
                    .expect("first backup account")
            ))
            .expect("first backup lookup"),
            None
        );

        recover_keyring_transaction(&path).expect("recovery should retry cleanup");
        assert_eq!(
            resolve_secret_ref(&format!("keyring:{first_account}")).expect("first restore"),
            Some("first-old-secret".to_string())
        );
        assert_eq!(
            resolve_secret_ref(&format!("keyring:{second_account}")).expect("second restore"),
            Some("second-old-secret".to_string())
        );
        assert!(!journal_path.exists());
    }
}
