//! `--jq` post-processing: run a jq filter over the rendered JSON envelope.
//!
//! The filter sees exactly what the user would have seen (redaction and
//! `--output-limit` already applied), so it cannot widen the output.

use anyhow::{Context, Result, anyhow};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data};
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
    for out in filter.id.run((ctx, input)) {
        // Iterate the raw `ValX` stream instead of going through
        // `jaq_core::unwrap_valr`, which calls `std::process::exit` on a
        // `halt`/`halt_error` exception. A `--jq` filter must never be able
        // to set our exit code or terminate the process directly.
        let val = out.map_err(|exn| match exn.get_err() {
            Ok(error) => anyhow!("validation: --jq filter error: {error}"),
            Err(_) => anyhow!("validation: --jq filters may not call halt or halt_error"),
        })?;
        // `Val`'s Display is JSON for every finite value (and preserves big
        // integers exactly), but emits `NaN`/`Infinity` for non-finite floats.
        // Validate the syntax without re-materializing the value so precision
        // is never lost, and print the rendering itself.
        let rendered = val.to_string();
        serde_json::from_str::<serde::de::IgnoredAny>(&rendered).map_err(|_| {
            anyhow!(
                "validation: --jq produced a value that is not valid JSON (NaN or Infinity): {rendered}"
            )
        })?;
        lines.push(if raw_output && rendered.starts_with('"') {
            serde_json::from_str::<String>(&rendered).context("unescape --raw-output string")?
        } else {
            rendered
        });
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

    #[test]
    fn non_finite_results_are_validation_errors_not_output() {
        for filter in ["1/0", "0/0", "-1/0", "{a: (1/0)}", "[1, (0/0)]"] {
            let err = apply(filter, DOC, false).unwrap_err().to_string();
            assert!(err.starts_with("validation:"), "{filter}: {err}");
            assert!(err.contains("not valid JSON"), "{filter}: {err}");
        }
    }

    #[test]
    fn every_output_line_is_valid_json() {
        for filter in [
            ".",
            ".data.items",
            "\"a\\\"b\\\\c\"",
            "99999999999999999999999999999999",
            "null",
        ] {
            for line in apply(filter, DOC, false).unwrap() {
                serde_json::from_str::<serde_json::Value>(&line)
                    .unwrap_or_else(|e| panic!("{filter}: line {line:?} is not JSON: {e}"));
            }
        }
    }

    #[test]
    fn big_integers_round_trip_byte_exact() {
        for literal in [
            "99999999999999999999999999999999",
            "18446744073709551616",
            "-9223372036854775809",
            "18446744073709551615",
        ] {
            assert_eq!(
                apply(literal, DOC, false).unwrap(),
                vec![literal.to_string()],
                "{literal}"
            );
        }
    }

    #[test]
    fn raw_output_unescapes_json_string_syntax() {
        assert_eq!(
            apply("\"a\\\"b\\\\c\"", DOC, true).unwrap(),
            vec!["a\"b\\c"]
        );
    }

    #[test]
    fn halt_is_a_validation_error_not_an_exit() {
        for filter in ["halt", "\"x\" | halt_error", "halt_error"] {
            let err = apply(filter, DOC, false).unwrap_err().to_string();
            assert!(err.starts_with("validation:"), "{filter}: {err}");
        }
    }
}
