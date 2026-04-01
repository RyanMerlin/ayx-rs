use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use roxmltree::Document;
use serde::Serialize;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use walkdir::WalkDir;
use zip::read::ZipArchive;
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowReplacement {
    pub find: String,
    pub replace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowIssue {
    pub path: String,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRules {
    pub replacements: Vec<WorkflowReplacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowMatch {
    pub path: String,
    pub matches: Vec<String>,
}

fn workflow_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
    {
        Some(ext) if ext == "yxmd" => "workflow",
        Some(ext) if ext == "yxmc" => "macro",
        Some(ext) if ext == "yxzp" => "package",
        Some(ext) if ext == "yxdb" => "data",
        Some(ext) if ext == "xml" => "xml",
        _ => "other",
    }
}

fn is_xml_like(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "yxmd" || ext == "yxmc" || ext == "xml"
    )
}

fn is_workflow_artifact(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "yxmd" || ext == "yxmc" || ext == "yxzp" || ext == "yxdb" || ext == "xml"
    )
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn validate_xml_text(text: &str) -> Result<()> {
    let normalized = normalize_text(text);
    Document::parse(&normalized).context("failed to parse workflow xml")?;
    Ok(())
}

fn apply_replacements(text: &str, replacements: &[WorkflowReplacement]) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut matches = Vec::new();
    for replacement in replacements {
        if out.contains(&replacement.find) {
            matches.push(replacement.find.clone());
            out = out.replace(&replacement.find, &replacement.replace);
        }
    }
    (out, matches)
}

fn scan_text(text: &str, replacements: &[WorkflowReplacement]) -> Vec<String> {
    let mut matches = Vec::new();
    for replacement in replacements {
        if text.contains(&replacement.find) {
            matches.push(replacement.find.clone());
        }
    }
    matches
}

pub fn load_rules(path: &Path) -> Result<WorkflowRules> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow rules '{}'", path.display()))?;
    let yaml: YamlValue = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse workflow rules '{}'", path.display()))?;
    let replacements = yaml
        .get("replacements")
        .and_then(|value| value.as_sequence())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow rules '{}' missing replacements array",
                path.display()
            )
        })?
        .iter()
        .map(|item| {
            let find = item
                .get("find")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "workflow rules '{}' replacement missing find",
                        path.display()
                    )
                })?;
            let replace = item
                .get("replace")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "workflow rules '{}' replacement missing replace",
                        path.display()
                    )
                })?;
            Ok(WorkflowReplacement {
                find: find.to_string(),
                replace: replace.to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(WorkflowRules { replacements })
}

fn scan_path(path: &Path, replacements: &[WorkflowReplacement]) -> Result<Vec<WorkflowMatch>> {
    if path.is_dir() {
        let mut results = Vec::new();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                let text = if entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|s| s.eq_ignore_ascii_case("yxzp"))
                    .unwrap_or(false)
                {
                    continue;
                } else {
                    read_text(entry.path())?
                };
                let matches = scan_text(&text, replacements);
                if !matches.is_empty() {
                    results.push(WorkflowMatch {
                        path: entry.path().display().to_string(),
                        matches,
                    });
                }
            }
        }
        return Ok(results);
    }

    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
        let mut results = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_string();
            let entry_path = Path::new(&name);
            if is_xml_like(entry_path) {
                let mut buf = String::new();
                entry.read_to_string(&mut buf)?;
                let matches = scan_text(&buf, replacements);
                if !matches.is_empty() {
                    results.push(WorkflowMatch {
                        path: name,
                        matches,
                    });
                }
            }
        }
        return Ok(results);
    }

    let text = read_text(path)?;
    let matches = scan_text(&text, replacements);
    Ok(if matches.is_empty() {
        Vec::new()
    } else {
        vec![WorkflowMatch {
            path: path.display().to_string(),
            matches,
        }]
    })
}

pub fn scan(path: &Path, replacements: &[WorkflowReplacement]) -> Result<Value> {
    let matches = scan_path(path, replacements)?;
    Ok(json!({
        "path": path.display().to_string(),
        "match_count": matches.len(),
        "matches": matches,
    }))
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(text),
        Err(_) => Ok(String::from_utf8_lossy(&bytes).to_string()),
    }
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

fn inspect_file(path: &Path) -> Result<Value> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat '{}'", path.display()))?;
    let kind = workflow_kind(path);
    let content = if is_xml_like(path) {
        let text = read_text(path)?;
        let valid = validate_xml_text(&text).is_ok();
        json!({
            "xml_valid": valid,
            "contains": {
                "workflow": text.contains("<Nodes") || text.contains("<Node"),
                "macro": text.contains("Macro") || text.contains("<EngineSettings"),
            }
        })
    } else {
        json!({})
    };

    Ok(json!({
        "path": path.display().to_string(),
        "kind": kind,
        "size_bytes": metadata.len(),
        "xml": content,
    }))
}

fn inspect_package(path: &Path) -> Result<Value> {
    let file =
        fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
    let mut entries = Vec::new();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let entry_path = Path::new(&name);
        let kind = workflow_kind(entry_path).to_string();
        *counts.entry(kind).or_insert(0) += 1;
        entries.push(json!({
            "name": name,
            "kind": workflow_kind(entry_path),
            "size_bytes": entry.size(),
        }));
    }
    Ok(json!({
        "path": path.display().to_string(),
        "kind": "package",
        "entry_count": entries.len(),
        "kind_counts": counts,
        "entries": entries,
    }))
}

pub fn inspect(path: &Path) -> Result<Value> {
    if path.is_dir() {
        let mut items = Vec::new();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                items.push(inspect_file(entry.path())?);
            }
        }
        return Ok(json!({
            "path": path.display().to_string(),
            "kind": "directory",
            "item_count": items.len(),
            "items": items,
        }));
    }

    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        return inspect_package(path);
    }

    inspect_file(path)
}

pub fn unpack_package(input: &Path, output_dir: &Path) -> Result<Value> {
    let file =
        fs::File::open(input).with_context(|| format!("failed to open '{}'", input.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive '{}'", input.display()))?;
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create '{}'", output_dir.display()))?;

    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let out_path = output_dir.join(&name);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("failed to create '{}'", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create '{}'", parent.display()))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create '{}'", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)?;
        entries.push(name);
    }

    Ok(json!({
        "input": input.display().to_string(),
        "output_dir": output_dir.display().to_string(),
        "entry_count": entries.len(),
        "entries": entries,
    }))
}

pub fn repackage_dir(input_dir: &Path, output_path: &Path) -> Result<Value> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let file = fs::File::create(output_path)
        .with_context(|| format!("failed to create '{}'", output_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut entry_count = 0usize;

    for entry in WalkDir::new(input_dir) {
        let entry = entry?;
        let path = entry.path();
        if path == input_dir {
            continue;
        }
        let rel = path
            .strip_prefix(input_dir)
            .with_context(|| format!("failed to strip prefix '{}'", input_dir.display()))?;
        if entry.file_type().is_dir() {
            let rel_name = format!("{}/", rel.to_string_lossy().replace('\\', "/"));
            zip.add_directory(rel_name, options)?;
            continue;
        }
        zip.start_file(rel.to_string_lossy().replace('\\', "/"), options)?;
        let mut input = fs::File::open(path)?;
        std::io::copy(&mut input, &mut zip)?;
        entry_count += 1;
    }
    zip.finish()?;

    Ok(json!({
        "input_dir": input_dir.display().to_string(),
        "output": output_path.display().to_string(),
        "entry_count": entry_count,
    }))
}

pub fn validate(path: &Path) -> Result<Value> {
    let mut issues = Vec::<WorkflowIssue>::new();
    let mut validated = Vec::new();

    if path.is_dir() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_xml_like(entry.path()) {
                let text = read_text(entry.path())?;
                match validate_xml_text(&text) {
                    Ok(()) => validated.push(entry.path().display().to_string()),
                    Err(err) => issues.push(WorkflowIssue {
                        path: entry.path().display().to_string(),
                        issue: err.to_string(),
                    }),
                }
            }
        }
    } else if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_string();
            let entry_path = Path::new(&name);
            if is_xml_like(entry_path) {
                let mut buf = String::new();
                entry.read_to_string(&mut buf)?;
                match validate_xml_text(&buf) {
                    Ok(()) => validated.push(name),
                    Err(err) => issues.push(WorkflowIssue {
                        path: name,
                        issue: err.to_string(),
                    }),
                }
            }
        }
    } else if is_xml_like(path) {
        let text = read_text(path)?;
        validate_xml_text(&text)?;
        validated.push(path.display().to_string());
    } else {
        bail!("workflow validate expects a .yxmd, .yxmc, .yxzp, or directory");
    }

    Ok(json!({
        "path": path.display().to_string(),
        "validated": validated,
        "issues": issues,
        "ok": issues.is_empty(),
    }))
}

pub fn replace(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input.is_dir() {
        fs::create_dir_all(output)
            .with_context(|| format!("failed to create '{}'", output.display()))?;
        let mut touched = Vec::new();
        for entry in WalkDir::new(input)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                let rel = entry
                    .path()
                    .strip_prefix(input)
                    .with_context(|| format!("failed to strip prefix '{}'", input.display()))?;
                let out_path = output.join(rel);
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let text = read_text(entry.path())?;
                let (replaced, found) = apply_replacements(&text, replacements);
                write_text(&out_path, &replaced)?;
                touched.push(json!({
                    "path": rel.to_string_lossy(),
                    "matches": found,
                }));
            }
        }
        let validation = if validate_after {
            Some(validate(output)?)
        } else {
            None
        };
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "directory",
            "touched": touched,
            "validation": validation,
        }));
    }

    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let unpack_dir = output.with_extension("unpacked");
        if unpack_dir.exists() {
            fs::remove_dir_all(&unpack_dir)?;
        }
        unpack_package(input, &unpack_dir)?;
        let result = replace(&unpack_dir, &unpack_dir, replacements, validate_after)?;
        repackage_dir(&unpack_dir, output)?;
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "package",
            "unpacked_dir": unpack_dir.display().to_string(),
            "replace_result": result,
        }));
    }

    let text = read_text(input)?;
    let (replaced, found) = apply_replacements(&text, replacements);
    if validate_after && is_xml_like(input) {
        validate_xml_text(&replaced)?;
    }
    write_text(output, &replaced)?;
    Ok(json!({
        "input": input.display().to_string(),
        "output": output.display().to_string(),
        "mode": "file",
        "matches": found,
        "validated": validate_after,
    }))
}

pub fn migrate(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    replace(input, output, replacements, validate_after)
}

fn recurse_directory(
    input_dir: &Path,
    output_dir: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input_dir == output_dir {
        let mut touched = Vec::new();
        let mut nested = Vec::new();
        for entry in WalkDir::new(input_dir)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|s| s.eq_ignore_ascii_case("yxzp"))
                .unwrap_or(false)
            {
                let unpack_dir = path.with_extension("unpacked");
                if unpack_dir.exists() {
                    fs::remove_dir_all(&unpack_dir)?;
                }
                unpack_package(path, &unpack_dir)?;
                let nested_result =
                    recurse_directory(&unpack_dir, &unpack_dir, replacements, validate_after)?;
                repackage_dir(&unpack_dir, path)?;
                nested.push(json!({
                    "package": path.display().to_string(),
                    "result": nested_result,
                }));
                continue;
            }
            if is_workflow_artifact(path) {
                let text = read_text(path)?;
                let (replaced, found) = apply_replacements(&text, replacements);
                if validate_after && is_xml_like(path) {
                    validate_xml_text(&replaced)?;
                }
                write_text(path, &replaced)?;
                let rel = path.strip_prefix(input_dir).unwrap_or(path);
                touched.push(json!({
                    "path": rel.to_string_lossy(),
                    "matches": found,
                }));
            }
        }
        let validation = if validate_after {
            Some(validate(input_dir)?)
        } else {
            None
        };
        return Ok(json!({
            "input": input_dir.display().to_string(),
            "output": output_dir.display().to_string(),
            "mode": "directory",
            "touched": touched,
            "nested_packages": nested,
            "validation": validation,
        }));
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create '{}'", output_dir.display()))?;
    let mut touched = Vec::new();
    let mut nested = Vec::new();
    for entry in WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(input_dir)
            .with_context(|| format!("failed to strip prefix '{}'", input_dir.display()))?;
        let out_path = output_dir.join(rel);
        if entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("yxzp"))
            .unwrap_or(false)
        {
            let nested_unpack = out_path.with_extension("unpacked");
            if nested_unpack.exists() {
                fs::remove_dir_all(&nested_unpack)?;
            }
            unpack_package(entry.path(), &nested_unpack)?;
            let nested_result =
                recurse_directory(&nested_unpack, &nested_unpack, replacements, validate_after)?;
            repackage_dir(&nested_unpack, &out_path)?;
            nested.push(json!({
                "package": rel.to_string_lossy(),
                "result": nested_result,
            }));
            continue;
        }
        if is_workflow_artifact(entry.path()) {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let text = read_text(entry.path())?;
            let (replaced, found) = apply_replacements(&text, replacements);
            if validate_after && is_xml_like(entry.path()) {
                validate_xml_text(&replaced)?;
            }
            write_text(&out_path, &replaced)?;
            touched.push(json!({
                "path": rel.to_string_lossy(),
                "matches": found,
            }));
        }
    }
    let validation = if validate_after {
        Some(validate(output_dir)?)
    } else {
        None
    };
    Ok(json!({
        "input": input_dir.display().to_string(),
        "output": output_dir.display().to_string(),
        "mode": "directory",
        "touched": touched,
        "nested_packages": nested,
        "validation": validation,
    }))
}

pub fn recurse(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let unpack_dir = output.with_extension("unpacked");
        if unpack_dir.exists() {
            fs::remove_dir_all(&unpack_dir)?;
        }
        unpack_package(input, &unpack_dir)?;
        let result = recurse_directory(&unpack_dir, &unpack_dir, replacements, validate_after)?;
        if output
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("yxzp"))
            .unwrap_or(false)
        {
            repackage_dir(&unpack_dir, output)?;
        }
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "package",
            "unpacked_dir": unpack_dir.display().to_string(),
            "result": result,
        }));
    }

    if input.is_dir() {
        return recurse_directory(input, output, replacements, validate_after);
    }

    replace(input, output, replacements, validate_after)
}

pub fn package_summary(path: &Path) -> Result<Value> {
    inspect(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ayx-workflow-{}-{}-{}",
            std::process::id(),
            nanos,
            name
        ))
    }

    #[test]
    fn validate_xml_and_replace_text() {
        let input = temp_path("workflow.yxmd");
        write_text(
            &input,
            "<AlteryxDocument><Node>abc</Node></AlteryxDocument>",
        )
        .unwrap();
        let output = temp_path("workflow-out.yxmd");
        let result = replace(
            &input,
            &output,
            &[WorkflowReplacement {
                find: "abc".into(),
                replace: "xyz".into(),
            }],
            true,
        )
        .unwrap();
        assert!(result["matches"].as_array().unwrap().len() == 1);
        let text = read_text(&output).unwrap();
        assert!(text.contains("xyz"));
        let _ = fs::remove_file(&input);
        let _ = fs::remove_file(&output);
    }

    #[test]
    fn inspect_xml_file() {
        let input = temp_path("workflow.yxmc");
        write_text(
            &input,
            "<AlteryxDocument><Node>abc</Node></AlteryxDocument>",
        )
        .unwrap();
        let result = inspect(&input).unwrap();
        assert_eq!(result["kind"], "macro");
        let _ = fs::remove_file(&input);
    }
}
