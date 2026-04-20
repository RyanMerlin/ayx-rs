use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use ayx_core::definitions::DEFAULT_RUNTIME_SETTINGS_PATH;
use ayx_core::profile::{
    normalize_alteryx_base_url, AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded,
    MongoManaged, MongoMode, MongoProfile, ServerProfile, SqlServerConnectionProfile,
    SqlServerProfile, TlsConfig, WorkspaceConfig,
};
use ayx_server::util::runtime_settings_summary;

pub fn run_onboarding(
    profile_path: &Path,
    environment: Option<&str>,
    non_interactive: bool,
    workspace_mode: bool,
) -> Result<Value> {
    if workspace_mode {
        let active_environment = environment.unwrap_or("dev");
        return write_workspace_template(profile_path, active_environment, "dev", "prod");
    }
    let existing = load_existing_config(profile_path, environment).ok();
    let mut config = existing.unwrap_or_else(default_config);
    let mut env_updates = BTreeMap::new();

    if non_interactive {
        let validation = summarize_onboarding_validation(&config);
        return Ok(json!({
            "profile": profile_path.display().to_string(),
            "saved": false,
            "mode": "non-interactive",
            "summary": summarize_config(&config),
            "validation": validation,
            "env_updates": [],
            "notes": [
                "Non-interactive onboarding validates an existing config without prompting",
                "Use interactive onboarding to create or repair missing secrets and values",
            ],
        }));
    }

    println!("AYX onboarding");
    println!("Press Enter to accept a default. Existing values are reused unless you choose to change them.");

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

    let backend = prompt_backend(config.mongo.mode.clone())?;
    match backend {
        BackendChoice::Embedded => {
            let mut embedded = config
                .mongo
                .embedded
                .take()
                .unwrap_or_else(default_embedded);
            let designer_install = prompt_yes_no("Designer user install", false, false)?;
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
                embedded.runtime_settings_path = Some(runtime_settings_path.display().to_string());
            } else {
                embedded.runtime_settings_path = None;
            }
            let detected_service_path =
                detect_alteryx_service_path(runtime_settings_path.as_deref(), designer_install);
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
            embedded.restore_target_path = prompt_optional_path(
                "Embedded Mongo restore target path",
                embedded.restore_target_path.as_deref().map(Path::new),
            )?;
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
                managed.auth_database =
                    prompt_optional_text("Mongo auth database", managed.auth_database.as_deref())?;
            }
            managed.username = prompt_optional_text("Mongo username", managed.username.as_deref())?;
            managed.password = Some(prompt_secret(
                "Mongo password",
                managed.password.as_deref().unwrap_or(""),
                "stored",
                "AYX_MONGO_MANAGED_PASSWORD",
            )?);
            env_updates.insert(
                "AYX_MONGO_MANAGED_PASSWORD".to_string(),
                managed.password.clone().unwrap_or_default(),
            );
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
                &mut env_updates,
                "AYX_SQL_CONTROLLER_PASSWORD",
                true,
            )?;
            let server_ui = prompt_sql_connection(
                "Server UI",
                sqlserver.server_ui.take(),
                &mut env_updates,
                "AYX_SQL_SERVER_UI_PASSWORD",
                false,
            )?;
            sqlserver.controller = Some(controller);
            sqlserver.server_ui = Some(server_ui);
            config.sqlserver = Some(sqlserver);
        }
    }

    let validation = summarize_onboarding_validation(&config);
    write_config(profile_path, &config, &env_updates)?;

    Ok(json!({
        "profile": profile_path.display().to_string(),
        "saved": true,
        "mode": "interactive",
        "summary": summarize_config(&config),
        "validation": validation,
        "env_updates": env_updates.keys().collect::<Vec<_>>(),
        "warnings": collect_onboarding_warnings(&config),
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
                default_config_with_profile("dev"),
            ),
            (
                target_environment.to_string(),
                default_config_with_profile("prod"),
            ),
        ]),
    };

    if let Some(parent) = profile_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(profile_path, serde_yaml::to_string(&workspace)?)?;

    Ok(json!({
        "profile": profile_path.display().to_string(),
            "saved": true,
            "mode": "workspace-template",
            "workspace": {
                "workspace_name": workspace.workspace_name,
                "active_environment": workspace.active_environment,
                "environments": [source_environment, target_environment],
            },
            "notes": [
                "workspace.yaml is the canonical multi-environment file",
            "Use --environment dev or --environment prod to select the active environment for a run",
        ],
    }))
}

fn load_existing_config(profile_path: &Path, environment: Option<&str>) -> Result<Config> {
    ayx_core::profile::Config::load_from_path_with_environment(profile_path, environment)
        .map_err(|err| anyhow::anyhow!(err))
}

fn default_config() -> Config {
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
        verify_tls: Some(true),
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
        oauth_client_id: None,
        token_endpoint_url: None,
        access_token: None,
        refresh_token: None,
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
    env_updates: &mut BTreeMap<String, String>,
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
    env_updates.insert(env_key.to_string(), secret.clone());
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

fn write_config(
    path: &Path,
    config: &Config,
    env_updates: &BTreeMap<String, String>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_yaml::to_string(config)?)?;

    let env_path = path.parent().unwrap_or_else(|| Path::new(".")).join(".env");
    let mut current = read_env_map(&env_path)?;
    for (k, v) in env_updates {
        current.insert(k.clone(), v.clone());
    }
    write_env_map(&env_path, &current)?;
    Ok(())
}

fn read_env_map(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    if !path.exists() {
        return Ok(values);
    }
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .trim_matches('\'');
        if !key.is_empty() {
            values.insert(key.to_string(), value.to_string());
        }
    }
    Ok(values)
}

fn write_env_map(path: &Path, values: &BTreeMap<String, String>) -> Result<()> {
    let mut out = String::new();
    for (k, v) in values {
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn summarize_config(config: &Config) -> Value {
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

fn summarize_onboarding_validation(config: &Config) -> Value {
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

fn detect_alteryx_service_path(
    runtime_settings_path: Option<&Path>,
    designer_install: bool,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(runtime_settings_path) = runtime_settings_path {
        if let Some(root) = runtime_settings_path.parent() {
            candidates.push(root.join("bin").join("AlteryxService.exe"));
            candidates.push(root.join("AlteryxService.exe"));
        }
    }

    if designer_install {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data);
            candidates.push(base.join("Alteryx").join("bin").join("AlteryxService.exe"));
            candidates.push(base.join("Alteryx").join("AlteryxService.exe"));
        }
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
        Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile, SqlServerConnectionProfile,
        SqlServerProfile,
    };

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
        assert!(!summarize_onboarding_validation(&cfg)["ok"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn onboarding_validator_accepts_complete_sql_profile() {
        let cfg = base_config();
        assert!(summarize_onboarding_validation(&cfg)["ok"]
            .as_bool()
            .unwrap());
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
    fn workspace_template_writes_named_environments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("workspace.yaml");
        let detail = write_workspace_template(&path, "prod", "dev", "prod").unwrap();
        assert_eq!(detail["mode"], "workspace-template");
        let loaded = Config::load_from_path_with_environment(&path, Some("prod")).unwrap();
        assert_eq!(loaded.profile_name, "prod");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("workspace_name"));
        assert!(content.contains("active_environment"));
    }
}
