use std::fs;
use std::path::Path;

use ayx_api::workflow_transfer_owner_envelope;
use ayx_core::profile::{
    ApiAuth, ApiAuthMode, ApiProfile, Config, MongoDatabases, MongoEmbedded, MongoMode,
    MongoProfile, ServerProfile,
};

fn test_profile() -> Config {
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
                runtime_settings_path: Some("examples/RuntimeSettings.xml".to_string()),
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
fn workflow_transfer_owner_dry_run_writes_audit() {
    let profile = test_profile();
    let audit_dir = temp_dir("ayx-api-audit");
    let env =
        workflow_transfer_owner_envelope(&profile, "wf-1", "owner-2", true, false, &audit_dir)
            .expect("dry run should succeed");

    assert_eq!(env.data["dry_run"], true);
    let artifact = env.data["audit_artifact"]
        .as_str()
        .expect("artifact path missing");
    assert!(Path::new(artifact).exists());
}
