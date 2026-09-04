//! `--jq` post-processing: run a jq filter over the rendered JSON envelope.
//!
//! The filter sees exactly what the user would have seen (redaction and
//! `--output-limit` already applied), so it cannot widen the output.

use anyhow::{Result, anyhow};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::{Val, read};

/// Run `filter_src` over `json_document`; return one line per output value.
/// With `raw_output`, string results print without quotes (like `jq -r`).
pub fn apply(filter_src: &str, json_document: &str, raw_output: bool) -> Result<Vec<String>> {
    let input: Val = read::parse_single(json_document.as_bytes())
        .map_err(|e| anyhow!("internal: rendered envelope is not valid JSON: {e}"))?;

    let program = File {
        code: filter_src,
        path: (),
    };
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs());

    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader
        .load(&arena, program)
        .map_err(|errs| anyhow!("validation: --jq filter failed to parse: {errs:?}"))?;
    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| anyhow!("validation: --jq filter failed to compile: {errs:?}"))?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut lines = Vec::new();
    for out in filter.id.run((ctx, input)).map(unwrap_valr) {
        let val = out.map_err(|e| anyhow!("validation: --jq filter error: {e}"))?;
        let line = match (&val, raw_output) {
            // Only text strings unquote under `-r`, matching `jq -r` semantics;
            // every other value type (including the byte-string superset type)
            // keeps its JSON rendering.
            (Val::TStr(bytes), true) => jaq_json::bstr(bytes.as_ref()).to_string(),
            _ => val.to_string(),
        };
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"ok":true,"message":"hello","data":{"items":[{"id":"a"},{"id":"b"}]}}"#;

    #[test]
    fn selects_a_field() {
        assert_eq!(apply(".ok", DOC, false).unwrap(), vec!["true"]);
    }

    #[test]
    fn raw_output_unquotes_strings_only() {
        assert_eq!(apply(".message", DOC, false).unwrap(), vec!["\"hello\""]);
        assert_eq!(apply(".message", DOC, true).unwrap(), vec!["hello"]);
        assert_eq!(apply(".ok", DOC, true).unwrap(), vec!["true"]);
    }

    #[test]
    fn iterates_one_line_per_result() {
        assert_eq!(
            apply(".data.items[].id", DOC, true).unwrap(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn std_functions_are_available() {
        assert_eq!(
            apply(".data.items | length", DOC, false).unwrap(),
            vec!["2"]
        );
    }

    #[test]
    fn bad_filter_is_a_validation_error() {
        let err = apply(".[", DOC, false).unwrap_err().to_string();
        assert!(err.starts_with("validation:"), "{err}");
    }
}
