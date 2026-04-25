use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use roxmltree::{Document, Node};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
pub struct CloudConversionOptions {
    pub fail_on_unsupported: bool,
}

impl Default for CloudConversionOptions {
    fn default() -> Self {
        Self {
            fail_on_unsupported: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudConversionWarning {
    pub tool_id: String,
    pub plugin: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudConversionReport {
    pub input: String,
    pub content: Value,
    pub content_checksum: String,
    pub warnings: Vec<CloudConversionWarning>,
    pub unsupported_tools: Vec<CloudConversionWarning>,
    pub removed_tools: Vec<String>,
    pub converted_tool_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldSchema {
    name: String,
    record_type: String,
    trifacta_type: String,
}

impl FieldSchema {
    fn record_info(&self) -> Value {
        json!({
            "@name": self.name,
            "@type": self.record_type,
            "@trifactaType": self.trifacta_type,
        })
    }
}

#[derive(Debug, Clone)]
struct NodeSpec {
    tool_id: String,
    plugin: String,
    xml: String,
}

#[derive(Debug, Clone)]
struct ConnectionSpec {
    origin_tool_id: String,
    destination_tool_id: String,
    destination_connection: String,
}

const SUPPORTED_PLUGINS: &[&str] = &[
    "DateTimeNow",
    "AlteryxBasePluginsGui.DbFileInput.DbFileInput",
    "AlteryxBasePluginsGui.DbFileOutput.DbFileOutput",
    "AlteryxBasePluginsGui.TextInput.TextInput",
    "AlteryxBasePluginsGui.AutoField.AutoField",
    "AlteryxBasePluginsGui.GenerateRows.GenerateRows",
    "Create_Samples.yxmc",
    "AlteryxBasePluginsGui.DataCleansePro.DataCleansePro",
    "Cleanse.yxmc",
    "AlteryxBasePluginsGui.Filter.Filter",
    "AlteryxBasePluginsGui.FuzzyMatch.FuzzyMatch",
    "Imputation_v3.yxmc",
    "MultiFieldBinning_v2.yxmc",
    "AlteryxBasePluginsGui.Formula.Formula",
    "AlteryxBasePluginsGui.MultiFieldFormula.MultiFieldFormula",
    "AlteryxBasePluginsGui.MultiRowFormula.MultiRowFormula",
    "RandomRecords.yxmc",
    "Oversample_Field.yxmc",
    "AlteryxBasePluginsGui.RecordID.RecordID",
    "AlteryxBasePluginsGui.Sample.Sample",
    "SelectRecords.yxmc",
    "AlteryxBasePluginsGui.SelectRecords.SelectRecords",
    "CountRecords.yxmc",
    "AlteryxBasePluginsGui.Sort.Sort",
    "AlteryxBasePluginsGui.Tile.Tile",
    "AlteryxBasePluginsGui.Unique.Unique",
    "AlteryxBasePluginsGui.AppendFields.AppendFields",
    "AlteryxBasePluginsGui.FindReplace.FindReplace",
    "AlteryxBasePluginsGui.Join.Join",
    "AlteryxBasePluginsGui.MakeGroup.MakeGroup",
    "AlteryxBasePluginsGui.JoinMultiple.JoinMultiple",
    "AlteryxBasePluginsGui.RegEx.RegEx",
    "AlteryxBasePluginsGui.DateTime.DateTime",
    "AlteryxBasePluginsGui.TextToColumns.TextToColumns",
    "AlteryxBasePluginsGui.XMLParse.XMLParse",
    "AlteryxBasePluginsGui.Arrange.Arrange",
    "AlteryxBasePluginsGui.CrossTab.CrossTab",
    "AlteryxBasePluginsGui.MakeColumns.MakeColumns",
    "AlteryxBasePluginsGui.RunningTotal.RunningTotal",
    "AlteryxBasePluginsGui.Transpose.Transpose",
    "WeightedAvg.yxmc",
    "AlteryxBasePluginsGui.FieldInfo.FieldInfo",
    "AlteryxBasePluginsGui.DynamicRename.DynamicRename",
    "AlteryxBasePluginsGui.DynamicReplace.DynamicReplace",
    "AlteryxBasePluginsGui.DynamicSelect.DynamicSelect",
    "AlteryxBasePluginsGui.JSONBuild.JSONBuild",
    "AlteryxBasePluginsGui.JSONParse.JSONParse",
    "AlteryxBasePluginsGui.Rank.Rank",
    "AlteryxGuiToolkit.TextBox.TextBox",
    "AlteryxGuiToolkit.ToolContainer.ToolContainer",
    "AlteryxBasePluginsGui.AlteryxSelect.AlteryxSelect",
    "AlteryxSpatialPluginsGui.Summarize.Summarize",
    "AlteryxBasePluginsGui.Union.Union",
];

const REMOVABLE_PLUGINS: &[&str] = &["AlteryxBasePluginsGui.BrowseV2.BrowseV2"];

const CLOUD_PLUGIN_REWRITES: &[(&str, &str)] = &[
    (
        "SelectRecords.yxmc",
        "AlteryxBasePluginsGui.SelectRecords.SelectRecords",
    ),
    (
        "AlteryxBasePluginsGui.DbFileInput.DbFileInput",
        "AlteryxBasePluginsGui.UniversalInput.UniversalInput",
    ),
    (
        "AlteryxBasePluginsGui.DbFileOutput.DbFileOutput",
        "AlteryxBasePluginsGui.UniversalOutput.UniversalOutput",
    ),
];

const MACRO_PATH_TO_PLUGIN_ID: &[(&str, &str)] = &[
    ("DateTimeNow/Supporting_Macros/DTNEngine.yxmc", "DateTimeNow"),
    ("Create_Samples.yxmc", "Create_Samples.yxmc"),
    ("Cleanse.yxmc", "Cleanse.yxmc"),
    ("Imputation_v3.yxmc", "Imputation_v3.yxmc"),
    ("MultiFieldBinning_v2.yxmc", "MultiFieldBinning_v2.yxmc"),
    ("RandomRecords.yxmc", "RandomRecords.yxmc"),
    ("Oversample_Field.yxmc", "Oversample_Field.yxmc"),
    ("SelectRecords.yxmc", "SelectRecords.yxmc"),
    ("CountRecords.yxmc", "CountRecords.yxmc"),
    ("WeightedAvg.yxmc", "WeightedAvg.yxmc"),
];

const CONNECTION_REWRITES: &[((&str, &str, &str), &str)] = &[
    (("DateTimeNow", "origin", "DTN"), "Output"),
    (("Cleanse.yxmc", "origin", "Output26"), "Output"),
    (("Cleanse.yxmc", "destination", "Input2"), "Input"),
    (("CountRecords.yxmc", "origin", "Output9"), "Output"),
    (("CountRecords.yxmc", "destination", "Input8"), "Input"),
];

const TYPE_MAP: &[(&str, (&str, &str))] = &[
    ("Bool", ("Bool", "Bool")),
    ("Byte", ("Int64", "Integer")),
    ("Int16", ("Int64", "Integer")),
    ("Int32", ("Int64", "Integer")),
    ("Int64", ("Int64", "Integer")),
    ("FixedDecimal", ("Double", "Float")),
    ("Float", ("Double", "Float")),
    ("Double", ("Double", "Float")),
    ("String", ("V_WString", "String")),
    ("WString", ("V_WString", "String")),
    ("V_String", ("V_WString", "String")),
    ("V_WString", ("V_WString", "String")),
    ("Date", ("Date", "Datetime")),
    ("Time", ("DateTime", "Datetime")),
    ("DateTime", ("DateTime", "Datetime")),
];

const NUMERIC_ACTIONS: &[(&str, (&str, &str))] = &[
    ("Count", ("Int64", "Integer")),
    ("CountDistinct", ("Int64", "Integer")),
    ("Sum", ("Double", "Float")),
    ("Average", ("Double", "Float")),
    ("Mean", ("Double", "Float")),
    ("Min", ("Double", "Float")),
    ("Max", ("Double", "Float")),
    ("Median", ("Double", "Float")),
    ("Percentile", ("Double", "Float")),
    ("StandardDeviation", ("Double", "Float")),
    ("Variance", ("Double", "Float")),
];

fn lookup_map<'a>(map: &'a [(&str, &'a str)], key: &str) -> Option<&'a str> {
    map.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn normalize_type(desktop_type: Option<&str>) -> (String, String) {
    TYPE_MAP
        .iter()
        .find(|(name, _)| Some(*name) == desktop_type)
        .map(|(_, (record, logical))| (record.to_string(), logical.to_string()))
        .unwrap_or_else(|| ("V_WString".to_string(), "String".to_string()))
}

fn normalize_path(value: Option<&str>) -> String {
    value.unwrap_or("").replace('\\', "/")
}

fn child_text(node: Node<'_, '_>, tag: &str, default: &str) -> String {
    node.children()
        .find(|child| child.has_tag_name(tag))
        .and_then(|child| child.text())
        .unwrap_or(default)
        .to_string()
}

fn get_plugin(node: Node<'_, '_>) -> String {
    node.children()
        .find(|child| child.has_tag_name("GuiSettings"))
        .and_then(|gui| gui.attribute("Plugin"))
        .filter(|plugin| !plugin.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| infer_macro_plugin(node))
}

fn infer_macro_plugin(node: Node<'_, '_>) -> String {
    let Some(engine) = node.children().find(|child| child.has_tag_name("EngineSettings")) else {
        return String::new();
    };
    let macro_path = normalize_path(
        engine
            .attribute("Macro")
            .or_else(|| engine.attribute("EngineDllEntryPoint")),
    );
    if macro_path.is_empty() {
        return String::new();
    }
    if let Some(plugin) = lookup_map(MACRO_PATH_TO_PLUGIN_ID, &macro_path) {
        return plugin.to_string();
    }
    let basename = macro_path.rsplit('/').next().unwrap_or("");
    lookup_map(MACRO_PATH_TO_PLUGIN_ID, basename)
        .unwrap_or("")
        .to_string()
}

fn cloud_plugin(source_plugin: &str) -> String {
    lookup_map(CLOUD_PLUGIN_REWRITES, source_plugin)
        .unwrap_or(source_plugin)
        .to_string()
}

fn flatten_nodes(root: Node<'_, '_>, source: &str) -> Vec<NodeSpec> {
    fn visit(node: Node<'_, '_>, source: &str, out: &mut Vec<NodeSpec>) {
        let plugin = get_plugin(node);
        let range = node.range();
        out.push(NodeSpec {
            tool_id: node.attribute("ToolID").unwrap_or("").to_string(),
            plugin,
            xml: source[range.start..range.end].to_string(),
        });
        if let Some(child_nodes) = node.children().find(|child| child.has_tag_name("ChildNodes")) {
            for child in child_nodes.children().filter(|child| child.has_tag_name("Node")) {
                visit(child, source, out);
            }
        }
    }

    let mut out = Vec::new();
    for node in root
        .children()
        .filter(|child| child.has_tag_name("Nodes"))
        .flat_map(|nodes| nodes.children().filter(|child| child.has_tag_name("Node")))
    {
        visit(node, source, &mut out);
    }
    out
}

fn parse_connections(root: Node<'_, '_>) -> Vec<ConnectionSpec> {
    let mut out = Vec::new();
    for connection in root
        .children()
        .filter(|child| child.has_tag_name("Connections"))
        .flat_map(|connections| connections.children().filter(|child| child.has_tag_name("Connection")))
    {
        let Some(origin) = connection.children().find(|child| child.has_tag_name("Origin")) else {
            continue;
        };
        let Some(destination) = connection.children().find(|child| child.has_tag_name("Destination")) else {
            continue;
        };
        out.push(ConnectionSpec {
            origin_tool_id: origin.attribute("ToolID").unwrap_or("").to_string(),
            destination_tool_id: destination.attribute("ToolID").unwrap_or("").to_string(),
            destination_connection: destination.attribute("Connection").unwrap_or("").to_string(),
        });
    }
    out
}

fn generic_convert(node: Node<'_, '_>) -> Value {
    let children: Vec<_> = node.children().filter(|child| child.is_element()).collect();
    let text = node.text().unwrap_or("").trim();
    if children.is_empty() && node.attributes().count() == 0 {
        return Value::String(text.to_string());
    }
    let mut out = Map::new();
    for attr in node.attributes() {
        out.insert(format!("@{}", attr.name()), Value::String(attr.value().to_string()));
    }
    if children.is_empty() {
        if !text.is_empty() {
            out.insert("#text".to_string(), Value::String(text.to_string()));
        }
        return Value::Object(out);
    }
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for child in children {
        grouped
            .entry(child.tag_name().name().to_string())
            .or_default()
            .push(generic_convert(child));
    }
    for (tag, values) in grouped {
        out.insert(tag, if values.len() == 1 { values.into_iter().next().unwrap() } else { Value::Array(values) });
    }
    if !text.is_empty() {
        out.insert("#text".to_string(), Value::String(text.to_string()));
    }
    Value::Object(out)
}

fn infer_scalar_type(value: &str) -> (String, String) {
    if value.is_empty() {
        return ("V_WString".to_string(), "String".to_string());
    }
    let lower = value.to_ascii_lowercase();
    if lower == "true" || lower == "false" {
        return ("Bool".to_string(), "Bool".to_string());
    }
    if value.chars().all(|c| c == '+' || c == '-' || c.is_ascii_digit()) {
        return ("Int64".to_string(), "Integer".to_string());
    }
    if value.contains('.') && value.chars().all(|c| c == '+' || c == '-' || c == '.' || c.is_ascii_digit()) {
        return ("Double".to_string(), "Float".to_string());
    }
    if value.len() == 10 && value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-') {
        return ("Date".to_string(), "Datetime".to_string());
    }
    if value.len() >= 16 && value.as_bytes().get(4) == Some(&b'-') {
        return ("DateTime".to_string(), "Datetime".to_string());
    }
    ("V_WString".to_string(), "String".to_string())
}

fn merge_types(types: &[(String, String)]) -> (String, String) {
    if types.is_empty() {
        return ("V_WString".to_string(), "String".to_string());
    }
    if types.iter().all(|(record, _)| record == &types[0].0) {
        return types[0].clone();
    }
    let set: BTreeSet<_> = types.iter().map(|(record, _)| record.as_str()).collect();
    if set.iter().all(|record| *record == "Int64" || *record == "Double") {
        return ("Double".to_string(), "Float".to_string());
    }
    ("V_WString".to_string(), "String".to_string())
}

fn schema_from_meta_info(node: Node<'_, '_>) -> Option<Vec<FieldSchema>> {
    let meta = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("MetaInfo")))
        .and_then(|meta| meta.children().find(|child| child.has_tag_name("RecordInfo")))?;
    let mut fields = Vec::new();
    for field in meta.children().filter(|child| child.has_tag_name("Field")) {
        let (record_type, trifacta_type) = normalize_type(field.attribute("type"));
        fields.push(FieldSchema {
            name: field.attribute("name").unwrap_or("").to_string(),
            record_type,
            trifacta_type,
        });
    }
    Some(fields)
}

fn schema_text_input(node: Node<'_, '_>) -> Vec<FieldSchema> {
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return Vec::new();
    };
    let fields: Vec<_> = configuration
        .descendants()
        .filter(|n| n.has_tag_name("Field") && n.parent().map(|p| p.has_tag_name("Fields")).unwrap_or(false))
        .collect();
    let rows: Vec<Vec<String>> = configuration
        .descendants()
        .filter(|n| n.has_tag_name("r"))
        .map(|row| row.children().filter(|c| c.has_tag_name("c")).map(|c| c.text().unwrap_or("").to_string()).collect())
        .collect();
    fields
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let name = field.attribute("name").unwrap_or(&format!("Field_{}", idx + 1)).to_string();
            let inferred = merge_types(
                &rows
                    .iter()
                    .filter_map(|row| row.get(idx))
                    .map(|value| infer_scalar_type(value))
                    .collect::<Vec<_>>(),
            );
            FieldSchema {
                name,
                record_type: inferred.0,
                trifacta_type: inferred.1,
            }
        })
        .collect()
}

fn schema_passthrough(input: &[FieldSchema]) -> Vec<FieldSchema> {
    input.to_vec()
}

fn schema_select(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return schema_passthrough(input);
    };
    let field_map: HashMap<_, _> = input.iter().map(|field| (field.name.clone(), field)).collect();
    let mut output = Vec::new();
    let mut include_unknown = false;
    let mut explicit = BTreeSet::new();
    for select in configuration.descendants().filter(|n| n.has_tag_name("SelectField")) {
        let field_name = select.attribute("field").unwrap_or("");
        if field_name == "*Unknown" {
            include_unknown = !select
                .attribute("selected")
                .unwrap_or("True")
                .eq_ignore_ascii_case("false");
            continue;
        }
        explicit.insert(field_name.to_string());
        if select.attribute("selected").unwrap_or("True").eq_ignore_ascii_case("false") {
            continue;
        }
        let output_name = select.attribute("rename").unwrap_or(field_name).to_string();
        if let Some(source) = field_map.get(field_name) {
            let mut cloned = (*source).clone();
            cloned.name = output_name;
            if let Some(type_name) = select.attribute("type") {
                let (record_type, trifacta_type) = normalize_type(Some(type_name));
                cloned.record_type = record_type;
                cloned.trifacta_type = trifacta_type;
            }
            output.push(cloned);
        } else {
            let (record_type, trifacta_type) = normalize_type(select.attribute("type"));
            output.push(FieldSchema {
                name: output_name,
                record_type,
                trifacta_type,
            });
        }
    }
    if include_unknown {
        for field in input {
            if !explicit.contains(&field.name) {
                output.push(field.clone());
            }
        }
    }
    output
}

fn schema_summarize(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return Vec::new();
    };
    let field_map: HashMap<_, _> = input.iter().map(|field| (field.name.clone(), field)).collect();
    let mut output = Vec::new();
    for summarize in configuration.descendants().filter(|n| n.has_tag_name("SummarizeField")) {
        let source_name = summarize.attribute("field").unwrap_or("");
        let action = summarize.attribute("action").unwrap_or("");
        let output_name = summarize
            .attribute("rename")
            .unwrap_or(if !source_name.is_empty() { source_name } else { action })
            .to_string();
        if action == "GroupBy" {
            if let Some(source) = field_map.get(source_name) {
                let mut cloned = (*source).clone();
                cloned.name = output_name;
                output.push(cloned);
            } else {
                output.push(FieldSchema {
                    name: output_name,
                    record_type: "V_WString".to_string(),
                    trifacta_type: "String".to_string(),
                });
            }
            continue;
        }
        if let Some((record_type, trifacta_type)) = NUMERIC_ACTIONS
            .iter()
            .find(|(name, _)| *name == action)
            .map(|(_, pair)| pair)
        {
            output.push(FieldSchema {
                name: output_name,
                record_type: record_type.to_string(),
                trifacta_type: trifacta_type.to_string(),
            });
            continue;
        }
        if let Some(source) = field_map.get(source_name) {
            let mut cloned = (*source).clone();
            cloned.name = output_name;
            output.push(cloned);
        } else {
            output.push(FieldSchema {
                name: output_name,
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            });
        }
    }
    output
}

fn schema_union(schemas: &[Vec<FieldSchema>]) -> Vec<FieldSchema> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for schema in schemas {
        for field in schema {
            if seen.insert(field.name.clone()) {
                output.push(field.clone());
            }
        }
    }
    output
}

fn schema_formula(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = input.to_vec();
    let mut index = HashMap::<String, usize>::new();
    for (i, field) in output.iter().enumerate() {
        index.insert(field.name.clone(), i);
    }
    for formula in node.descendants().filter(|n| n.has_tag_name("FormulaField")) {
        let name = formula.attribute("field").unwrap_or("").to_string();
        let (record_type, trifacta_type) = normalize_type(formula.attribute("type"));
        let schema = FieldSchema {
            name: name.clone(),
            record_type,
            trifacta_type,
        };
        if let Some(i) = index.get(&name).copied() {
            output[i] = schema;
        } else {
            index.insert(name, output.len());
            output.push(schema);
        }
    }
    output
}

fn schema_append(target: &[FieldSchema], source: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = target.to_vec();
    output.extend(source.iter().cloned());
    output
}

fn schema_regex(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = input.to_vec();
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return output;
    };
    let method = child_text(configuration, "Method", "");
    if method == "ParseComplex" {
        for field in configuration.descendants().filter(|n| n.has_tag_name("Field")) {
            let (record_type, trifacta_type) = normalize_type(field.attribute("type"));
            output.push(FieldSchema {
                name: field.attribute("field").unwrap_or("").to_string(),
                record_type,
                trifacta_type,
            });
        }
    } else if method == "ParseSimple" {
        let root_name = child_text(configuration, "RootName", "Parsed");
        let count = child_text(configuration, "NumFields", "1").parse::<usize>().unwrap_or(1);
        for idx in 0..count {
            output.push(FieldSchema {
                name: format!("{}{}", root_name, idx + 1),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            });
        }
    } else if method == "Match" {
        let match_field = child_text(configuration, "Match/Field", "Matched");
        output.push(FieldSchema {
            name: match_field,
            record_type: "Bool".to_string(),
            trifacta_type: "Bool".to_string(),
        });
    }
    output
}

fn schema_rank(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = input.to_vec();
    for mode in node.descendants().filter(|n| n.has_tag_name("Mode")) {
        output.push(FieldSchema {
            name: format!("{}Ranking", mode.attribute("value").unwrap_or("Rank")),
            record_type: "Int64".to_string(),
            trifacta_type: "Integer".to_string(),
        });
    }
    output
}

fn schema_generate_rows(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = input.to_vec();
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return output;
    };
    let update_field_name = child_text(configuration, "UpdateField_Name", "");
    let create_field_name = child_text(configuration, "CreateField_Name", "");
    let create_field_type = child_text(configuration, "CreateField_Type", "");
    let (record_type, trifacta_type) = normalize_type(Some(&create_field_type));
    if configuration
        .descendants()
        .find(|n| n.has_tag_name("UpdateField"))
        .and_then(|n| n.attribute("value"))
        == Some("True")
    {
        for field in &mut output {
            if field.name == update_field_name {
                field.record_type = record_type;
                field.trifacta_type = trifacta_type;
                return output;
            }
        }
    }
    if !create_field_name.is_empty() {
        output.push(FieldSchema {
            name: create_field_name,
            record_type,
            trifacta_type,
        });
    }
    output
}

fn schema_fuzzy_match(node: Node<'_, '_>) -> Vec<FieldSchema> {
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return Vec::new();
    };
    let record_id_field = child_text(configuration, "RecordIdField", "RecordID");
    let mut output = vec![
        FieldSchema {
            name: record_id_field.clone(),
            record_type: "Int64".to_string(),
            trifacta_type: "Integer".to_string(),
        },
        FieldSchema {
            name: format!("{}2", record_id_field),
            record_type: "Int64".to_string(),
            trifacta_type: "Integer".to_string(),
        },
    ];
    if configuration
        .descendants()
        .find(|n| n.has_tag_name("OutputScore"))
        .and_then(|n| n.attribute("value"))
        == Some("True")
    {
        output.push(FieldSchema {
            name: "MatchScore".to_string(),
            record_type: "Int64".to_string(),
            trifacta_type: "Integer".to_string(),
        });
    }
    output
}

fn schema_field_info(node: Node<'_, '_>) -> Vec<FieldSchema> {
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return vec![
            FieldSchema {
                name: "Name".to_string(),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            },
            FieldSchema {
                name: "Type".to_string(),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            },
        ];
    };
    let requested: Vec<_> = configuration
        .descendants()
        .filter(|n| n.has_tag_name("Field"))
        .filter_map(|n| n.text().map(str::to_string))
        .collect();
    let mut output = Vec::new();
    for name in requested {
        if name == "Name" {
            output.push(FieldSchema {
                name,
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            });
        } else if name == "Type" || name == "SimplifiedType" {
            output.push(FieldSchema {
                name: "Type".to_string(),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            });
        }
    }
    if output.is_empty() {
        vec![
            FieldSchema {
                name: "Name".to_string(),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            },
            FieldSchema {
                name: "Type".to_string(),
                record_type: "V_WString".to_string(),
                trifacta_type: "String".to_string(),
            },
        ]
    } else {
        output
    }
}

fn schema_dynamic_rename(node: Node<'_, '_>, input: &[FieldSchema]) -> Vec<FieldSchema> {
    let mut output = input.to_vec();
    let Some(configuration) = node
        .children()
        .find(|child| child.has_tag_name("Properties"))
        .and_then(|props| props.children().find(|child| child.has_tag_name("Configuration")))
    else {
        return output;
    };
    if child_text(configuration, "RenameMode", "") == "Formula"
        && (child_text(configuration, "Expression", "").contains("LowerCase([_CurrentField_])")
            || child_text(configuration, "Expression", "").contains("LowerCase([_CurrentColumn_])"))
    {
        for field in &mut output {
            field.name = field.name.to_lowercase();
        }
    }
    output
}

fn infer_node_schemas(root: Node<'_, '_>, source: &str) -> HashMap<String, Vec<FieldSchema>> {
    let nodes = flatten_nodes(root, source);
    let connections = parse_connections(root);
    let mut incoming: HashMap<String, Vec<ConnectionSpec>> = HashMap::new();
    for connection in connections {
        incoming
            .entry(connection.destination_tool_id.clone())
            .or_default()
            .push(connection);
    }
    let mut schemas = HashMap::<String, Vec<FieldSchema>>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &nodes {
            let plugin = node.plugin.as_str();
            let schema = if let Ok(doc) = Document::parse(&node.xml) {
                let dom = doc.root_element();
                match plugin {
                    "AlteryxBasePluginsGui.TextInput.TextInput" => Some(schema_text_input(dom)),
                    "AlteryxBasePluginsGui.DbFileInput.DbFileInput" => Some(Vec::new()),
                    "DateTimeNow" => schema_from_meta_info(dom).or(Some(Vec::new())),
                    "AlteryxBasePluginsGui.FuzzyMatch.FuzzyMatch" => Some(schema_fuzzy_match(dom)),
                    "AlteryxBasePluginsGui.AlteryxSelect.AlteryxSelect" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input.map(|schema| schema_select(dom, &schema))
                    }
                    "AlteryxSpatialPluginsGui.Summarize.Summarize" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input.map(|schema| schema_summarize(dom, &schema))
                    }
                    "AlteryxBasePluginsGui.Union.Union" => {
                        let inputs = incoming.get(&node.tool_id).cloned().unwrap_or_default();
                        let schemas_in: Vec<_> = inputs
                            .iter()
                            .filter_map(|conn| schemas.get(&conn.origin_tool_id).cloned())
                            .collect();
                        if !schemas_in.is_empty() && schemas_in.len() == inputs.len() {
                            Some(schema_union(&schemas_in))
                        } else {
                            None
                        }
                    }
                    "AlteryxBasePluginsGui.Formula.Formula" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input.map(|schema| schema_formula(dom, &schema))
                    }
                    "AlteryxBasePluginsGui.GenerateRows.GenerateRows" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned()
                            .unwrap_or_default();
                        Some(schema_generate_rows(dom, &input))
                    }
                    "AlteryxBasePluginsGui.AppendFields.AppendFields" => {
                        let inputs = incoming.get(&node.tool_id).cloned().unwrap_or_default();
                        let targets = inputs.iter().find(|item| item.destination_connection == "Targets");
                        let source = inputs.iter().find(|item| item.destination_connection == "Source");
                        match (
                            targets.and_then(|conn| schemas.get(&conn.origin_tool_id)),
                            source.and_then(|conn| schemas.get(&conn.origin_tool_id)),
                        ) {
                            (Some(target), Some(source)) => Some(schema_append(target, source)),
                            _ => None,
                        }
                    }
                    "AlteryxBasePluginsGui.RegEx.RegEx" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input.map(|schema| schema_regex(dom, &schema))
                    }
                    "AlteryxBasePluginsGui.Rank.Rank" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input.map(|schema| schema_rank(dom, &schema))
                    }
                    "AlteryxBasePluginsGui.MultiFieldFormula.MultiFieldFormula" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input
                    }
                    "AlteryxBasePluginsGui.MultiRowFormula.MultiRowFormula" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned();
                        input
                    }
                    "AlteryxBasePluginsGui.RecordID.RecordID" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned()
                            .unwrap_or_default();
                        let mut schema = input;
                        schema.push(FieldSchema {
                            name: child_text(dom, "Configuration", "RecordID"),
                            record_type: "Int64".to_string(),
                            trifacta_type: "Integer".to_string(),
                        });
                        Some(schema)
                    }
                    "AlteryxBasePluginsGui.Tile.Tile" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned()
                            .unwrap_or_default();
                        let mut schema = input;
                        schema.push(FieldSchema {
                            name: "Tile_Num".to_string(),
                            record_type: "Int64".to_string(),
                            trifacta_type: "Integer".to_string(),
                        });
                        Some(schema)
                    }
                    "AlteryxBasePluginsGui.FindReplace.FindReplace" => {
                        let inputs = incoming.get(&node.tool_id).cloned().unwrap_or_default();
                        let targets = inputs.iter().find(|item| item.destination_connection == "Targets");
                        let source = inputs.iter().find(|item| item.destination_connection == "Source");
                        match (
                            targets.and_then(|conn| schemas.get(&conn.origin_tool_id)),
                            source.and_then(|conn| schemas.get(&conn.origin_tool_id)),
                        ) {
                            (Some(target), Some(source)) => {
                                let mut schema = target.clone();
                                schema.extend(source.iter().cloned());
                                Some(schema)
                            }
                            _ => None,
                        }
                    }
                    "AlteryxBasePluginsGui.MakeGroup.MakeGroup" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned()
                            .unwrap_or_default();
                        let mut schema = input;
                        schema.push(FieldSchema {
                            name: "Group".to_string(),
                            record_type: "V_WString".to_string(),
                            trifacta_type: "String".to_string(),
                        });
                        Some(schema)
                    }
                    "AlteryxBasePluginsGui.DateTime.DateTime" => {
                        let input = incoming
                            .get(&node.tool_id)
                            .and_then(|items| items.first())
                            .and_then(|conn| schemas.get(&conn.origin_tool_id))
                            .cloned()
                            .unwrap_or_default();
                        let mut schema = input;
                        let output_name = child_text(dom, "OutputFieldName", "");
                        if !output_name.is_empty() {
                            schema.push(FieldSchema {
                                name: output_name,
                                record_type: "Date".to_string(),
                                trifacta_type: "Datetime".to_string(),
                            });
                        }
                        Some(schema)
                    }
                    "AlteryxBasePluginsGui.Arrange.Arrange" => incoming
                        .get(&node.tool_id)
                        .and_then(|items| items.first())
                        .and_then(|conn| schemas.get(&conn.origin_tool_id))
                        .cloned(),
                    "AlteryxBasePluginsGui.CrossTab.CrossTab"
                    | "AlteryxBasePluginsGui.DynamicReplace.DynamicReplace"
                    | "AlteryxBasePluginsGui.DynamicSelect.DynamicSelect"
                    | "AlteryxBasePluginsGui.JSONBuild.JSONBuild"
                    | "AlteryxBasePluginsGui.JSONParse.JSONParse" => schema_from_meta_info(dom),
                    "AlteryxBasePluginsGui.MakeColumns.MakeColumns" => incoming
                        .get(&node.tool_id)
                        .and_then(|items| items.first())
                        .and_then(|conn| schemas.get(&conn.origin_tool_id))
                        .cloned(),
                    "AlteryxBasePluginsGui.RunningTotal.RunningTotal" => incoming
                        .get(&node.tool_id)
                        .and_then(|items| items.first())
                        .and_then(|conn| schemas.get(&conn.origin_tool_id))
                        .cloned(),
                    "AlteryxBasePluginsGui.Transpose.Transpose" => incoming
                        .get(&node.tool_id)
                        .and_then(|items| items.first())
                        .and_then(|conn| schemas.get(&conn.origin_tool_id))
                        .cloned(),
                    "WeightedAvg.yxmc" => Some(vec![]),
                    "CountRecords.yxmc" => Some(vec![FieldSchema {
                        name: "Count".to_string(),
                        record_type: "Int64".to_string(),
                        trifacta_type: "Integer".to_string(),
                    }]),
                    "AlteryxBasePluginsGui.FieldInfo.FieldInfo" => Some(schema_field_info(dom)),
                    "AlteryxBasePluginsGui.DynamicRename.DynamicRename" => {
                        let meta = schema_from_meta_info(dom);
                        if meta.is_some() {
                            meta
                        } else {
                            let input = incoming
                                .get(&node.tool_id)
                                .and_then(|items| items.first())
                                .and_then(|conn| schemas.get(&conn.origin_tool_id))
                                .cloned();
                            input.map(|schema| schema_dynamic_rename(dom, &schema))
                        }
                    }
                    _ => incoming
                        .get(&node.tool_id)
                        .and_then(|items| items.first())
                        .and_then(|conn| schemas.get(&conn.origin_tool_id))
                        .cloned(),
                }
            } else {
                None
            };
            if let Some(schema) = schema {
                if schemas.get(&node.tool_id) != Some(&schema) {
                    schemas.insert(node.tool_id.clone(), schema);
                    changed = true;
                }
            }
        }
    }
    schemas
}

fn patch_record_info(node: &mut Value, schema: &[FieldSchema]) {
    let props = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut);
    if let Some(props) = props {
        props.insert(
            "MetaInfo".to_string(),
            json!({
                "RecordInfo": {"Field": schema.iter().map(FieldSchema::record_info).collect::<Vec<_>>()},
                "@connection": "Output",
            }),
        );
    }
}

fn set_multi_connection_meta_info(node: &mut Value, connection_schemas: Vec<(String, Vec<FieldSchema>)>) {
    if let Some(props) = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut) {
        props.insert(
            "MetaInfo".to_string(),
            Value::Array(
                connection_schemas
                    .into_iter()
                    .map(|(connection, schema)| {
                        json!({
                            "RecordInfo": {"Field": schema.iter().map(FieldSchema::record_info).collect::<Vec<_>>()},
                            "@connection": connection,
                        })
                    })
                    .collect(),
            ),
        );
    }
}

fn patch_macro_plugin_node(node: &mut Value, source_plugin: &str) {
    if let Some(gui) = node.as_object_mut().and_then(|obj| obj.get_mut("GuiSettings")).and_then(Value::as_object_mut) {
        gui.insert("Plugin".to_string(), Value::String(cloud_plugin(source_plugin)));
    }
    if let Some(engine) = node.as_object_mut().and_then(|obj| obj.get_mut("EngineSettings")).and_then(Value::as_object_mut) {
        if let Some(value) = engine.get("Macro").and_then(Value::as_str) {
            engine.insert("Macro".to_string(), Value::String(normalize_path(Some(value))));
        }
        if let Some(value) = engine.get("EngineDllEntryPoint").and_then(Value::as_str) {
            engine.insert("EngineDllEntryPoint".to_string(), Value::String(normalize_path(Some(value))));
        }
        if source_plugin == "SelectRecords.yxmc" {
            engine.remove("Macro");
            engine.insert("EngineDll".to_string(), Value::String("AlteryxBasePluginsEngine.dll".to_string()));
            engine.insert("EngineDllEntryPoint".to_string(), Value::String("AlteryxSelectRecords".to_string()));
        }
    }
}

fn patch_universal_input_node(node: &mut Value) {
    if let Some(gui) = node.as_object_mut().and_then(|obj| obj.get_mut("GuiSettings")).and_then(Value::as_object_mut) {
        gui.insert(
            "Plugin".to_string(),
            Value::String("AlteryxBasePluginsGui.UniversalInput.UniversalInput".to_string()),
        );
    }
    if let Some(props) = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut) {
        props.insert(
            "Configuration".to_string(),
            json!({
                "__page": "LIST_CONNECTIONS",
                "DatasetId": "",
                "VendorName": "",
                "ConnectionId": "",
                "SampleFileUri": "",
                "ConnectionName": "",
                "__previousPage": "LIST_CONNECTIONS",
            }),
        );
        props.insert("MetaInfo".to_string(), json!({"RecordInfo": {"Field": []}, "@connection": "Output"}));
    }
    if let Some(engine) = node.as_object_mut().and_then(|obj| obj.get_mut("EngineSettings")).and_then(Value::as_object_mut) {
        engine.remove("Macro");
        engine.insert("EngineDll".to_string(), Value::String("UniversalInputTool.dll".to_string()));
        engine.insert("EngineDllEntryPoint".to_string(), Value::String("UniversalInputTool".to_string()));
    }
}

fn patch_universal_output_node(node: &mut Value) {
    if let Some(gui) = node.as_object_mut().and_then(|obj| obj.get_mut("GuiSettings")).and_then(Value::as_object_mut) {
        gui.insert(
            "Plugin".to_string(),
            Value::String("AlteryxBasePluginsGui.UniversalOutput.UniversalOutput".to_string()),
        );
    }
    if let Some(props) = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut) {
        props.insert(
            "Configuration".to_string(),
            json!({
                "Path": "",
                "Delim": ",",
                "Format": "csv",
                "Header": false,
                "__page": "LIST_CONNECTIONS",
                "FileName": "",
                "DatasetId": "",
                "HasQuotes": false,
                "SheetName": "Sheet1",
                "TableName": "",
                "TargetType": "",
                "VendorName": "",
                "__isLoaded": false,
                "TableSchema": "",
                "ConnectionId": "",
                "PartitionName": "",
                "PartitionType": "",
                "ConnectionName": "",
                "OutputObjectId": "",
                "__previousPage": "LIST_CONNECTIONS",
                "LastBrowsedPath": "",
                "OnEveryRunAction": "",
                "SelectedProtocol": "",
                "DatasetOriginator": true,
                "IncludeMismatches": false,
                "FileWriterSettingsId": "",
                "PartitionUsingColumn": "",
                "PublicationSettingsId": "",
                "ConversionOutputSettings": {},
                "SelectedColumnInclusionInPartition": false,
            }),
        );
        props.remove("MetaInfo");
    }
    if let Some(engine) = node.as_object_mut().and_then(|obj| obj.get_mut("EngineSettings")).and_then(Value::as_object_mut) {
        engine.remove("Macro");
        engine.insert("EngineDll".to_string(), Value::String("UniversalOutputTool.dll".to_string()));
        engine.insert("EngineDllEntryPoint".to_string(), Value::String("UniversalOutputTool".to_string()));
    }
}

fn patch_cloud_defaults(content: &mut Value) {
    if let Some(obj) = content.as_object_mut() {
        obj.insert("@yxmdVer".to_string(), Value::String("2021.4".to_string()));
        let props = obj.entry("Properties".to_string()).or_insert_with(|| json!({}));
        if let Some(props) = props.as_object_mut() {
            props.entry("WorkflowMode".to_string()).or_insert_with(|| json!({"@value": "standard"}));
            props.entry("CloudDisableAutorename".to_string()).or_insert_with(|| json!({"@value": "True"}));
        }
    }
}

fn normalize_repeated_structures(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                normalize_repeated_structures(item);
            }
        }
        Value::Object(obj) => {
            for key in [
                "Node",
                "Connection",
                "Field",
                "FormulaField",
                "SelectField",
                "SummarizeField",
                "MatchField",
                "JoinInfo",
                "Mode",
                "Method",
                "r",
            ] {
                if let Some(value) = obj.get_mut(key) {
                    if value.is_object() {
                        let old = value.take();
                        *value = Value::Array(vec![old]);
                    }
                }
            }
            if let Some(rows) = obj.get_mut("r").and_then(Value::as_array_mut) {
                for row in rows {
                    if let Some(cells) = row.as_object_mut().and_then(|obj| obj.get_mut("c")) {
                        if cells.is_object() {
                            let old = cells.take();
                            *cells = Value::Array(vec![old]);
                        }
                    }
                }
            }
            let keys: Vec<_> = obj.keys().cloned().collect();
            for key in keys {
                if let Some(child) = obj.get_mut(&key) {
                    normalize_repeated_structures(child);
                }
            }
        }
        _ => {}
    }
}

fn lookup_connection_rewrite(plugin: &str, direction: &str, connection: &str) -> Option<&'static str> {
    CONNECTION_REWRITES
        .iter()
        .find(|((p, d, c), _)| *p == plugin && *d == direction && *c == connection)
        .map(|(_, replacement)| *replacement)
}

fn patch_connections(content: &mut Value, nodes: &HashMap<String, NodeSpec>) {
    let Some(connections) = content
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Connections"))
        .and_then(Value::as_object_mut)
        .and_then(|obj| obj.get_mut("Connection"))
    else {
        return;
    };
    let list = match connections {
        Value::Array(items) => items,
        Value::Object(_) => {
            let old = connections.take();
            *connections = Value::Array(vec![old]);
            connections.as_array_mut().unwrap()
        }
        _ => return,
    };
    for connection in list {
        let Some(obj) = connection.as_object_mut() else {
            continue;
        };
        if let Some(origin) = obj.get_mut("Origin").and_then(Value::as_object_mut) {
            if let Some(plugin) = origin
                .get("@ToolID")
                .and_then(Value::as_str)
                .and_then(|tool_id| nodes.get(tool_id))
                .map(|node| node.plugin.as_str())
            {
                if let Some(new_conn) = lookup_connection_rewrite(
                    plugin,
                    "origin",
                    origin.get("@Connection").and_then(Value::as_str).unwrap_or(""),
                ) {
                    origin.insert("@Connection".to_string(), Value::String(new_conn.to_string()));
                }
            }
        }
        if let Some(dest) = obj.get_mut("Destination").and_then(Value::as_object_mut) {
            if let Some(plugin) = dest
                .get("@ToolID")
                .and_then(Value::as_str)
                .and_then(|tool_id| nodes.get(tool_id))
                .map(|node| node.plugin.as_str())
            {
                if let Some(new_conn) = lookup_connection_rewrite(
                    plugin,
                    "destination",
                    dest.get("@Connection").and_then(Value::as_str).unwrap_or(""),
                ) {
                    dest.insert("@Connection".to_string(), Value::String(new_conn.to_string()));
                }
            }
        }
    }
}

fn patch_regex_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if configuration.get("Method").and_then(Value::as_str) != Some("ParseComplex") {
        return;
    }
    let fields = configuration.get("ParseComplex").and_then(|value| value.get("Field"));
    let Some(fields) = fields else {
        return;
    };
    let list = match fields {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![fields.clone()],
        _ => return,
    };
    let expression = configuration
        .get("RegExExpression")
        .and_then(|value| value.get("@value"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let groups = split_capture_groups(expression);
    if groups.len() != list.len() || groups.is_empty() {
        return;
    }
    let rewritten: Vec<_> = list
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let obj = field.as_object().cloned().unwrap_or_default();
            json!({
                "__id": idx.to_string(),
                "@field": obj.get("@field").and_then(Value::as_str).unwrap_or(""),
                "@type": normalize_type(obj.get("@type").and_then(Value::as_str)).1,
                "__expression": groups[idx],
            })
        })
        .collect();
    configuration.insert("ParseComplex".to_string(), json!({"Field": rewritten}));
}

fn split_capture_groups(expression: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in expression.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '(' {
            if depth == 0 {
                current.clear();
            }
            current.push(ch);
            depth += 1;
            continue;
        }
        if depth == 0 {
            continue;
        }
        current.push(ch);
        if ch == ')' {
            depth -= 1;
            if depth == 0 {
                groups.push(current.clone());
                current.clear();
            }
        }
    }
    groups
}

fn patch_summarize_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(fields) = configuration
        .get("SummarizeFields")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("SummarizeField"))
    else {
        return;
    };
    let list = match fields {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![fields.clone()],
        _ => return,
    };
    let rewritten: Vec<_> = list
        .into_iter()
        .enumerate()
        .map(|(idx, mut item)| {
            if let Some(obj) = item.as_object_mut() {
                obj.entry("__id".to_string()).or_insert_with(|| Value::String(format!("{}", idx + 1)));
            }
            item
        })
        .collect();
    configuration.insert("SummarizeFields".to_string(), json!({"SummarizeField": rewritten}));
}

fn patch_generate_rows_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(value) = configuration.get("CreateField_Type").and_then(Value::as_str) {
        configuration.insert("CreateField_Type".to_string(), Value::String(normalize_type(Some(value)).0));
    }
    configuration.insert("CreateField_Size".to_string(), Value::String("254".to_string()));
    configuration.entry("Row_Limit_Num".to_string()).or_insert_with(|| json!(100000000u64));
    configuration.remove("Expression");
    configuration.remove("RecordCount");
}

fn patch_filter_configuration(node: &mut Value, schema: Option<&[FieldSchema]>) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if configuration.get("Mode").and_then(Value::as_str) == Some("Simple") {
        let field_name = configuration
            .get("Simple")
            .and_then(Value::as_object)
            .and_then(|simple| simple.get("Field"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let operator = configuration
            .get("Simple")
            .and_then(Value::as_object)
            .and_then(|simple| simple.get("Operator"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let operand = configuration
            .get("Simple")
            .and_then(Value::as_object)
            .and_then(|simple| simple.get("Operands"))
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("Operand"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(simple) = configuration.get_mut("Simple").and_then(Value::as_object_mut) {
            if !simple.contains_key("FieldType") {
                if let Some(schema) = schema {
                    if let Some(field) = schema.iter().find(|item| item.name == field_name) {
                        simple.insert("FieldType".to_string(), Value::String(field.record_type.clone()));
                    }
                }
            }
        }
        if !configuration.contains_key("Expression") && !field_name.is_empty() {
            let expression = match operator.as_str() {
                "IsNotNull" => Some(format!("not IsNull([{}])", field_name)),
                "IsNull" => Some(format!("IsNull([{}])", field_name)),
                _ if !operator.is_empty() && !operand.is_empty() => {
                    Some(format!("[{}] {} {}", field_name, operator, operand))
                }
                _ => None,
            };
            if let Some(expression) = expression {
                configuration.insert("Expression".to_string(), Value::String(expression));
            }
        }
    } else if configuration.get("Mode").and_then(Value::as_str) == Some("Custom") {
        configuration.entry("Simple".to_string()).or_insert_with(|| {
            json!({
                "Field": "",
                "Operands": {"Operand": ""},
                "Operator": "=",
            })
        });
    }
}

fn patch_select_records_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(value) = configuration.get("Value").cloned() else {
        return;
    };
    let list = match value {
        Value::Array(items) => items,
        Value::Object(_) => vec![value],
        _ => return,
    };
    let Some(first) = list.first().and_then(Value::as_object) else {
        return;
    };
    configuration.insert(
        "Value".to_string(),
        json!({
            "@name": first.get("@name").and_then(Value::as_str).unwrap_or(""),
            "@text": first.get("#text").and_then(Value::as_str).unwrap_or(""),
        }),
    );
    configuration.insert("SortInfo".to_string(), json!({"@locale": "1033"}));
}

fn patch_fuzzy_match_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(fields) = configuration
        .get("MatchFields")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("MatchField"))
    else {
        return;
    };
    let list = match fields {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![fields.clone()],
        _ => return,
    };
    let rewritten: Vec<_> = list
        .into_iter()
        .enumerate()
        .map(|(idx, mut item)| {
            if let Some(obj) = item.as_object_mut() {
                obj.entry("__id".to_string()).or_insert_with(|| Value::String(format!("{}", idx + 1)));
            }
            item
        })
        .collect();
    configuration.insert("MatchFields".to_string(), json!({"MatchField": rewritten}));
}

fn patch_field_info_configuration(node: &mut Value) {
    if let Some(props) = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut) {
        props.insert("Configuration".to_string(), json!({"Fields": {"Field": ["Name", "SimplifiedType"]}}));
    }
}

fn patch_dynamic_rename_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if configuration.get("Expression").and_then(Value::as_str) == Some("LowerCase([_CurrentField_])") {
        configuration.insert("Expression".to_string(), Value::String("LowerCase([_CurrentColumn_])".to_string()));
    }
    configuration.entry("FirstRow".to_string()).or_insert_with(|| json!({"OnError": "Ignore"}));
    configuration.entry("AddPrefixSuffix".to_string()).or_insert_with(|| json!({"Text": "", "Type": "Suffix", "OnError": "Ignore"}));
    configuration.entry("NamesFromMetadata".to_string()).or_insert_with(|| json!({"OnError": "Ignore", "ChangeFields": "False"}));
    configuration.entry("RemovePrefixSuffix".to_string()).or_insert_with(|| json!({"Text": "", "Type": "Suffix", "OnError": "Ignore"}));
}

fn patch_multi_field_formula_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(expression) = configuration.get("Expression").and_then(Value::as_str) {
        configuration.insert(
            "Expression".to_string(),
            Value::String(
                expression
                    .replace("_CurrentField_", "_CurrentColumn_")
                    .replace("_CurrentFieldName_", "_CurrentColumnName_")
                    .replace("_CurrentFieldType_", "_CurrentColumnType_"),
            ),
        );
    }
}

fn patch_record_id_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(field_type) = configuration.get("FieldType").and_then(Value::as_str) {
        let (normalized_type, cloud_type) = normalize_type(Some(field_type));
        configuration.insert(
            "FieldType".to_string(),
            Value::String(if cloud_type == "Integer" {
                "Integer".to_string()
            } else {
                normalized_type
            }),
        );
    }
    configuration.entry("SortInfo".to_string()).or_insert_with(|| json!({"Field": [], "@locale": "1033"}));
    configuration.entry("GroupFields".to_string()).or_insert_with(|| json!({"Field": [], "@orderChanged": "False"}));
}

fn patch_date_time_now_configuration(node: &mut Value) {
    if let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    {
        let mut rewritten = Map::new();
        for (source, target) in [("Language", "Language"), ("OutputFormat", "Format")] {
            if let Some(value) = configuration.get(source).cloned() {
                rewritten.insert(target.to_string(), value);
            }
        }
        *configuration = rewritten;
    }
}

fn patch_dynamic_replace_configuration(node: &mut Value) {
    if let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    {
        if let Some(output_field_type) = configuration.get_mut("OutputFieldType").and_then(Value::as_object_mut) {
            output_field_type.remove("@size");
        }
    }
}

fn patch_dynamic_select_configuration(node: &mut Value) {
    if let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    {
        configuration.entry("FieldTypes".to_string()).or_insert_with(|| Value::String("Double,Bool,Int64,Date,DateTime,V_WString".to_string()));
    }
}

fn patch_json_parse_configuration(node: &mut Value) {
    if let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    {
        configuration.entry("ErrorWarn".to_string()).or_insert_with(|| Value::String("Ignore".to_string()));
    }
}

fn patch_json_build_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(group_fields) = configuration.get_mut("GroupFields").and_then(Value::as_object_mut) {
        group_fields.entry("Field".to_string()).or_insert_with(|| Value::Array(Vec::new()));
    }
    for key in ["IntValue_Field", "FloatValue_Field", "BoolValue_Field"] {
        configuration.entry(key.to_string()).or_insert_with(|| Value::String(String::new()));
    }
}

fn patch_count_records_configuration(node: &mut Value) {
    if let Some(props) = node.as_object_mut().and_then(|obj| obj.get_mut("Properties")).and_then(Value::as_object_mut) {
        props.insert("Configuration".to_string(), json!({"Value": []}));
    }
}

fn patch_arrange_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(output_fields) = configuration.get_mut("OutputFields").and_then(Value::as_object_mut) {
        if let Some(data) = output_fields.get_mut("Data").and_then(Value::as_object_mut) {
            if let Some(rows) = data.get_mut("r").and_then(Value::as_array_mut) {
                let mut rewritten_rows = Vec::new();
                for row in rows.iter() {
                    let values = row
                        .as_object()
                        .and_then(|obj| obj.get("c"))
                        .map(|value| match value {
                            Value::Array(items) => items.clone(),
                            Value::Object(_) => vec![value.clone()],
                            _ => Vec::new(),
                        })
                        .unwrap_or_default();
                    let non_empty: Vec<_> = values.into_iter().filter(|value| !value.is_null() && value != "").collect();
                    rewritten_rows.push(json!({"c": non_empty, "__id": format!("{}", rewritten_rows.len() + 1)}));
                }
                data.insert("r".to_string(), Value::Array(rewritten_rows));
            }
        }
    }
}

fn patch_cross_tab_configuration(node: &mut Value) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if let Some(methods) = configuration.get_mut("Methods").and_then(Value::as_object_mut) {
        if let Some(method) = methods.get("Method").cloned() {
            methods.insert(
                "Method".to_string(),
                match method {
                    Value::Array(items) => Value::Array(items),
                    Value::Object(_) => Value::Array(vec![method]),
                    _ => Value::Array(Vec::new()),
                },
            );
        }
        methods.entry("FieldSize".to_string()).or_insert_with(|| json!({"@value": 2048}));
        methods.entry("Separator".to_string()).or_insert_with(|| Value::String(",".to_string()));
    }
    if let Some(header) = configuration.get("HeaderField").and_then(Value::as_str) {
        configuration.insert("HeaderField".to_string(), json!({"@field": header}));
    }
    if let Some(data) = configuration.get("DataField").and_then(Value::as_str) {
        configuration.insert("DataField".to_string(), json!({"@field": data}));
    }
    configuration.entry("HeaderValues".to_string()).or_insert_with(|| json!({}));
}

fn patch_running_total_configuration(node: &mut Value, input_schema: Option<&[FieldSchema]>) {
    let Some(configuration) = node
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Properties"))
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("Configuration"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    configuration.entry("SortInfo".to_string()).or_insert_with(|| json!({"Field": [], "@locale": "1033"}));
    let schema_map: HashMap<_, _> = input_schema.unwrap_or(&[]).iter().map(|field| (field.name.clone(), field)).collect();
    for section_name in ["GroupByFields", "RunningTotalFields"] {
        if let Some(section) = configuration.get_mut(section_name).and_then(Value::as_object_mut) {
            let list = section
                .get("Field")
                .cloned()
                .map(|value| match value {
                    Value::Array(items) => items,
                    Value::Object(_) => vec![value],
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            let rewritten: Vec<_> = list
                .into_iter()
                .map(|mut item| {
                    if let Some(obj) = item.as_object_mut() {
                        let name = obj.get("@field").and_then(Value::as_str).unwrap_or("");
                        if let Some(source) = schema_map.get(name) {
                            obj.insert("__type".to_string(), Value::String(source.record_type.clone()));
                        }
                    }
                    item
                })
                .collect();
            section.insert("Field".to_string(), Value::Array(rewritten));
        }
    }
}

fn patch_universal_nodes(node: &mut Value, plugin: &str) {
    match plugin {
        "AlteryxBasePluginsGui.DbFileInput.DbFileInput" => patch_universal_input_node(node),
        "AlteryxBasePluginsGui.DbFileOutput.DbFileOutput" => patch_universal_output_node(node),
        _ => {}
    }
}

fn patch_node(node: &mut Value, spec: &NodeSpec, schemas: &HashMap<String, Vec<FieldSchema>>, incoming: &HashMap<String, Vec<ConnectionSpec>>) {
    let plugin = spec.plugin.as_str();
    let schema = schemas.get(&spec.tool_id);
    if plugin == "AlteryxBasePluginsGui.Filter.Filter" {
        if let Some(schema) = schema {
            set_multi_connection_meta_info(
                node,
                vec![
                    ("True".to_string(), schema.clone()),
                    ("False".to_string(), schema.clone()),
                ],
            );
        }
    } else if plugin == "AlteryxBasePluginsGui.Unique.Unique" {
        if let Some(schema) = schema {
            set_multi_connection_meta_info(
                node,
                vec![
                    ("Unique".to_string(), schema.clone()),
                    ("Duplicates".to_string(), schema.clone()),
                ],
            );
        }
    } else if plugin == "AlteryxBasePluginsGui.Join.Join" {
        if let Some(inputs) = incoming.get(&spec.tool_id) {
            let left = inputs.iter().find(|c| c.destination_connection == "Left");
            let right = inputs.iter().find(|c| c.destination_connection == "Right");
            let join_schema = schema.cloned().unwrap_or_default();
            let left_schema = left.and_then(|c| schemas.get(&c.origin_tool_id)).cloned().unwrap_or_default();
            let right_schema = right.and_then(|c| schemas.get(&c.origin_tool_id)).cloned().unwrap_or_default();
            set_multi_connection_meta_info(
                node,
                vec![
                    ("Left".to_string(), left_schema),
                    ("Join".to_string(), join_schema),
                    ("Right".to_string(), right_schema),
                ],
            );
        }
    } else if plugin == "Create_Samples.yxmc" {
        let input_schema = incoming
            .get(&spec.tool_id)
            .and_then(|items| items.first())
            .and_then(|conn| schemas.get(&conn.origin_tool_id))
            .cloned()
            .unwrap_or_default();
        set_multi_connection_meta_info(
            node,
            vec![
                ("Estimation".to_string(), input_schema.clone()),
                ("Validation".to_string(), input_schema.clone()),
                ("Holdout".to_string(), input_schema),
            ],
        );
    } else if plugin == "AlteryxBasePluginsGui.DynamicReplace.DynamicReplace" {
        if let Some(schema) = schema {
            set_multi_connection_meta_info(
                node,
                vec![
                    ("Output".to_string(), schema.clone()),
                    ("Counts".to_string(), Vec::new()),
                ],
            );
        }
    } else if let Some(schema) = schema {
        if !matches!(
            plugin,
            "AlteryxGuiToolkit.TextBox.TextBox"
                | "AlteryxGuiToolkit.ToolContainer.ToolContainer"
                | "AlteryxBasePluginsGui.DbFileOutput.DbFileOutput"
        ) {
            patch_record_info(node, schema);
        }
    }

    if plugin == "DateTimeNow" {
        patch_date_time_now_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.GenerateRows.GenerateRows" {
        patch_generate_rows_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.Filter.Filter" {
        patch_filter_configuration(node, schema.map(|schema| schema.as_slice()));
    }
    if plugin == "AlteryxBasePluginsGui.FuzzyMatch.FuzzyMatch" {
        patch_fuzzy_match_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.TextToColumns.TextToColumns" {
        if let Some(configuration) = node
            .as_object_mut()
            .and_then(|obj| obj.get_mut("Properties"))
            .and_then(Value::as_object_mut)
            .and_then(|props| props.get_mut("Configuration"))
            .and_then(Value::as_object_mut)
        {
            if let Some(mode) = configuration.remove("Mode") {
                let mode_value = mode.get("@value").and_then(Value::as_str).unwrap_or("");
                if mode_value == "Rows" {
                    configuration.insert("RootName".to_string(), Value::String("Column".to_string()));
                    configuration.insert("NumFields".to_string(), json!({"@value": "1"}));
                    configuration.insert("ErrorHandling".to_string(), Value::String("Last".to_string()));
                }
            }
        }
    }
    if plugin == "AlteryxBasePluginsGui.XMLParse.XMLParse" {
        if let Some(configuration) = node
            .as_object_mut()
            .and_then(|obj| obj.get_mut("Properties"))
            .and_then(Value::as_object_mut)
            .and_then(|props| props.get_mut("Configuration"))
            .and_then(Value::as_object_mut)
        {
            configuration.remove("IncludeInOutput");
        }
    }
    if plugin == "AlteryxBasePluginsGui.Arrange.Arrange" {
        patch_arrange_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.CrossTab.CrossTab" {
        patch_cross_tab_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.RunningTotal.RunningTotal" {
        let input_schema = incoming
            .get(&spec.tool_id)
            .and_then(|items| items.first())
            .and_then(|conn| schemas.get(&conn.origin_tool_id))
            .cloned();
        patch_running_total_configuration(node, input_schema.as_deref());
    }
    if plugin == "AlteryxBasePluginsGui.FieldInfo.FieldInfo" {
        patch_field_info_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.DynamicRename.DynamicRename" {
        patch_dynamic_rename_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.MultiFieldFormula.MultiFieldFormula" {
        patch_multi_field_formula_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.RecordID.RecordID" {
        patch_record_id_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.DynamicReplace.DynamicReplace" {
        patch_dynamic_replace_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.DynamicSelect.DynamicSelect" {
        patch_dynamic_select_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.JSONBuild.JSONBuild" {
        patch_json_build_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.JSONParse.JSONParse" {
        patch_json_parse_configuration(node);
    }
    if plugin == "CountRecords.yxmc" {
        patch_count_records_configuration(node);
    }
    if plugin == "SelectRecords.yxmc" || plugin == "AlteryxBasePluginsGui.SelectRecords.SelectRecords" {
        patch_select_records_configuration(node);
    }
    if plugin == "AlteryxBasePluginsGui.RegEx.RegEx" {
        patch_regex_configuration(node);
    }
    if plugin == "AlteryxSpatialPluginsGui.Summarize.Summarize" {
        patch_summarize_configuration(node);
    }
    patch_universal_nodes(node, plugin);
    if plugin == "SelectRecords.yxmc"
        || plugin == "Create_Samples.yxmc"
        || plugin == "CountRecords.yxmc"
        || plugin == "WeightedAvg.yxmc"
        || plugin == "DateTimeNow"
    {
        patch_macro_plugin_node(node, plugin);
    }
}

fn patch_nodes(content: &mut Value, specs: &HashMap<String, NodeSpec>, schemas: &HashMap<String, Vec<FieldSchema>>, incoming: &HashMap<String, Vec<ConnectionSpec>>) {
    fn walk(
        value: &mut Value,
        specs: &HashMap<String, NodeSpec>,
        schemas: &HashMap<String, Vec<FieldSchema>>,
        incoming: &HashMap<String, Vec<ConnectionSpec>>,
    ) {
        match value {
            Value::Array(items) => {
                for item in items {
                    walk(item, specs, schemas, incoming);
                }
            }
            Value::Object(obj) => {
                let tool_id = obj.get("@ToolID").and_then(Value::as_str).map(|s| s.to_string());
                let keys: Vec<_> = obj.keys().cloned().collect();
                let _ = obj;
                if let Some(tool_id) = tool_id {
                    if let Some(spec) = specs.get(&tool_id) {
                        patch_node(value, spec, schemas, incoming);
                    }
                }
                if let Some(obj) = value.as_object_mut() {
                    for key in keys {
                        if let Some(child) = obj.get_mut(&key) {
                            walk(child, specs, schemas, incoming);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    walk(content, specs, schemas, incoming);
}

fn prune_removed_nodes_and_connections(root: Node<'_, '_>, content: &mut Value, removed: &BTreeSet<String>) {
    let _ = root;
    if let Some(nodes) = content
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Nodes"))
        .and_then(Value::as_object_mut)
        .and_then(|obj| obj.get_mut("Node"))
        .and_then(Value::as_array_mut)
    {
        nodes.retain(|node| {
            let tool_id = node
                .as_object()
                .and_then(|obj| obj.get("@ToolID"))
                .and_then(Value::as_str)
                .unwrap_or("");
            !removed.contains(tool_id)
        });
    }
    if let Some(connections) = content
        .as_object_mut()
        .and_then(|obj| obj.get_mut("Connections"))
        .and_then(Value::as_object_mut)
        .and_then(|obj| obj.get_mut("Connection"))
        .and_then(Value::as_array_mut)
    {
        connections.retain(|connection| {
            let origin_id = connection
                .as_object()
                .and_then(|obj| obj.get("Origin"))
                .and_then(Value::as_object)
                .and_then(|obj| obj.get("@ToolID"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let dest_id = connection
                .as_object()
                .and_then(|obj| obj.get("Destination"))
                .and_then(Value::as_object)
                .and_then(|obj| obj.get("@ToolID"))
                .and_then(Value::as_str)
                .unwrap_or("");
            !removed.contains(origin_id) && !removed.contains(dest_id)
        });
    }
}

fn unsupported_tools(root: Node<'_, '_>, source: &str) -> Vec<CloudConversionWarning> {
    let mut warnings = Vec::new();
    for node in flatten_nodes(root, source) {
        if !SUPPORTED_PLUGINS.contains(&node.plugin.as_str()) {
            warnings.push(CloudConversionWarning {
                tool_id: node.tool_id.clone(),
                plugin: node.plugin.clone(),
                message: "unsupported tool preserved generically".to_string(),
            });
        }
    }
    warnings
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical_json).collect::<Vec<_>>().join(",")
        ),
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!("{}:{}", serde_json::to_string(key).unwrap(), canonical_json(&map[key])))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn checksum(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(value).as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn convert_desktop_to_cloud(input: &Path, options: CloudConversionOptions) -> Result<CloudConversionReport> {
    let source = fs::read_to_string(input)
        .with_context(|| format!("failed to read workflow '{}'", input.display()))?;
    let doc = Document::parse(&source).context("failed to parse workflow xml")?;
    let root = doc.root_element();
    let warnings = unsupported_tools(root, &source);
    if options.fail_on_unsupported && !warnings.is_empty() {
        let summary = warnings
            .iter()
            .map(|warning| format!("ToolID {}: {}", warning.tool_id, warning.plugin))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("unsupported tools found: {}", summary);
    }

    let nodes = flatten_nodes(root, &source);
    let node_specs: HashMap<_, _> = nodes
        .iter()
        .cloned()
        .map(|node| (node.tool_id.clone(), node))
        .collect();
    let removed_ids: BTreeSet<_> = nodes
        .iter()
        .filter(|node| REMOVABLE_PLUGINS.contains(&node.plugin.as_str()))
        .map(|node| node.tool_id.clone())
        .collect();
    let removed_tools = removed_ids.iter().cloned().collect::<Vec<_>>();
    let connections = parse_connections(root);
    let mut incoming: HashMap<String, Vec<ConnectionSpec>> = HashMap::new();
    for connection in connections {
        incoming
            .entry(connection.destination_tool_id.clone())
            .or_default()
            .push(connection);
    }

    let mut content = generic_convert(root);
    prune_removed_nodes_and_connections(root, &mut content, &removed_ids);
    let schemas = infer_node_schemas(root, &source);
    patch_nodes(&mut content, &node_specs, &schemas, &incoming);
    patch_connections(&mut content, &node_specs);
    patch_cloud_defaults(&mut content);
    normalize_repeated_structures(&mut content);
    let content_checksum = checksum(&content);
    Ok(CloudConversionReport {
        input: input.display().to_string(),
        content,
        content_checksum,
        warnings: warnings.clone(),
        unsupported_tools: warnings,
        removed_tools,
        converted_tool_count: node_specs.len(),
    })
}
