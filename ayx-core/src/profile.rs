use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerProfile {
    pub webapi_url: String,
    pub curator_api_key: String,
    pub curator_api_secret: String,
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
        let path_str = path.display().to_string();
        let content = fs::read_to_string(path).map_err(|source| ProfileError::Read {
            path: path_str.clone(),
            source,
        })?;
        let env_path = path
            .parent()
            .map(|parent| parent.join(".env"))
            .unwrap_or_else(|| Path::new(".env").to_path_buf());
        let env_values =
            read_env_file_if_present(&env_path).map_err(|source| ProfileError::Read {
                path: env_path.display().to_string(),
                source,
            })?;
        let expanded = expand_env_placeholders(&content, &env_values);

        let config_value: serde_yaml::Value =
            serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
                path: path_str.clone(),
                source,
            })?;
        let config_value = flatten_alteryx_server_block(config_value);
        let config: Self =
            serde_yaml::from_value(config_value).map_err(|source| ProfileError::Parse {
                path: path_str,
                source,
            })?;
        let config = config.with_server_api_overrides()?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_from_path_with_environment(
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
        let env_values =
            read_env_file_if_present(&env_path).map_err(|source| ProfileError::Read {
                path: env_path.display().to_string(),
                source,
            })?;
        let expanded = expand_env_placeholders(&content, &env_values);

        let value: serde_yaml::Value =
            serde_yaml::from_str(&expanded).map_err(|source| ProfileError::Parse {
                path: path_str.clone(),
                source,
            })?;
        let value = flatten_alteryx_server_block(value);
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
            let config = config.clone().with_server_api_overrides()?;
            config.validate()?;
            return Ok(config);
        }

        let config: Self = serde_yaml::from_value(value).map_err(|source| ProfileError::Parse {
            path: path_str,
            source,
        })?;
        let config = config.with_server_api_overrides()?;
        config.validate()?;
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
                    verify_tls: None,
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
                refresh_token: None,
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
        assert_eq!(
            normalize_alteryx_base_url("http://host"),
            "http://host"
        );
    }
}
