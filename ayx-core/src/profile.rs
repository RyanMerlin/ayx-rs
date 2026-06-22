use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secrets::resolve_secret_ref;
use crate::sensitive::write_sensitive_file;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read config file '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config yaml '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid config: {0}")]
    Invalid(String),
    #[error("failed to write config file '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub const DEFAULT_PROFILE_FILE: &str = "config.yaml";
pub const DEFAULT_ENVIRONMENTS_FILE: &str = "environments.yaml";
const LEGACY_WORKSPACE_FILE: &str = "workspace.yaml";
const LEGACY_DEFAULT_PROFILE_FILE: &str = "default.yaml";
const DEFAULT_ACTIVE_PROFILE_NAME: &str = "default";
const DEFAULT_ACTIVE_WORKSPACE_NAME: &str = "default";

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AyxState {
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedProfilePath {
    pub requested_path: String,
    pub resolved_path: String,
    pub source: String,
    pub active_profile: Option<String>,
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProfileResolution {
    pub config_home: String,
    pub selected_profile: String,
    pub selection_source: String,
    pub resolved_profile_path: String,
    pub active_profile: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub profile_name: String,
    #[serde(default = "default_mongo_profile")]
    pub mongo: MongoProfile,
    pub alteryx_one: Option<AlteryxOneProfile>,
    #[serde(default)]
    pub observability: Option<ObservabilityProfile>,
    #[serde(default)]
    pub server_api: Option<ServerApiProfile>,
    #[serde(default)]
    pub api: Option<ApiProfile>,
    #[serde(default)]
    pub server: Option<ServerProfile>,
    #[serde(default)]
    pub sqlserver: Option<SqlServerProfile>,
    #[serde(default)]
    pub upgrade: Option<UpgradeProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceConfig {
    pub workspace_name: String,
    pub active_environment: String,
    pub environments: HashMap<String, Config>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerDeploymentProfile {
    pub api: ServerApiProfile,
    pub storage: ServerStorageProfile,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerStorageProfile {
    pub kind: ServerStorageKind,
    #[serde(default)]
    pub mongo: Option<MongoProfile>,
    #[serde(default)]
    pub sqlserver: Option<SqlServerProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerStorageKind {
    EmbeddedMongo,
    ManagedMongo,
    SqlServer,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoProfile {
    #[serde(default)]
    pub mode: MongoMode,
    pub databases: MongoDatabases,
    pub embedded: Option<MongoEmbedded>,
    pub managed: Option<MongoManaged>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoDatabases {
    pub gallery_name: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MongoMode {
    #[default]
    Embedded,
    Managed,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoEmbedded {
    #[serde(default = "default_runtime_settings_path")]
    pub runtime_settings_path: Option<String>,
    pub alteryx_service_path: Option<String>,
    pub restore_target_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MongoManaged {
    pub url: Option<String>,
    pub host: Option<String>,
    #[serde(default = "default_mongo_port")]
    pub port: u16,
    pub auth_database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub password_ref: Option<String>,
    pub tls: TlsConfig,
    pub timeout_ms: Option<u64>,
    pub retry_count: Option<u32>,
    pub max_pool_size: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    pub ca_path: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub allow_invalid_hostnames: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiProfile {
    pub base_url: String,
    pub auth: ApiAuth,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiAuth {
    pub mode: ApiAuthMode,
    pub pat: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiAuthMode {
    Pat,
    Oauth2ClientCredentials,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpgradeProfile {
    pub target_version: Option<String>,
    pub deployment: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityProfile {
    pub api_logging: Option<ApiLoggingProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiLoggingProfile {
    pub enabled: bool,
    pub path: Option<String>,
    pub redact_bodies: Option<bool>,
    pub log_requests: Option<bool>,
    pub log_responses: Option<bool>,
}

/// Whether to acquire tokens via the interactive user/refresh flow or the
/// non-interactive service-principal `client_credentials` flow.  The user
/// flow is the verified default and matches the official `ayx-cli` behaviour;
/// `service-principal` is experimental until the regional-JWKS trust boundary
/// is resolved (see docs/auth-model.md).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    #[default]
    User,
    ServicePrincipal,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WorkspaceCredential {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_url: Option<String>,
    /// Service-principal client ID — distinct from the user `oauth_client_id`.
    /// When set, this credential uses `client_credentials` grant with
    /// `client_secret_post` against `token_endpoint_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_id: Option<String>,
    /// ULID of the workspace — used as the `scope=w:<gid>` value in SP token
    /// requests.  For user flow this is informational only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_gid: Option<String>,
    /// Override the API base URL for this credential (e.g. a regional cell
    /// host for SP tokens).  Falls back to the profile `base_url` default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlteryxOneProfile {
    pub account_email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_credentials: BTreeMap<String, WorkspaceCredential>,
    /// Expected workspace id for mutation safety preflight.
    ///
    /// When set, every mutating One API request (after `--apply`) makes a
    /// `GET /v4/workspaces/current` call and fails closed if the returned
    /// workspace id does not match this value. Set per-environment to
    /// prevent accidentally mutating the wrong workspace when tokens are
    /// shared or stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_workspace_id: Option<String>,
    /// Account-level service-principal client ID.  Resolved workspace-first
    /// via `resolved_sp_client_id()`.  Set `auth_mode: service-principal` to
    /// activate the SP flow (see docs/auth-model.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_client_id: Option<String>,
    /// SP token endpoint URL at the account level (e.g. the regional Ping
    /// issuer `https://pingauth-us1-4.alteryxcloud.com/as/token`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp_token_endpoint_url: Option<String>,
    /// Workspace ULID used as `scope=w:<gid>` in SP token requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_gid: Option<String>,
    /// Token acquisition strategy.  Defaults to `user` (refresh_token flow).
    /// Set to `service-principal` to use the `client_credentials` SP flow.
    #[serde(default, skip_serializing_if = "is_default_auth_mode")]
    pub auth_mode: AuthMode,
}

impl Default for AlteryxOneProfile {
    fn default() -> Self {
        Self {
            account_email: String::new(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::User,
        }
    }
}

impl AlteryxOneProfile {
    pub fn normalized_base_url(&self) -> Option<String> {
        self.base_url
            .as_deref()
            .and_then(normalize_alteryx_one_base_url)
    }

    pub fn workspace_credential_for(
        &self,
        workspace_id: Option<&str>,
    ) -> Option<&WorkspaceCredential> {
        let workspace_id = workspace_id?;
        self.workspace_credentials.get(workspace_id)
    }

    pub fn active_workspace_id(&self) -> Option<&str> {
        if let Some(expected_workspace_id) = self.expected_workspace_id.as_deref()
            && self
                .workspace_credentials
                .contains_key(expected_workspace_id)
        {
            return Some(expected_workspace_id);
        }
        if self.workspace_credentials.len() == 1 {
            return self.workspace_credentials.keys().next().map(String::as_str);
        }
        None
    }

    pub fn active_workspace_credential(&self) -> Option<&WorkspaceCredential> {
        self.active_workspace_id()
            .and_then(|workspace_id| self.workspace_credential_for(Some(workspace_id)))
    }

    pub fn resolved_access_token(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.access_token.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.access_token
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_refresh_token(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.refresh_token.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.refresh_token
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_oauth_client_id(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.oauth_client_id.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.oauth_client_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    pub fn resolved_client_secret(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|credential| credential.client_secret.as_deref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.client_secret
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    /// Service-principal client ID — workspace-first, then account-level.
    pub fn resolved_sp_client_id(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.sp_client_id.as_deref())
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                self.sp_client_id
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
            })
    }

    /// Workspace ULID for SP `scope=w:<gid>` — workspace credential first,
    /// then account-level.
    pub fn resolved_workspace_gid(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.workspace_gid.as_deref())
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                self.workspace_gid
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
            })
    }

    /// SP token endpoint URL — workspace credential's `token_endpoint_url`
    /// first, then account-level `sp_token_endpoint_url`, both normalized.
    pub fn effective_sp_token_endpoint_url(&self) -> Option<String> {
        if let Some(credential) = self.active_workspace_credential()
            && let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.sp_token_endpoint_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(normalize_alteryx_one_token_endpoint)
    }

    /// Per-credential API base URL override for SP — used when the SP token
    /// is scoped to a regional cell that differs from the global API base.
    pub fn resolved_sp_api_base_url(&self) -> Option<&str> {
        self.active_workspace_credential()
            .and_then(|c| c.api_base_url.as_deref())
            .filter(|v| !v.trim().is_empty())
    }

    pub fn effective_token_endpoint_url_for_workspace(
        &self,
        workspace_id: Option<&str>,
    ) -> Option<String> {
        if let Some(credential) = self.workspace_credential_for(workspace_id)
            && let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.effective_token_endpoint_url()
    }

    pub fn effective_token_endpoint_url(&self) -> Option<String> {
        if let Some(url) = self
            .token_endpoint_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(normalize_alteryx_one_token_endpoint(url));
        }
        self.normalized_base_url()
            .map(|base_url| derive_alteryx_one_token_endpoint(&base_url))
    }

    pub fn canonicalize(&mut self) {
        if let Some(base_url) = self.normalized_base_url() {
            self.base_url = Some(base_url.clone());
            if self
                .token_endpoint_url
                .as_deref()
                .and_then(infer_alteryx_one_base_url)
                .is_some_and(|inferred| inferred == base_url)
            {
                self.token_endpoint_url = None;
            }
        }
        for credential in self.workspace_credentials.values_mut() {
            if let Some(url) = credential
                .token_endpoint_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                credential.token_endpoint_url = Some(normalize_alteryx_one_token_endpoint(url));
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerProfile {
    pub webapi_url: String,
    pub curator_api_key: String,
    pub curator_api_secret: String,
    #[serde(default)]
    pub curator_api_secret_ref: Option<String>,
    pub verify_tls: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerApiProfile {
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SqlServerProfile {
    pub controller: Option<SqlServerConnectionProfile>,
    pub server_ui: Option<SqlServerConnectionProfile>,
    #[serde(default)]
    pub legacy_connection_string: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SqlServerConnectionProfile {
    pub connection_string: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub password_ref: Option<String>,
    pub password_env: Option<String>,
    pub integrated_security: Option<bool>,
    pub encrypt: Option<bool>,
    pub trust_server_certificate: Option<bool>,
    pub multi_subnet_failover: Option<bool>,
}

impl ServerProfile {
    pub fn verify_tls(&self) -> bool {
        self.verify_tls.unwrap_or(true)
    }
}

impl Config {
    pub fn load_from_path(path: &Path) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path(&resolved)
    }

    pub fn load_from_path_with_environment(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_or_workspace_path(path)?;
        Self::load_from_resolved_path_with_environment(&resolved, environment)
    }

    pub fn load_from_path_lenient(path: &Path) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path_lenient(&resolved)
    }

    pub fn load_from_path_lenient_without_active_overlay(
        path: &Path,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_path(path)?;
        Self::load_from_resolved_path_lenient_without_active_overlay(&resolved)
    }

    pub fn load_from_path_with_environment_lenient(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_or_workspace_path(path)?;
        Self::load_from_resolved_path_with_environment_lenient(&resolved, environment)
    }

    pub fn load_runtime_profile_with_environment(
        profile: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolution = resolve_runtime_profile(profile)?;
        Self::load_from_resolved_path_with_environment(
            Path::new(&resolution.resolved_profile_path),
            environment,
        )
    }

    pub fn load_runtime_profile_with_environment_lenient(
        profile: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolution = resolve_runtime_profile(profile)?;
        Self::load_from_resolved_path_with_environment_lenient(
            Path::new(&resolution.resolved_profile_path),
            environment,
        )
    }

    fn load_from_resolved_path(path: &Path) -> Result<Self, ProfileError> {
        let config = Self::load_from_resolved_path_lenient(path)?;
        config.validate()?;
        Ok(config)
    }

    fn load_from_resolved_path_lenient(path: &Path) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_from_value(path, path_str, value, env_values, None)
    }

    fn load_from_resolved_path_lenient_without_active_overlay(
        path: &Path,
    ) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_without_active_overlay(path, path_str, value, env_values, None)
    }

    fn load_from_resolved_path_with_environment(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = Self::load_from_resolved_path_with_environment_lenient(path, environment)?;
        config.validate()?;
        Ok(config)
    }

    fn load_from_resolved_path_with_environment_lenient(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let (path_str, env_values, value) = Self::read_profile_value(path)?;
        Self::load_config_from_value(path, path_str, value, env_values, environment)
    }

    fn read_profile_value(
        path: &Path,
    ) -> Result<(String, HashMap<String, String>, serde_yaml::Value), ProfileError> {
        let path_str = path.display().to_string();
        let content = fs::read_to_string(path).map_err(|source| ProfileError::Read {
            path: path_str.clone(),
            source,
        })?;
        let env_path = path
            .parent()
            .map(|parent| parent.join(".env"))
            .unwrap_or_else(|| Path::new(".env").to_path_buf());
        let env_values = collect_env_overrides(path).map_err(|source| ProfileError::Read {
            path: env_path.display().to_string(),
            source,
        })?;
        let expanded = expand_env_placeholders(&content, &env_values);
        let value: serde_yaml::Value =
            serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
                path: path_str.clone(),
                source,
            })?;
        Ok((path_str, env_values, normalize_profile_value(value)?))
    }

    fn load_config_from_value(
        path: &Path,
        path_str: String,
        value: serde_yaml::Value,
        env_values: HashMap<String, String>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = if is_workspace_value(&value) {
            let workspace: WorkspaceConfig =
                serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                    path: path_str.clone(),
                    source,
                })?;
            let active = environment.unwrap_or(&workspace.active_environment);
            workspace.environments.get(active).cloned().ok_or_else(|| {
                ProfileError::Invalid(format!(
                    "workspace '{}' does not contain environment '{}'",
                    workspace.workspace_name, active
                ))
            })?
        } else {
            serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                path: path_str,
                source,
            })?
        };

        Self::finalize_loaded_config(config, env_values, path)
    }

    fn load_config_without_active_overlay(
        path: &Path,
        path_str: String,
        value: serde_yaml::Value,
        env_values: HashMap<String, String>,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let config = if is_workspace_value(&value) {
            let workspace: WorkspaceConfig =
                serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                    path: path_str.clone(),
                    source,
                })?;
            let active = environment.unwrap_or(&workspace.active_environment);
            workspace.environments.get(active).cloned().ok_or_else(|| {
                ProfileError::Invalid(format!(
                    "workspace '{}' does not contain environment '{}'",
                    workspace.workspace_name, active
                ))
            })?
        } else {
            serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                path: path_str,
                source,
            })?
        };

        Self::finalize_loaded_config_without_overlay(config, env_values, path)
    }

    fn finalize_loaded_config(
        config: Self,
        env_values: HashMap<String, String>,
        current_path: &Path,
    ) -> Result<Self, ProfileError> {
        let config = apply_env_fallbacks(config, &env_values);
        let config = config.with_server_api_overrides()?.resolve_secret_refs()?;
        Ok(overlay_active_profile_one_from_state(config, current_path))
    }

    fn finalize_loaded_config_without_overlay(
        config: Self,
        env_values: HashMap<String, String>,
        _current_path: &Path,
    ) -> Result<Self, ProfileError> {
        let config = apply_env_fallbacks(config, &env_values);
        let config = config.with_server_api_overrides()?.resolve_secret_refs()?;
        Ok(config)
    }

    fn with_server_api_overrides(mut self) -> Result<Self, ProfileError> {
        if let Some(shared) = &self.server_api {
            if self.api.is_none() {
                self.api = Some(ApiProfile {
                    base_url: normalize_alteryx_base_url(&shared.base_url),
                    auth: ApiAuth {
                        mode: ApiAuthMode::Oauth2ClientCredentials,
                        pat: None,
                        client_id: Some(shared.client_id.clone()),
                        client_secret: Some(shared.client_secret.clone()),
                        client_secret_ref: None,
                        scope: Some(String::new()),
                    },
                    timeout_ms: None,
                });
            }

            if self.server.is_none() {
                self.server = Some(ServerProfile {
                    webapi_url: normalize_alteryx_base_url(&shared.base_url),
                    curator_api_key: shared.client_id.clone(),
                    curator_api_secret: shared.client_secret.clone(),
                    curator_api_secret_ref: None,
                    verify_tls: None,
                });
            }
        }

        Ok(self)
    }

    fn resolve_secret_refs(mut self) -> Result<Self, ProfileError> {
        if let Some(one) = self.alteryx_one.as_mut() {
            if one.access_token.is_none()
                && let Some(reference) = one.access_token_ref.as_deref()
            {
                one.access_token = resolve_secret_ref(reference)?;
            }
            if one.refresh_token.is_none()
                && let Some(reference) = one.refresh_token_ref.as_deref()
            {
                one.refresh_token = resolve_secret_ref(reference)?;
            }
            if one.client_secret.is_none()
                && let Some(reference) = one.client_secret_ref.as_deref()
            {
                one.client_secret = resolve_secret_ref(reference)?;
            }
            for credential in one.workspace_credentials.values_mut() {
                if credential.access_token.is_none()
                    && let Some(reference) = credential.access_token_ref.as_deref()
                {
                    credential.access_token = resolve_secret_ref(reference)?;
                }
                if credential.refresh_token.is_none()
                    && let Some(reference) = credential.refresh_token_ref.as_deref()
                {
                    credential.refresh_token = resolve_secret_ref(reference)?;
                }
                if credential.client_secret.is_none()
                    && let Some(reference) = credential.client_secret_ref.as_deref()
                {
                    credential.client_secret = resolve_secret_ref(reference)?;
                }
            }
            one.canonicalize();
        }

        if let Some(api) = self.api.as_mut()
            && api.auth.client_secret.is_none()
            && let Some(reference) = api.auth.client_secret_ref.as_deref()
        {
            api.auth.client_secret = resolve_secret_ref(reference)?;
        }

        if let Some(server) = self.server.as_mut()
            && server.curator_api_secret.is_empty()
            && let Some(reference) = server.curator_api_secret_ref.as_deref()
            && let Some(secret) = resolve_secret_ref(reference)?
        {
            server.curator_api_secret = secret;
        }

        if let Some(sqlserver) = self.sqlserver.as_mut() {
            for conn in [sqlserver.controller.as_mut(), sqlserver.server_ui.as_mut()]
                .into_iter()
                .flatten()
            {
                if conn.password.is_none()
                    && let Some(reference) = conn.password_ref.as_deref()
                {
                    conn.password = resolve_secret_ref(reference)?;
                }
            }
        }

        if let Some(mongo) = self.mongo.managed.as_mut()
            && mongo.password.is_none()
            && let Some(reference) = mongo.password_ref.as_deref()
        {
            mongo.password = resolve_secret_ref(reference)?;
        }

        Ok(self)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.profile_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "profile_name cannot be empty".to_string(),
            ));
        }

        if self.mongo.databases.gallery_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "mongo.databases.gallery_name cannot be empty".to_string(),
            ));
        }

        if self.mongo.databases.service_name.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "mongo.databases.service_name cannot be empty".to_string(),
            ));
        }

        match self.mongo.mode {
            MongoMode::Embedded => {
                self.mongo.embedded.as_ref().ok_or_else(|| {
                    ProfileError::Invalid("mongo.mode=embedded requires mongo.embedded".to_string())
                })?;
            }
            MongoMode::Managed => {
                let managed = self.mongo.managed.as_ref().ok_or_else(|| {
                    ProfileError::Invalid("mongo.mode=managed requires mongo.managed".to_string())
                })?;

                let has_url = managed.url.as_ref().is_some_and(|u| !u.trim().is_empty());
                let has_host = managed.host.as_ref().is_some_and(|h| !h.trim().is_empty());

                if !has_url && !has_host {
                    return Err(ProfileError::Invalid(
                        "mongo.managed requires either url or host".to_string(),
                    ));
                }

                if managed.port == 0 {
                    return Err(ProfileError::Invalid(
                        "mongo.managed.port must be greater than 0".to_string(),
                    ));
                }
            }
        }

        if let Some(api) = &self.api {
            if api.base_url.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "api.base_url cannot be empty".to_string(),
                ));
            }

            match api.auth.mode {
                ApiAuthMode::Pat => {
                    let has_pat = api.auth.pat.as_ref().is_some_and(|p| !p.trim().is_empty());
                    if !has_pat {
                        return Err(ProfileError::Invalid(
                            "api.auth.mode=pat requires api.auth.pat".to_string(),
                        ));
                    }
                }
                ApiAuthMode::Oauth2ClientCredentials => {
                    let has_client_id = api
                        .auth
                        .client_id
                        .as_ref()
                        .is_some_and(|v| !v.trim().is_empty());
                    let has_client_secret = api
                        .auth
                        .client_secret
                        .as_ref()
                        .is_some_and(|v| !v.trim().is_empty());
                    if !has_client_id || !has_client_secret {
                        return Err(ProfileError::Invalid(
                            "api.auth.mode=oauth2_client_credentials requires client_id and client_secret"
                                .to_string(),
                        ));
                    }
                }
            }
        }

        if let Some(one) = &self.alteryx_one {
            if !one.account_email.contains('@') {
                return Err(ProfileError::Invalid(
                    "alteryx_one.account_email must be a valid email".to_string(),
                ));
            }
            if one.normalized_base_url().is_none() {
                return Err(ProfileError::Invalid(
                    "alteryx_one.base_url is required".to_string(),
                ));
            }
            if let Some(client_id) = &one.oauth_client_id
                && client_id.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.oauth_client_id cannot be empty when set".to_string(),
                ));
            }
            if let Some(client_secret) = &one.client_secret {
                if client_secret.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.client_secret cannot be empty when set".to_string(),
                    ));
                }
                if one
                    .oauth_client_id
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.oauth_client_id is required when client_secret is set"
                            .to_string(),
                    ));
                }
            }
            if let Some(url) = &one.base_url
                && url.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.base_url cannot be empty when set".to_string(),
                ));
            }
            if let Some(url) = &one.token_endpoint_url
                && url.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.token_endpoint_url cannot be empty when set".to_string(),
                ));
            }
            if let Some(token) = &one.access_token
                && token.trim().is_empty()
            {
                return Err(ProfileError::Invalid(
                    "alteryx_one.access_token cannot be empty when set".to_string(),
                ));
            }
            if let Some(token) = &one.refresh_token {
                if token.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.refresh_token cannot be empty when set".to_string(),
                    ));
                }
                if one
                    .oauth_client_id
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.oauth_client_id is required when refresh_token is set"
                            .to_string(),
                    ));
                }
            }
            for (workspace_id, credential) in &one.workspace_credentials {
                let access_token_present = credential
                    .access_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty());
                if !access_token_present {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].access_token is required"
                    )));
                }
                if credential
                    .refresh_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                    && credential
                        .oauth_client_id
                        .as_ref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].oauth_client_id is required when refresh_token is set"
                    )));
                }
                if credential
                    .client_secret
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                    && credential
                        .oauth_client_id
                        .as_ref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(format!(
                        "alteryx_one.workspace_credentials['{workspace_id}'].oauth_client_id is required when client_secret is set"
                    )));
                }
            }
        }

        if let Some(observability) = &self.observability
            && let Some(api_logging) = &observability.api_logging
            && api_logging.enabled
            && api_logging
                .path
                .as_ref()
                .is_some_and(|path| path.trim().is_empty())
        {
            return Err(ProfileError::Invalid(
                "observability.api_logging.path cannot be empty when enabled".to_string(),
            ));
        }

        if let Some(server) = &self.server {
            if server.webapi_url.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.webapi_url cannot be empty".to_string(),
                ));
            }
            if server.curator_api_key.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.curator_api_key cannot be empty".to_string(),
                ));
            }
            if server.curator_api_secret.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "server.curator_api_secret cannot be empty".to_string(),
                ));
            }
        }

        if let Some(sql) = &self.sqlserver {
            validate_sql_connection(sql.controller.as_ref(), "sqlserver.controller")?;
            validate_sql_connection(sql.server_ui.as_ref(), "sqlserver.server_ui")?;
        }

        Ok(())
    }
}

pub fn load_workspace_config(path: &Path) -> Result<WorkspaceConfig, ProfileError> {
    let resolved = resolve_profile_or_workspace_path(path)?;
    load_workspace_config_from_resolved(&resolved)
}

fn is_default_auth_mode(mode: &AuthMode) -> bool {
    *mode == AuthMode::default()
}

pub fn normalize_alteryx_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    let stripped = trimmed
        .strip_suffix("/webapi")
        .or_else(|| trimmed.strip_suffix("/gallery"))
        .unwrap_or(trimmed);
    stripped.trim_end_matches('/').to_string()
}

pub fn normalize_alteryx_one_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn derive_alteryx_one_token_endpoint(base_url: &str) -> String {
    format!("{}/as/token", base_url.trim().trim_end_matches('/'))
}

pub fn normalize_alteryx_one_token_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/as") {
        derive_alteryx_one_token_endpoint(trimmed.trim_end_matches("/as").trim_end_matches('/'))
    } else {
        trimmed.to_string()
    }
}

pub fn infer_alteryx_one_base_url(token_endpoint_url: &str) -> Option<String> {
    let trimmed = token_endpoint_url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/as/token")
        .or_else(|| trimmed.strip_suffix("/as"))
        .and_then(normalize_alteryx_one_base_url)
}

fn validate_sql_connection(
    conn: Option<&SqlServerConnectionProfile>,
    field: &str,
) -> Result<(), ProfileError> {
    if let Some(conn) = conn {
        if conn
            .connection_string
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
        {
            return Err(ProfileError::Invalid(format!(
                "{field}.connection_string cannot be empty when set"
            )));
        }
        if conn.host.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.host cannot be empty when set"
            )));
        }
        if conn.database.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.database cannot be empty when set"
            )));
        }
        if conn.password.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(ProfileError::Invalid(format!(
                "{field}.password cannot be empty when set"
            )));
        }
    }
    Ok(())
}

fn read_env_file_if_present(path: &Path) -> std::io::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    if !path.exists() {
        return Ok(values);
    }

    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        values.insert(
            key.to_string(),
            value.trim_matches('"').trim_matches('\'').to_string(),
        );
    }
    Ok(values)
}

fn collect_env_overrides(profile_path: &Path) -> std::io::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    if let Ok(cwd) = env::current_dir() {
        values.extend(read_env_file_if_present(&cwd.join(".env"))?);
    }
    if let Some(parent) = profile_path.parent() {
        values.extend(read_env_file_if_present(&parent.join(".env"))?);
    }
    Ok(values)
}

fn expand_env_placeholders(input: &str, env_values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            let _ = chars.next();
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c == '}' {
                    break;
                }
                name.push(c);
            }
            if let Some(value) = env_values.get(&name) {
                out.push_str(value);
            } else if let Ok(value) = std::env::var(&name) {
                out.push_str(&value);
            } else {
                out.push_str("${");
                out.push_str(&name);
                out.push('}');
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn flatten_alteryx_server_block(value: serde_yaml::Value) -> serde_yaml::Value {
    let Some(root) = value.as_mapping() else {
        return value;
    };

    let alteryx_server_key = serde_yaml::Value::String("alteryx_server".to_string());
    let Some(alteryx_server_value) = root.get(&alteryx_server_key) else {
        return value;
    };
    let Some(alteryx_server_map) = alteryx_server_value.as_mapping() else {
        return value;
    };

    let mut merged = root.clone();
    for key in ["server_api", "mongo"] {
        let key_value = serde_yaml::Value::String(key.to_string());
        if merged.contains_key(&key_value) {
            continue;
        }
        if let Some(child) = alteryx_server_map.get(&key_value) {
            merged.insert(key_value, child.clone());
        }
    }

    serde_yaml::Value::Mapping(merged)
}

fn is_workspace_value(value: &serde_yaml::Value) -> bool {
    value.as_mapping().is_some_and(|map| {
        map.contains_key(serde_yaml::Value::String("workspace_name".to_string()))
            && map.contains_key(serde_yaml::Value::String("active_environment".to_string()))
            && map.contains_key(serde_yaml::Value::String("environments".to_string()))
    })
}

fn env_value(env_values: &HashMap<String, String>, name: &str) -> Option<String> {
    env_values
        .get(name)
        .cloned()
        .or_else(|| env::var(name).ok())
}

fn apply_env_fallbacks(mut config: Config, env_values: &HashMap<String, String>) -> Config {
    let account_email = env_value(env_values, "AYX_ACCOUNT_EMAIL");
    let base_url = env_value(env_values, "AYX_ONE_BASE_URL");
    let oauth_client_id = env_value(env_values, "AYX_ONE_OAUTH_CLIENT_ID")
        .or_else(|| env_value(env_values, "AYX_ONE_CLIENT_ID"));
    let token_endpoint_url = env_value(env_values, "AYX_ONE_TOKEN_ENDPOINT_URL");
    let access_token = env_value(env_values, "AYX_ONE_API_ACCESS_TOKEN");
    let refresh_token = env_value(env_values, "AYX_ONE_API_REFRESH_TOKEN");
    let client_secret = env_value(env_values, "AYX_ONE_CLIENT_SECRET");
    // SP creds: canonical names first, then the workspace-namespaced variants
    // already present in the user's .env (AYX_ONE_ALTERYX_FDE_*).
    let sp_client_id = env_value(env_values, "AYX_ONE_SP_CLIENT_ID")
        .or_else(|| env_value(env_values, "AYX_ONE_ALTERYX_FDE_SP007_CLIENT_ID"));
    let sp_client_secret = env_value(env_values, "AYX_ONE_SP_CLIENT_SECRET")
        .or_else(|| env_value(env_values, "AYX_ONE_ALTERYX_FDE_SA007_SECRET"));
    let sp_token_endpoint_url = env_value(env_values, "AYX_ONE_SP_TOKEN_ENDPOINT_URL")
        .or_else(|| env_value(env_values, "AYX_ONE_ALTERYX_FDE_TOKEN_ENDPOINT"));
    let workspace_gid = env_value(env_values, "AYX_ONE_WORKSPACE_GID");

    if account_email.is_some()
        || base_url.is_some()
        || oauth_client_id.is_some()
        || token_endpoint_url.is_some()
        || access_token.is_some()
        || refresh_token.is_some()
        || client_secret.is_some()
        || sp_client_id.is_some()
        || sp_client_secret.is_some()
        || sp_token_endpoint_url.is_some()
        || workspace_gid.is_some()
    {
        let mut one = config.alteryx_one.unwrap_or(AlteryxOneProfile {
            account_email: account_email.clone().unwrap_or_default(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: BTreeMap::new(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });
        if let Some(value) = account_email {
            one.account_email = value;
        }
        // Gap-fill rule: env vars fill only when the profile value is absent
        // or empty. A non-empty profile value always wins. This is consistent
        // with the token fields below and makes profiles authoritative.
        if one
            .base_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.base_url = base_url;
        }
        if one
            .oauth_client_id
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.oauth_client_id = oauth_client_id;
        }
        if one
            .client_secret
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.client_secret = client_secret;
        }
        if one
            .token_endpoint_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.token_endpoint_url = token_endpoint_url;
        }
        // Only apply env-fallback tokens when there is no _ref already in the
        // profile.  A _ref (inline or keyring) is the authoritative stored
        // credential; the env var is a last-resort fallback.
        if one
            .access_token
            .as_ref()
            .is_none_or(|v| v.trim().is_empty())
            && one
                .access_token_ref
                .as_ref()
                .is_none_or(|v| v.trim().is_empty())
        {
            one.access_token = access_token;
        }
        if one
            .refresh_token
            .as_ref()
            .is_none_or(|v| v.trim().is_empty())
            && one
                .refresh_token_ref
                .as_ref()
                .is_none_or(|v| v.trim().is_empty())
        {
            one.refresh_token = refresh_token;
        }
        if one
            .sp_client_id
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.sp_client_id = sp_client_id;
        }
        // SP client secret reuses the shared client_secret field when no
        // dedicated sp_client_secret is available.
        if let Some(secret) = sp_client_secret
            && one
                .client_secret
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            one.client_secret = Some(secret);
        }
        if one
            .sp_token_endpoint_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.sp_token_endpoint_url = sp_token_endpoint_url;
        }
        if one
            .workspace_gid
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.workspace_gid = workspace_gid;
        }
        one.canonicalize();
        config.alteryx_one = Some(one);
    }

    config
}

/// Overlay One credentials from the active central profile, unless the
/// profile currently being loaded is itself the active profile file.
fn overlay_active_profile_one_from_state(mut config: Config, current_path: &Path) -> Config {
    let Some(shared_one) = load_active_profile_one_from_state(current_path) else {
        return config;
    };

    config.alteryx_one = match config.alteryx_one.take() {
        Some(current_one) => Some(merge_one_profiles(current_one, &shared_one)),
        None => Some(shared_one),
    };

    config
}

fn load_active_profile_one_from_state(current_path: &Path) -> Option<AlteryxOneProfile> {
    let state = load_ayx_state().ok()?;
    let profile_name = state.active_profile?;
    let path = profile_storage_path(&profile_name).ok()?;
    if path == current_path {
        return None;
    }
    Config::load_from_path_lenient(&path)
        .ok()?
        .alteryx_one
        .map(|mut one| {
            one.canonicalize();
            one
        })
}

fn merge_one_profiles(
    mut current: AlteryxOneProfile,
    fallback: &AlteryxOneProfile,
) -> AlteryxOneProfile {
    if current.account_email.trim().is_empty() {
        current.account_email = fallback.account_email.clone();
    }
    if current
        .base_url
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.base_url = fallback.base_url.clone();
    }
    if current
        .oauth_client_id
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.oauth_client_id = fallback.oauth_client_id.clone();
    }
    if current
        .client_secret
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.client_secret = fallback.client_secret.clone();
    }
    if current.client_secret_ref.is_none() {
        current.client_secret_ref = fallback.client_secret_ref.clone();
    }
    if current
        .token_endpoint_url
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.token_endpoint_url = fallback.token_endpoint_url.clone();
    }
    if current
        .access_token
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.access_token = fallback.access_token.clone();
    }
    if current.access_token_ref.is_none() {
        current.access_token_ref = fallback.access_token_ref.clone();
    }
    if current
        .refresh_token
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        current.refresh_token = fallback.refresh_token.clone();
    }
    if current.refresh_token_ref.is_none() {
        current.refresh_token_ref = fallback.refresh_token_ref.clone();
    }
    for (workspace_id, credential) in &fallback.workspace_credentials {
        current
            .workspace_credentials
            .entry(workspace_id.clone())
            .or_insert_with(|| credential.clone());
    }
    if current
        .sp_client_id
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.sp_client_id = fallback.sp_client_id.clone();
    }
    if current
        .sp_token_endpoint_url
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.sp_token_endpoint_url = fallback.sp_token_endpoint_url.clone();
    }
    if current
        .workspace_gid
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        current.workspace_gid = fallback.workspace_gid.clone();
    }
    current.canonicalize();
    current
}

fn normalize_profile_value(value: serde_yaml::Value) -> Result<serde_yaml::Value, ProfileError> {
    let value = normalize_canonical_server_block(value)?;
    let value = flatten_alteryx_server_block(value);
    if let Some(workspace_value) = value.as_mapping()
        && workspace_value.contains_key(serde_yaml::Value::String("environments".to_string()))
    {
        return normalize_workspace_environments(value);
    }
    Ok(value)
}

fn default_mongo_profile() -> MongoProfile {
    MongoProfile {
        mode: MongoMode::Embedded,
        databases: MongoDatabases {
            gallery_name: "AlteryxGallery".to_string(),
            service_name: "AlteryxService".to_string(),
        },
        embedded: Some(MongoEmbedded {
            runtime_settings_path: Some("RuntimeSettings.xml".to_string()),
            alteryx_service_path: None,
            restore_target_path: None,
        }),
        managed: None,
    }
}

fn default_runtime_settings_path() -> Option<String> {
    Some("RuntimeSettings.xml".to_string())
}

fn default_mongo_port() -> u16 {
    27017
}

pub fn profile_shape_label(value: &serde_yaml::Value) -> &'static str {
    let Some(root) = value.as_mapping() else {
        return "unknown";
    };
    if root.contains_key(serde_yaml::Value::String("workspace_name".to_string())) {
        if let Some(environments) = root
            .get(serde_yaml::Value::String("environments".to_string()))
            .and_then(|value| value.as_mapping())
            && environments.values().any(|env| {
                env.as_mapping().is_some_and(|map| {
                    map.contains_key(serde_yaml::Value::String("server".to_string()))
                })
            })
        {
            return "workspace-canonical";
        }
        return "workspace-legacy";
    }
    if let Some(server) = root
        .get(serde_yaml::Value::String("server".to_string()))
        .and_then(|value| value.as_mapping())
    {
        if server.contains_key(serde_yaml::Value::String("api".to_string()))
            || server.contains_key(serde_yaml::Value::String("storage".to_string()))
        {
            return "canonical";
        }
        return "legacy";
    }
    if root.contains_key(serde_yaml::Value::String("alteryx_server".to_string()))
        || root.contains_key(serde_yaml::Value::String("server_api".to_string()))
        || root.contains_key(serde_yaml::Value::String("mongo".to_string()))
        || root.contains_key(serde_yaml::Value::String("sqlserver".to_string()))
    {
        return "legacy";
    }
    "unknown"
}

fn normalize_workspace_environments(
    value: serde_yaml::Value,
) -> Result<serde_yaml::Value, ProfileError> {
    let Some(root) = value.as_mapping() else {
        return Ok(value);
    };
    let mut merged = root.clone();
    let env_key = serde_yaml::Value::String("environments".to_string());
    let Some(envs_value) = merged.get_mut(&env_key) else {
        return Ok(serde_yaml::Value::Mapping(merged));
    };
    let Some(envs_map) = envs_value.as_mapping_mut() else {
        return Err(ProfileError::Invalid(
            "workspace.environments must be a mapping".to_string(),
        ));
    };
    for env_value in envs_map.values_mut() {
        let normalized = normalize_canonical_server_block(env_value.clone())?;
        *env_value = flatten_alteryx_server_block(normalized);
    }
    Ok(serde_yaml::Value::Mapping(merged))
}

fn normalize_canonical_server_block(
    value: serde_yaml::Value,
) -> Result<serde_yaml::Value, ProfileError> {
    let Some(root) = value.as_mapping() else {
        return Ok(value);
    };

    let server_key = serde_yaml::Value::String("server".to_string());
    let Some(server_value) = root.get(&server_key) else {
        return Ok(value);
    };
    let Some(server_map) = server_value.as_mapping() else {
        return Ok(value);
    };

    let api_key = serde_yaml::Value::String("api".to_string());
    let storage_key = serde_yaml::Value::String("storage".to_string());
    if !server_map.contains_key(&api_key) && !server_map.contains_key(&storage_key) {
        return Ok(value);
    }

    let mut merged = root.clone();
    let mut legacy_server_api = None;
    let mut legacy_mongo = None;
    let mut legacy_sqlserver = None;

    if let Some(api_value) = server_map.get(&api_key) {
        legacy_server_api = Some(api_value.clone());
    }

    if let Some(storage_value) = server_map.get(&storage_key) {
        let Some(storage_map) = storage_value.as_mapping() else {
            return Err(ProfileError::Invalid(
                "server.storage must be a mapping".to_string(),
            ));
        };
        let kind_key = serde_yaml::Value::String("kind".to_string());
        let kind = storage_map
            .get(&kind_key)
            .and_then(|value| value.as_str())
            .unwrap_or("embedded-mongo");
        let mongo_key = serde_yaml::Value::String("mongo".to_string());
        let sqlserver_key = serde_yaml::Value::String("sqlserver".to_string());
        if let Some(mongo_value) = storage_map.get(&mongo_key) {
            legacy_mongo = Some(mongo_value.clone());
        }
        if let Some(sql_value) = storage_map.get(&sqlserver_key) {
            legacy_sqlserver = Some(sql_value.clone());
        }
        match kind {
            "embedded-mongo" | "managed-mongo" | "sqlserver" | "sql-server" => {}
            other => {
                return Err(ProfileError::Invalid(format!(
                    "server.storage.kind '{}' is not supported",
                    other
                )));
            }
        }
    }

    merged.remove(&server_key);
    if let Some(value) = legacy_server_api {
        merged.insert(serde_yaml::Value::String("server_api".to_string()), value);
    }
    if let Some(value) = legacy_mongo {
        merged.insert(serde_yaml::Value::String("mongo".to_string()), value);
    }
    if let Some(value) = legacy_sqlserver {
        merged.insert(serde_yaml::Value::String("sqlserver".to_string()), value);
    }
    Ok(serde_yaml::Value::Mapping(merged))
}

pub fn canonical_profile_value(config: &Config) -> Result<serde_yaml::Value, ProfileError> {
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("profile_name".to_string()),
        serde_yaml::to_value(&config.profile_name).map_err(|source| ProfileError::Parse {
            path: "profile_name".to_string(),
            source,
        })?,
    );
    if let Some(one) = &config.alteryx_one {
        root.insert(
            serde_yaml::Value::String("alteryx_one".to_string()),
            serde_yaml::to_value(one).map_err(|source| ProfileError::Parse {
                path: "alteryx_one".to_string(),
                source,
            })?,
        );
    }
    if let Some(observability) = &config.observability {
        root.insert(
            serde_yaml::Value::String("observability".to_string()),
            serde_yaml::to_value(observability).map_err(|source| ProfileError::Parse {
                path: "observability".to_string(),
                source,
            })?,
        );
    }
    if let Some(upgrade) = &config.upgrade {
        root.insert(
            serde_yaml::Value::String("upgrade".to_string()),
            serde_yaml::to_value(upgrade).map_err(|source| ProfileError::Parse {
                path: "upgrade".to_string(),
                source,
            })?,
        );
    }
    if let Some(server) = canonical_server_value(config)? {
        root.insert(serde_yaml::Value::String("server".to_string()), server);
    }
    Ok(serde_yaml::Value::Mapping(root))
}

pub fn canonical_workspace_value(
    workspace: &WorkspaceConfig,
) -> Result<serde_yaml::Value, ProfileError> {
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("workspace_name".to_string()),
        serde_yaml::to_value(&workspace.workspace_name).map_err(|source| ProfileError::Parse {
            path: "workspace_name".to_string(),
            source,
        })?,
    );
    root.insert(
        serde_yaml::Value::String("active_environment".to_string()),
        serde_yaml::to_value(&workspace.active_environment).map_err(|source| {
            ProfileError::Parse {
                path: "active_environment".to_string(),
                source,
            }
        })?,
    );
    let mut env_map = serde_yaml::Mapping::new();
    let mut env_names = workspace.environments.keys().cloned().collect::<Vec<_>>();
    env_names.sort();
    for name in env_names {
        let config = workspace.environments.get(&name).ok_or_else(|| {
            ProfileError::Invalid(format!(
                "workspace '{}' is missing environment '{}'",
                workspace.workspace_name, name
            ))
        })?;
        env_map.insert(
            serde_yaml::Value::String(name),
            canonical_profile_value(config)?,
        );
    }
    root.insert(
        serde_yaml::Value::String("environments".to_string()),
        serde_yaml::Value::Mapping(env_map),
    );
    Ok(serde_yaml::Value::Mapping(root))
}

fn canonical_server_value(config: &Config) -> Result<Option<serde_yaml::Value>, ProfileError> {
    let api = config.server_api.clone().or_else(|| {
        config
            .api
            .as_ref()
            .and_then(api_profile_to_server_api)
            .or_else(|| {
                config.server.as_ref().map(|server| ServerApiProfile {
                    base_url: server.webapi_url.clone(),
                    client_id: server.curator_api_key.clone(),
                    client_secret: server.curator_api_secret.clone(),
                })
            })
    });
    let Some(api) = api else {
        return Ok(None);
    };

    let mut storage = serde_yaml::Mapping::new();
    let kind = if config.sqlserver.is_some() {
        ServerStorageKind::SqlServer
    } else {
        match config.mongo.mode {
            MongoMode::Embedded => ServerStorageKind::EmbeddedMongo,
            MongoMode::Managed => ServerStorageKind::ManagedMongo,
        }
    };
    storage.insert(
        serde_yaml::Value::String("kind".to_string()),
        serde_yaml::to_value(kind).map_err(|source| ProfileError::Parse {
            path: "server.storage.kind".to_string(),
            source,
        })?,
    );
    storage.insert(
        serde_yaml::Value::String("mongo".to_string()),
        serde_yaml::to_value(&config.mongo).map_err(|source| ProfileError::Parse {
            path: "server.storage.mongo".to_string(),
            source,
        })?,
    );
    if let Some(sqlserver) = &config.sqlserver {
        storage.insert(
            serde_yaml::Value::String("sqlserver".to_string()),
            serde_yaml::to_value(sqlserver).map_err(|source| ProfileError::Parse {
                path: "server.storage.sqlserver".to_string(),
                source,
            })?,
        );
    }

    let mut server = serde_yaml::Mapping::new();
    server.insert(
        serde_yaml::Value::String("api".to_string()),
        serde_yaml::to_value(api).map_err(|source| ProfileError::Parse {
            path: "server.api".to_string(),
            source,
        })?,
    );
    server.insert(
        serde_yaml::Value::String("storage".to_string()),
        serde_yaml::Value::Mapping(storage),
    );
    Ok(Some(serde_yaml::Value::Mapping(server)))
}

fn api_profile_to_server_api(api: &ApiProfile) -> Option<ServerApiProfile> {
    let client_id = api.auth.client_id.as_ref()?.clone();
    let client_secret = api.auth.client_secret.as_ref()?.clone();
    Some(ServerApiProfile {
        base_url: api.base_url.clone(),
        client_id,
        client_secret,
    })
}

pub fn ayx_config_home() -> Result<PathBuf, ProfileError> {
    if let Some(path) = env::var_os("AYX_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("ayx"));
    }
    if cfg!(windows)
        && let Some(path) = env::var_os("APPDATA")
    {
        return Ok(PathBuf::from(path).join("ayx"));
    }
    if let Some(path) = env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".config").join("ayx"));
    }
    if cfg!(windows)
        && let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH"))
    {
        return Ok(PathBuf::from(format!(
            "{}{}",
            PathBuf::from(drive).display(),
            PathBuf::from(path).display()
        ))
        .join(".config")
        .join("ayx"));
    }
    Err(ProfileError::Invalid(
        "unable to resolve ayx config home; set AYX_CONFIG_HOME".to_string(),
    ))
}

pub fn ayx_profiles_dir() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("profiles"))
}

pub fn ayx_workspaces_dir() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("workspaces"))
}

pub fn ayx_state_path() -> Result<PathBuf, ProfileError> {
    Ok(ayx_config_home()?.join("state.yaml"))
}

pub fn load_ayx_state() -> Result<AyxState, ProfileError> {
    let path = ayx_state_path()?;
    if !path.exists() {
        return Ok(AyxState::default());
    }
    let content = fs::read_to_string(&path).map_err(|source| ProfileError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let mut state: AyxState =
        serde_yaml::from_str(&content).map_err(|source| ProfileError::Parse {
            path: path.display().to_string(),
            source,
        })?;
    state.active_profile = state
        .active_profile
        .map(|name| normalize_storage_name(&name));
    state.active_workspace = state
        .active_workspace
        .map(|name| normalize_storage_name(&name));
    Ok(state)
}

pub fn save_ayx_state(state: &AyxState) -> Result<(), ProfileError> {
    let path = ayx_state_path()?;
    let body = serde_yaml::to_string(state).map_err(|source| ProfileError::Parse {
        path: path.display().to_string(),
        source,
    })?;
    write_sensitive_file(&path, body.as_bytes()).map_err(|err| match err {
        crate::sensitive::SensitiveIoError::CreateDir { path, source }
        | crate::sensitive::SensitiveIoError::Write { path, source }
        | crate::sensitive::SensitiveIoError::Append { path, source } => {
            ProfileError::Write { path, source }
        }
    })
}

pub fn profile_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_profiles_dir()?.join(format!("{}.yaml", normalize_storage_name(name))))
}

pub fn workspace_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_workspaces_dir()?.join(format!("{name}.yaml")))
}

pub fn default_profile_storage_path() -> Result<PathBuf, ProfileError> {
    let state = load_ayx_state()?;
    profile_path_for_name(
        state
            .active_profile
            .as_deref()
            .unwrap_or(DEFAULT_ACTIVE_PROFILE_NAME),
    )
}

pub fn default_workspace_storage_path() -> Result<PathBuf, ProfileError> {
    let state = load_ayx_state()?;
    workspace_storage_path(
        state
            .active_workspace
            .as_deref()
            .unwrap_or(DEFAULT_ACTIVE_WORKSPACE_NAME),
    )
}

pub fn resolve_profile_path(path: &Path) -> Result<PathBuf, ProfileError> {
    resolve_path_internal(path, false)
}

pub fn resolve_profile_or_workspace_path(path: &Path) -> Result<PathBuf, ProfileError> {
    resolve_path_internal(path, true)
}

pub fn profile_resolution_detail(path: &Path) -> Result<ResolvedProfilePath, ProfileError> {
    let state = load_ayx_state()?;
    let requested = path.display().to_string();
    let resolved = resolve_profile_or_workspace_path(path)?;
    let source = if resolved == path {
        "explicit".to_string()
    } else if is_default_environments_request(path) {
        "environments-state".to_string()
    } else if is_default_profile_request(path) {
        "profile-state".to_string()
    } else {
        "resolved".to_string()
    };
    Ok(ResolvedProfilePath {
        requested_path: requested,
        resolved_path: resolved.display().to_string(),
        source,
        active_profile: state.active_profile,
        active_workspace: state.active_workspace,
    })
}

pub fn resolve_runtime_profile(
    profile: Option<&str>,
) -> Result<RuntimeProfileResolution, ProfileError> {
    let config_home = ayx_config_home()?.display().to_string();
    let state = load_ayx_state()?;
    let (selected_profile, selection_source) =
        match profile.map(str::trim).filter(|v| !v.is_empty()) {
            Some(name) => (normalize_runtime_profile_name(name)?, "cli".to_string()),
            None => {
                if let Ok(env_profile) = env::var("AYX_PROFILE") {
                    let env_profile = env_profile.trim();
                    if !env_profile.is_empty() {
                        (
                            normalize_runtime_profile_name(env_profile)?,
                            "environment".to_string(),
                        )
                    } else if let Some(active) = state.active_profile.clone() {
                        (active, "state".to_string())
                    } else {
                        (
                            DEFAULT_ACTIVE_PROFILE_NAME.to_string(),
                            "default".to_string(),
                        )
                    }
                } else if let Some(active) = state.active_profile.clone() {
                    (active, "state".to_string())
                } else {
                    (
                        DEFAULT_ACTIVE_PROFILE_NAME.to_string(),
                        "default".to_string(),
                    )
                }
            }
        };
    let resolved_profile_path = profile_path_for_name(&selected_profile)?
        .display()
        .to_string();
    Ok(RuntimeProfileResolution {
        config_home,
        selected_profile,
        selection_source,
        resolved_profile_path,
        active_profile: state.active_profile,
    })
}

pub fn list_central_profiles() -> Result<Vec<String>, ProfileError> {
    let mut names = list_named_yaml_entries(&ayx_profiles_dir()?)?;
    if ayx_config_home()?
        .join(LEGACY_DEFAULT_PROFILE_FILE)
        .exists()
        && !names.iter().any(|name| name == DEFAULT_ACTIVE_PROFILE_NAME)
    {
        names.push(DEFAULT_ACTIVE_PROFILE_NAME.to_string());
        names.sort();
    }
    Ok(names)
}

pub fn list_central_workspaces() -> Result<Vec<String>, ProfileError> {
    list_named_yaml_entries(&ayx_workspaces_dir()?)
}

fn list_named_yaml_entries(dir: &Path) -> Result<Vec<String>, ProfileError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| ProfileError::Read {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| ProfileError::Read {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|v| v.to_str()) {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn resolve_path_internal(path: &Path, allow_workspace: bool) -> Result<PathBuf, ProfileError> {
    if is_explicit_path(path) {
        return Ok(path.to_path_buf());
    }

    if allow_workspace && is_default_environments_request(path) {
        if let Some(workspace) = env::var_os("AYX_WORKSPACE") {
            return Ok(PathBuf::from(workspace));
        }
        let state = load_ayx_state()?;
        if let Some(name) = state.active_workspace {
            return workspace_storage_path(&name);
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return workspace_storage_path(DEFAULT_ACTIVE_WORKSPACE_NAME);
    }

    if is_default_profile_request(path) {
        if let Some(profile) = env::var_os("AYX_PROFILE") {
            return Ok(PathBuf::from(profile));
        }
        let state = load_ayx_state()?;
        if let Some(name) = state.active_profile {
            return profile_path_for_name(&name);
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return profile_path_for_name(DEFAULT_ACTIVE_PROFILE_NAME);
    }

    Ok(path.to_path_buf())
}

fn normalize_runtime_profile_name(name: &str) -> Result<String, ProfileError> {
    let trimmed = normalize_storage_name(name);
    if trimmed.is_empty() {
        return Err(ProfileError::Invalid(
            "runtime profile name must not be empty".to_string(),
        ));
    }
    let candidate = Path::new(&trimmed);
    if candidate.is_absolute()
        || candidate.components().count() > 1
        || trimmed == DEFAULT_PROFILE_FILE
        || trimmed == DEFAULT_ENVIRONMENTS_FILE
        || trimmed == LEGACY_WORKSPACE_FILE
    {
        return Err(ProfileError::Invalid(format!(
            "runtime profile '{trimmed}' must be a central profile name, not a path or config file"
        )));
    }
    Ok(trimmed)
}

fn normalize_storage_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(stripped) = trimmed.strip_suffix(".yaml") {
        return stripped.to_string();
    }
    if let Some(stripped) = trimmed.strip_suffix(".yml") {
        return stripped.to_string();
    }
    trimmed.to_string()
}

fn profile_path_for_name(name: &str) -> Result<PathBuf, ProfileError> {
    let normalized = normalize_storage_name(name);
    let canonical = profile_storage_path(&normalized)?;
    if canonical.exists() {
        return Ok(canonical);
    }
    if normalized == DEFAULT_ACTIVE_PROFILE_NAME {
        let legacy = ayx_config_home()?.join(LEGACY_DEFAULT_PROFILE_FILE);
        if legacy.exists() {
            return Ok(legacy);
        }
    }
    Ok(canonical)
}

fn is_default_profile_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_PROFILE_FILE)
}

fn is_default_environments_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_ENVIRONMENTS_FILE)
        || is_single_component_file(path, LEGACY_WORKSPACE_FILE)
}

fn is_single_component_file(path: &Path, file_name: &str) -> bool {
    path.file_name().and_then(|v| v.to_str()) == Some(file_name) && path.components().count() == 1
}

fn is_explicit_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::RootDir
            )
        })
        || (!is_default_profile_request(path) && !is_default_environments_request(path))
}

fn load_workspace_config_from_resolved(path: &Path) -> Result<WorkspaceConfig, ProfileError> {
    let path_str = path.display().to_string();
    let content = fs::read_to_string(path).map_err(|source| ProfileError::Read {
        path: path_str.clone(),
        source,
    })?;
    let env_path = path
        .parent()
        .map(|parent| parent.join(".env"))
        .unwrap_or_else(|| Path::new(".env").to_path_buf());
    let env_values = read_env_file_if_present(&env_path).map_err(|source| ProfileError::Read {
        path: env_path.display().to_string(),
        source,
    })?;
    let expanded = expand_env_placeholders(&content, &env_values);

    let value: serde_yaml::Value =
        serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
            path: path_str.clone(),
            source,
        })?;
    let value = normalize_profile_value(value)?;
    let workspace: WorkspaceConfig =
        serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
    if !workspace
        .environments
        .contains_key(&workspace.active_environment)
    {
        return Err(ProfileError::Invalid(format!(
            "workspace '{}' does not contain active environment '{}'",
            workspace.workspace_name, workspace.active_environment
        )));
    }
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    static TEST_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var(key).ok();
            // Tests serialize env access with TEST_ENV_LOCK.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                // Tests serialize env access with TEST_ENV_LOCK.
                unsafe {
                    std::env::set_var(self.key, old);
                }
            } else {
                // Tests serialize env access with TEST_ENV_LOCK.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    struct CurrentDirGuard {
        old: PathBuf,
    }

    impl CurrentDirGuard {
        fn set(dir: &Path) -> Self {
            let old = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self { old }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.old);
        }
    }

    fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn base_config(profile_name: &str, database: &str) -> Config {
        Config {
            profile_name: profile_name.to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: Some("RuntimeSettings.xml".to_string()),
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(AlteryxOneProfile {
                account_email: "user@example.com".to_string(),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: None,
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
                workspace_credentials: Default::default(),
                expected_workspace_id: None,
                sp_client_id: None,
                sp_token_endpoint_url: None,
                workspace_gid: None,
                auth_mode: AuthMode::default(),
            }),
            observability: None,
            server_api: Some(ServerApiProfile {
                base_url: "http://localhost/webapi/".to_string(),
                client_id: "client".to_string(),
                client_secret: "secret".to_string(),
            }),
            api: None,
            server: None,
            sqlserver: Some(SqlServerProfile {
                controller: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("localhost".to_string()),
                    port: Some(1433),
                    database: Some(database.to_string()),
                    username: Some("sa".to_string()),
                    password: Some("secret".to_string()),
                    password_ref: None,
                    password_env: Some("AYX_SQL_CONTROLLER_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(false),
                }),
                server_ui: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("localhost".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxServerUI".to_string()),
                    username: Some("sa".to_string()),
                    password: Some("secret".to_string()),
                    password_ref: None,
                    password_env: Some("AYX_SQL_SERVER_UI_PASSWORD".to_string()),
                    integrated_security: Some(false),
                    encrypt: Some(true),
                    trust_server_certificate: Some(false),
                    multi_subnet_failover: Some(false),
                }),
                legacy_connection_string: None,
            }),
            upgrade: None,
        }
    }

    #[test]
    fn loads_active_workspace_environment() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        let workspace = serde_yaml::to_string(&serde_yaml::Value::Mapping(
            [("workspace_name", "lab"), ("active_environment", "dev")]
                .into_iter()
                .map(|(k, v)| {
                    (
                        serde_yaml::Value::String(k.to_string()),
                        serde_yaml::Value::String(v.to_string()),
                    )
                })
                .chain(std::iter::once((
                    serde_yaml::Value::String("environments".to_string()),
                    serde_yaml::to_value(serde_yaml::Mapping::from_iter([
                        (
                            serde_yaml::Value::String("dev".to_string()),
                            serde_yaml::to_value(base_config("dev", "AlteryxService")).unwrap(),
                        ),
                        (
                            serde_yaml::Value::String("prod".to_string()),
                            serde_yaml::to_value(base_config("prod", "ProdService")).unwrap(),
                        ),
                    ]))
                    .unwrap(),
                )))
                .collect(),
        ))
        .unwrap();
        file.write_all(workspace.as_bytes()).unwrap();

        let cfg = Config::load_from_path_with_environment(temp.path(), None).unwrap();
        assert_eq!(cfg.profile_name, "dev");
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("AlteryxService")
        );
    }

    #[test]
    fn loads_named_workspace_environment_override() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        let workspace = serde_yaml::to_string(&serde_yaml::Value::Mapping(
            [("workspace_name", "lab"), ("active_environment", "dev")]
                .into_iter()
                .map(|(k, v)| {
                    (
                        serde_yaml::Value::String(k.to_string()),
                        serde_yaml::Value::String(v.to_string()),
                    )
                })
                .chain(std::iter::once((
                    serde_yaml::Value::String("environments".to_string()),
                    serde_yaml::to_value(serde_yaml::Mapping::from_iter([
                        (
                            serde_yaml::Value::String("dev".to_string()),
                            serde_yaml::to_value(base_config("dev", "DevService")).unwrap(),
                        ),
                        (
                            serde_yaml::Value::String("prod".to_string()),
                            serde_yaml::to_value(base_config("prod", "ProdService")).unwrap(),
                        ),
                    ]))
                    .unwrap(),
                )))
                .collect(),
        ))
        .unwrap();
        file.write_all(workspace.as_bytes()).unwrap();

        let cfg = Config::load_from_path_with_environment(temp.path(), Some("prod")).unwrap();
        assert_eq!(cfg.profile_name, "prod");
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("ProdService")
        );
    }

    #[test]
    fn workspace_inherits_active_profile_one_credentials() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        fs::create_dir_all(config_home.join("profiles")).unwrap();
        fs::create_dir_all(config_home.join("workspaces")).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        let state = AyxState {
            active_profile: Some("shared".to_string()),
            active_workspace: Some("lab".to_string()),
        };
        save_ayx_state(&state).unwrap();

        let mut profile = base_config("shared", "SharedService");
        profile.alteryx_one.as_mut().unwrap().account_email = "shared@example.com".to_string();
        let profile_path = profile_storage_path("shared").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let mut workspace_env = base_config("dev", "DevService");
        workspace_env.alteryx_one = None;
        let workspace = WorkspaceConfig {
            workspace_name: "lab".to_string(),
            active_environment: "dev".to_string(),
            environments: HashMap::from([(String::from("dev"), workspace_env)]),
        };
        let workspace_path = workspace_storage_path("lab").unwrap();
        fs::write(&workspace_path, serde_yaml::to_string(&workspace).unwrap()).unwrap();

        let cfg = Config::load_from_path_with_environment(
            std::path::Path::new("environments.yaml"),
            None,
        )
        .unwrap();
        assert_eq!(
            cfg.alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("shared@example.com")
        );
        assert_eq!(
            cfg.sqlserver
                .as_ref()
                .unwrap()
                .controller
                .as_ref()
                .unwrap()
                .database
                .as_deref(),
            Some("DevService")
        );
    }

    #[test]
    fn active_profile_one_overlay_does_not_recurse_on_self() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut profile = base_config("default", "ServiceDb");
        profile.alteryx_one.as_mut().unwrap().account_email = "self@example.com".to_string();
        let profile_path = profile_storage_path("default").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient(&profile_path).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("self@example.com")
        );
    }

    #[test]
    fn load_from_path_lenient_without_active_overlay_keeps_source_profile() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        fs::create_dir_all(config_home.join("profiles")).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("shared".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut shared = base_config("shared", "SharedService");
        shared.alteryx_one.as_mut().unwrap().account_email = "shared@example.com".to_string();
        let shared_path = profile_storage_path("shared").unwrap();
        fs::write(&shared_path, serde_yaml::to_string(&shared).unwrap()).unwrap();

        let mut local = base_config("local", "LocalService");
        local.alteryx_one.as_mut().unwrap().account_email = "local@example.com".to_string();
        let local_path = profile_storage_path("local").unwrap();
        fs::write(&local_path, serde_yaml::to_string(&local).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient_without_active_overlay(&local_path).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("local@example.com")
        );
    }

    #[test]
    fn one_token_endpoint_normalizes_issuer_root() {
        let profile = AlteryxOneProfile {
            account_email: "user@example.com".to_string(),
            base_url: Some("https://pingauth.alteryxcloud.com".to_string()),
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(
            profile.effective_token_endpoint_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
        assert_eq!(
            profile.normalized_base_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com")
        );
    }

    #[test]
    fn one_token_endpoint_does_not_infer_api_base_url_from_auth_host() {
        let profile = AlteryxOneProfile {
            account_email: "user@example.com".to_string(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.normalized_base_url(), None);
        assert_eq!(
            profile.effective_token_endpoint_url().as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
    }

    #[test]
    fn one_prefers_expected_workspace_credential_over_legacy_fields() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-1".to_string(),
            WorkspaceCredential {
                access_token: Some("workspace-access".to_string()),
                access_token_ref: None,
                refresh_token: Some("workspace-refresh".to_string()),
                refresh_token_ref: None,
                oauth_client_id: Some("workspace-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: Some("https://pingauth.alteryxcloud.com/as".to_string()),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some("https://legacy.example/as".to_string()),
            access_token: Some("legacy-access".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_credentials,
            expected_workspace_id: Some("ws-1".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.active_workspace_id(), Some("ws-1"));
        assert_eq!(profile.resolved_access_token(), Some("workspace-access"));
        assert_eq!(profile.resolved_refresh_token(), Some("workspace-refresh"));
        assert_eq!(profile.resolved_oauth_client_id(), Some("workspace-client"));
        assert_eq!(
            profile
                .effective_token_endpoint_url_for_workspace(profile.active_workspace_id())
                .as_deref(),
            Some("https://pingauth.alteryxcloud.com/as/token")
        );
    }

    #[test]
    fn one_uses_single_workspace_credential_without_expected_workspace_id() {
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-2".to_string(),
            WorkspaceCredential {
                access_token: Some("single-access".to_string()),
                access_token_ref: None,
                refresh_token: Some("single-refresh".to_string()),
                refresh_token_ref: None,
                oauth_client_id: Some("single-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: Some("https://tenant.example/as".to_string()),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );

        let profile = AlteryxOneProfile {
            account_email: "user@example.com".to_string(),
            base_url: Some("https://us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: Some("legacy-access".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_credentials,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        };

        assert_eq!(profile.active_workspace_id(), Some("ws-2"));
        assert_eq!(profile.resolved_access_token(), Some("single-access"));
        assert_eq!(profile.resolved_refresh_token(), Some("single-refresh"));
        assert_eq!(profile.resolved_oauth_client_id(), Some("single-client"));
        assert_eq!(
            profile
                .effective_token_endpoint_url_for_workspace(profile.active_workspace_id())
                .as_deref(),
            Some("https://tenant.example/as/token")
        );
    }

    #[test]
    fn runtime_profile_loader_does_not_recurse_on_self() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let mut profile = base_config("default", "ServiceDb");
        profile.alteryx_one.as_mut().unwrap().account_email = "runtime@example.com".to_string();
        let profile_path = profile_storage_path("default").unwrap();
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_runtime_profile_with_environment_lenient(None, None).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("runtime@example.com")
        );
    }

    #[test]
    fn runtime_profile_loader_supports_legacy_profile_shape_without_top_level_mongo() {
        let _lock = test_env_lock();
        // Register an in-memory keyring so the loader's secret-ref resolution is
        // hermetic: CI runners are headless (no Secret Service / D-Bus / keychain),
        // so without a store `Entry::new` fails and the load errors. With the mock
        // store present, the unset entries resolve to `None` and the profile loads.
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("ayx-home");
        let profiles_dir = config_home.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();

        let _guard = EnvGuard::set("AYX_CONFIG_HOME", config_home.to_str().unwrap());
        save_ayx_state(&AyxState {
            active_profile: Some("default".to_string()),
            active_workspace: None,
        })
        .unwrap();

        let legacy_profile = r#"
profile_name: local-dev
alteryx_one:
  account_email: ryan.merlin@alteryx.com
  base_url: https://us1.alteryxcloud.com
  oauth_client_id: client-id
  token_endpoint_url: https://pingauth.alteryxcloud.com/as
  access_token_ref: keyring:default/alteryx_one.access_token
  refresh_token_ref: keyring:default/alteryx_one.refresh_token
observability:
  api_logging:
    enabled: false
    path: logs/api-events.jsonl
    redact_bodies: true
    log_requests: false
    log_responses: false
upgrade:
  target_version: "2025.2"
  deployment: embedded-mongo
server:
  api:
    base_url: http://localhost/webapi/
    client_id: client-id
    client_secret: secret
  storage:
    kind: sql-server
    mongo:
      mode: embedded
      databases:
        gallery_name: AlteryxGallery
        service_name: AlteryxService
      embedded:
        runtime_settings_path: null
        alteryx_service_path: null
        restore_target_path: null
      managed:
        url: null
        host: localhost
        port: 27017
        auth_database: admin
        username: user
        password: null
        password_ref: keyring:default/server.storage.mongo.managed.password
        tls:
          enabled: false
          ca_path: null
          cert_path: null
          key_path: null
          allow_invalid_hostnames: false
        timeout_ms: 15000
        retry_count: 2
        max_pool_size: 20
    sqlserver:
      controller:
        connection_string: null
        host: localhost
        port: 1433
        database: AlteryxService
        username: sa
        password: null
        password_ref: keyring:default/server.storage.sqlserver.controller.password
        password_env: AYX_SQL_CONTROLLER_PASSWORD
        integrated_security: false
        encrypt: true
        trust_server_certificate: false
        multi_subnet_failover: false
      server_ui:
        connection_string: null
        host: localhost
        port: 1433
        database: AlteryxServerUI
        username: sa
        password: null
        password_ref: keyring:default/server.storage.sqlserver.server_ui.password
        password_env: AYX_SQL_SERVER_UI_PASSWORD
        integrated_security: false
        encrypt: true
        trust_server_certificate: false
        multi_subnet_failover: false
      legacy_connection_string: null
sqlserver:
  controller:
    connection_string: null
    host: localhost
    port: 1433
    database: AlteryxService
    username: sa
    password: null
    password_ref: keyring:default/server.storage.sqlserver.controller.password
    password_env: AYX_SQL_CONTROLLER_PASSWORD
    integrated_security: false
    encrypt: true
    trust_server_certificate: false
    multi_subnet_failover: false
  server_ui:
    connection_string: null
    host: localhost
    port: 1433
    database: AlteryxServerUI
    username: sa
    password: null
    password_ref: keyring:default/server.storage.sqlserver.server_ui.password
    password_env: AYX_SQL_SERVER_UI_PASSWORD
    integrated_security: false
    encrypt: true
    trust_server_certificate: false
    multi_subnet_failover: false
  legacy_connection_string: null
"#;
        fs::write(profiles_dir.join("default.yaml"), legacy_profile).unwrap();

        let loaded = Config::load_runtime_profile_with_environment_lenient(None, None).unwrap();
        assert_eq!(
            loaded
                .alteryx_one
                .as_ref()
                .map(|one| one.account_email.as_str()),
            Some("ryan.merlin@alteryx.com")
        );
    }

    #[test]
    fn env_file_overrides_stale_profile_auth_fields() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _cwd = CurrentDirGuard::set(temp.path());

        let env_file = temp.path().join(".env");
        fs::write(
            &env_file,
            "AYX_ACCOUNT_EMAIL=fresh@example.com\nAYX_ONE_API_ACCESS_TOKEN=fresh-access\nAYX_ONE_API_REFRESH_TOKEN=fresh-refresh\nAYX_ONE_TOKEN_ENDPOINT_URL=https://pingauth.example.com/as\n",
        )
        .unwrap();

        let profile_path = temp.path().join("config.yaml");
        let profile = base_config("default", "ServiceDb");
        fs::write(&profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();

        let loaded = Config::load_from_path_lenient(&profile_path).unwrap();
        let one = loaded.alteryx_one.as_ref().unwrap();
        assert_eq!(one.account_email, "fresh@example.com");
        assert_eq!(one.access_token.as_deref(), Some("fresh-access"));
        assert_eq!(one.refresh_token.as_deref(), Some("fresh-refresh"));
        assert_eq!(
            one.token_endpoint_url.as_deref(),
            Some("https://pingauth.example.com/as")
        );
    }

    #[test]
    fn normalizes_alteryx_base_urls() {
        assert_eq!(
            normalize_alteryx_base_url("http://host/webapi/"),
            "http://host"
        );
        assert_eq!(
            normalize_alteryx_base_url("http://host/gallery"),
            "http://host"
        );
        assert_eq!(normalize_alteryx_base_url("http://host"), "http://host");
    }

    #[test]
    fn loads_canonical_server_shape() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let canonical = r#"
profile_name: canonical
alteryx_one:
  account_email: user@example.com
  base_url: https://us1.alteryxcloud.com
server:
  api:
    base_url: http://localhost/webapi
    client_id: client
    client_secret: secret
  storage:
    kind: embedded-mongo
    mongo:
      mode: embedded
      databases:
        gallery_name: AlteryxGallery
        service_name: AlteryxService
      embedded:
        runtime_settings_path: RuntimeSettings.xml
"#;
        std::fs::write(temp.path(), canonical).unwrap();
        let cfg = Config::load_from_path(temp.path()).unwrap();
        assert_eq!(cfg.profile_name, "canonical");
        assert_eq!(cfg.server.as_ref().unwrap().webapi_url, "http://localhost");
        assert_eq!(
            cfg.server_api.as_ref().unwrap().base_url,
            "http://localhost/webapi"
        );
        assert!(matches!(cfg.mongo.mode, MongoMode::Embedded));
        assert!(cfg.server.is_some());
    }

    #[test]
    fn resolves_default_profile_from_central_state() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", &temp.path().display().to_string());
        let profiles_dir = temp.path().join("profiles");
        std::fs::create_dir_all(&profiles_dir).unwrap();
        std::fs::write(temp.path().join("state.yaml"), "active_profile: central\n").unwrap();
        std::fs::write(
            profiles_dir.join("central.yaml"),
            serde_yaml::to_string(&base_config("central", "CentralDb")).unwrap(),
        )
        .unwrap();

        let cfg = Config::load_from_path(Path::new("config.yaml")).unwrap();
        assert_eq!(cfg.profile_name, "central");
    }

    #[test]
    fn resolves_legacy_root_default_profile() {
        let _lock = test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", &temp.path().display().to_string());
        std::fs::write(
            temp.path().join("default.yaml"),
            serde_yaml::to_string(&base_config("legacy", "LegacyDb")).unwrap(),
        )
        .unwrap();
        std::fs::write(
            temp.path().join("state.yaml"),
            "active_profile: default.yaml\n",
        )
        .unwrap();

        let path = default_profile_storage_path().unwrap();
        assert_eq!(path, temp.path().join("default.yaml"));

        let cfg = Config::load_from_path(Path::new("config.yaml")).unwrap();
        assert_eq!(cfg.profile_name, "legacy");
    }
}
