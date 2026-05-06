use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secrets::resolve_secret_ref;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub profile_name: String,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MongoProfile {
    pub mode: MongoMode,
    pub databases: MongoDatabases,
    pub embedded: Option<MongoEmbedded>,
    pub managed: Option<MongoManaged>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MongoDatabases {
    pub gallery_name: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MongoMode {
    Embedded,
    Managed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MongoEmbedded {
    pub runtime_settings_path: Option<String>,
    pub alteryx_service_path: Option<String>,
    pub restore_target_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MongoManaged {
    pub url: Option<String>,
    pub host: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlteryxOneProfile {
    pub account_email: String,
    pub oauth_client_id: Option<String>,
    pub token_endpoint_url: Option<String>,
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_token_ref: Option<String>,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_token_ref: Option<String>,
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

    pub fn load_from_path_with_environment_lenient(
        path: &Path,
        environment: Option<&str>,
    ) -> Result<Self, ProfileError> {
        let resolved = resolve_profile_or_workspace_path(path)?;
        Self::load_from_resolved_path_with_environment_lenient(&resolved, environment)
    }

    fn load_from_resolved_path(path: &Path) -> Result<Self, ProfileError> {
        let config = Self::load_from_resolved_path_lenient(path)?;
        config.validate()?;
        Ok(config)
    }

    fn load_from_resolved_path_lenient(path: &Path) -> Result<Self, ProfileError> {
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
        let value = normalize_profile_value(value)?;
        let config: Self = serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
        let config = apply_env_fallbacks(config, &env_values);
        let config = config.with_server_api_overrides()?.resolve_secret_refs()?;
        Ok(config)
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
        let value = normalize_profile_value(value)?;
        if is_workspace_value(&value) {
            let workspace: WorkspaceConfig =
                serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
                    path: path_str.clone(),
                    source,
                })?;
            let active = environment.unwrap_or(&workspace.active_environment);
            let config = workspace.environments.get(active).ok_or_else(|| {
                ProfileError::Invalid(format!(
                    "workspace '{}' does not contain environment '{}'",
                    workspace.workspace_name, active
                ))
            })?;
            let config = apply_env_fallbacks(config.clone(), &env_values);
            let config = config.with_server_api_overrides()?.resolve_secret_refs()?;
            return Ok(config);
        }
        let config: Self = serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
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
        if self.alteryx_one.is_some() {
            let one = self.alteryx_one.as_mut().unwrap();
            if one.access_token.is_none() {
                if let Some(reference) = one.access_token_ref.as_deref() {
                    one.access_token = resolve_secret_ref(reference)?;
                }
            }
            if one.refresh_token.is_none() {
                if let Some(reference) = one.refresh_token_ref.as_deref() {
                    one.refresh_token = resolve_secret_ref(reference)?;
                }
            }
        }

        if let Some(api) = self.api.as_mut() {
            if api.auth.client_secret.is_none() {
                if let Some(reference) = api.auth.client_secret_ref.as_deref() {
                    api.auth.client_secret = resolve_secret_ref(reference)?;
                }
            }
        }

        if let Some(server) = self.server.as_mut() {
            if server.curator_api_secret.is_empty() {
                if let Some(reference) = server.curator_api_secret_ref.as_deref() {
                    if let Some(secret) = resolve_secret_ref(reference)? {
                        server.curator_api_secret = secret;
                    }
                }
            }
        }

        if let Some(sqlserver) = self.sqlserver.as_mut() {
            for conn in [sqlserver.controller.as_mut(), sqlserver.server_ui.as_mut()] {
                if let Some(conn) = conn {
                    if conn.password.is_none() {
                        if let Some(reference) = conn.password_ref.as_deref() {
                            conn.password = resolve_secret_ref(reference)?;
                        }
                    }
                }
            }
        }

        if let Some(mongo) = self.mongo.managed.as_mut() {
            if mongo.password.is_none() {
                if let Some(reference) = mongo.password_ref.as_deref() {
                    mongo.password = resolve_secret_ref(reference)?;
                }
            }
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
            if let Some(client_id) = &one.oauth_client_id {
                if client_id.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.oauth_client_id cannot be empty when set".to_string(),
                    ));
                }
            }
            if let Some(url) = &one.token_endpoint_url {
                if url.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.token_endpoint_url cannot be empty when set".to_string(),
                    ));
                }
            }
            if let Some(token) = &one.access_token {
                if token.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.access_token cannot be empty when set".to_string(),
                    ));
                }
            }
            if let Some(token) = &one.refresh_token {
                if token.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "alteryx_one.refresh_token cannot be empty when set".to_string(),
                    ));
                }
            }
        }

        if let Some(observability) = &self.observability {
            if let Some(api_logging) = &observability.api_logging {
                if api_logging.enabled
                    && api_logging
                        .path
                        .as_ref()
                        .is_some_and(|path| path.trim().is_empty())
                {
                    return Err(ProfileError::Invalid(
                        "observability.api_logging.path cannot be empty when enabled".to_string(),
                    ));
                }
            }
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

pub fn normalize_alteryx_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    let stripped = trimmed
        .strip_suffix("/webapi")
        .or_else(|| trimmed.strip_suffix("/gallery"))
        .unwrap_or(trimmed);
    stripped.trim_end_matches('/').to_string()
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
    env_values.get(name).cloned().or_else(|| env::var(name).ok())
}

fn apply_env_fallbacks(mut config: Config, env_values: &HashMap<String, String>) -> Config {
    let account_email = env_value(env_values, "AYX_ACCOUNT_EMAIL");
    let oauth_client_id = env_value(env_values, "AYX_ONE_OAUTH_CLIENT_ID");
    let token_endpoint_url = env_value(env_values, "AYX_ONE_TOKEN_ENDPOINT_URL");
    let access_token = env_value(env_values, "AYX_ONE_API_ACCESS_TOKEN");
    let refresh_token = env_value(env_values, "AYX_ONE_API_REFRESH_TOKEN");

    if account_email.is_some()
        || oauth_client_id.is_some()
        || token_endpoint_url.is_some()
        || access_token.is_some()
        || refresh_token.is_some()
    {
        let mut one = config.alteryx_one.unwrap_or(AlteryxOneProfile {
            account_email: account_email.clone().unwrap_or_default(),
            oauth_client_id: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
        });
        if let Some(value) = account_email {
            one.account_email = value;
        }
        if one
            .oauth_client_id
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.oauth_client_id = oauth_client_id;
        }
        if one
            .token_endpoint_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.token_endpoint_url = token_endpoint_url;
        }
        if one
            .access_token
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.access_token = access_token;
        }
        if one
            .refresh_token
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            one.refresh_token = refresh_token;
        }
        config.alteryx_one = Some(one);
    }

    config
}

fn normalize_profile_value(value: serde_yaml::Value) -> Result<serde_yaml::Value, ProfileError> {
    let value = normalize_canonical_server_block(value)?;
    let value = flatten_alteryx_server_block(value);
    if let Some(workspace_value) = value.as_mapping() {
        if workspace_value.contains_key(serde_yaml::Value::String("environments".to_string())) {
            return normalize_workspace_environments(value);
        }
    }
    Ok(value)
}

pub fn profile_shape_label(value: &serde_yaml::Value) -> &'static str {
    let Some(root) = value.as_mapping() else {
        return "unknown";
    };
    if root.contains_key(serde_yaml::Value::String("workspace_name".to_string())) {
        if let Some(environments) = root
            .get(serde_yaml::Value::String("environments".to_string()))
            .and_then(|value| value.as_mapping())
        {
            if environments.values().any(|env| {
                env.as_mapping().is_some_and(|map| {
                    map.contains_key(serde_yaml::Value::String("server".to_string()))
                })
            }) {
                return "workspace-canonical";
            }
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
            return Err(ProfileError::Invalid("server.storage must be a mapping".to_string()));
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
            "embedded-mongo" | "managed-mongo" | "sqlserver" => {}
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
    root.insert(
        serde_yaml::Value::String("server".to_string()),
        canonical_server_value(config)?,
    );
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
        serde_yaml::to_value(&workspace.active_environment).map_err(|source| ProfileError::Parse {
            path: "active_environment".to_string(),
            source,
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

fn canonical_server_value(config: &Config) -> Result<serde_yaml::Value, ProfileError> {
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
    let api = api.ok_or_else(|| {
        ProfileError::Invalid("server.api requires server_api or api credentials".to_string())
    })?;

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
    Ok(serde_yaml::Value::Mapping(server))
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
    if cfg!(windows) {
        if let Some(path) = env::var_os("APPDATA") {
            return Ok(PathBuf::from(path).join("ayx"));
        }
    }
    if let Some(path) = env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".config").join("ayx"));
    }
    if cfg!(windows) {
        if let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
            return Ok(PathBuf::from(format!(
                "{}{}",
                PathBuf::from(drive).display(),
                PathBuf::from(path).display()
            ))
            .join(".config")
            .join("ayx"));
        }
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
    serde_yaml::from_str(&content).map_err(|source| ProfileError::Parse {
        path: path.display().to_string(),
        source,
    })
}

pub fn save_ayx_state(state: &AyxState) -> Result<(), ProfileError> {
    let path = ayx_state_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ProfileError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(&path, serde_yaml::to_string(state).map_err(|source| ProfileError::Parse {
        path: path.display().to_string(),
        source,
    })?)
    .map_err(|source| ProfileError::Write {
        path: path.display().to_string(),
        source,
    })
}

pub fn profile_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_profiles_dir()?.join(format!("{name}.yaml")))
}

pub fn workspace_storage_path(name: &str) -> Result<PathBuf, ProfileError> {
    Ok(ayx_workspaces_dir()?.join(format!("{name}.yaml")))
}

pub fn default_profile_storage_path() -> Result<PathBuf, ProfileError> {
    let state = load_ayx_state()?;
    profile_storage_path(
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

pub fn list_central_profiles() -> Result<Vec<String>, ProfileError> {
    list_named_yaml_entries(&ayx_profiles_dir()?)
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
            return Ok(workspace_storage_path(&name)?);
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
            return Ok(profile_storage_path(&name)?);
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return profile_storage_path(DEFAULT_ACTIVE_PROFILE_NAME);
    }

    Ok(path.to_path_buf())
}

fn is_default_profile_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_PROFILE_FILE)
}

fn is_default_environments_request(path: &Path) -> bool {
    is_single_component_file(path, DEFAULT_ENVIRONMENTS_FILE)
        || is_single_component_file(path, LEGACY_WORKSPACE_FILE)
}

fn is_single_component_file(path: &Path, file_name: &str) -> bool {
    path.file_name().and_then(|v| v.to_str()) == Some(file_name)
        && path.components().count() == 1
}

fn is_explicit_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::CurDir | Component::RootDir)
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

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
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
                oauth_client_id: None,
                token_endpoint_url: None,
                access_token: None,
                access_token_ref: None,
                refresh_token: None,
                refresh_token_ref: None,
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
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("AYX_CONFIG_HOME", &temp.path().display().to_string());
        let profiles_dir = temp.path().join("profiles");
        std::fs::create_dir_all(&profiles_dir).unwrap();
        std::fs::write(
            temp.path().join("state.yaml"),
            "active_profile: central\n",
        )
        .unwrap();
        std::fs::write(
            profiles_dir.join("central.yaml"),
            serde_yaml::to_string(&base_config("central", "CentralDb")).unwrap(),
        )
        .unwrap();

        let cfg = Config::load_from_path(Path::new("config.yaml")).unwrap();
        assert_eq!(cfg.profile_name, "central");
    }
}
