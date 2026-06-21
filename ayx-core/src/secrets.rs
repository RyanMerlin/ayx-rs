use std::env;
use std::sync::Once;

use keyring_core::{Entry, Error};

use crate::profile::ProfileError;

const SECRET_SERVICE: &str = "ayx";

/// Register the platform credential store as keyring-core's default, exactly once.
///
/// keyring 4.x (keyring-core) selects the credential store at runtime rather than
/// via a compile-time feature, so a store must be registered before any `Entry`
/// is created. If the store cannot be created — e.g. no Secret Service / D-Bus
/// session on a headless host — we leave the default unset; subsequent `Entry`
/// operations then return `NoDefaultStore`, which callers already treat as
/// "keyring unavailable" (inline fallback where permitted).
fn ensure_keyring_store() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
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
                )))
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
    Ok(None)
}

/// Store a secret in the OS keyring.
///
/// On failure (no keyring backend, denied access, etc.) returns an error so
/// callers must decide explicitly whether to fall back. Use
/// [`store_secret_with_fallback`] when an inline fallback is acceptable.
pub fn store_keyring_secret(account: &str, secret: &str) -> Result<String, ProfileError> {
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
            let env_opt_in = matches!(
                env::var("AYX_ALLOW_INLINE_SECRETS").ok().as_deref(),
                Some("1") | Some("true") | Some("yes") | Some("TRUE") | Some("YES")
            );
            if allow_inline || env_opt_in {
                Ok((format!("inline:{secret}"), true))
            } else {
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inline_secret_refs() {
        assert_eq!(
            resolve_secret_ref("inline:fresh-token").expect("inline ref should resolve"),
            Some("fresh-token".to_string())
        );
    }

    #[test]
    fn keyring_ref_never_panics_or_fabricates() {
        // Exercises the keyring-core runtime store registration. Whether the host
        // has a Secret Service backend or not, the result must be `Ok(None)` for a
        // missing entry — headless hosts (CI) now return `Ok(None)` via the
        // `NoDefaultStore` path rather than hard-erroring. `Err(_)` is still
        // accepted as a safety valve for unexpected backend failures.
        match resolve_secret_ref("keyring:ayx-core-nonexistent-test-account") {
            Ok(None) => {}
            Ok(Some(_)) => panic!("must not fabricate a secret for a missing keyring entry"),
            Err(_) => {}
        }
    }
}
