use std::fs;

use ayx_server::upgrade::compute_path;
use ayx_server::util::{backup_plan, runtime_settings_summary};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    path.push(format!("{}-{}-{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

#[test]
fn runtime_settings_summary_parses_expected_fields() {
    let dir = unique_temp_dir("ayx-server-runtime");
    let path = dir.join("RuntimeSettings.xml");
    fs::write(
        &path,
        r#"
<AlteryxConfiguration>
  <AuthenticationType>Windows</AuthenticationType>
  <EmbeddedMongoDBEnabled>true</EmbeddedMongoDBEnabled>
  <EmbeddedMongoDBRootPath>C:\ProgramData\Alteryx\Service\Persistence\MongoDB</EmbeddedMongoDBRootPath>
  <MongoHost>localhost</MongoHost>
  <MongoPort>27018</MongoPort>
  <MongoUser>admin</MongoUser>
</AlteryxConfiguration>
"#,
    )
    .expect("runtime settings should be writable");

    let summary = runtime_settings_summary(&path).expect("summary should parse");
    assert_eq!(
        summary["system_settings"]["controller"]["embedded_mongo_enabled"],
        "true"
    );
    assert_eq!(summary["system_settings"]["mongo"]["host"], "localhost");
}

#[test]
fn backup_plan_reports_expected_artifacts() {
    let dir = unique_temp_dir("ayx-server-backup");
    let plan = backup_plan(&dir).expect("backup plan should generate");
    assert_eq!(plan["backup_dir"], dir.display().to_string());
    assert!(
        plan["commands"]
            .as_array()
            .map(|steps| !steps.is_empty())
            .unwrap_or(false)
    );
    assert!(
        plan["settings_files"]
            .as_array()
            .map(|steps| !steps.is_empty())
            .unwrap_or(false)
    );
}

#[test]
fn upgrade_path_is_computable() {
    let detail = compute_path("2024.1", "2025.1", "embedded-mongo");
    assert_eq!(detail["source"], "2024.1");
    assert_eq!(detail["target"], "2025.1");
    assert_eq!(detail["deployment"], "embedded-mongo");
}
