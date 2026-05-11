use std::env;

use keyring::Entry;

use crate::profile::ProfileError;

const SECRET_SERVICE: &str = "ayx";

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
        let entry = Entry::new(SECRET_SERVICE, account).map_err(|source| {
            ProfileError::Invalid(format!(
                "unable to open keyring entry '{}': {}",
                account, source
            ))
        })?;
        return match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                let message = err.to_string();
                if message.contains("NoEntry") {
                    Ok(None)
                } else {
                    Err(ProfileError::Invalid(format!(
                        "unable to load keyring secret '{}': {}",
                        account, err
                    )))
                }
            }
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
}
