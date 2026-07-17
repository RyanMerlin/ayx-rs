//! An in-tree JSON Schema **subset** validator for action/workflow I/O
//! contracts — not a general JSON Schema implementation, and no new crate
//! dependency. `Registry`, `Action`, and `Workflow` gain optional
//! `input_schema` / `output_schema` fields (Task 2) holding YAML-authored,
//! JSON-Schema-shaped `serde_json::Value` documents; this module is the
//! grammar + instance validator those fields are checked against, both at
//! load time and at run time (Task 3).
//!
//! # Supported subset
//!
//! | Keyword / location | Supported behavior |
//! | --- | --- |
//! | Every schema node | `type` (one of `object`, `array`, `string`, `integer`, `number`, `boolean`, `null`), non-empty `description`, `const`, and `enum` |
//! | Object nodes | `properties`, `required`, `additionalProperties` (boolean only) |
//! | Array nodes | `items` and `minItems` |
//! | String nodes | `minLength` |
//!
//! Every other keyword is rejected: type unions, `$ref`, `$defs`,
//! `allOf`/`anyOf`/`oneOf`/`not`, `pattern`, format assertions, and anything
//! else outside the table above. There is no `$schema` keyword support —
//! this table (surfaced via Rustdoc and CLI help text) is the subset
//! identifier, not a claim of draft conformance. Every schema node must be a
//! JSON object and must declare `type`; there is no permissive fallback for
//! an untyped node.
//!
//! Both root schemas must be `type: object`. [`SchemaRole::Input`] adds
//! three requirements on top of the generic grammar: the root must set
//! `additionalProperties: false`, every direct property must carry a
//! non-empty `description`, and every direct property's `type` must be
//! `string` — the CLI's `--param key=value` interface is lexical-string-only
//! by design (see the plan's Contract Decisions), not a gap to widen later.
//! [`SchemaRole::Output`] permits the full supported-type table and the
//! ordinary JSON Schema permissive `additionalProperties` default.
//!
//! `validate_schema` is wired into the registry loader (`Registry::finalize`)
//! and `validate_instance` into the executor's input/output contract checks.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

/// Which side of an action/workflow I/O contract a schema describes. The
/// two roles share one grammar but diverge on how strict the root object
/// must be — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaRole {
    /// Describes the parameter object assembled from `--param` /
    /// `--param-file`. Closed contract: `additionalProperties: false`,
    /// every property a described `string`.
    Input,
    /// Describes the `ActionRun` / `WorkflowRun` value serialized into
    /// `Envelope.data` on a successful run. Open contract: any supported
    /// type, permissive `additionalProperties` default.
    Output,
}

/// One grammar or instance validation failure. `path` is a JSON-pointer-style
/// location (for example `/properties/profile/minLength` when validating a
/// schema document itself, or `/profile` when validating an instance against
/// an already-valid schema); `reason` is a concise, human-readable
/// explanation. Deliberately holds only plain strings — no parser internals
/// — so a caller can surface it directly in an executor/registry error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    pub path: String,
    pub reason: String,
}

impl SchemaViolation {
    fn new(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

/// The `type` values this subset recognizes. No unions: `type` must be one
/// string from this list, never an array.
const SUPPORTED_TYPES: &[&str] = &[
    "object", "array", "string", "integer", "number", "boolean", "null",
];

/// The complete keyword vocabulary this subset understands, regardless of
/// which type they apply to. Anything else on a schema node is an unknown
/// keyword — no permissive fallback.
const KNOWN_KEYWORDS: &[&str] = &[
    "type",
    "description",
    "const",
    "enum",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "minItems",
    "minLength",
];

// ---------------------------------------------------------------------
// Grammar validation (Step 1): is a schema document itself well-formed
// under the supported subset?
// ---------------------------------------------------------------------

/// Validate that `schema` is a well-formed document in the supported JSON
/// Schema subset for the given `role`. Returns every violation found (not
/// just the first), sorted by pointer path.
pub(crate) fn validate_schema(
    schema: &Value,
    role: SchemaRole,
) -> Result<(), Vec<SchemaViolation>> {
    let mut violations = Vec::new();
    validate_node(schema, "", true, &mut violations);

    if role == SchemaRole::Input
        && let Some(obj) = schema.as_object()
        && obj.get("type").and_then(Value::as_str) == Some("object")
    {
        validate_input_root(obj, &mut violations);
    }

    violations.sort_by(|a, b| a.path.cmp(&b.path));
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Recursively validate one schema node (and, via `properties`/`items`, its
/// descendants). `is_root` gates the "must be `type: object`" rule, which
/// only applies at the top of the document, not to every nested property.
/// Role-specific strictness (`SchemaRole::Input`'s closed-contract checks)
/// is applied separately by `validate_input_root`, only at the schema root
/// — this recursive walker is role-agnostic grammar checking.
fn validate_node(node: &Value, path: &str, is_root: bool, violations: &mut Vec<SchemaViolation>) {
    let Some(obj) = node.as_object() else {
        violations.push(SchemaViolation::new(
            path,
            "schema node must be a JSON object",
        ));
        return;
    };

    for key in obj.keys() {
        if !KNOWN_KEYWORDS.contains(&key.as_str()) {
            violations.push(SchemaViolation::new(
                format!("{path}/{key}"),
                format!("unknown keyword '{key}' is not part of the supported schema subset"),
            ));
        }
    }

    let ty: Option<&str> = match obj.get("type") {
        None => {
            violations.push(SchemaViolation::new(
                format!("{path}/type"),
                "schema node must declare 'type'",
            ));
            None
        }
        Some(Value::String(s)) if SUPPORTED_TYPES.contains(&s.as_str()) => Some(s.as_str()),
        Some(Value::String(s)) => {
            violations.push(SchemaViolation::new(
                format!("{path}/type"),
                format!("unsupported type '{s}'"),
            ));
            None
        }
        Some(_) => {
            violations.push(SchemaViolation::new(
                format!("{path}/type"),
                "'type' must be a single string; type unions are not supported",
            ));
            None
        }
    };

    if is_root {
        match ty {
            Some("object") => {}
            Some(other) => violations.push(SchemaViolation::new(
                format!("{path}/type"),
                format!("root schema must declare type: object (found '{other}')"),
            )),
            // Already reported as a missing/invalid 'type' violation above;
            // don't double-report the same root cause.
            None => {}
        }
    }

    let Some(ty) = ty else { return };

    for (key, value) in obj {
        match key.as_str() {
            "type" => {}
            "description" => match value {
                Value::String(s) if s.is_empty() => violations.push(SchemaViolation::new(
                    format!("{path}/description"),
                    "description must not be empty",
                )),
                Value::String(_) => {}
                _ => violations.push(SchemaViolation::new(
                    format!("{path}/description"),
                    "description must be a string",
                )),
            },
            "const" => {
                if !value_matches_type(value, ty) {
                    violations.push(SchemaViolation::new(
                        format!("{path}/const"),
                        format!("const value does not match declared type '{ty}'"),
                    ));
                }
            }
            "enum" => {
                match value.as_array() {
                    None => violations.push(SchemaViolation::new(
                        format!("{path}/enum"),
                        "enum must be an array",
                    )),
                    Some(items) if items.is_empty() => violations.push(SchemaViolation::new(
                        format!("{path}/enum"),
                        "enum must not be empty",
                    )),
                    Some(items) => {
                        for (i, v) in items.iter().enumerate() {
                            if !value_matches_type(v, ty) {
                                violations.push(SchemaViolation::new(
                                format!("{path}/enum/{i}"),
                                format!("enum value at index {i} does not match declared type '{ty}'"),
                            ));
                            }
                        }
                    }
                }
            }
            "properties" => {
                if ty != "object" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/properties"),
                        "'properties' is only valid on an object schema",
                    ));
                } else {
                    match value.as_object() {
                        None => violations.push(SchemaViolation::new(
                            format!("{path}/properties"),
                            "'properties' must be an object",
                        )),
                        Some(props) => {
                            for (name, sub) in props {
                                validate_node(
                                    sub,
                                    &format!("{path}/properties/{name}"),
                                    false,
                                    violations,
                                );
                            }
                        }
                    }
                }
            }
            "required" => {
                if ty != "object" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/required"),
                        "'required' is only valid on an object schema",
                    ));
                } else {
                    match value.as_array() {
                        None => violations.push(SchemaViolation::new(
                            format!("{path}/required"),
                            "'required' must be an array of strings",
                        )),
                        Some(items) => {
                            let known: BTreeSet<&str> = obj
                                .get("properties")
                                .and_then(Value::as_object)
                                .map(|p| p.keys().map(String::as_str).collect())
                                .unwrap_or_default();
                            for (i, v) in items.iter().enumerate() {
                                match v.as_str() {
                                    None => violations.push(SchemaViolation::new(
                                        format!("{path}/required/{i}"),
                                        "'required' entries must be strings",
                                    )),
                                    Some(name) => {
                                        if !known.contains(name) {
                                            violations.push(SchemaViolation::new(
                                                format!("{path}/required/{i}"),
                                                format!(
                                                    "required property '{name}' is not defined in 'properties'"
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "additionalProperties" => {
                if ty != "object" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/additionalProperties"),
                        "'additionalProperties' is only valid on an object schema",
                    ));
                } else if !value.is_boolean() {
                    violations.push(SchemaViolation::new(
                        format!("{path}/additionalProperties"),
                        "'additionalProperties' must be a boolean",
                    ));
                }
            }
            "items" => {
                if ty != "array" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/items"),
                        "'items' is only valid on an array schema",
                    ));
                } else {
                    validate_node(value, &format!("{path}/items"), false, violations);
                }
            }
            "minItems" => {
                if ty != "array" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/minItems"),
                        "'minItems' is only valid on an array schema",
                    ));
                } else if non_negative_integer(value).is_none() {
                    violations.push(SchemaViolation::new(
                        format!("{path}/minItems"),
                        "'minItems' must be a non-negative integer",
                    ));
                }
            }
            "minLength" => {
                if ty != "string" {
                    violations.push(SchemaViolation::new(
                        format!("{path}/minLength"),
                        "'minLength' is only valid on a string schema",
                    ));
                } else if non_negative_integer(value).is_none() {
                    violations.push(SchemaViolation::new(
                        format!("{path}/minLength"),
                        "'minLength' must be a non-negative integer",
                    ));
                }
            }
            // Anything else was already reported by the unknown-keyword pass
            // above; avoid a duplicate finding for the same key.
            _ => {}
        }
    }
}

/// `SchemaRole::Input`-only checks against an already-confirmed object root:
/// `additionalProperties` must be the literal `false`, and every direct
/// property must be `type: string` with a non-empty `description`. The
/// CLI's `--param key=value` interface is lexical-string-only by design —
/// this is intentional, not a gap to relax later.
fn validate_input_root(root: &Map<String, Value>, violations: &mut Vec<SchemaViolation>) {
    match root.get("additionalProperties") {
        Some(Value::Bool(false)) => {}
        Some(Value::Bool(true)) => violations.push(SchemaViolation::new(
            "/additionalProperties",
            "input schemas must set additionalProperties: false; the CLI's --param interface is a closed contract",
        )),
        // A non-boolean value here was already reported by the generic
        // grammar pass; don't double-report it.
        Some(_) => {}
        None => violations.push(SchemaViolation::new(
            "/additionalProperties",
            "input schemas must declare additionalProperties: false",
        )),
    }

    let Some(props) = root.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, prop_schema) in props {
        let prop_obj = prop_schema.as_object();
        let has_description = prop_obj
            .and_then(|o| o.get("description"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        if !has_description {
            violations.push(SchemaViolation::new(
                format!("/properties/{name}/description"),
                "input properties must have a non-empty description",
            ));
        }
        let prop_ty = prop_obj.and_then(|o| o.get("type")).and_then(Value::as_str);
        if prop_ty != Some("string") {
            violations.push(SchemaViolation::new(
                format!("/properties/{name}/type"),
                format!(
                    "input properties must be type 'string' (the CLI's --param values are lexical strings); found {}",
                    prop_ty.unwrap_or("<missing>")
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------
// Instance validation (Step 2): does a JSON value satisfy an
// already-grammar-valid schema?
// ---------------------------------------------------------------------

/// Validate `instance` against `schema`. Assumes `schema` already passed
/// [`validate_schema`] — this function does not re-check schema grammar.
/// Returns every deterministic failure (not just the first), sorted by
/// pointer path, so a caller gets one actionable payload instead of an
/// edit/run/fail loop. No coercion: a JSON string that looks like `"true"`
/// or `"42"` never satisfies a `boolean`/`integer` schema.
pub(crate) fn validate_instance(schema: &Value, instance: &Value) -> Vec<SchemaViolation> {
    let mut violations = Vec::new();
    validate_instance_node(schema, instance, "", &mut violations);
    violations.sort_by(|a, b| a.path.cmp(&b.path));
    violations
}

fn validate_instance_node(
    schema: &Value,
    instance: &Value,
    path: &str,
    violations: &mut Vec<SchemaViolation>,
) {
    let Some(sobj) = schema.as_object() else {
        // A malformed schema is a grammar error, not an instance error;
        // validate_schema is responsible for catching it. Nothing to check.
        return;
    };

    if let Some(ty) = sobj.get("type").and_then(Value::as_str)
        && !value_matches_type(instance, ty)
    {
        violations.push(SchemaViolation::new(
            path,
            format!("expected type '{ty}', found {}", json_type_name(instance)),
        ));
        // Further constraints (minLength, items, ...) presume the right
        // JSON type; skip them rather than cascade confusing errors.
        return;
    }

    if let Some(const_val) = sobj.get("const")
        && instance != const_val
    {
        violations.push(SchemaViolation::new(
            path,
            format!("value must equal const {const_val}"),
        ));
    }

    if let Some(enum_vals) = sobj.get("enum").and_then(Value::as_array)
        && !enum_vals.contains(instance)
    {
        violations.push(SchemaViolation::new(
            path,
            "value is not one of the schema's allowed enum values",
        ));
    }

    match instance {
        Value::String(s) => {
            if let Some(min_len) = sobj.get("minLength").and_then(non_negative_integer) {
                let len = s.chars().count() as u64;
                if len < min_len {
                    violations.push(SchemaViolation::new(
                        path,
                        format!("string length {len} is less than minLength {min_len}"),
                    ));
                }
            }
        }
        Value::Array(items) => {
            if let Some(min_items) = sobj.get("minItems").and_then(non_negative_integer) {
                let len = items.len() as u64;
                if len < min_items {
                    violations.push(SchemaViolation::new(
                        path,
                        format!("array has {len} item(s), fewer than minItems {min_items}"),
                    ));
                }
            }
            if let Some(item_schema) = sobj.get("items") {
                for (i, item) in items.iter().enumerate() {
                    validate_instance_node(item_schema, item, &format!("{path}/{i}"), violations);
                }
            }
        }
        Value::Object(fields) => {
            let props = sobj.get("properties").and_then(Value::as_object);

            if let Some(required) = sobj.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    if !fields.contains_key(name) {
                        violations.push(SchemaViolation::new(
                            format!("{path}/{name}"),
                            format!("missing required property '{name}'"),
                        ));
                    }
                }
            }

            // Absent optional properties are valid — only present keys are
            // walked below, so a missing non-required key never fires.
            let additional_allowed = sobj
                .get("additionalProperties")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            for (key, value) in fields {
                let child_path = format!("{path}/{key}");
                match props.and_then(|p| p.get(key)) {
                    Some(prop_schema) => {
                        validate_instance_node(prop_schema, value, &child_path, violations)
                    }
                    None if additional_allowed => {}
                    None => violations.push(SchemaViolation::new(
                        child_path,
                        format!("unexpected property '{key}' (additionalProperties: false)"),
                    )),
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Build the effective input contract for a legacy action/workflow with no
/// explicit `input_schema`: every recursively discovered `<name>`
/// placeholder becomes a required `string` property, and
/// `additionalProperties` stays permissive so existing extra `--param` keys
/// keep working exactly as they do today. Task 2 owns calling this from the
/// loader.
pub(crate) fn inferred_string_object(required: BTreeSet<String>) -> Value {
    let mut properties = Map::new();
    for name in &required {
        properties.insert(
            name.clone(),
            json!({
                "type": "string",
                "description": format!(
                    "Inferred parameter '{name}' (legacy action/workflow; no explicit input_schema declared)."
                ),
            }),
        );
    }
    json!({
        "type": "object",
        "description": "Inferred input contract: required string parameters recursively discovered from command placeholders. Extra parameters are permitted for compatibility with existing callers.",
        "properties": Value::Object(properties),
        "required": required.into_iter().collect::<Vec<_>>(),
        "additionalProperties": true,
    })
}

/// The set of property names declared under a schema node's `properties`
/// map, or empty if absent / the node isn't an object schema. Used to
/// compare a declared contract's property set against a recursively
/// computed placeholder set without re-parsing JSON by hand.
pub(crate) fn object_property_names(schema: &Value) -> BTreeSet<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

/// Does `value`'s JSON runtime type match the declared subset `type` name?
/// `integer` requires the JSON number to have been represented without a
/// fractional/exponent part (`n.is_i64() || n.is_u64()`); `5.0` is a
/// `number`, not an `integer`. No coercion of any kind — a JSON string is
/// never treated as satisfying a non-string type, however it reads.
fn value_matches_type(value: &Value, ty: &str) -> bool {
    match ty {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => matches!(value, Value::Number(n) if n.is_i64() || n.is_u64()),
        "number" => value.is_number(),
        _ => false,
    }
}

/// Extract a JSON integer if present and non-negative — used for both the
/// `minItems`/`minLength` grammar check and their instance-side comparison.
/// A float (`5.0`) or a negative integer returns `None`.
fn non_negative_integer(value: &Value) -> Option<u64> {
    if let Some(u) = value.as_u64() {
        Some(u)
    } else {
        value.as_i64().filter(|i| *i >= 0).map(|i| i as u64)
    }
}

/// Human-readable JSON runtime type name, for instance-validation messages.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation_paths(violations: &[SchemaViolation]) -> Vec<&str> {
        violations.iter().map(|v| v.path.as_str()).collect()
    }

    // -- Step 1: schema grammar -----------------------------------------

    #[test]
    fn valid_output_schema_covers_every_supported_node_kind() {
        let schema = json!({
            "type": "object",
            "description": "Root description.",
            "additionalProperties": true,
            "required": ["name", "count", "flag", "empty", "tags", "nested"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "A name.",
                    "minLength": 1,
                    "enum": ["a", "b", "c"]
                },
                "count": {
                    "type": "integer",
                    "description": "A count.",
                    "const": 5
                },
                "ratio": {
                    "type": "number",
                    "description": "An optional ratio."
                },
                "flag": {
                    "type": "boolean",
                    "description": "A flag."
                },
                "empty": {
                    "type": "null",
                    "description": "Always null.",
                    "const": null
                },
                "tags": {
                    "type": "array",
                    "description": "Tags.",
                    "minItems": 1,
                    "items": {
                        "type": "string",
                        "description": "One tag.",
                        "minLength": 1
                    }
                },
                "nested": {
                    "type": "object",
                    "description": "Nested object.",
                    "properties": {
                        "inner": {"type": "string", "description": "Inner value."}
                    },
                    "required": ["inner"],
                    "additionalProperties": false
                }
            }
        });

        assert_eq!(validate_schema(&schema, SchemaRole::Output), Ok(()));
    }

    #[test]
    fn valid_input_schema_with_only_string_properties() {
        let schema = json!({
            "type": "object",
            "description": "Parameters.",
            "additionalProperties": false,
            "required": ["profile"],
            "properties": {
                "profile": {
                    "type": "string",
                    "description": "Named profile.",
                    "minLength": 1,
                    "enum": ["prod", "staging"]
                },
                "ts": {
                    "type": "string",
                    "description": "Timestamp label.",
                    "const": "fixed"
                }
            }
        });

        assert_eq!(validate_schema(&schema, SchemaRole::Input), Ok(()));
    }

    #[test]
    fn root_must_be_object() {
        let schema = json!({"type": "string", "description": "not an object root"});
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/type"]);
    }

    #[test]
    fn unknown_keyword_is_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {},
            "pattern": "^[a-z]+$"
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        assert!(violation_paths(&err).contains(&"/pattern"), "{err:?}");
    }

    #[test]
    fn required_name_must_exist_in_properties() {
        let schema = json!({
            "type": "object",
            "properties": {},
            "required": ["missing"]
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/required/0"]);
    }

    #[test]
    fn keyword_used_on_wrong_type_is_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {
                "foo": {"type": "object", "description": "x", "minLength": 1}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/properties/foo/minLength"]);
    }

    #[test]
    fn input_property_missing_description_is_rejected() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "profile": {"type": "string"}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Input).unwrap_err();
        assert!(
            violation_paths(&err).contains(&"/properties/profile/description"),
            "{err:?}"
        );
    }

    #[test]
    fn input_additional_properties_true_is_rejected() {
        let schema = json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "profile": {"type": "string", "description": "x"}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Input).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/additionalProperties"]);
    }

    #[test]
    fn input_additional_properties_absent_is_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {
                "profile": {"type": "string", "description": "x"}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Input).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/additionalProperties"]);
    }

    #[test]
    fn non_string_input_property_is_rejected() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "count": {"type": "integer", "description": "x"}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Input).unwrap_err();
        assert!(
            violation_paths(&err).contains(&"/properties/count/type"),
            "{err:?}"
        );
    }

    #[test]
    fn negative_min_length_and_min_items_are_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "x", "minLength": -1},
                "tags": {
                    "type": "array",
                    "description": "y",
                    "minItems": -1,
                    "items": {"type": "string", "description": "z"}
                }
            }
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        let paths = violation_paths(&err);
        assert!(paths.contains(&"/properties/name/minLength"), "{paths:?}");
        assert!(paths.contains(&"/properties/tags/minItems"), "{paths:?}");
    }

    #[test]
    fn non_boolean_additional_properties_is_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {},
            "additionalProperties": "no"
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        assert_eq!(violation_paths(&err), vec!["/additionalProperties"]);
    }

    #[test]
    fn const_and_enum_values_must_match_declared_type() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {"type": "integer", "description": "x", "const": "five"},
                "name": {"type": "string", "description": "y", "enum": ["ok", 5]}
            }
        });
        let err = validate_schema(&schema, SchemaRole::Output).unwrap_err();
        let paths = violation_paths(&err);
        assert!(paths.contains(&"/properties/count/const"), "{paths:?}");
        assert!(paths.contains(&"/properties/name/enum/1"), "{paths:?}");
    }

    // -- Step 2: instance validation -------------------------------------

    fn sample_schema() -> Value {
        json!({
            "type": "object",
            "description": "Sample.",
            "additionalProperties": false,
            "required": ["profile"],
            "properties": {
                "profile": {
                    "type": "string",
                    "description": "x",
                    "minLength": 1,
                    "enum": ["prod", "staging"]
                },
                "ts": {"type": "string", "description": "y"}
            }
        })
    }

    #[test]
    fn valid_instance_passes_with_optional_property_absent() {
        let instance = json!({"profile": "prod"});
        assert_eq!(validate_instance(&sample_schema(), &instance), Vec::new());
    }

    #[test]
    fn missing_required_key_is_reported_at_expected_pointer() {
        let instance = json!({});
        let violations = validate_instance(&sample_schema(), &instance);
        assert_eq!(violation_paths(&violations), vec!["/profile"]);
    }

    #[test]
    fn unknown_key_is_reported_when_additional_properties_false() {
        let instance = json!({"profile": "prod", "extra": "nope"});
        let violations = validate_instance(&sample_schema(), &instance);
        assert_eq!(violation_paths(&violations), vec!["/extra"]);
    }

    #[test]
    fn min_length_violation_is_reported() {
        let instance = json!({"profile": ""});
        let violations = validate_instance(&sample_schema(), &instance);
        // Empty string still satisfies "present" (required ok) but fails
        // both minLength and enum — both point at the same instance
        // location.
        assert!(
            violation_paths(&violations)
                .iter()
                .all(|p| *p == "/profile")
        );
        assert_eq!(violations.len(), 2, "{violations:?}");
    }

    #[test]
    fn enum_violation_is_reported() {
        let instance = json!({"profile": "not-a-real-profile"});
        let violations = validate_instance(&sample_schema(), &instance);
        assert_eq!(violation_paths(&violations), vec!["/profile"]);
    }

    #[test]
    fn const_violation_is_reported() {
        let schema = json!({
            "type": "object",
            "properties": {
                "action_id": {"type": "string", "description": "x", "const": "mongo.backup"}
            }
        });
        let instance = json!({"action_id": "wrong.id"});
        let violations = validate_instance(&schema, &instance);
        assert_eq!(violation_paths(&violations), vec!["/action_id"]);
    }

    #[test]
    fn nested_object_property_violation_is_reported() {
        let schema = json!({
            "type": "object",
            "properties": {
                "step": {
                    "type": "object",
                    "description": "x",
                    "properties": {
                        "kind": {"type": "string", "description": "y", "minLength": 3}
                    },
                    "required": ["kind"]
                }
            }
        });
        let instance = json!({"step": {"kind": "ab"}});
        let violations = validate_instance(&schema, &instance);
        assert_eq!(violation_paths(&violations), vec!["/step/kind"]);
    }

    #[test]
    fn array_item_violation_is_reported() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "description": "x",
                    "items": {"type": "string", "description": "y", "minLength": 2}
                }
            }
        });
        let instance = json!({"tags": ["ok", "a"]});
        let violations = validate_instance(&schema, &instance);
        assert_eq!(violation_paths(&violations), vec!["/tags/1"]);
    }

    #[test]
    fn multiple_violations_are_returned_sorted_by_pointer() {
        // `required` is declared out of alphabetical order on purpose: the
        // required-key loop naturally emits "/z" before "/a", and the
        // additional-property loop emits "/m" last (after both). Without an
        // explicit sort, the returned order would be ["/z", "/a", "/m"] —
        // this test only passes if validate_instance actually sorts.
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["z", "a"],
            "properties": {
                "a": {"type": "string", "description": "x"},
                "z": {"type": "string", "description": "y"}
            }
        });
        let instance = json!({"m": "unexpected"});
        let violations = validate_instance(&schema, &instance);
        assert_eq!(violation_paths(&violations), vec!["/a", "/m", "/z"]);
    }

    #[test]
    fn string_values_are_not_coerced_to_other_types() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {"type": "integer", "description": "x"},
                "flag": {"type": "boolean", "description": "y"}
            }
        });
        // A CLI --param value is always a JSON string, even when it looks
        // like an integer or a boolean literal.
        let instance = json!({"count": "42", "flag": "true"});
        let violations = validate_instance(&schema, &instance);
        assert_eq!(violation_paths(&violations), vec!["/count", "/flag"]);
    }

    // -- Helpers -----------------------------------------------------------

    #[test]
    fn inferred_string_object_has_required_string_properties_and_open_additional_properties() {
        let required: BTreeSet<String> = ["ts".to_string(), "profile".to_string()]
            .into_iter()
            .collect();
        let schema = inferred_string_object(required);

        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(true));
        assert_eq!(schema["required"], json!(["profile", "ts"]));

        let props = schema["properties"].as_object().expect("properties object");
        for name in ["profile", "ts"] {
            let prop = &props[name];
            assert_eq!(prop["type"], json!("string"));
            assert!(
                prop["description"].as_str().is_some_and(|d| !d.is_empty()),
                "property '{name}' must have a non-empty description"
            );
        }

        // The inferred schema is deliberately permissive
        // (additionalProperties: true), so it can never satisfy the closed
        // SchemaRole::Input contract — that strictness is reserved for
        // explicitly YAML-authored schemas (Step 1's `validate_input_root`).
        // What it must be is well-formed subset JSON, which SchemaRole::
        // Output's generic grammar rules confirm without imposing the
        // closed-contract requirement.
        assert_eq!(validate_schema(&schema, SchemaRole::Output), Ok(()));
    }

    #[test]
    fn object_property_names_returns_declared_keys() {
        let schema = json!({
            "type": "object",
            "properties": {
                "b": {"type": "string"},
                "a": {"type": "string"}
            }
        });
        let names: BTreeSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert_eq!(object_property_names(&schema), names);
    }

    #[test]
    fn object_property_names_empty_when_no_properties() {
        let schema = json!({"type": "object"});
        assert_eq!(object_property_names(&schema), BTreeSet::new());
    }
}
