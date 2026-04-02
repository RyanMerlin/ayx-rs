use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ayx_workflow::{
    inspect, load_rules, package_summary, read_yxdb, recurse, repackage_dir, scan, unpack_package,
    validate,
};

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ayx-workflow-canary-{}-{}-{}",
        std::process::id(),
        nanos,
        name
    ))
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ayx-rs")
        .join("tests")
        .join("fixtures")
        .join("workflow-canary")
}

fn yxdb_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ayx-rs")
        .join("tests")
        .join("fixtures")
        .join("yxdb")
        .join("RuntimeSettings.yxdb")
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == src {
            continue;
        }
        let rel = path.strip_prefix(src).unwrap();
        let out = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&out).unwrap();
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(path, &out).unwrap();
        }
    }
}

#[test]
fn workflow_fixture_scan_and_recurse_round_trip() {
    let src = fixture_dir();
    let working = temp_path("working");
    copy_dir(&src, &working);

    let rules = load_rules(&working.join("rules.yaml")).unwrap();
    let scan_result = scan(&working, &rules.replacements).unwrap();
    assert_eq!(scan_result["match_count"].as_u64().unwrap(), 2);
    let matches = scan_result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);

    let output = temp_path("recurse-out");
    let recurse_result = recurse(&working, &output, &rules.replacements, true).unwrap();
    assert_eq!(recurse_result["mode"], "directory");

    let rewritten_root = fs::read_to_string(output.join("root.yxmd")).unwrap();
    assert!(rewritten_root.contains("new.domain.com"));
    assert!(rewritten_root.contains(r"D:\NewPath\input"));

    let rewritten_macro = fs::read_to_string(output.join("macros").join("helper.yxmc")).unwrap();
    assert!(rewritten_macro.contains("server-b"));

    let validate_result = validate(&output).unwrap();
    assert!(validate_result["ok"].as_bool().unwrap());

    let package = temp_path("package.yxzp");
    let repack_result = repackage_dir(&output, &package).unwrap();
    assert_eq!(repack_result["entry_count"].as_u64().unwrap(), 2);

    let inspect_result = inspect(&package).unwrap();
    assert_eq!(inspect_result["kind"], "package");
    let summary = package_summary(&package).unwrap();
    assert_eq!(summary["kind"], "package");
    let entries = inspect_result["entries"].as_array().unwrap();
    assert!(entries.len() >= 2);
    let entry_names: Vec<_> = entries
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert!(entry_names.iter().any(|name| name.ends_with("root.yxmd")));
    assert!(entry_names.iter().any(|name| name.ends_with("helper.yxmc")));

    let unpacked = temp_path("unpacked");
    let unpack_result = unpack_package(&package, &unpacked).unwrap();
    assert!(unpack_result["entry_count"].as_u64().unwrap() >= 2);

    let unpacked_root = fs::read_to_string(unpacked.join("root.yxmd")).unwrap();
    assert!(unpacked_root.contains("new.domain.com"));

    let _ = fs::remove_dir_all(&working);
    let _ = fs::remove_dir_all(&output);
    let _ = fs::remove_file(&package);
    let _ = fs::remove_dir_all(&unpacked);
}

#[test]
fn workflow_fixture_rules_load() {
    let rules = load_rules(&fixture_dir().join("rules.yaml")).unwrap();
    assert_eq!(rules.replacements.len(), 3);
}

#[test]
fn yxdb_fixture_reads_and_exports_csv() {
    let fixture = yxdb_fixture();
    let out_csv = temp_path("runtime-settings.csv");
    let result = read_yxdb(&fixture, Some(&out_csv)).unwrap();
    assert!(result["field_count"].as_u64().unwrap() > 0);
    assert!(result["row_count"].as_u64().unwrap() > 0);
    assert!(out_csv.exists());
    let csv = fs::read_to_string(&out_csv).unwrap();
    assert!(csv.lines().count() > 1);
    let _ = fs::remove_file(&out_csv);
}
