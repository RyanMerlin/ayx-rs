use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use roxmltree::Document;
use serde_json::{json, Value};

use ayx_core::envelope::Envelope;
use ayx_workflow::{inspect as inspect_workflow, validate as validate_workflow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyMode {
    ReadOnly,
    Mutating,
}

impl SafetyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Mutating => "mutating",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityProvider {
    DesignerLocal,
    CloudRemote,
    Hybrid,
}

impl CapabilityProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesignerLocal => "designer_local",
            Self::CloudRemote => "cloud_remote",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityDescriptor {
    pub id: &'static str,
    pub summary: &'static str,
    pub tags: &'static [&'static str],
    pub safety: SafetyMode,
    pub provider: CapabilityProvider,
    pub input_schema: Value,
    pub output_schema: Value,
    pub notes: &'static [&'static str],
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DesignerMessageEnvelope {
    pub version: String,
    pub message_type: String,
    pub correlation_id: Option<String>,
    pub capability_id: Option<String>,
    pub payload: Value,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct DesignerIpcAdapter {
    pending: HashMap<String, DesignerMessageEnvelope>,
    buffered_events: Vec<DesignerMessageEnvelope>,
}

#[allow(dead_code)]
impl DesignerIpcAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn outbound(
        &mut self,
        capability_id: &str,
        correlation_id: &str,
        payload: Value,
    ) -> DesignerMessageEnvelope {
        let message = DesignerMessageEnvelope {
            version: "eel.nexus.v1".to_string(),
            message_type: "request".to_string(),
            correlation_id: Some(correlation_id.to_string()),
            capability_id: Some(capability_id.to_string()),
            payload,
        };
        self.pending
            .insert(correlation_id.to_string(), message.clone());
        message
    }

    pub fn receive(&mut self, message: DesignerMessageEnvelope) {
        match message.correlation_id.as_deref() {
            Some(correlation_id) if self.pending.contains_key(correlation_id) => {
                self.pending.insert(correlation_id.to_string(), message);
            }
            _ => self.buffered_events.push(message),
        }
    }

    pub fn take_response(&mut self, correlation_id: &str) -> Option<DesignerMessageEnvelope> {
        self.pending.remove(correlation_id).and_then(|message| {
            if message.message_type == "response" {
                Some(message)
            } else {
                None
            }
        })
    }

    pub fn buffered_events(&self) -> &[DesignerMessageEnvelope] {
        &self.buffered_events
    }
}

#[derive(Debug, Clone, Default)]
pub struct CloudCapabilityAdapter {
    supported: HashMap<String, bool>,
}

impl CloudCapabilityAdapter {
    pub fn discover(response: &Value) -> Self {
        let mut supported = HashMap::new();
        for capability in response
            .get("capabilities")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = capability.get("id").and_then(Value::as_str) {
                let available = capability
                    .get("available")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                supported.insert(id.to_string(), available);
            }
        }
        Self { supported }
    }

    pub fn from_env() -> Result<Self> {
        let Some(path) = std::env::var_os("AYX_CLOUD_CAPABILITIES_FILE") else {
            return Ok(Self::default());
        };
        let path = PathBuf::from(path);
        let raw = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read cloud capability discovery '{}'",
                path.display()
            )
        })?;
        let value: Value = serde_json::from_str(&raw).with_context(|| {
            format!(
                "failed to parse cloud capability discovery '{}'",
                path.display()
            )
        })?;
        Ok(Self::discover(&value))
    }

    pub fn supports(&self, capability_id: &str) -> bool {
        self.supported.get(capability_id).copied().unwrap_or(false)
    }
}

pub trait CapabilityExecutor {
    fn execute(&self, input: &Value, dry_run: bool) -> Result<Value>;
}

struct FnCapabilityExecutor(fn(&Value, bool) -> Result<Value>);

impl CapabilityExecutor for FnCapabilityExecutor {
    fn execute(&self, input: &Value, dry_run: bool) -> Result<Value> {
        (self.0)(input, dry_run)
    }
}

struct CapabilityRegistration {
    descriptor: CapabilityDescriptor,
    executor: Box<dyn CapabilityExecutor + Send + Sync>,
}

fn string_schema(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn boolean_schema(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn designer_capabilities() -> Vec<CapabilityRegistration> {
    vec![
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.workflow.context",
                summary: "Build local workflow context from a workflow XML artifact.",
                tags: &["designer", "workflow", "context", "local"],
                safety: SafetyMode::ReadOnly,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path"],
                    "properties": {
                        "workflow_path": string_schema("Path to a .yxmd or .yxmc workflow file."),
                        "selected_tool_ids": {
                    "type": "array",
                    "items": string_schema("Tool id to highlight in the response.")
                        }
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow": { "type": "object" },
                        "tools": { "type": "array" },
                        "selected_tools": { "type": "array" },
                        "connections": { "type": "array" }
                    }
                }),
                notes: &[
                    "Uses local workflow/XML parsing today.",
                    "This is the stable context shape the future Designer IPC adapter will feed.",
                ],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_workflow_context)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.workflow.run",
                summary: "Run the local workflow capability surface with dry-run-aware validation.",
                tags: &["designer", "workflow", "run", "local"],
                safety: SafetyMode::ReadOnly,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path"],
                    "properties": {
                        "workflow_path": string_schema("Path to the workflow to validate/run."),
                        "run_label": string_schema("Optional caller-supplied run label.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "run_mode": string_schema("Execution mode used by ayx-rs."),
                        "validation": { "type": "object" },
                        "context": { "type": "object" }
                    }
                }),
                notes: &[
                    "The first native slice runs local validation and context capture.",
                    "Live Designer execution can replace the backend without changing the public capability id.",
                ],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_workflow_run)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.tool.add",
                summary: "Add a tool node to a local workflow XML document.",
                tags: &["designer", "tool", "mutating", "local"],
                safety: SafetyMode::Mutating,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path", "node_xml"],
                    "properties": {
                        "workflow_path": string_schema("Path to the workflow to update."),
                        "node_xml": string_schema("Exact <Node> XML to add inside <Nodes>."),
                        "write_back": boolean_schema("Persist the mutation to the source workflow. Defaults to true.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow_path": string_schema("Mutated workflow path."),
                        "tool_count": { "type": "integer" },
                        "updated_xml": string_schema("Updated workflow XML when write_back is false or dry_run is true.")
                    }
                }),
                notes: &["Mutations support dry-run previews without rewriting the file."],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_tool_add)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.tool.edit",
                summary: "Replace a tool node in a local workflow XML document.",
                tags: &["designer", "tool", "mutating", "local"],
                safety: SafetyMode::Mutating,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path", "tool_id", "node_xml"],
                    "properties": {
                        "workflow_path": string_schema("Path to the workflow to update."),
                        "tool_id": string_schema("ToolID to replace."),
                        "node_xml": string_schema("Replacement <Node> XML."),
                        "write_back": boolean_schema("Persist the mutation to the source workflow. Defaults to true.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow_path": string_schema("Mutated workflow path."),
                        "tool_id": string_schema("Edited tool id."),
                        "updated_xml": string_schema("Updated workflow XML when write_back is false or dry_run is true.")
                    }
                }),
                notes: &["Mutations support dry-run previews without rewriting the file."],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_tool_edit)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.tool.remove",
                summary: "Remove a tool node and related connections from a local workflow XML document.",
                tags: &["designer", "tool", "mutating", "local"],
                safety: SafetyMode::Mutating,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path", "tool_id"],
                    "properties": {
                        "workflow_path": string_schema("Path to the workflow to update."),
                        "tool_id": string_schema("ToolID to remove."),
                        "write_back": boolean_schema("Persist the mutation to the source workflow. Defaults to true.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow_path": string_schema("Mutated workflow path."),
                        "tool_id": string_schema("Removed tool id."),
                        "updated_xml": string_schema("Updated workflow XML when write_back is false or dry_run is true.")
                    }
                }),
                notes: &["Mutations support dry-run previews without rewriting the file."],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_tool_remove)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "designer.tool.replace-connections",
                summary: "Apply connection-fragment replacements inside a local workflow XML document.",
                tags: &["designer", "tool", "connections", "mutating", "local"],
                safety: SafetyMode::Mutating,
                provider: CapabilityProvider::DesignerLocal,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_path", "replacements"],
                    "properties": {
                        "workflow_path": string_schema("Path to the workflow to update."),
                        "replacements": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["find", "replace"],
                                "properties": {
                                    "find": string_schema("Exact connection fragment to replace."),
                                    "replace": string_schema("Replacement connection fragment.")
                                }
                            }
                        },
                        "write_back": boolean_schema("Persist the mutation to the source workflow. Defaults to true.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow_path": string_schema("Mutated workflow path."),
                        "replacement_count": { "type": "integer" },
                        "updated_xml": string_schema("Updated workflow XML when write_back is false or dry_run is true.")
                    }
                }),
                notes: &[
                    "This first slice operates on explicit connection XML fragments.",
                    "Future Designer IPC transport can map higher-level connection contracts onto the same capability id.",
                ],
            },
            executor: Box::new(FnCapabilityExecutor(execute_designer_tool_replace_connections)),
        },
    ]
}

fn cloud_capabilities() -> Vec<CapabilityRegistration> {
    vec![
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "cloud.docs.search",
                summary:
                    "Search cloud-side documentation capabilities when remote support is available.",
                tags: &["cloud", "docs", "search", "remote"],
                safety: SafetyMode::ReadOnly,
                provider: CapabilityProvider::CloudRemote,
                input_schema: json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": string_schema("Search query."),
                        "limit": { "type": "integer", "minimum": 1 }
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": string_schema("Search query."),
                        "results": { "type": "array" }
                    }
                }),
                notes: &["Execution is gated behind remote capability discovery."],
            },
            executor: Box::new(FnCapabilityExecutor(execute_cloud_capability_stub)),
        },
        CapabilityRegistration {
            descriptor: CapabilityDescriptor {
                id: "cloud.workflow.summarize",
                summary: "Summarize cloud workflow posture when the remote contract is available.",
                tags: &["cloud", "workflow", "hybrid"],
                safety: SafetyMode::ReadOnly,
                provider: CapabilityProvider::Hybrid,
                input_schema: json!({
                    "type": "object",
                    "required": ["workflow_id"],
                    "properties": {
                        "workflow_id": string_schema("Cloud workflow id.")
                    }
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "workflow_id": string_schema("Cloud workflow id."),
                        "summary": { "type": "object" }
                    }
                }),
                notes: &["Execution is gated behind remote capability discovery."],
            },
            executor: Box::new(FnCapabilityExecutor(execute_cloud_capability_stub)),
        },
    ]
}

fn registry() -> Vec<CapabilityRegistration> {
    let mut all = designer_capabilities();
    all.extend(cloud_capabilities());
    all
}

pub fn list_capabilities(tag: Option<&str>, full: bool) -> Result<Vec<Value>> {
    let cloud = CloudCapabilityAdapter::from_env()?;
    let entries = registry()
        .into_iter()
        .filter(|registration| {
            tag.map(|needle| {
                registration
                    .descriptor
                    .tags
                    .iter()
                    .any(|tag| tag == &needle)
            })
            .unwrap_or(true)
        })
        .map(|registration| descriptor_to_value(&registration.descriptor, full, &cloud))
        .collect();
    Ok(entries)
}

pub fn describe(identifier: &str) -> Result<Option<Value>> {
    let cloud = CloudCapabilityAdapter::from_env()?;
    let value = registry()
        .into_iter()
        .find(|registration| registration.descriptor.id == identifier)
        .map(|registration| descriptor_to_value(&registration.descriptor, true, &cloud));
    Ok(value)
}

pub fn run(capability_id: &str, input: &Value, dry_run: bool) -> Result<Envelope> {
    let cloud = CloudCapabilityAdapter::from_env()?;
    let registration = registry()
        .into_iter()
        .find(|registration| registration.descriptor.id == capability_id)
        .ok_or_else(|| anyhow!("capability '{}' not found", capability_id))?;

    let available = match registration.descriptor.provider {
        CapabilityProvider::DesignerLocal => true,
        CapabilityProvider::CloudRemote | CapabilityProvider::Hybrid => {
            cloud.supports(capability_id)
        }
    };

    if !available {
        bail!(
            "capability '{}' is not available in the current environment",
            capability_id
        );
    }

    let result = registration.executor.execute(input, dry_run)?;
    Ok(Envelope::ok_with_data(
        if dry_run {
            "capability dry-run prepared"
        } else {
            "capability executed"
        },
        json!({
            "kind": "capability_run",
            "capability": descriptor_to_value(&registration.descriptor, true, &cloud),
            "dry_run": dry_run,
            "input": input,
            "result": result,
        }),
    ))
}

fn descriptor_to_value(
    descriptor: &CapabilityDescriptor,
    full: bool,
    cloud: &CloudCapabilityAdapter,
) -> Value {
    let available = match descriptor.provider {
        CapabilityProvider::DesignerLocal => true,
        CapabilityProvider::CloudRemote | CapabilityProvider::Hybrid => {
            cloud.supports(descriptor.id)
        }
    };
    let mut base = json!({
        "kind": "capability",
        "id": descriptor.id,
        "summary": descriptor.summary,
        "tags": descriptor.tags,
        "safety": descriptor.safety.as_str(),
        "provider": descriptor.provider.as_str(),
        "available": available,
    });
    if full {
        base["input_schema"] = descriptor.input_schema.clone();
        base["output_schema"] = descriptor.output_schema.clone();
        base["notes"] = json!(descriptor.notes);
    }
    base
}

fn input_required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("input.{} is required", key))
}

fn input_bool_or(input: &Value, key: &str, default: bool) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn workflow_path(input: &Value) -> Result<PathBuf> {
    Ok(PathBuf::from(input_required_str(input, "workflow_path")?))
}

fn read_workflow_xml(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow '{}'", path.display()))
}

fn write_workflow_xml(path: &Path, xml: &str) -> Result<()> {
    fs::write(path, xml).with_context(|| format!("failed to write workflow '{}'", path.display()))
}

fn node_span(xml: &str, tool_id: &str) -> Result<Range<usize>> {
    let doc = Document::parse(xml).context("failed to parse workflow xml")?;
    doc.descendants()
        .find(|node| node.has_tag_name("Node") && node.attribute("ToolID") == Some(tool_id))
        .map(|node| node.range())
        .ok_or_else(|| anyhow!("tool '{}' not found", tool_id))
}

fn connection_spans_for_tool(xml: &str, tool_id: &str) -> Result<Vec<Range<usize>>> {
    let doc = Document::parse(xml).context("failed to parse workflow xml")?;
    let mut spans = Vec::new();
    for node in doc
        .descendants()
        .filter(|node| node.has_tag_name("Connection"))
    {
        if node
            .descendants()
            .any(|child| child.attribute("ToolID") == Some(tool_id))
        {
            spans.push(node.range());
        }
    }
    Ok(spans)
}

fn remove_ranges(mut xml: String, ranges: &[Range<usize>]) -> String {
    let mut ordered = ranges.to_vec();
    ordered.sort_by_key(|range| range.start);
    for range in ordered.into_iter().rev() {
        xml.replace_range(range, "");
    }
    xml
}

fn insert_before_closing(xml: &str, closing_tag: &str, addition: &str) -> Result<String> {
    let Some(index) = xml.find(closing_tag) else {
        bail!("workflow xml missing '{}'", closing_tag);
    };
    let mut updated = String::with_capacity(xml.len() + addition.len() + 1);
    updated.push_str(&xml[..index]);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(addition);
    updated.push('\n');
    updated.push_str(&xml[index..]);
    Ok(updated)
}

fn parse_tools_and_connections(xml: &str, selected_tool_ids: &[String]) -> Result<Value> {
    let doc = Document::parse(xml).context("failed to parse workflow xml")?;
    let mut tools = Vec::new();
    let mut selected = Vec::new();
    let mut plugins = Vec::<String>::new();
    for node in doc.descendants().filter(|node| node.has_tag_name("Node")) {
        let tool = json!({
            "tool_id": node.attribute("ToolID"),
            "plugin": node
                .descendants()
                .find(|child| child.has_tag_name("GuiSettings"))
                .and_then(|child| child.attribute("Plugin")),
            "engine_dll": node
                .descendants()
                .find(|child| child.has_tag_name("EngineSettings"))
                .and_then(|child| child.attribute("EngineDll")),
            "xml": &xml[node.range()],
        });
        if let Some(plugin) = tool.get("plugin").and_then(Value::as_str) {
            plugins.push(plugin.to_string());
        }
        if let Some(tool_id) = tool.get("tool_id").and_then(Value::as_str) {
            if selected_tool_ids
                .iter()
                .any(|selected_id| selected_id == tool_id)
            {
                selected.push(tool.clone());
            }
        }
        tools.push(tool);
    }

    let connections: Vec<Value> = doc
        .descendants()
        .filter(|node| node.has_tag_name("Connection"))
        .map(|node| {
            let origin = node
                .descendants()
                .find(|child| child.has_tag_name("Origin"));
            let destination = node
                .descendants()
                .find(|child| child.has_tag_name("Destination"));
            json!({
                "xml": &xml[node.range()],
                "origin_tool_id": origin.and_then(|child| child.attribute("ToolID")),
                "origin_anchor": origin.and_then(|child| child.attribute("Connection")),
                "destination_tool_id": destination.and_then(|child| child.attribute("ToolID")),
                "destination_anchor": destination.and_then(|child| child.attribute("Connection")),
            })
        })
        .collect();

    plugins.sort();
    plugins.dedup();

    Ok(json!({
        "workflow": {
            "tool_count": tools.len(),
            "connection_count": connections.len(),
            "plugin_ids": plugins,
        },
        "tools": tools,
        "selected_tools": selected,
        "connections": connections,
    }))
}

fn execute_designer_workflow_context(input: &Value, _dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    let xml = read_workflow_xml(&workflow_path)?;
    let selected_tool_ids: Vec<String> = input
        .get("selected_tool_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect();
    let parsed = parse_tools_and_connections(&xml, &selected_tool_ids)?;
    Ok(json!({
        "workflow_path": workflow_path.display().to_string(),
        "inspection": inspect_workflow(&workflow_path)?,
        "xml": xml,
        "workflow": parsed["workflow"].clone(),
        "tools": parsed["tools"].clone(),
        "selected_tools": parsed["selected_tools"].clone(),
        "connections": parsed["connections"].clone(),
    }))
}

fn execute_designer_workflow_run(input: &Value, dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    if dry_run {
        return Ok(json!({
        "workflow_path": workflow_path.display().to_string(),
        "run_mode": "offline-validation",
                "would_execute": "validate workflow xml and gather context",
                "transport_target": "eel.dll / Nexus localhost WebSocket adapter",
            }));
    }
    Ok(json!({
        "workflow_path": workflow_path.display().to_string(),
        "run_mode": "offline-validation",
        "validation": validate_workflow(&workflow_path)?,
        "context": execute_designer_workflow_context(input, false)?,
        "run_label": input.get("run_label").cloned().unwrap_or(Value::Null),
    }))
}

fn execute_designer_tool_add(input: &Value, dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    let xml = read_workflow_xml(&workflow_path)?;
    let node_xml = input_required_str(input, "node_xml")?;
    let updated = insert_before_closing(&xml, "</Nodes>", node_xml)?;
    complete_mutation_result(
        &workflow_path,
        updated,
        dry_run,
        input_bool_or(input, "write_back", true),
    )
}

fn execute_designer_tool_edit(input: &Value, dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    let xml = read_workflow_xml(&workflow_path)?;
    let tool_id = input_required_str(input, "tool_id")?;
    let replacement = input_required_str(input, "node_xml")?;
    let span = node_span(&xml, tool_id)?;
    let mut updated = xml.clone();
    updated.replace_range(span, replacement);
    let mut result = complete_mutation_result(
        &workflow_path,
        updated,
        dry_run,
        input_bool_or(input, "write_back", true),
    )?;
    result["tool_id"] = json!(tool_id);
    Ok(result)
}

fn execute_designer_tool_remove(input: &Value, dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    let xml = read_workflow_xml(&workflow_path)?;
    let tool_id = input_required_str(input, "tool_id")?;
    let mut ranges = vec![node_span(&xml, tool_id)?];
    ranges.extend(connection_spans_for_tool(&xml, tool_id)?);
    let updated = remove_ranges(xml, &ranges);
    let mut result = complete_mutation_result(
        &workflow_path,
        updated,
        dry_run,
        input_bool_or(input, "write_back", true),
    )?;
    result["tool_id"] = json!(tool_id);
    Ok(result)
}

fn execute_designer_tool_replace_connections(input: &Value, dry_run: bool) -> Result<Value> {
    let workflow_path = workflow_path(input)?;
    let mut xml = read_workflow_xml(&workflow_path)?;
    let replacements = input
        .get("replacements")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("input.replacements is required"))?;

    let mut replacement_count = 0usize;
    for replacement in replacements {
        let find = replacement
            .get("find")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("each replacement.find is required"))?;
        let replace = replacement
            .get("replace")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("each replacement.replace is required"))?;
        let matches = xml.matches(find).count();
        if matches > 0 {
            replacement_count += matches;
            xml = xml.replace(find, replace);
        }
    }

    let mut result = complete_mutation_result(
        &workflow_path,
        xml,
        dry_run,
        input_bool_or(input, "write_back", true),
    )?;
    result["replacement_count"] = json!(replacement_count);
    Ok(result)
}

fn execute_cloud_capability_stub(input: &Value, dry_run: bool) -> Result<Value> {
    if dry_run {
        return Ok(json!({
            "mode": "dry-run",
            "message": "remote capability invocation is not performed during dry-run",
            "input": input,
        }));
    }
    bail!("remote capability execution is not yet implemented")
}

fn complete_mutation_result(
    workflow_path: &Path,
    updated_xml: String,
    dry_run: bool,
    write_back: bool,
) -> Result<Value> {
    let should_write = write_back && !dry_run;
    if should_write {
        write_workflow_xml(workflow_path, &updated_xml)?;
    }
    let parsed = parse_tools_and_connections(&updated_xml, &[])?;
    Ok(json!({
        "workflow_path": workflow_path.display().to_string(),
        "applied": should_write,
        "dry_run": dry_run,
        "tool_count": parsed["workflow"]["tool_count"].clone(),
        "connection_count": parsed["workflow"]["connection_count"].clone(),
        "updated_xml": if should_write { Value::Null } else { json!(updated_xml) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("ayx-capability-{stamp}-{name}.yxmd"))
    }

    fn sample_workflow() -> String {
        r#"<AlteryxDocument yxmdVer="2025.1"><Nodes><Node ToolID="1"><GuiSettings Plugin="AlteryxBasePluginsGui.TextInput.TextInput"/></Node><Node ToolID="2"><GuiSettings Plugin="AlteryxBasePluginsGui.BrowseV2.BrowseV2"/></Node></Nodes><Connections><Connection><Origin ToolID="1" Connection="Output"/><Destination ToolID="2" Connection="Input"/></Connection></Connections></AlteryxDocument>"#.to_string()
    }

    #[test]
    fn designer_ipc_adapter_correlates_responses_and_buffers_events() {
        let mut adapter = DesignerIpcAdapter::new();
        let outbound = adapter.outbound("designer.workflow.context", "corr-1", json!({"ok": true}));
        assert_eq!(outbound.correlation_id.as_deref(), Some("corr-1"));

        adapter.receive(DesignerMessageEnvelope {
            version: "nexus.v1".to_string(),
            message_type: "event".to_string(),
            correlation_id: None,
            capability_id: Some("designer.workflow.context".to_string()),
            payload: json!({"event": "selectionChanged"}),
        });
        assert_eq!(adapter.buffered_events().len(), 1);

        adapter.receive(DesignerMessageEnvelope {
            version: "nexus.v1".to_string(),
            message_type: "response".to_string(),
            correlation_id: Some("corr-1".to_string()),
            capability_id: Some("designer.workflow.context".to_string()),
            payload: json!({"ok": true}),
        });
        let response = adapter.take_response("corr-1").expect("response");
        assert_eq!(response.message_type, "response");
    }

    #[test]
    fn cloud_discovery_maps_supported_capabilities() {
        let adapter = CloudCapabilityAdapter::discover(&json!({
            "capabilities": [
                { "id": "cloud.docs.search", "available": true },
                { "id": "cloud.workflow.summarize", "available": false }
            ]
        }));
        assert!(adapter.supports("cloud.docs.search"));
        assert!(!adapter.supports("cloud.workflow.summarize"));
        assert!(!adapter.supports("designer.workflow.context"));
    }

    #[test]
    fn capability_list_filters_by_tag() {
        let listed = list_capabilities(Some("designer"), false).expect("list");
        assert!(listed.iter().all(|item| {
            item["tags"]
                .as_array()
                .expect("tags")
                .iter()
                .filter_map(Value::as_str)
                .any(|tag| tag == "designer")
        }));
        assert!(listed
            .iter()
            .any(|item| item["id"] == "designer.workflow.context"));
    }

    #[test]
    fn workflow_context_collects_tools_and_connections() {
        let path = temp_path("context");
        fs::write(&path, sample_workflow()).expect("write sample");
        let result = execute_designer_workflow_context(
            &json!({
                "workflow_path": path.display().to_string(),
                "selected_tool_ids": ["2"]
            }),
            false,
        )
        .expect("context");
        assert_eq!(result["workflow"]["tool_count"], 2);
        assert_eq!(
            result["connections"].as_array().expect("connections").len(),
            1
        );
        assert_eq!(
            result["selected_tools"].as_array().expect("selected").len(),
            1
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tool_mutations_support_dry_run_without_write_back() {
        let path = temp_path("mutate");
        let original = sample_workflow();
        fs::write(&path, &original).expect("write sample");
        let result = execute_designer_tool_add(
            &json!({
                "workflow_path": path.display().to_string(),
                "node_xml": r#"<Node ToolID="3"><GuiSettings Plugin="AlteryxBasePluginsGui.Select.Select"/></Node>"#,
                "write_back": true
            }),
            true,
        )
        .expect("add");
        assert_eq!(result["applied"], false);
        assert!(result["updated_xml"]
            .as_str()
            .expect("updated xml")
            .contains(r#"ToolID="3""#));
        let persisted = fs::read_to_string(&path).expect("read after dry-run");
        assert_eq!(persisted, original);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tool_remove_cleans_related_connections() {
        let path = temp_path("remove");
        fs::write(&path, sample_workflow()).expect("write sample");
        let result = execute_designer_tool_remove(
            &json!({
                "workflow_path": path.display().to_string(),
                "tool_id": "2",
                "write_back": false
            }),
            false,
        )
        .expect("remove");
        let xml = result["updated_xml"].as_str().expect("xml");
        assert!(!xml.contains(r#"ToolID="2""#));
        assert!(!xml.contains("<Connection>"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn cloud_run_is_gated_when_capability_missing() {
        let error = run(
            "cloud.workflow.summarize",
            &json!({"workflow_id": "abc"}),
            false,
        )
        .expect_err("cloud capability should be gated");
        assert!(error
            .to_string()
            .contains("is not available in the current environment"));
    }
}
