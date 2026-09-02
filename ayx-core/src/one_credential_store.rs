//! Keyring-backed persistence for rotating Alteryx One credentials.
//!
//! This module deliberately knows nothing about HTTP or OAuth. It owns only
//! the local credential boundary: selecting the exact workspace slot, taking
//! the profile lock, reloading the profile under that lock, and replacing the
//! secret behind an already-bound keyring reference.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::auth::{CredentialBinding, binding_matches};
use crate::profile::{Config, profile_storage_path};
use crate::secrets::{
    recover_keyring_transaction_locked, resolve_secret_ref, store_keyring_secret,
};
use crate::sensitive::SensitiveFileLock;

const REFRESH_TOKEN_FIELD: &str = "refresh_token";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialSlot {
    TopLevel,
    Workspace { workspace_id: String },
}

/// A local store for one exact, workspace-bound refresh-token credential.
///
/// The store is intentionally constructed from the loaded profile but reloads
/// that profile after taking the stable lock before it returns a refresh lease.
/// This prevents concurrent CLI processes from both exchanging the same
/// rotating refresh token.
pub struct OneCredentialStore {
    profile_path: PathBuf,
    slot: CredentialSlot,
    binding: CredentialBinding,
    account: String,
    access_account: Option<String>,
}

/// A lock-held view of the exact refresh credential selected by the store.
///
/// The lock remains held while the caller performs the remote token exchange
/// and commits any replacement token. This type intentionally has no `Debug`
/// implementation because it owns secret-bearing in-memory configuration.
pub struct OneCredentialRefreshLease {
    _lock: SensitiveFileLock,
    config: Config,
    refresh_token: String,
    account: String,
    access_account: Option<String>,
}

#[derive(Debug, Error)]
pub enum OneCredentialStoreError {
    #[error("One credential store requires an alteryx_one profile")]
    MissingProfile,
    #[error("One credential store could not determine an exact refresh-token slot")]
    AmbiguousSlot,
    #[error(
        "One credential store refresh-token slot is not backed by a canonical keyring reference"
    )]
    UnsupportedReference,
    #[error(
        "One credential store refresh-token reference does not match the expected workspace binding"
    )]
    BindingMismatch,
    #[error("One credential store refresh-token reference is unresolved")]
    UnresolvedReference,
    #[error("One credential store refresh-token value is empty")]
    EmptyRefreshToken,
    #[error("One credential store replacement token is empty")]
    EmptyReplacement,
    #[error("One credential store profile operation failed: {0}")]
    Profile(String),
    #[error("One credential store keyring operation failed: {0}")]
    Keyring(String),
    #[error("One credential store profile binding changed while refreshing")]
    BindingChanged,
    #[error("One credential store refresh reference changed while refreshing")]
    ReferenceChanged,
}

impl OneCredentialStore {
    /// Build a store when the selected refresh credential is eligible for
    /// silent rotation. `Ok(None)` means no refresh credential is configured.
    /// Non-keyring credentials are rejected rather than silently relocated.
    pub fn from_config(config: &Config) -> Result<Option<Self>, OneCredentialStoreError> {
        let one = config
            .alteryx_one
            .as_ref()
            .ok_or(OneCredentialStoreError::MissingProfile)?;
        if one.resolved_refresh_token().is_none() {
            return Ok(None);
        }

        let (slot, field, reference) = selected_refresh_slot(config)?;
        let reference = reference.ok_or(OneCredentialStoreError::UnsupportedReference)?;
        let account = reference
            .strip_prefix("keyring:")
            .filter(|account| account.starts_with("v1/"))
            .ok_or(OneCredentialStoreError::UnsupportedReference)?
            .to_string();
        let workspace_id = match &slot {
            CredentialSlot::TopLevel => None,
            CredentialSlot::Workspace { workspace_id } => Some(workspace_id.as_str()),
        };
        let binding = credential_binding_for_one(config, workspace_id)?;
        let expected_account = binding.keyring_account(&field);
        if account != expected_account {
            return Err(OneCredentialStoreError::BindingMismatch);
        }
        let access_field = access_field_for_slot(&slot);
        let access_account = selected_secret_reference(config, &slot)
            .and_then(|reference| reference.strip_prefix("keyring:").map(str::to_string))
            .filter(|account| account == &binding.keyring_account(&access_field));

        Ok(Some(Self {
            profile_path: profile_storage_path(&config.profile_name)
                .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))?,
            slot,
            binding,
            account,
            access_account,
        }))
    }

    /// Take the stable profile lock, recover interrupted persistence, reload
    /// the profile, and return the exact current refresh token.
    pub fn acquire_refresh(&self) -> Result<OneCredentialRefreshLease, OneCredentialStoreError> {
        let lock = SensitiveFileLock::acquire(&self.profile_path)
            .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))?;
        recover_keyring_transaction_locked(&self.profile_path, &lock)
            .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))?;
        lock.remove_sibling(".tmp")
            .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))?;

        let mut config = Config::load_from_path_lenient_locked(&self.profile_path, &lock)
            .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))?;
        if let CredentialSlot::Workspace { workspace_id } = &self.slot {
            config
                .alteryx_one
                .as_mut()
                .ok_or(OneCredentialStoreError::MissingProfile)?
                .active_workspace_id = Some(workspace_id.clone());
        }

        let workspace_id = match &self.slot {
            CredentialSlot::TopLevel => None,
            CredentialSlot::Workspace { workspace_id } => Some(workspace_id.as_str()),
        };
        let actual_binding = credential_binding_for_one(&config, workspace_id)?;
        if !binding_matches(&self.binding, &actual_binding) {
            return Err(OneCredentialStoreError::BindingChanged);
        }
        let (reference, refresh_token) = selected_refresh_values(&config, &self.slot)?;
        let expected_reference = format!("keyring:{}", self.account);
        if reference.as_deref() != Some(expected_reference.as_str()) {
            return Err(OneCredentialStoreError::ReferenceChanged);
        }
        let refresh_token = refresh_token.ok_or(OneCredentialStoreError::UnresolvedReference)?;
        if refresh_token.trim().is_empty() {
            return Err(OneCredentialStoreError::EmptyRefreshToken);
        }
        let resolved = resolve_secret_ref(&format!("keyring:{}", self.account))
            .map_err(|err| OneCredentialStoreError::Keyring(err.to_string()))?;
        if resolved.as_deref() != Some(refresh_token.as_str()) {
            return Err(OneCredentialStoreError::ReferenceChanged);
        }

        Ok(OneCredentialRefreshLease {
            _lock: lock,
            config,
            refresh_token,
            account: self.account.clone(),
            access_account: self.access_account.clone(),
        })
    }

    pub fn profile_path(&self) -> &Path {
        &self.profile_path
    }
}

impl OneCredentialRefreshLease {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    /// Replace the value behind the already-validated keyring reference while
    /// retaining the profile lock. A failed replacement is returned to the
    /// caller; the remote token exchange must not be retried afterward.
    pub fn commit_replacement(&self, replacement: &str) -> Result<(), OneCredentialStoreError> {
        if replacement.trim().is_empty() {
            return Err(OneCredentialStoreError::EmptyReplacement);
        }
        store_keyring_secret(&self.account, replacement)
            .map(|_| ())
            .map_err(|err| OneCredentialStoreError::Keyring(err.to_string()))
    }

    /// Commit the newly issued access token as well as the rotating refresh
    /// token. The access token is stored only when the profile already has a
    /// matching canonical keyring reference; otherwise the current request may
    /// use it, but the next process will safely refresh again.
    pub fn commit_rotation(
        &self,
        access_token: &str,
        replacement: Option<&str>,
    ) -> Result<(), OneCredentialStoreError> {
        if access_token.trim().is_empty() {
            return Err(OneCredentialStoreError::Keyring(
                "access token is empty".to_string(),
            ));
        }
        if let Some(replacement) = replacement {
            self.commit_replacement(replacement)?;
        }
        if let Some(access_account) = self.access_account.as_deref() {
            store_keyring_secret(access_account, access_token)
                .map_err(|err| OneCredentialStoreError::Keyring(err.to_string()))?;
        }
        Ok(())
    }
}

fn access_field_for_slot(slot: &CredentialSlot) -> String {
    match slot {
        CredentialSlot::TopLevel => "alteryx_one.access_token".to_string(),
        CredentialSlot::Workspace { workspace_id } => {
            format!("alteryx_one.workspace_credentials['{workspace_id}'].access_token")
        }
    }
}

fn selected_secret_reference(config: &Config, slot: &CredentialSlot) -> Option<String> {
    let one = config.alteryx_one.as_ref()?;
    match slot {
        CredentialSlot::TopLevel => one.access_token_ref.clone(),
        CredentialSlot::Workspace { workspace_id } => one
            .workspace_credentials
            .get(workspace_id)
            .and_then(|credential| credential.access_token_ref.clone()),
    }
}

fn selected_refresh_slot(
    config: &Config,
) -> Result<(CredentialSlot, String, Option<String>), OneCredentialStoreError> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or(OneCredentialStoreError::MissingProfile)?;
    if let Some(workspace_id) = one.active_workspace_id() {
        let credential = one
            .workspace_credentials
            .get(workspace_id)
            .ok_or(OneCredentialStoreError::AmbiguousSlot)?;
        return Ok((
            CredentialSlot::Workspace {
                workspace_id: workspace_id.to_string(),
            },
            format!("alteryx_one.workspace_credentials['{workspace_id}'].{REFRESH_TOKEN_FIELD}"),
            credential.refresh_token_ref.clone(),
        ));
    }
    if one.workspace_credentials.len() > 1 {
        return Err(OneCredentialStoreError::AmbiguousSlot);
    }
    Ok((
        CredentialSlot::TopLevel,
        "alteryx_one.refresh_token".to_string(),
        one.refresh_token_ref.clone(),
    ))
}

fn selected_refresh_values(
    config: &Config,
    slot: &CredentialSlot,
) -> Result<(Option<String>, Option<String>), OneCredentialStoreError> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or(OneCredentialStoreError::MissingProfile)?;
    match slot {
        CredentialSlot::TopLevel => Ok((one.refresh_token_ref.clone(), one.refresh_token.clone())),
        CredentialSlot::Workspace { workspace_id } => {
            let credential = one
                .workspace_credentials
                .get(workspace_id)
                .ok_or(OneCredentialStoreError::AmbiguousSlot)?;
            Ok((
                credential.refresh_token_ref.clone(),
                credential.refresh_token.clone(),
            ))
        }
    }
}

fn credential_binding_for_one(
    config: &Config,
    workspace_id: Option<&str>,
) -> Result<CredentialBinding, OneCredentialStoreError> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or(OneCredentialStoreError::MissingProfile)?;
    let base_url = one.normalized_base_url().ok_or_else(|| {
        OneCredentialStoreError::Profile("alteryx_one.base_url is required".into())
    })?;
    let issuer = one
        .effective_token_endpoint_url_for_workspace(workspace_id)
        .unwrap_or_else(|| base_url.clone());
    let region = url::Url::parse(&base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .and_then(|host| host.split('.').next().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let workspace_gid = workspace_id
        .and_then(|id| one.workspace_credentials.get(id))
        .and_then(|credential| credential.workspace_gid.clone())
        .or_else(|| one.workspace_gid.clone());
    CredentialBinding::new(
        one.account_email.clone(),
        issuer,
        region,
        base_url,
        workspace_id.map(str::to_string),
        workspace_gid,
    )
    .map_err(|err| OneCredentialStoreError::Profile(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{AlteryxOneProfile, Config, MongoProfile, WorkspaceCredential};

    fn test_config() -> Config {
        Config {
            profile_name: "one-credential-store-test".to_string(),
            mongo: MongoProfile::default(),
            alteryx_one: Some(AlteryxOneProfile {
                account_email: "user@example.com".to_string(),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: Some("client-id".to_string()),
                refresh_token: Some("refresh-value".to_string()),
                ..AlteryxOneProfile::default()
            }),
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn non_keyring_refresh_sources_are_not_auto_rotated() {
        let mut config = test_config();
        config
            .alteryx_one
            .as_mut()
            .expect("One profile")
            .refresh_token_ref = Some("env:AYX_ONE_API_REFRESH_TOKEN".to_string());

        let error = match OneCredentialStore::from_config(&config) {
            Ok(_) => panic!("environment-backed credentials must not be mutated"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            OneCredentialStoreError::UnsupportedReference
        ));
    }

    #[test]
    fn bound_top_level_refresh_source_is_selected_exactly() {
        let mut config = test_config();
        let binding = credential_binding_for_one(&config, None).expect("binding");
        let account = binding.keyring_account("alteryx_one.refresh_token");
        config
            .alteryx_one
            .as_mut()
            .expect("One profile")
            .refresh_token_ref = Some(format!("keyring:{account}"));

        let store = OneCredentialStore::from_config(&config)
            .expect("bound source should be accepted")
            .expect("refresh token should select a store");
        assert_eq!(store.account, account);
        assert!(
            store
                .profile_path()
                .ends_with("one-credential-store-test.yaml")
        );
    }

    #[test]
    fn bound_refresh_source_rejects_a_different_workspace_identity() {
        let mut config = test_config();
        let binding = credential_binding_for_one(&config, None).expect("binding");
        let account = binding.keyring_account("alteryx_one.refresh_token");
        let one = config.alteryx_one.as_mut().expect("One profile");
        one.refresh_token_ref = Some(format!("keyring:{account}"));
        one.workspace_gid = Some("01DIFFERENTWORKSPACEGID000000".to_string());

        let error = match OneCredentialStore::from_config(&config) {
            Ok(_) => panic!("changed workspace identity must invalidate the binding"),
            Err(error) => error,
        };
        assert!(matches!(error, OneCredentialStoreError::BindingMismatch));
    }

    #[test]
    fn bound_workspace_access_source_is_retained_for_rotation() {
        let mut config = test_config();
        let workspace_id = "91946";
        let workspace_gid = "01KMGF85WTTEJZ397MW1RBD9ZB";
        {
            let one = config.alteryx_one.as_mut().expect("One profile");
            one.active_workspace_id = Some(workspace_id.to_string());
            one.workspace_credentials.insert(
                workspace_id.to_string(),
                WorkspaceCredential {
                    workspace_id: Some(workspace_id.to_string()),
                    workspace_gid: Some(workspace_gid.to_string()),
                    workspace_name: Some("alteryx-fde".to_string()),
                    access_token: Some("access-value".to_string()),
                    refresh_token: Some("refresh-value".to_string()),
                    ..WorkspaceCredential::default()
                },
            );
        }
        let binding = credential_binding_for_one(&config, Some(workspace_id)).expect("binding");
        let access_field = "alteryx_one.workspace_credentials['91946'].access_token";
        let refresh_field = "alteryx_one.workspace_credentials['91946'].refresh_token";
        {
            let one = config.alteryx_one.as_mut().expect("One profile");
            let credential = one
                .workspace_credentials
                .get_mut(workspace_id)
                .expect("workspace credential");
            credential.access_token_ref =
                Some(format!("keyring:{}", binding.keyring_account(access_field)));
            credential.refresh_token_ref = Some(format!(
                "keyring:{}",
                binding.keyring_account(refresh_field)
            ));
        }

        let store = OneCredentialStore::from_config(&config)
            .expect("bound workspace source should be accepted")
            .expect("refresh token should select a store");
        let access_account = binding.keyring_account(access_field);
        assert_eq!(
            store.access_account.as_deref(),
            Some(access_account.as_str())
        );
    }
}
