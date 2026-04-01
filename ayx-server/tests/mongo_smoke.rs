use std::fs;
use std::path::Path;

use ayx_core::profile::{
    ApiAuth, ApiAuthMode, ApiProfile, Config, MongoDatabases, MongoEmbedded, MongoMode,
    MongoProfile, ServerProfile,
};
use ayx_server::mongo::{backup_envelope, inventory_envelope, restore_envelope, status_envelope};

fn test_profile() -> Config {
    let runtime_settings = temp_dir("ayx-runtime-settings").join("RuntimeSettings.xml");
    fs::write(
        &runtime_settings,
        r#"
<AlteryxConfiguration>
  <AuthenticationType>Windows</AuthenticationType>
  <EmbeddedMongoDBEnabled>true</EmbeddedMongoDBEnabled>
  <EmbeddedMongoDBRootPath>C:\ProgramData\Alteryx\Service\Persistence\MongoDB</EmbeddedMongoDBRootPath>
  <WorkingPath>C:\ProgramData\Alteryx\Service\Persistence\MongoDB</WorkingPath>
</AlteryxConfiguration>
"#,
    )
    .expect("runtime settings should be writable");

    Config {
        profile_name: "test".to_string(),
        server_api: None,
        mongo: MongoProfile {
            mode: MongoMode::Embedded,
            databases: MongoDatabases {
                gallery_name: "AlteryxGallery".to_string(),
                service_name: "AlteryxService".to_string(),
            },
            embedded: Some(MongoEmbedded {
                runtime_settings_path: Some(runtime_settings.display().to_string()),
                alteryx_service_path: None,
                restore_target_path: None,
            }),
            managed: None,
        },
        api: Some(ApiProfile {
            base_url: "http://localhost/webapi/".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Pat,
                pat: Some("abc".to_string()),
                client_id: None,
                client_secret: None,
                scope: None,
            },
            timeout_ms: Some(1000),
        }),
        alteryx_one: None,
        observability: None,
        server: Some(ServerProfile {
            webapi_url: "http://localhost/webapi/".to_string(),
            curator_api_key: "abc".to_string(),
            curator_api_secret: "secret".to_string(),
            verify_tls: Some(true),
        }),
        upgrade: None,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    path.push(format!("{}-{}-{}", name, std::process::id(), nanos));
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

#[test]
fn mongo_status_and_inventory_shape() {
    let profile = test_profile();
    let status = status_envelope(&profile).expect("status should resolve");
    assert_eq!(status.data["profile"], "test");
    assert_eq!(status.data["mode"], "embedded");

    let inventory = inventory_envelope(&profile).expect("inventory should resolve");
    assert_eq!(inventory.data["profile"], "test");
    assert!(!inventory.data["operations"].as_array().unwrap().is_empty());
}

#[test]
fn mongo_backup_and_restore_dry_run_write_audits() {
    let profile = test_profile();
    let backup_dir = temp_dir("ayx-mongo-backup");
    let audit_dir = temp_dir("ayx-mongo-audit");

    let backup = backup_envelope(&profile, &backup_dir, false, &audit_dir)
        .expect("backup dry-run should succeed");
    assert_eq!(backup.data["dry_run"], true);
    assert!(Path::new(backup.data["audit_artifact"].as_str().unwrap()).exists());

    let input_path = backup_dir.join("restore-input.zip");
    fs::write(&input_path, b"placeholder").expect("restore input should be writable");
    let restore = restore_envelope(&profile, &input_path, false, &audit_dir)
        .expect("restore dry-run should succeed");
    assert_eq!(restore.data["dry_run"], true);
    assert!(Path::new(restore.data["audit_artifact"].as_str().unwrap()).exists());
}
