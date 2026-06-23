use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::profile::{
    AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded, MongoManaged, MongoMode,
    MongoProfile, ServerProfile, SqlServerConnectionProfile, SqlServerProfile, TlsConfig,
    WorkspaceConfig, canonical_profile_value, canonical_workspace_value,
    default_profile_storage_path, default_workspace_storage_path, detect_secret_conflict,
    normalize_alteryx_base_url,
};
use ayx_core::secrets::{keyring_account, resolve_secret_ref, store_secret_with_fallback};
use ayx_core::sensitive::write_sensitive_file;
use ayx_server::util::runtime_settings_summary;

pub fn run_onboarding(
    profile_path: &Path,
    environment: Option<&str>,
    non_interactive: bool,
    workspace_mode: bool,
) -> Result<Value> {
    let resolved_path = if workspace_mode {
        if profile_path == Path::new("environments.yaml")
            || profile_path == Path::new("workspace.yaml")
        {
            default_workspace_storage_path().map_err(anyhow::Error::from)?
        } else {
            profile_path.to_path_buf()
        }
    } else if profile_path == Path::new("config.yaml") {
        default_profile_storage_path().map_err(anyhow::Error::from)?
    } else {
        profile_path.to_path_buf()
    };

    if workspace_mode {
        let active_environment = environment.unwrap_or("dev");
        return write_workspace_template(&resolved_path, active_environment, "dev", "prod");
    }
    let existing = load_existing_config(&resolved_path, environment).ok();
    let mut config = existing.unwrap_or_else(default_config);
    let mut secret_refs = BTreeMap::new();

    if non_interactive {
        let validation = summarize_onboarding_validation(&config);
        return Ok(json!({
            "profile": resolved_path.display().to_string(),
            "saved": false,
            "mode": "non-interactive",
            "summary": summarize_config(&config),
            "validation": validation,
            "secret_refs": [],
            "notes": [
                "Non-interactive onboarding validates an existing config without prompting",
                "Use interactive onboarding to create or repair missing secrets and values",
            ],
        }));
    }

    println!("AYX onboarding");
    println!(
        "Press Enter to accept a default. Existing values are reused unless you choose to change them."
    );

    config.profile_name = prompt_text(
        "Profile name",
        Some(&config.profile_name),
        Some("local"),
        false,
    )?;

    let email_default = config
        .alteryx_one
        .as_ref()
        .map(|one| one.account_email.as_str());
    let account_email = prompt_text("Email address", email_default, None, true)?;
    config.alteryx_one = Some(update_or_create_one(config.alteryx_one, account_email));

    let configure_server =
        prompt_yes_no("Configure Alteryx Server", config.server.is_some(), true)?;
    if configure_server {
        let local_server = prompt_yes_no("Is the Server localhost", true, true)?;
        let mut server = config.server.take().unwrap_or_else(default_server);
        println!("Enter the bare Server base URL only.");
        println!("Do not include /webapi or /gallery. Example: http://10.1.1.1");
        let server_base_default = if server.webapi_url.trim().is_empty() {
            if local_server {
                Some("http://10.1.1.1")
            } else {
                None
            }
        } else {
            Some(server.webapi_url.as_str())
        };
        server.webapi_url = normalize_alteryx_base_url(&prompt_text(
            "Server base URL",
            server_base_default,
            Some("http://10.1.1.1"),
            true,
        )?);
        server.curator_api_key = prompt_secret(
            "Server API key",
            server.curator_api_key.as_str(),
            "curator",
            "AYX_SERVER_CURATOR_API_KEY",
        )?;
        server.curator_api_secret = prompt_secret(
            "Server API secret",
            server.curator_api_secret.as_str(),
            "curator secret",
            "AYX_SERVER_CURATOR_API_SECRET",
        )?;
        server.verify_tls = Some(prompt_yes_no(
            "Verify TLS certificates",
            server.verify_tls(),
            true,
        )?);
        config.server = Some(server);
    }

    if configure_server {
        let backend = prompt_backend(config.mongo.mode.clone())?;
        match backend {
            BackendChoice::Embedded => {
                let mut embedded = config
                    .mongo
                    .embedded
                    .take()
                    .unwrap_or_else(default_embedded);
                let runtime_settings_input = prompt_text(
                    "RuntimeSettings.xml path",
                    embedded.runtime_settings_path.as_deref(),
                    Some(DEFAULT_RUNTIME_SETTINGS_PATH),
                    false,
                )?;
                let runtime_settings_path = if runtime_settings_input.trim().is_empty() {
                    None
                } else {
                    Some(PathBuf::from(runtime_settings_input))
                };
                if let Some(runtime_settings_path) = runtime_settings_path.as_ref() {
                    if runtime_settings_path.exists() {
                        let summary = runtime_settings_summary(runtime_settings_path)?;
                        println!("Detected runtime settings:");
                        println!("{}", serde_yaml::to_string(&summary)?);
                    }
                    embedded.runtime_settings_path =
                        Some(runtime_settings_path.display().to_string());
                } else {
                    embedded.runtime_settings_path = None;
                }
                let detected_service_path =
                    detect_alteryx_service_path(runtime_settings_path.as_deref());
                if let Some(path) = &detected_service_path {
                    println!("Detected AlteryxService.exe: {}", path.display());
                }
                embedded.alteryx_service_path = prompt_optional_path(
                    "AlteryxService.exe path",
                    embedded
                        .alteryx_service_path
                        .as_deref()
                        .map(Path::new)
                        .or(detected_service_path.as_deref()),
                )?;
                // Restore target path is resolved at restore time from RuntimeSettings.xml;
                // not prompted at onboarding. Existing values are preserved.
                config.mongo = MongoProfile {
                    mode: MongoMode::Embedded,
                    databases: config.mongo.databases,
                    embedded: Some(embedded),
                    managed: None,
                };
            }
            BackendChoice::ManagedMongo => {
                let mut managed = config
                    .mongo
                    .managed
                    .take()
                    .unwrap_or_else(default_managed_mongo);
                let use_url = prompt_yes_no(
                    "Use a MongoDB URL connection string",
                    managed.url.is_some(),
                    false,
                )?;
                if use_url {
                    managed.url = Some(prompt_text(
                        "MongoDB URL",
                        managed.url.as_deref(),
                        None,
                        true,
                    )?);
                    managed.host = None;
                } else {
                    managed.url = None;
                    managed.host = Some(prompt_text(
                        "Mongo host",
                        managed.host.as_deref(),
                        None,
                        true,
                    )?);
                    managed.port = prompt_u16("Mongo port", managed.port, 27017)?;
                    managed.auth_database = prompt_optional_text(
                        "Mongo auth database",
                        managed.auth_database.as_deref(),
                    )?;
                }
                managed.username =
                    prompt_optional_text("Mongo username", managed.username.as_deref())?;
                managed.password = Some(prompt_secret(
                    "Mongo password",
                    managed.password.as_deref().unwrap_or(""),
                    "stored",
                    "AYX_MONGO_MANAGED_PASSWORD",
                )?);
                managed.tls.enabled = prompt_yes_no("Enable TLS", managed.tls.enabled, true)?;
                if managed.tls.enabled {
                    managed.tls.ca_path = prompt_optional_path(
                        "Mongo TLS CA path",
                        managed.tls.ca_path.as_deref().map(Path::new),
                    )?;
                    managed.tls.cert_path = prompt_optional_path(
                        "Mongo TLS cert path",
                        managed.tls.cert_path.as_deref().map(Path::new),
                    )?;
                    managed.tls.key_path = prompt_optional_path(
                        "Mongo TLS key path",
                        managed.tls.key_path.as_deref().map(Path::new),
                    )?;
                    managed.tls.allow_invalid_hostnames = Some(prompt_yes_no(
                        "Allow invalid Mongo TLS hostnames",
                        managed.tls.allow_invalid_hostnames.unwrap_or(false),
                        false,
                    )?);
                }
                config.mongo = MongoProfile {
                    mode: MongoMode::Managed,
                    databases: config.mongo.databases,
                    embedded: None,
                    managed: Some(managed),
                };
            }
            BackendChoice::SqlServer => {
                let mut sqlserver = config.sqlserver.take().unwrap_or_else(default_sqlserver);
                let controller = prompt_sql_connection(
                    "Controller",
                    sqlserver.controller.take(),
                    &mut secret_refs,
                    "AYX_SQL_CONTROLLER_PASSWORD",
                    true,
                )?;
                let server_ui = prompt_sql_connection(
                    "Server UI",
                    sqlserver.server_ui.take(),
                    &mut secret_refs,
                    "AYX_SQL_SERVER_UI_PASSWORD",
                    false,
                )?;
                sqlserver.controller = Some(controller);
                sqlserver.server_ui = Some(server_ui);
                config.sqlserver = Some(sqlserver);
            }
        }
    }

    let validation = summarize_onboarding_validation(&config);
    let secretize = write_config_with_policy(&resolved_path, &config, InlineSecretPolicy::Allow)?;
    let _ = secret_refs; // Preserved for API stability; refs come from secretize.

    let mut warnings = collect_onboarding_warnings(&config);
    if let Some(msg) = inline_secret_warning(&secretize.inline_fields) {
        warnings.push(msg);
    }

    Ok(json!({
        "profile": resolved_path.display().to_string(),
        "saved": true,
        "mode": "interactive",
        "summary": summarize_config(&config),
        "validation": validation,
        "secret_refs": secretize.refs.keys().collect::<Vec<_>>(),
        "inline_secret_fields": secretize.inline_fields,
        "warnings": warnings,
    }))
}

pub fn write_workspace_template(
    profile_path: &Path,
    active_environment: &str,
    source_environment: &str,
    target_environment: &str,
) -> Result<Value> {
    let workspace = WorkspaceConfig {
        workspace_name: "workspace".to_string(),
        active_environment: active_environment.to_string(),
        environments: HashMap::from([
            (
                source_environment.to_string(),
                template_config_with_profile("dev"),
            ),
            (
                target_environment.to_string(),
                template_config_with_profile("prod"),
            ),
        ]),
    };
    serialize_workspace_to(profile_path, &workspace)?;

    Ok(json!({
            "profile": profile_path.display().to_string(),
            "saved": true,
            "mode": "environments-template",
            "environments_file": {
                "workspace_name": workspace.workspace_name,
                "active_environment": workspace.active_environment,
                "environments": [source_environment, target_environment],
            },
            "notes": [
                "environments.yaml is the canonical multi-environment file",
            "Use --environment dev or --environment prod to select the active environment for a run",
        ],
    }))
}

/// Serialize a workspace to disk verbatim, without secretizing — used by the
/// template writer, which intentionally emits editable placeholder secrets
/// (e.g. `curator_api_secret: replace-me`) for the user to fill in.
fn serialize_workspace_to(path: &Path, workspace: &WorkspaceConfig) -> Result<()> {
    let body = serde_yaml::to_string(&canonical_workspace_value(workspace)?)?;
    write_sensitive_file(path, body.as_bytes())?;
    Ok(())
}

/// Persist a workspace, secretizing each environment's secrets first.
///
/// Secure by default: a secret that was loaded into a workspace environment
/// (e.g. an `env:`/`keyring:` ref resolved to a concrete value, or a freshly
/// minted token) is moved behind a secret ref rather than serialized as
/// plaintext YAML. Without this pass the workspace-save path materialized
/// resolved secrets to disk in the clear (red-team High #2). Use
/// [`serialize_workspace_to`] for the template path, which keeps placeholders.
///
/// Returns the merged [`SecretizeOutput`] across all environments so callers
/// can surface the `inline_fields` warning when the OS keyring is unavailable.
pub(crate) fn write_workspace_config(
    path: &Path,
    workspace: &WorkspaceConfig,
) -> Result<SecretizeOutput> {
    let mut secured = workspace.clone();
    let mut merged = SecretizeOutput::default();
    for config in secured.environments.values_mut() {
        let scope = config.profile_name.clone();
        let out = secretize_config(config, &scope, InlineSecretPolicy::Allow)?;
        merged.refs.extend(out.refs);
        merged.inline_fields.extend(out.inline_fields);
    }
    serialize_workspace_to(path, &secured)?;
    Ok(merged)
}

fn template_config_with_profile(profile_name: &str) -> Config {
    let mut config = default_config_with_profile(profile_name);
    config.server = Some(ServerProfile {
        webapi_url: "http://localhost/".to_string(),
        curator_api_key: "replace-me".to_string(),
        curator_api_secret: "replace-me".to_string(),
        curator_api_secret_ref: None,
        verify_tls: Some(true),
        derived: false,
    });
    config
}

pub(crate) fn load_existing_config(
    profile_path: &Path,
    environment: Option<&str>,
) -> Result<Config> {
    ayx_core::profile::Config::load_from_path_with_environment(profile_path, environment)
        .map_err(|err| anyhow::anyhow!(err))
}

fn secret_scope(scope: &str, field: &str) -> String {
    keyring_account(scope, field)
}

/// Inline-secret fallback policy for `secretize_config`.
///
/// `Forbid` (default) returns an error if the keyring is unavailable.
/// `Allow` falls back to inline (plaintext in YAML) and records the field name
/// in `SecretizeOutput::inline_fields` so the caller can warn the user.
/// The `AYX_ALLOW_INLINE_SECRETS=1` env var also enables fallback for
/// automation/headless scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Forbid is reserved for non-interactive enterprise mode (future flag).
pub(crate) enum InlineSecretPolicy {
    Forbid,
    Allow,
}

#[derive(Debug, Default)]
pub(crate) struct SecretizeOutput {
    pub refs: BTreeMap<String, String>,
    pub inline_fields: Vec<String>,
}

fn store(
    account: &str,
    value: &str,
    field: &str,
    policy: InlineSecretPolicy,
    out: &mut SecretizeOutput,
) -> Result<String> {
    let allow = matches!(policy, InlineSecretPolicy::Allow);
    let (reference, was_inline) = store_secret_with_fallback(account, value, allow)
        .map_err(|err| anyhow::anyhow!("{field}: {err}"))?;
    if was_inline {
        out.inline_fields.push(field.to_string());
    }
    Ok(reference)
}

/// Persist a secret field, preserving an existing `env:` indirection.
///
/// On load, `env:`-backed refs (`access_token_ref: env:FOO`) are resolved into a
/// concrete in-memory value. Without this guard a subsequent save would re-store
/// that resolved value into keyring/inline storage and overwrite the `env:` ref,
/// silently relocating a secret the user chose to keep in their environment
/// (red-team security L1). When the existing ref is an `env:` ref that still
/// resolves to the unchanged value, keep it as-is and store nothing. A changed
/// value (e.g. a fresh `auth login` token) no longer matches the env ref and is
/// secretized normally.
fn persist_secret_field(
    existing_ref: Option<&str>,
    account: &str,
    value: &str,
    field: &str,
    policy: InlineSecretPolicy,
    out: &mut SecretizeOutput,
) -> Result<String> {
    if let Some(reference) = existing_ref
        && reference.starts_with("env:")
        // The `env:` gate makes `.ok()` lossless: `resolve_secret_ref` is infallible
        // for the `env:` branch (it returns `Ok(env::var(..).ok())`), so `.ok()` can
        // never hide a real error here. If the value differs or the env var is unset,
        // we fall through and secretize the live value — the safe direction.
        && resolve_secret_ref(reference).ok().flatten().as_deref() == Some(value)
    {
        return Ok(reference.to_string());
    }
    store(account, value, field, policy, out)
}

pub(crate) fn secretize_config(
    config: &mut Config,
    scope: &str,
    policy: InlineSecretPolicy,
) -> Result<SecretizeOutput> {
    let mut out = SecretizeOutput::default();

    if let Some(one) = config.alteryx_one.as_mut() {
        if let Some(value) = one.access_token.take() {
            let existing_ref = one.access_token_ref.clone();
            let account = secret_scope(scope, "alteryx_one.access_token");
            let reference = persist_secret_field(
                existing_ref.as_deref(),
                &account,
                &value,
                "alteryx_one.access_token",
                policy,
                &mut out,
            )?;
            one.access_token_ref = Some(reference.clone());
            out.refs
                .insert("alteryx_one.access_token".to_string(), reference);
        }
        if let Some(value) = one.refresh_token.take() {
            let existing_ref = one.refresh_token_ref.clone();
            let account = secret_scope(scope, "alteryx_one.refresh_token");
            let reference = persist_secret_field(
                existing_ref.as_deref(),
                &account,
                &value,
                "alteryx_one.refresh_token",
                policy,
                &mut out,
            )?;
            one.refresh_token_ref = Some(reference.clone());
            out.refs
                .insert("alteryx_one.refresh_token".to_string(), reference);
        }
        if let Some(value) = one.client_secret.take() {
            let existing_ref = one.client_secret_ref.clone();
            let account = secret_scope(scope, "alteryx_one.client_secret");
            let reference = persist_secret_field(
                existing_ref.as_deref(),
                &account,
                &value,
                "alteryx_one.client_secret",
                policy,
                &mut out,
            )?;
            one.client_secret_ref = Some(reference.clone());
            out.refs
                .insert("alteryx_one.client_secret".to_string(), reference);
        }
        for (workspace_id, credential) in one.workspace_credentials.iter_mut() {
            if let Some(value) = credential.access_token.take() {
                let existing_ref = credential.access_token_ref.clone();
                let field =
                    format!("alteryx_one.workspace_credentials['{workspace_id}'].access_token");
                let account = secret_scope(scope, &field);
                let reference = persist_secret_field(
                    existing_ref.as_deref(),
                    &account,
                    &value,
                    &field,
                    policy,
                    &mut out,
                )?;
                credential.access_token_ref = Some(reference.clone());
                out.refs.insert(field, reference);
            }
            if let Some(value) = credential.refresh_token.take() {
                let existing_ref = credential.refresh_token_ref.clone();
                let field =
                    format!("alteryx_one.workspace_credentials['{workspace_id}'].refresh_token");
                let account = secret_scope(scope, &field);
                let reference = persist_secret_field(
                    existing_ref.as_deref(),
                    &account,
                    &value,
                    &field,
                    policy,
                    &mut out,
                )?;
                credential.refresh_token_ref = Some(reference.clone());
                out.refs.insert(field, reference);
            }
            if let Some(value) = credential.client_secret.take() {
                let existing_ref = credential.client_secret_ref.clone();
                let field =
                    format!("alteryx_one.workspace_credentials['{workspace_id}'].client_secret");
                let account = secret_scope(scope, &field);
                let reference = persist_secret_field(
                    existing_ref.as_deref(),
                    &account,
                    &value,
                    &field,
                    policy,
                    &mut out,
                )?;
                credential.client_secret_ref = Some(reference.clone());
                out.refs.insert(field, reference);
            }
        }
    }

    // Secretize the api/server views only when they are user-authored (not derived
    // from server_api by with_server_api_overrides). A derived view carries the same
    // logical secret as server_api and would create a duplicate or orphan keyring
    // account if secretized independently.
    if let Some(api) = config.api.as_mut()
        && !api.is_derived()
        && let Some(value) = api.auth.client_secret.take()
    {
        let existing_ref = api.auth.client_secret_ref.clone();
        let account = secret_scope(scope, "server.api.client_secret");
        let reference = persist_secret_field(
            existing_ref.as_deref(),
            &account,
            &value,
            "server.api.client_secret",
            policy,
            &mut out,
        )?;
        api.auth.client_secret_ref = Some(reference.clone());
        out.refs
            .insert("server.api.client_secret".to_string(), reference);
    }

    if let Some(server_api) = config.server_api.as_mut()
        && !server_api.client_secret.is_empty()
    {
        let value = std::mem::take(&mut server_api.client_secret);
        let existing_ref = server_api.client_secret_ref.clone();
        let account = secret_scope(scope, "server.api.client_secret");
        let reference = persist_secret_field(
            existing_ref.as_deref(),
            &account,
            &value,
            "server.api.client_secret",
            policy,
            &mut out,
        )?;
        server_api.client_secret_ref = Some(reference.clone());
        out.refs
            .insert("server.api.client_secret".to_string(), reference);
    }

    if let Some(server) = config.server.as_mut()
        && !server.is_derived()
        && !server.curator_api_secret.trim().is_empty()
    {
        let existing_ref = server.curator_api_secret_ref.clone();
        let value = std::mem::take(&mut server.curator_api_secret);
        let account = secret_scope(scope, "server.curator_api_secret");
        let reference = persist_secret_field(
            existing_ref.as_deref(),
            &account,
            &value,
            "server.curator_api_secret",
            policy,
            &mut out,
        )?;
        server.curator_api_secret_ref = Some(reference.clone());
        out.refs
            .insert("server.curator_api_secret".to_string(), reference);
    }

    if let Some(mongo) = config.mongo.managed.as_mut()
        && let Some(value) = mongo.password.take()
    {
        let existing_ref = mongo.password_ref.clone();
        let account = secret_scope(scope, "server.storage.mongo.managed.password");
        let reference = persist_secret_field(
            existing_ref.as_deref(),
            &account,
            &value,
            "server.storage.mongo.managed.password",
            policy,
            &mut out,
        )?;
        mongo.password_ref = Some(reference.clone());
        out.refs.insert(
            "server.storage.mongo.managed.password".to_string(),
            reference,
        );
    }

    if let Some(sql) = config.sqlserver.as_mut() {
        for (label, conn) in [
            (
                "server.storage.sqlserver.controller.password",
                sql.controller.as_mut(),
            ),
            (
                "server.storage.sqlserver.server_ui.password",
                sql.server_ui.as_mut(),
            ),
        ] {
            if let Some(conn) = conn
                && let Some(value) = conn.password.take()
            {
                let existing_ref = conn.password_ref.clone();
                let account = secret_scope(scope, label);
                let reference = persist_secret_field(
                    existing_ref.as_deref(),
                    &account,
                    &value,
                    label,
                    policy,
                    &mut out,
                )?;
                conn.password_ref = Some(reference.clone());
                out.refs.insert(label.to_string(), reference);
            }
        }
    }

    Ok(out)
}

pub(crate) fn default_config() -> Config {
    default_config_with_profile("local")
}

fn default_config_with_profile(profile_name: &str) -> Config {
    Config {
        profile_name: profile_name.to_string(),
        mongo: MongoProfile {
            mode: MongoMode::Embedded,
            databases: MongoDatabases {
                gallery_name: "AlteryxGallery".to_string(),
                service_name: "AlteryxService".to_string(),
            },
            embedded: Some(default_embedded()),
            managed: None,
        },
        alteryx_one: None,
        observability: None,
        server_api: None,
        api: None,
        server: None,
        sqlserver: None,
        upgrade: None,
    }
}

fn default_server() -> ServerProfile {
    ServerProfile {
        webapi_url: "http://localhost/".to_string(),
        curator_api_key: String::new(),
        curator_api_secret: String::new(),
        curator_api_secret_ref: None,
        verify_tls: Some(true),
        derived: false,
    }
}

fn default_embedded() -> MongoEmbedded {
    MongoEmbedded {
        runtime_settings_path: Some(DEFAULT_RUNTIME_SETTINGS_PATH.to_string()),
        alteryx_service_path: None,
        restore_target_path: None,
    }
}

fn default_managed_mongo() -> MongoManaged {
    MongoManaged {
        url: None,
        host: None,
        port: 27017,
        auth_database: None,
        username: None,
        password: None,
        password_ref: None,
        tls: TlsConfig {
            enabled: false,
            ca_path: None,
            cert_path: None,
            key_path: None,
            allow_invalid_hostnames: None,
        },
        timeout_ms: None,
        retry_count: None,
        max_pool_size: None,
    }
}

fn default_sqlserver() -> SqlServerProfile {
    SqlServerProfile {
        controller: Some(default_sql_connection(
            "AlteryxService",
            "AYX_SQL_CONTROLLER_PASSWORD",
            true,
        )),
        server_ui: Some(default_sql_connection(
            "AlteryxServerUI",
            "AYX_SQL_SERVER_UI_PASSWORD",
            false,
        )),
        legacy_connection_string: None,
    }
}

fn default_sql_connection(
    database: &str,
    password_env: &str,
    controller: bool,
) -> SqlServerConnectionProfile {
    SqlServerConnectionProfile {
        connection_string: None,
        host: Some("localhost".to_string()),
        port: Some(1433),
        database: Some(database.to_string()),
        username: Some("sa".to_string()),
        password: None,
        password_ref: None,
        password_env: Some(password_env.to_string()),
        integrated_security: Some(!controller),
        encrypt: Some(true),
        trust_server_certificate: Some(false),
        multi_subnet_failover: Some(false),
    }
}

fn update_or_create_one(
    existing: Option<AlteryxOneProfile>,
    account_email: String,
) -> AlteryxOneProfile {
    let mut one = existing.unwrap_or(AlteryxOneProfile {
        account_email: account_email.clone(),
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
        auth_mode: Default::default(),
    });
    one.account_email = account_email;
    one
}

fn prompt_backend(current: MongoMode) -> Result<BackendChoice> {
    println!("Storage backend:");
    println!("  1) Embedded Mongo");
    println!("  2) User-managed Mongo");
    println!("  3) SQL Server");
    let default = match current {
        MongoMode::Embedded => 1,
        MongoMode::Managed => 2,
    };
    loop {
        let input = prompt_raw(&format!("Choose backend [{}]", default))?;
        let choice = if input.trim().is_empty() {
            default
        } else {
            input.trim().parse::<u32>().unwrap_or(0)
        };
        match choice {
            1 => return Ok(BackendChoice::Embedded),
            2 => return Ok(BackendChoice::ManagedMongo),
            3 => return Ok(BackendChoice::SqlServer),
            _ => println!("Enter 1, 2, or 3."),
        }
    }
}

enum BackendChoice {
    Embedded,
    ManagedMongo,
    SqlServer,
}

fn prompt_sql_connection(
    label: &str,
    existing: Option<SqlServerConnectionProfile>,
    secret_refs: &mut BTreeMap<String, String>,
    env_key: &str,
    use_driver: bool,
) -> Result<SqlServerConnectionProfile> {
    let mut conn = existing.unwrap_or_else(|| {
        default_sql_connection(
            if use_driver {
                "AlteryxService"
            } else {
                "AlteryxServerUI"
            },
            env_key,
            use_driver,
        )
    });
    let connection_string_default = conn.connection_string.as_deref();
    conn.host = Some(prompt_text(
        &format!("{label} SQL host"),
        conn.host.as_deref(),
        Some("localhost"),
        true,
    )?);
    conn.port = Some(prompt_u16(
        &format!("{label} SQL port"),
        conn.port.unwrap_or(1433),
        1433,
    )?);
    conn.database = Some(prompt_text(
        &format!("{label} database"),
        conn.database.as_deref(),
        None,
        true,
    )?);
    conn.username = prompt_optional_text(&format!("{label} username"), conn.username.as_deref())?;
    let secret = prompt_secret(
        &format!("{label} password"),
        conn.password.as_deref().unwrap_or(""),
        "stored",
        env_key,
    )?;
    secret_refs.insert(env_key.to_string(), "keyring".to_string());
    conn.password = Some(secret);
    conn.password_env = Some(env_key.to_string());
    conn.integrated_security = Some(prompt_yes_no(
        &format!("{label} use integrated security"),
        conn.integrated_security.unwrap_or(!use_driver),
        true,
    )?);
    conn.encrypt = Some(prompt_yes_no(
        &format!("{label} enable encryption"),
        conn.encrypt.unwrap_or(true),
        true,
    )?);
    conn.trust_server_certificate = Some(prompt_yes_no(
        &format!("{label} trust server certificate"),
        conn.trust_server_certificate.unwrap_or(false),
        false,
    )?);
    conn.multi_subnet_failover = Some(prompt_yes_no(
        &format!("{label} multi-subnet failover"),
        conn.multi_subnet_failover.unwrap_or(false),
        false,
    )?);
    conn.connection_string = prompt_optional_text(
        &format!("{label} connection string"),
        connection_string_default,
    )?;
    if conn
        .password
        .as_ref()
        .is_some_and(|password| password.trim().is_empty())
    {
        return Err(anyhow::anyhow!("{label} password cannot be empty"));
    }
    Ok(conn)
}

/// Write a profile and return the full [`SecretizeOutput`] (refs + inline-fallback fields).
///
/// The `_secret_refs` parameter is kept for call-site compatibility but is no longer
/// used; refs come from the returned `SecretizeOutput`. Callers should check
/// [`inline_secret_warning`] on `output.inline_fields` to surface the advisory to users.
pub(crate) fn write_config(
    path: &Path,
    config: &Config,
    _secret_refs: &BTreeMap<String, String>,
) -> Result<SecretizeOutput> {
    write_config_with_policy(path, config, InlineSecretPolicy::Allow)
}

/// Write a profile file under `path`. Returns refs + inline-fallback warnings.
///
/// The file is created with restrictive permissions (0o600 on Unix) so secrets
/// that fell back to inline storage are not group/world-readable.
pub(crate) fn write_config_with_policy(
    path: &Path,
    config: &Config,
    policy: InlineSecretPolicy,
) -> Result<SecretizeOutput> {
    // Hard-error when multiple secret representations carry different resolved
    // values.  This is a write-time guard: it prevents silently persisting a
    // mixed-state config that would be ambiguous to reload.
    detect_secret_conflict(config).map_err(anyhow::Error::from)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut export = config.clone();
    let out = secretize_config(&mut export, &config.profile_name, policy)?;
    let body = serde_yaml::to_string(&canonical_profile_value(&export)?)?;
    write_restricted(path, body.as_bytes())?;
    Ok(out)
}

/// Returns a warning string if any secrets were stored inline (keyring unavailable), else None.
///
/// Used by both the onboarding flow and `auth login` to surface the same advisory to the user.
pub(crate) fn inline_secret_warning(inline_fields: &[String]) -> Option<String> {
    if inline_fields.is_empty() {
        return None;
    }
    Some(format!(
        "Stored {} secret(s) inline in plaintext YAML because the OS keyring was unavailable. Configure a keyring backend (Secret Service / Keychain / DPAPI) and re-run to migrate.",
        inline_fields.len()
    ))
}

/// Write a file with 0o600 permissions on Unix. On other platforms falls back
/// to plain write. Existing files have their permissions tightened after write.
pub(crate) fn write_restricted(path: &Path, contents: &[u8]) -> Result<()> {
    write_sensitive_file(path, contents)?;
    Ok(())
}

pub(crate) fn summarize_config(config: &Config) -> Value {
    json!({
        "profile_name": config.profile_name,
        "server": config.server.as_ref().map(|server| json!({
            "webapi_url": server.webapi_url,
            "curator_api_key": mask_if_present(&server.curator_api_key),
            "curator_api_secret": mask_if_present(&server.curator_api_secret),
            "verify_tls": server.verify_tls(),
        })),
        "mongo": {
            "mode": match config.mongo.mode {
                MongoMode::Embedded => "embedded",
                MongoMode::Managed => "managed",
            },
            "databases": {
                "gallery_name": config.mongo.databases.gallery_name,
                "service_name": config.mongo.databases.service_name,
            }
        },
        "sqlserver": config.sqlserver.as_ref().map(|sql| json!({
            "controller": sql.controller.as_ref().map(summarize_secret_sql_connection),
            "server_ui": sql.server_ui.as_ref().map(summarize_secret_sql_connection),
        }))
    })
}

fn summarize_secret_sql_connection(conn: &SqlServerConnectionProfile) -> Value {
    json!({
        "host": conn.host,
        "port": conn.port,
        "database": conn.database,
        "username": conn.username,
        "password_env": conn.password_env,
        "integrated_security": conn.integrated_security,
        "encrypt": conn.encrypt,
        "trust_server_certificate": conn.trust_server_certificate,
        "multi_subnet_failover": conn.multi_subnet_failover,
    })
}

fn mask_if_present(value: &str) -> Option<&'static str> {
    if value.trim().is_empty() {
        None
    } else {
        Some("stored")
    }
}

fn prompt_optional_text(prompt: &str, default: Option<&str>) -> Result<Option<String>> {
    let value = prompt_text(prompt, default, None, false)?;
    Ok((!value.trim().is_empty()).then_some(value))
}

fn prompt_secret(prompt: &str, current: &str, label: &str, env_key: &str) -> Result<String> {
    let prompt_label = if current.trim().is_empty() {
        format!("{prompt} [{label}]")
    } else {
        format!("{prompt} [stored]")
    };
    let value = prompt_raw(&prompt_label)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        if current.trim().is_empty() {
            return Ok(String::new());
        }
        return Ok(current.to_string());
    }
    if trimmed.eq_ignore_ascii_case("curator") {
        if current.trim().is_empty() {
            return Ok(String::new());
        }
        return Ok(current.to_string());
    }
    if trimmed.eq_ignore_ascii_case("stored") {
        if current.trim().is_empty() {
            return Ok(String::new());
        }
        return Ok(current.to_string());
    }
    if trimmed.is_empty() {
        Err(anyhow::anyhow!("{prompt} cannot be empty"))
    } else {
        let _ = env_key;
        Ok(trimmed.to_string())
    }
}

fn prompt_optional_path(prompt: &str, default: Option<&Path>) -> Result<Option<String>> {
    let input = prompt_text(
        prompt,
        default.map(|p| p.display().to_string()).as_deref(),
        None,
        false,
    )?;
    Ok((!input.trim().is_empty()).then_some(input))
}

fn prompt_u16(prompt: &str, current: u16, default: u16) -> Result<u16> {
    loop {
        let input = prompt_text(
            prompt,
            Some(&current.to_string()),
            Some(&default.to_string()),
            true,
        )?;
        match input.trim().parse::<u16>() {
            Ok(value) if value > 0 => return Ok(value),
            _ => println!("Enter a number between 1 and 65535."),
        }
    }
}

fn prompt_yes_no(prompt: &str, current: bool, default: bool) -> Result<bool> {
    let default_text = if current { "Y/n" } else { "y/N" };
    loop {
        let input = prompt_raw(&format!("{} [{}]", prompt, default_text))?;
        let trimmed = input.trim().to_lowercase();
        if trimmed.is_empty() {
            return Ok(default);
        }
        match trimmed.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Enter yes or no."),
        }
    }
}

fn prompt_text(
    prompt: &str,
    current: Option<&str>,
    default: Option<&str>,
    required: bool,
) -> Result<String> {
    let mut prompt_label = prompt.to_string();
    if let Some(current) = current {
        if !current.trim().is_empty() {
            prompt_label.push_str(&format!(" [{}]", current));
        }
    } else if let Some(default) = default {
        prompt_label.push_str(&format!(" [{}]", default));
    }
    loop {
        let input = prompt_raw(&prompt_label)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            if let Some(current) = current {
                return Ok(current.to_string());
            }
            if let Some(default) = default {
                return Ok(default.to_string());
            }
            if !required {
                return Ok(String::new());
            }
        } else {
            return Ok(trimmed.to_string());
        }
        println!("A value is required.");
    }
}

fn prompt_raw(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context("failed to read interactive input")?;
    Ok(buf)
}

pub(crate) fn summarize_onboarding_validation(config: &Config) -> Value {
    let mut missing = Vec::new();
    if let Some(sqlserver) = &config.sqlserver {
        if let Err(err) = validate_connection_profile_for_onboarding(
            "sqlserver.controller",
            sqlserver.controller.as_ref(),
        ) {
            missing.push(err.to_string());
        }
        if let Err(err) = validate_connection_profile_for_onboarding(
            "sqlserver.server_ui",
            sqlserver.server_ui.as_ref(),
        ) {
            missing.push(err.to_string());
        }
    }
    json!({
        "ok": missing.is_empty(),
        "missing": missing,
    })
}

fn validate_connection_profile_for_onboarding(
    field: &str,
    conn: Option<&SqlServerConnectionProfile>,
) -> Result<()> {
    let conn = conn.ok_or_else(|| anyhow::anyhow!("{field} is missing"))?;
    if conn.host.as_deref().is_none_or(|v| v.trim().is_empty()) {
        return Err(anyhow::anyhow!("{field}.host is required"));
    }
    if conn.database.as_deref().is_none_or(|v| v.trim().is_empty()) {
        return Err(anyhow::anyhow!("{field}.database is required"));
    }
    if conn.password.as_deref().is_none_or(|v| v.trim().is_empty()) {
        return Err(anyhow::anyhow!("{field}.password is required"));
    }
    if conn
        .password_env
        .as_deref()
        .is_none_or(|v| v.trim().is_empty())
    {
        return Err(anyhow::anyhow!("{field}.password_env is required"));
    }
    Ok(())
}

fn collect_onboarding_warnings(config: &Config) -> Vec<String> {
    let mut warnings = Vec::new();

    if config
        .server
        .as_ref()
        .is_none_or(|server| server.webapi_url.trim().is_empty())
    {
        warnings.push("server.webapi_url is missing".to_string());
    }
    if config
        .server
        .as_ref()
        .is_none_or(|server| server.curator_api_key.trim().is_empty())
    {
        warnings.push("server.curator_api_key is missing".to_string());
    }
    if config
        .server
        .as_ref()
        .is_none_or(|server| server.curator_api_secret.trim().is_empty())
    {
        warnings.push("server.curator_api_secret is missing".to_string());
    }
    if config
        .mongo
        .embedded
        .as_ref()
        .is_none_or(|embedded| embedded.runtime_settings_path.is_none())
    {
        warnings.push("mongo.embedded.runtime_settings_path is missing".to_string());
    }

    warnings
}

fn detect_alteryx_service_path(runtime_settings_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(runtime_settings_path) = runtime_settings_path
        && let Some(root) = runtime_settings_path.parent()
    {
        candidates.push(root.join("bin").join("AlteryxService.exe"));
        candidates.push(root.join("AlteryxService.exe"));
    }

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local_app_data);
        candidates.push(base.join("Alteryx").join("bin").join("AlteryxService.exe"));
        candidates.push(base.join("Alteryx").join("AlteryxService.exe"));
    }

    candidates.push(PathBuf::from(
        r"C:\Program Files\Alteryx\bin\AlteryxService.exe",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files (x86)\Alteryx\bin\AlteryxService.exe",
    ));

    candidates.into_iter().find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        ApiAuth, ApiAuthMode, ApiProfile, Config, MongoDatabases, MongoEmbedded, MongoMode,
        MongoProfile, ServerApiProfile, SqlServerConnectionProfile, SqlServerProfile,
        WorkspaceConfig, detect_secret_conflict, load_workspace_config,
    };
    use std::collections::HashMap;

    fn base_config() -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: None,
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: Some(SqlServerProfile {
                controller: Some(SqlServerConnectionProfile {
                    connection_string: None,
                    host: Some("sql.example.com".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxService".to_string()),
                    username: Some("svc".to_string()),
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
                    host: Some("sql.example.com".to_string()),
                    port: Some(1433),
                    database: Some("AlteryxServerUI".to_string()),
                    username: Some("svc".to_string()),
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
    fn onboarding_validator_rejects_empty_sql_password() {
        let mut cfg = base_config();
        cfg.sqlserver
            .as_mut()
            .unwrap()
            .controller
            .as_mut()
            .unwrap()
            .password = Some(String::new());
        assert!(
            !summarize_onboarding_validation(&cfg)["ok"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn onboarding_validator_accepts_complete_sql_profile() {
        let cfg = base_config();
        assert!(
            summarize_onboarding_validation(&cfg)["ok"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn onboarding_validator_allows_missing_sql_profile() {
        let mut cfg = base_config();
        cfg.sqlserver = None;
        let validation = summarize_onboarding_validation(&cfg);
        assert!(validation["ok"].as_bool().unwrap());
        assert!(validation["missing"].as_array().unwrap().is_empty());
    }

    #[test]
    fn environments_template_writes_named_environments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("environments.yaml");
        let detail = write_workspace_template(&path, "prod", "dev", "prod").unwrap();
        assert_eq!(detail["mode"], "environments-template");
        let loaded = Config::load_from_path_with_environment(&path, Some("prod")).unwrap();
        assert_eq!(loaded.profile_name, "prod");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("workspace_name"));
        assert!(content.contains("active_environment"));
    }

    #[test]
    fn workspace_config_save_preserves_environment_shape() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("environments.yaml");
        let workspace = WorkspaceConfig {
            workspace_name: "lab".to_string(),
            active_environment: "dev".to_string(),
            environments: HashMap::from([
                ("dev".to_string(), template_config_with_profile("dev")),
                ("prod".to_string(), template_config_with_profile("prod")),
            ]),
        };
        write_workspace_config(&path, &workspace).unwrap();

        let loaded = load_workspace_config(&path).unwrap();
        assert_eq!(loaded.active_environment, "dev");
        assert_eq!(loaded.environments.len(), 2);
        assert_eq!(loaded.environments["prod"].profile_name, "prod");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("environments:"));
        assert!(content.contains("dev:"));
        assert!(content.contains("prod:"));
    }

    #[test]
    fn round_trip_preserves_env_backed_token_ref() {
        // An `env:`-backed access_token_ref must survive a load -> save round-trip.
        // Resolving it at load and re-secretizing on save must NOT relocate the
        // secret into keyring/inline storage (red-team security L1).
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            "profile_name: envtest\n\
             alteryx_one:\n  \
               account_email: test@example.com\n  \
               base_url: https://us1.alteryxcloud.com\n  \
               access_token_ref: env:AYX_TEST_ROUNDTRIP_TOKEN\n",
        )
        .unwrap();

        // nextest process-isolates each test, so mutating an env var here is safe.
        unsafe { std::env::set_var("AYX_TEST_ROUNDTRIP_TOKEN", "secret-from-env") };

        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        // Sanity: the ref resolved to the env value in memory.
        assert_eq!(
            config.alteryx_one.as_ref().unwrap().access_token.as_deref(),
            Some("secret-from-env"),
            "precondition: env ref should resolve in memory"
        );

        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("AYX_TEST_ROUNDTRIP_TOKEN") };

        assert!(
            on_disk.contains("access_token_ref: env:AYX_TEST_ROUNDTRIP_TOKEN"),
            "env:-backed ref must be preserved as-is on round-trip, got:\n{on_disk}"
        );
        assert!(
            !on_disk.contains("inline:secret-from-env"),
            "secret value must not be materialized inline:\n{on_disk}"
        );
        assert!(
            !on_disk.contains("keyring:"),
            "env: ref must not be relocated into keyring storage:\n{on_disk}"
        );
    }

    #[test]
    fn fresh_token_overwrites_env_backed_ref() {
        // A changed value (e.g. a fresh `auth login` / rotation) no longer matches
        // the env ref and MUST be secretized — the env: indirection is overwritten,
        // so the new token actually persists. This guards the login boundary.
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            "profile_name: logintest\n\
             alteryx_one:\n  \
               account_email: test@example.com\n  \
               base_url: https://us1.alteryxcloud.com\n  \
               access_token_ref: env:AYX_TEST_LOGIN_TOKEN\n",
        )
        .unwrap();

        unsafe { std::env::set_var("AYX_TEST_LOGIN_TOKEN", "old-env-value") };

        let mut config = Config::load_from_path_with_environment(&path, None).unwrap();
        // Simulate `auth login` minting a new token onto the profile.
        config.alteryx_one.as_mut().unwrap().access_token = Some("new-rotated-token".to_string());

        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("AYX_TEST_LOGIN_TOKEN") };

        assert!(
            !on_disk.contains("env:AYX_TEST_LOGIN_TOKEN"),
            "a freshly minted token must overwrite the env: ref so it persists:\n{on_disk}"
        );
    }

    #[test]
    fn workspace_credential_env_ref_preserved_on_round_trip() {
        // A workspace-scoped credential whose access_token is env:-backed must
        // survive load->save like the top-level token — not get double-stored as
        // BOTH a resolved plaintext value AND the env: ref (red-team High #1).
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: wsenvtest
alteryx_one:
  account_email: test@example.com
  base_url: https://us1.alteryxcloud.com
  workspace_credentials:
    '91946':
      access_token_ref: env:AYX_TEST_WS_TOKEN
"#,
        )
        .unwrap();

        unsafe { std::env::set_var("AYX_TEST_WS_TOKEN", "ws-secret-from-env") };
        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("AYX_TEST_WS_TOKEN") };

        assert!(
            on_disk.contains("access_token_ref: env:AYX_TEST_WS_TOKEN"),
            "workspace env: ref must be preserved on round-trip:\n{on_disk}"
        );
        assert!(
            !on_disk.contains("ws-secret-from-env"),
            "workspace token must not be materialized as plaintext (double-store):\n{on_disk}"
        );
    }

    #[test]
    fn workspace_credential_fresh_token_is_secretized_not_plaintext() {
        // A fresh workspace-scoped token (e.g. `auth login --workspace-id`) must be
        // secretized on save, not written as bare plaintext YAML (red-team High #1).
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: wsfreshtest
alteryx_one:
  account_email: test@example.com
  base_url: https://us1.alteryxcloud.com
  workspace_credentials:
    '91946':
      access_token: fresh-ws-token
"#,
        )
        .unwrap();

        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();

        assert!(
            !on_disk.contains("access_token: fresh-ws-token"),
            "workspace token must be secretized to a ref, not bare plaintext:\n{on_disk}"
        );
        assert!(
            on_disk.contains("inline:fresh-ws-token") || on_disk.contains("keyring:"),
            "workspace token must be stored behind a secret ref:\n{on_disk}"
        );
    }

    #[test]
    fn write_workspace_config_secretizes_secrets() {
        // The workspace-save path must secretize secrets, not serialize resolved
        // plaintext straight to disk (red-team High #2).
        let temp = tempfile::tempdir().unwrap();
        let env_path = temp.path().join("env.yaml");
        std::fs::write(
            &env_path,
            r#"profile_name: dev
alteryx_one:
  account_email: test@example.com
  base_url: https://us1.alteryxcloud.com
  access_token: ws-plaintext-token
"#,
        )
        .unwrap();
        let env_config = Config::load_from_path_with_environment(&env_path, None).unwrap();

        let workspace = WorkspaceConfig {
            workspace_name: "lab".to_string(),
            active_environment: "dev".to_string(),
            environments: HashMap::from([("dev".to_string(), env_config)]),
        };
        let ws_path = temp.path().join("workspace.yaml");
        write_workspace_config(&ws_path, &workspace).unwrap();

        let on_disk = std::fs::read_to_string(&ws_path).unwrap();
        assert!(
            !on_disk.contains("access_token: ws-plaintext-token"),
            "write_workspace_config must secretize secrets, not write plaintext:\n{on_disk}"
        );
    }

    #[test]
    fn rotated_token_persists_and_resolves_after_env_removed() {
        // Strengthen the login-rotation guarantee (red-team Medium): a rotated token
        // must actually PERSIST — resolve after the env var is gone — not merely
        // cause the old env: ref to disappear.
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: rotatetest
alteryx_one:
  account_email: test@example.com
  base_url: https://us1.alteryxcloud.com
  access_token_ref: env:AYX_TEST_ROTATE_TOKEN
"#,
        )
        .unwrap();

        unsafe { std::env::set_var("AYX_TEST_ROTATE_TOKEN", "old-value") };
        let mut config = Config::load_from_path_with_environment(&path, None).unwrap();
        config.alteryx_one.as_mut().unwrap().access_token = Some("rotated-token".to_string());
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        unsafe { std::env::remove_var("AYX_TEST_ROTATE_TOKEN") };

        let reloaded = Config::load_from_path_with_environment(&path, None).unwrap();
        assert_eq!(
            reloaded.alteryx_one.unwrap().access_token.as_deref(),
            Some("rotated-token"),
            "rotated token must persist and resolve after the env var is removed"
        );
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("access_token: rotated-token"),
            "rotated token must be stored as a secret ref, not bare plaintext:\n{on_disk}"
        );
    }

    /// Tempdir that also points `AYX_CONFIG_HOME` at itself, so the strict config
    /// loader's active-profile overlay finds no host state to contaminate the
    /// fixture. nextest process-isolates each test, so the env mutation is safe.
    fn isolated_config_home() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("AYX_CONFIG_HOME", temp.path());
        }
        temp
    }

    #[test]
    fn server_api_sourced_secret_is_secretized_not_plaintext() {
        // A profile carrying a top-level `server_api:` section must not write its
        // client_secret to disk as plaintext (red-team High; the canonical
        // `server.api.client_secret` schema previously had no ref slot).
        let temp = isolated_config_home();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: saprobe
server_api:
  base_url: https://server.example.com
  client_id: cid
  client_secret: SERVERAPI-SECRET
"#,
        )
        .unwrap();
        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("client_secret: SERVERAPI-SECRET"),
            "server.api client_secret must not be plaintext on save:\n{on_disk}"
        );
        assert!(
            on_disk.contains("client_secret_ref:"),
            "server.api secret must be stored behind a ref:\n{on_disk}"
        );
    }

    #[test]
    fn api_sourced_server_secret_is_secretized_not_plaintext() {
        let temp = isolated_config_home();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: apisrc
api:
  base_url: https://server.example.com
  auth:
    mode: oauth2_client_credentials
    client_id: cid
    client_secret: API-CLIENT-SECRET
"#,
        )
        .unwrap();
        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("client_secret: API-CLIENT-SECRET"),
            "api-sourced server.api secret must not be bare plaintext on save:\n{on_disk}"
        );
        assert!(
            on_disk.contains("client_secret_ref:"),
            "api-sourced server.api secret must be a ref:\n{on_disk}"
        );
    }

    #[test]
    fn server_api_secret_round_trips_through_ref() {
        // Save (secretize) then reload — the resolved secret must survive.
        let temp = isolated_config_home();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: rtprobe
server_api:
  base_url: https://server.example.com
  client_id: cid
  client_secret: ROUNDTRIP-SECRET
"#,
        )
        .unwrap();
        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let reloaded = Config::load_from_path_with_environment(&path, None).unwrap();
        assert_eq!(
            reloaded
                .server_api
                .as_ref()
                .map(|s| s.client_secret.as_str()),
            Some("ROUNDTRIP-SECRET"),
            "server.api secret must resolve from its ref after a secretized save"
        );
    }

    #[test]
    fn server_api_env_ref_preserved_on_round_trip() {
        let temp = isolated_config_home();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: saenv
server_api:
  base_url: https://server.example.com
  client_id: cid
  client_secret_ref: env:AYX_TEST_SA_SECRET
"#,
        )
        .unwrap();
        unsafe { std::env::set_var("AYX_TEST_SA_SECRET", "sa-from-env") };
        let config = Config::load_from_path_with_environment(&path, None).unwrap();
        assert_eq!(
            config.server_api.as_ref().map(|s| s.client_secret.as_str()),
            Some("sa-from-env"),
            "precondition: server.api env ref should resolve in memory"
        );
        write_config_with_policy(&path, &config, InlineSecretPolicy::Allow).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("AYX_TEST_SA_SECRET") };
        assert!(
            on_disk.contains("client_secret_ref: env:AYX_TEST_SA_SECRET"),
            "server.api env: ref must be preserved:\n{on_disk}"
        );
        assert!(
            !on_disk.contains("sa-from-env"),
            "server.api env-backed secret must not be materialized:\n{on_disk}"
        );
    }

    // ---------------------------------------------------------------------------
    // Task 2: single-arm secretize — server_api source writes exactly one ref.
    // ---------------------------------------------------------------------------

    #[test]
    fn server_api_sourced_config_writes_single_keyring_account() {
        // server_api only -> the load path calls with_server_api_overrides, synthesizing
        // derived api+server copies. secretize_config must write exactly one keyring
        // account (via the server_api arm) and not create a second orphan account for
        // server.curator_api_secret.
        let temp = isolated_config_home();
        let path = temp.path().join("default.yaml");
        std::fs::write(
            &path,
            r#"profile_name: satest
server_api:
  base_url: https://x.example
  client_id: cid
  client_secret: shh
"#,
        )
        .unwrap();
        // load triggers with_server_api_overrides: api and server are synthesized (derived)
        let mut cfg = Config::load_from_path_with_environment(&path, None).unwrap();
        let out = secretize_config(&mut cfg, "wsenv", InlineSecretPolicy::Allow).unwrap();
        // one logical secret -> one ref key; NO server.curator_api_secret orphan
        assert_eq!(
            out.refs.len(),
            1,
            "exactly one secret persisted, got {:?}",
            out.refs
        );
        assert!(out.refs.contains_key("server.api.client_secret"));
        assert!(
            !out.refs.contains_key("server.curator_api_secret"),
            "no orphan curator account"
        );
    }

    // ---------------------------------------------------------------------------
    // Task 3: write_workspace_config must return SecretizeOutput — no silent discard.
    // ---------------------------------------------------------------------------

    /// Build a WorkspaceConfig with a single environment that carries one secret.
    fn workspace_with_one_env_secret(secret_value: &str) -> WorkspaceConfig {
        // Embed the secret as an inline access_token so secretize_config has
        // something to secretize (and fall back to inline when keyring absent).
        let yaml = format!(
            "profile_name: ws-env\nalteryx_one:\n  account_email: test@example.com\n  base_url: https://us1.alteryxcloud.com\n  access_token: {secret_value}\n"
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), yaml).unwrap();
        let env_cfg = Config::load_from_path_with_environment(tmp.path(), None).unwrap();
        WorkspaceConfig {
            workspace_name: "test-ws".to_string(),
            active_environment: "env".to_string(),
            environments: HashMap::from([("env".to_string(), env_cfg)]),
        }
    }

    #[test]
    fn workspace_save_surfaces_inline_fields_when_keyring_unavailable() {
        // Force the keyring-store step to fail deterministically so the inline
        // fallback path always runs, regardless of whether the host has a live
        // D-Bus Secret Service. AYX_FORCE_INLINE_SECRETS is an env-var lever
        // in store_keyring_secret (see ayx-core/src/secrets.rs). It is NOT
        // gated with #[cfg(test)] because cfg(test) inside a library crate is
        // inactive when that crate is compiled as a dependency — the env-var
        // guard is sufficient for production safety (undocumented, no CLI
        // surface, inert when unset). nextest process-isolates each test, so
        // the env mutation is safe.
        unsafe {
            std::env::set_var("AYX_FORCE_INLINE_SECRETS", "1");
        }
        let _home = isolated_config_home();
        let ws = workspace_with_one_env_secret("shh");
        let tmp = tempfile::tempdir().unwrap();
        let ws_path = tmp.path().join("ws.yaml");
        let out = write_workspace_config(&ws_path, &ws).unwrap();
        // Restore before any assertions so a panic doesn't leak the var.
        unsafe {
            std::env::remove_var("AYX_FORCE_INLINE_SECRETS");
        }
        // With the keyring forced unavailable, write_workspace_config must fall
        // back to inline storage AND surface that fact in SecretizeOutput.
        let on_disk = std::fs::read_to_string(&ws_path).unwrap();
        assert!(
            on_disk.contains("inline:"),
            "forced-unavailable keyring must produce inline: ref on disk"
        );
        assert!(
            !out.inline_fields.is_empty(),
            "inline fallback must be reported in SecretizeOutput, not swallowed"
        );
        assert!(
            out.inline_fields.iter().any(|f| f.contains("access_token")),
            "inline_fields must name the secretized field (access_token)"
        );
        // refs must always be populated (at least the one secretized field).
        assert!(
            !out.refs.is_empty(),
            "SecretizeOutput::refs must be populated on workspace save"
        );
    }

    // ---------------------------------------------------------------------------
    // Task 4: mixed-state secret conflict detection helpers + tests
    // ---------------------------------------------------------------------------

    /// Minimal Config skeleton — only the fields that differ per test are set.
    fn conflict_base(profile_name: &str) -> Config {
        Config {
            profile_name: profile_name.to_string(),
            mongo: MongoProfile::default(),
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    /// Config where `server_api.client_secret_ref = inline:{old}` and
    /// `api.auth.client_secret = {new_val}`.  Both are resolvable; they differ →
    /// `detect_secret_conflict` must error.
    fn config_with_conflicting_secrets(old: &str, new_val: &str) -> Config {
        let mut cfg = conflict_base("conflict-test");
        cfg.server_api = Some(ServerApiProfile {
            base_url: "https://example.com".to_string(),
            client_id: "cid".to_string(),
            client_secret: String::new(),
            client_secret_ref: Some(format!("inline:{old}")),
        });
        cfg.api = Some(ApiProfile {
            base_url: "https://example.com".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Oauth2ClientCredentials,
                pat: None,
                client_id: Some("cid".to_string()),
                client_secret: Some(new_val.to_string()),
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false, // user-authored — conflict detection must inspect this
        });
        cfg
    }

    /// Config where one side uses an env var that is NOT set → unresolvable →
    /// cannot prove conflict → `detect_secret_conflict` must return `Ok`.
    fn config_with_one_unresolvable_ref() -> Config {
        // Use a name that is extremely unlikely to be set in any real environment.
        let env_var = "AYX_CONFLICT_TEST_MISSING_VAR_49283747";
        // Ensure it is not set (safe because nextest process-isolates each test).
        unsafe { std::env::remove_var(env_var) };

        let mut cfg = conflict_base("unresolvable-test");
        cfg.server_api = Some(ServerApiProfile {
            base_url: "https://example.com".to_string(),
            client_id: "cid".to_string(),
            client_secret: String::new(),
            client_secret_ref: Some(format!("env:{env_var}")),
        });
        cfg.api = Some(ApiProfile {
            base_url: "https://example.com".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Oauth2ClientCredentials,
                pat: None,
                client_id: Some("cid".to_string()),
                client_secret: Some("DIFFERENT".to_string()),
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        });
        cfg
    }

    /// Config where `server_api.client_secret_ref = inline:{secret}` and
    /// `api.auth.client_secret = {secret}`.  Both resolve to the same value →
    /// `detect_secret_conflict` must return `Ok`.
    fn config_with_matching_secrets(secret: &str) -> Config {
        let mut cfg = conflict_base("matching-test");
        cfg.server_api = Some(ServerApiProfile {
            base_url: "https://example.com".to_string(),
            client_id: "cid".to_string(),
            client_secret: String::new(),
            client_secret_ref: Some(format!("inline:{secret}")),
        });
        cfg.api = Some(ApiProfile {
            base_url: "https://example.com".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Oauth2ClientCredentials,
                pat: None,
                client_id: Some("cid".to_string()),
                client_secret: Some(secret.to_string()),
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        });
        cfg
    }

    #[test]
    fn conflicting_resolved_secrets_error_at_write_boundary() {
        let _home = isolated_config_home();
        // server_api.client_secret_ref = inline:OLD ; api.auth.client_secret = NEW
        // (both resolve, differ → write must hard-error)
        let cfg = config_with_conflicting_secrets("OLD", "NEW");
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("c.yaml");
        let err = write_config_with_policy(&path, &cfg, InlineSecretPolicy::Allow).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("server_api"),
            "error must name the conflicting sources; got: {msg}"
        );
    }

    #[test]
    fn unresolvable_ref_does_not_error() {
        let _home = isolated_config_home();
        // one side env:MISSING → unresolvable → cannot prove conflict → Ok
        let cfg = config_with_one_unresolvable_ref();
        assert!(
            detect_secret_conflict(&cfg).is_ok(),
            "unresolvable ref must degrade to warn, not error"
        );
    }

    #[test]
    fn identical_secrets_across_reps_do_not_error() {
        let cfg = config_with_matching_secrets("SAME");
        assert!(
            detect_secret_conflict(&cfg).is_ok(),
            "identical resolved values must not error"
        );
    }
}
