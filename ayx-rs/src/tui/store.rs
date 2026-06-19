use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_yaml::Value;

use ayx_core::profile::{
    AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile,
    ObservabilityProfile, ServerProfile, UpgradeProfile,
};

use crate::onboard::{default_config, write_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileScope {
    One,
    Server,
    Combined,
}

#[derive(Debug, Clone)]
pub(crate) struct ProfileRecord {
    pub name: String,
    pub path: PathBuf,
    pub scope: ProfileScope,
}

fn validate_name(kind: &str, name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("{kind} name must not be empty");
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute()
        || candidate.components().count() != 1
        || trimmed.contains('\\')
        || trimmed.contains('/')
        || trimmed == "."
        || trimmed == ".."
    {
        bail!("{kind} name must be a simple file name, not a path");
    }
    Ok(trimmed.to_string())
}

fn profiles_dir_at(config_home: &Path) -> PathBuf {
    config_home.join("profiles")
}

fn profile_path_at(config_home: &Path, name: &str) -> Result<PathBuf> {
    let name = validate_name("profile", name)?;
    Ok(profiles_dir_at(config_home).join(format!("{name}.yaml")))
}

fn default_one_profile_template(profile_name: &str) -> Config {
    let mut config = default_config();
    config.profile_name = profile_name.to_string();
    config.alteryx_one = Some(AlteryxOneProfile {
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
    });
    config.server_api = None;
    config.api = None;
    config.server = None;
    config.sqlserver = None;
    config.observability = None;
    config.upgrade = None;
    config
}

fn default_server_profile_template(profile_name: &str) -> Config {
    let mut config = default_config();
    config.profile_name = profile_name.to_string();
    config.alteryx_one = None;
    config.server = Some(ServerProfile {
        webapi_url: "http://localhost/".to_string(),
        curator_api_key: String::new(),
        curator_api_secret: String::new(),
        curator_api_secret_ref: None,
        verify_tls: Some(true),
    });
    config.server_api = None;
    config.sqlserver = None;
    config
}

fn write_profile_at(config_home: &Path, name: &str, config: &Config) -> Result<PathBuf> {
    let name = validate_name("profile", name)?;
    let path = profile_path_at(config_home, &name)?;
    if path.exists() {
        bail!("profile '{name}' already exists");
    }
    let mut export = config.clone();
    export.profile_name = name.clone();
    write_config(&path, &export, &BTreeMap::new())?;
    Ok(path)
}

fn default_profile_template(profile_name: &str) -> Config {
    default_one_profile_template(profile_name)
}

pub(crate) fn list_profile_names_at(config_home: &Path) -> Result<Vec<String>> {
    Ok(list_profile_records_at(config_home)?
        .into_iter()
        .map(|record| record.name)
        .collect())
}

pub(crate) fn list_profile_records_at(config_home: &Path) -> Result<Vec<ProfileRecord>> {
    let dir = profiles_dir_at(config_home);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read profile directory '{}'", dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed to read profile directory '{}'", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
            let scope = match fs::read_to_string(&path).with_context(|| {
                format!("failed to read profile file '{}'", path.display())
            }) {
                Ok(contents) => match serde_yaml::from_str::<Value>(&contents) {
                    Ok(value) => classify_profile_scope_value(&value),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            records.push(ProfileRecord {
                name: stem.to_string(),
                path: path.clone(),
                scope,
            });
        }
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

pub(crate) fn load_profile_at(config_home: &Path, name: &str) -> Result<Config> {
    let path = profile_path_at(config_home, name)?;
    Config::load_from_path_lenient(&path).map_err(anyhow::Error::from)
}

pub(crate) fn create_profile_from_default_at(config_home: &Path, name: &str) -> Result<PathBuf> {
    create_profile_from_default_scope_at(config_home, name, ProfileScope::One)
}

pub(crate) fn create_profile_from_default_scope_at(
    config_home: &Path,
    name: &str,
    scope: ProfileScope,
) -> Result<PathBuf> {
    let template = match scope {
        ProfileScope::One => default_one_profile_template(name),
        ProfileScope::Server => default_server_profile_template(name),
        ProfileScope::Combined => default_config_with_profile(name),
    };
    write_profile_at(config_home, name, &template)
}

pub(crate) fn create_profile_from_config_at(
    config_home: &Path,
    name: &str,
    source: &Config,
) -> Result<PathBuf> {
    write_profile_at(config_home, name, source)
}

pub(crate) fn duplicate_profile_at(
    config_home: &Path,
    source_name: &str,
    new_name: &str,
) -> Result<PathBuf> {
    let source = load_profile_at(config_home, source_name)?;
    create_profile_from_config_at(config_home, new_name, &source)
}

fn classify_profile_scope_value(value: &Value) -> ProfileScope {
    let Some(root) = value.as_mapping() else {
        return ProfileScope::Combined;
    };
    let has_one = root.contains_key(Value::String("alteryx_one".to_string()));
    let has_server = root.contains_key(Value::String("server".to_string()))
        || root.contains_key(Value::String("server_api".to_string()))
        || root.contains_key(Value::String("sqlserver".to_string()))
        || root.contains_key(Value::String("api".to_string()));
    match (has_one, has_server) {
        (true, true) => ProfileScope::Combined,
        (true, false) => ProfileScope::One,
        (false, true) => ProfileScope::Server,
        (false, false) => ProfileScope::Combined,
    }
}

fn default_config_with_profile(profile_name: &str) -> Config {
    let mut config = default_config();
    config.profile_name = profile_name.to_string();
    config.mongo = MongoProfile {
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
    };
    config.observability = Some(ObservabilityProfile { api_logging: None });
    config.upgrade = Some(UpgradeProfile {
        target_version: None,
        deployment: None,
    });
    config
}

pub(crate) fn rename_profile_at(
    config_home: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<PathBuf> {
    let old_path = profile_path_at(config_home, old_name)?;
    if !old_path.exists() {
        bail!("profile '{old_name}' does not exist");
    }
    let source = load_profile_at(config_home, old_name)?;
    let new_path = profile_path_at(config_home, new_name)?;
    if new_path.exists() && new_path != old_path {
        bail!("profile '{new_name}' already exists");
    }
    let mut renamed = source;
    renamed.profile_name = validate_name("profile", new_name)?;
    write_config(&new_path, &renamed, &BTreeMap::new())?;
    if new_path != old_path {
        fs::remove_file(&old_path).with_context(|| {
            format!("failed to remove old profile file '{}'", old_path.display())
        })?;
    }
    Ok(new_path)
}

pub(crate) fn delete_profile_at(config_home: &Path, name: &str) -> Result<PathBuf> {
    let path = profile_path_at(config_home, name)?;
    if !path.exists() {
        bail!("profile '{name}' does not exist");
    }
    fs::remove_file(&path)
        .with_context(|| format!("failed to delete profile file '{}'", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_source_profile(config_home: &Path, name: &str, profile_name: &str) {
        let config = default_profile_template(profile_name);
        create_profile_from_config_at(config_home, name, &config).expect("write profile");
    }

    #[test]
    fn create_duplicate_rename_and_delete_profiles() {
        let home = temp_home();
        let config_home = home.path();

        let created = create_profile_from_default_at(config_home, "local").expect("create");
        assert!(created.exists());
        assert_eq!(
            list_profile_names_at(config_home).expect("list"),
            vec!["local"]
        );

        let duplicate =
            duplicate_profile_at(config_home, "local", "local-copy").expect("duplicate");
        assert!(duplicate.exists());

        let renamed = rename_profile_at(config_home, "local-copy", "lab").expect("rename");
        assert!(renamed.exists());
        assert!(!duplicate.exists());

        let deleted = delete_profile_at(config_home, "lab").expect("delete");
        assert_eq!(
            deleted.file_name().and_then(|v| v.to_str()),
            Some("lab.yaml")
        );
        assert!(!deleted.exists());
    }

    #[test]
    fn rename_profile_rejects_path_like_names() {
        let home = temp_home();
        let config_home = home.path();
        write_source_profile(config_home, "local", "local");

        let err = rename_profile_at(config_home, "local", "../other").unwrap_err();
        assert!(err.to_string().contains("simple file name"));
    }

    #[test]
    fn classifies_one_and_server_profiles() {
        let home = temp_home();
        let config_home = home.path();

        create_profile_from_default_scope_at(config_home, "one", ProfileScope::One)
            .expect("create one");
        create_profile_from_default_scope_at(config_home, "server", ProfileScope::Server)
            .expect("create server");

        let records = list_profile_records_at(config_home).expect("records");
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .any(|record| record.name == "one" && matches!(record.scope, ProfileScope::One)));
        assert!(records
            .iter()
            .any(|record| record.name == "server" && matches!(record.scope, ProfileScope::Server)));
    }
}
