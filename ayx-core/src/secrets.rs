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
    if let Some(name) = reference.strip_prefix("env:") {
        return Ok(env::var(name).ok());
    }
    if let Some(account) = reference.strip_prefix("keyring:") {
        let entry = Entry::new(SECRET_SERVICE, account).map_err(|source| {
            ProfileError::Invalid(format!("unable to open keyring entry '{}': {}", account, source))
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

pub fn store_keyring_secret(account: &str, secret: &str) -> Result<String, ProfileError> {
    let entry = Entry::new(SECRET_SERVICE, account).map_err(|source| {
        ProfileError::Invalid(format!("unable to open keyring entry '{}': {}", account, source))
    })?;
    entry.set_password(secret).map_err(|source| {
        ProfileError::Invalid(format!("unable to store keyring secret '{}': {}", account, source))
    })?;
    Ok(keyring_secret_ref(account))
}
