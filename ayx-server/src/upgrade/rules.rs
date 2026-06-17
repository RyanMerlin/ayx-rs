use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_yaml;

#[derive(Debug, Deserialize)]
struct PathsConfig {
    #[allow(dead_code)]
    versions: Vec<String>,
    edges: HashMap<String, Vec<[String; 2]>>,
}

#[derive(Debug, Deserialize)]
struct IssueDefinition {
    id: String,
    severity: String,
    message: String,
    when: IssueWhen,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct IssueWhen {
    deployment: Option<String>,
    source_gte: Option<String>,
    source_lte: Option<String>,
    target_gte: Option<String>,
    target_lte: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeIssue {
    pub issue_id: String,
    pub severity: String,
    pub message: String,
    pub applies: IssueWhen,
}

pub struct UpgradeRules {
    paths: PathsConfig,
    issues: Vec<IssueDefinition>,
}

impl UpgradeRules {
    pub fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = base.join("knowledge/upgrade");
        let paths =
            read_optional_yaml(folder.join("version_paths.yaml")).unwrap_or_else(|| PathsConfig {
                versions: vec![
                    "2023.1".into(),
                    "2023.2".into(),
                    "2024.1".into(),
                    "2024.2".into(),
                    "2025.1".into(),
                    "2025.2".into(),
                ],
                edges: HashMap::new(),
            });
        let issues = read_optional_yaml(folder.join("known_issues.yaml")).unwrap_or_default();
        UpgradeRules { paths, issues }
    }

    pub fn resolve_path(&self, source: &str, target: &str, deployment: &str) -> Vec<String> {
        if source == target {
            return vec![source.to_string()];
        }
        let edges = self.edges_for(deployment);
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in edges {
            adjacency.entry(from).or_default().push(to);
        }
        let mut queue: VecDeque<Vec<String>> = VecDeque::new();
        let mut seen: HashSet<String> = HashSet::new();
        queue.push_back(vec![source.to_string()]);
        seen.insert(source.to_string());
        while let Some(path) = queue.pop_front() {
            if let Some(last) = path.last()
                && let Some(neighbors) = adjacency.get(last)
            {
                for neighbor in neighbors {
                    if seen.contains(neighbor) {
                        continue;
                    }
                    let mut candidate = path.clone();
                    candidate.push(neighbor.clone());
                    if neighbor == target {
                        return candidate;
                    }
                    seen.insert(neighbor.clone());
                    queue.push_back(candidate);
                }
            }
        }
        vec![]
    }

    pub fn matching_issues(
        &self,
        source: &str,
        target: &str,
        deployment: &str,
    ) -> Vec<UpgradeIssue> {
        self.issues
            .iter()
            .filter(|issue| {
                let applies_dep = issue.when.deployment.as_deref().unwrap_or("any");
                if applies_dep != "any" && applies_dep != deployment {
                    return false;
                }
                version_in_range(
                    source,
                    issue.when.source_gte.as_deref(),
                    issue.when.source_lte.as_deref(),
                ) && version_in_range(
                    target,
                    issue.when.target_gte.as_deref(),
                    issue.when.target_lte.as_deref(),
                )
            })
            .map(|issue| UpgradeIssue {
                issue_id: issue.id.clone(),
                severity: issue.severity.clone(),
                message: issue.message.clone(),
                applies: issue.when.clone(),
            })
            .collect()
    }

    fn edges_for(&self, deployment: &str) -> Vec<(String, String)> {
        if let Some(edge) = self.paths.edges.get(deployment) {
            return edge
                .iter()
                .map(|pair| (pair[0].clone(), pair[1].clone()))
                .collect();
        }
        if let Some(default) = self.paths.edges.get("default") {
            return default
                .iter()
                .map(|pair| (pair[0].clone(), pair[1].clone()))
                .collect();
        }
        vec![]
    }
}

impl Default for UpgradeRules {
    fn default() -> Self {
        Self::new()
    }
}

fn read_optional_yaml<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Option<T> {
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_yaml::from_str(&text).ok()
}

fn version_in_range(version: &str, min: Option<&str>, max: Option<&str>) -> bool {
    if let Some(minimum) = min
        && compare_version(version, minimum) < 0
    {
        return false;
    }
    if let Some(maximum) = max
        && compare_version(version, maximum) > 0
    {
        return false;
    }
    true
}

fn compare_version(left: &str, right: &str) -> i32 {
    let left_parts: Vec<i32> = left
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    let right_parts: Vec<i32> = right
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    for (a, b) in left_parts.iter().zip(&right_parts) {
        if a != b {
            return if a < b { -1 } else { 1 };
        }
    }
    if left_parts.len() < right_parts.len() {
        -1
    } else if left_parts.len() > right_parts.len() {
        1
    } else {
        0
    }
}
