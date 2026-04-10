mod error;
mod parser;
mod path;

pub use error::FilterError;
pub use path::FilterCriteria;

use parser::Parser;

#[cfg(feature = "extension-module")]
use pyo3::prelude::*;

// ─── Public API ────────────────────────────────────────────────────────────

/// Filter `input` JSON, retaining only the keys that match the inclusion criteria.
///
/// Output is compact (no extra whitespace). Returns an error on malformed JSON.
pub fn filter_json(input: &str, criteria: &FilterCriteria) -> Result<String, FilterError> {
    let mut parser = Parser::new(input);
    let mut out = String::new();
    parser.filter_value_include(&[], criteria, &mut out)?;
    Ok(out)
}

/// Filter `input` JSON, removing any keys that match the exclusion criteria.
///
/// Output is compact (no extra whitespace). Returns an error on malformed JSON.
pub fn filter_json_exclude(input: &str, criteria: &FilterCriteria) -> Result<String, FilterError> {
    let mut parser = Parser::new(input);
    let mut out = String::new();
    parser.filter_value_exclude(&[], criteria, &mut out)?;
    Ok(out)
}

// ─── Python module (PyO3) ──────────────────────────────────────────────────

#[cfg(feature = "extension-module")]
#[pymodule]
mod filter_json_py {
    use pyo3::prelude::*;

    /// Placeholder — Python bindings to be added later.
    #[pyfunction]
    fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
        Ok((a + b).to_string())
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- Inclusion --

    #[test]
    fn include_nested_key() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = FilterCriteria::new(&["customer.name"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn include_top_level_key() {
        let json = r#"{"name": "Tom", "age": 24}"#;
        let c = FilterCriteria::new(&["name"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"name":"Tom"}"#);
    }

    #[test]
    fn include_multiple_criteria() {
        let json = r#"{"name": "Tom", "age": 24, "city": "London"}"#;
        let c = FilterCriteria::new(&["name", "city"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"name":"Tom","city":"London"}"#
        );
    }

    #[test]
    fn include_missing_key_returns_empty_object() {
        let json = r#"{"customer": {"other": 1}}"#;
        let c = FilterCriteria::new(&["customer.name"]);
        assert_eq!(filter_json(json, &c).unwrap(), "{}");
    }

    #[test]
    fn include_empty_input_object() {
        let c = FilterCriteria::new(&["name"]);
        assert_eq!(filter_json("{}", &c).unwrap(), "{}");
    }

    #[test]
    fn include_from_vec_str() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = FilterCriteria::from(vec!["customer.name"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn include_numeric_value() {
        let json = r#"{"x": 3.14, "y": 2}"#;
        let c = FilterCriteria::new(&["x"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"x":3.14}"#);
    }

    #[test]
    fn include_boolean_and_null() {
        let json = r#"{"a": true, "b": false, "c": null, "d": 1}"#;
        let c = FilterCriteria::new(&["a", "b", "c"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"a":true,"b":false,"c":null}"#
        );
    }

    // -- Inclusion: array selectors --

    #[test]
    fn include_single_array_index() {
        let json = r#"{"h": [{"v": 100}, {"v": 200}, {"v": 300}]}"#;
        let c = FilterCriteria::new(&["h[1].v"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"h":[{"v":200}]}"#);
    }

    #[test]
    fn include_all_array_elements() {
        let json = r#"{"items": [{"name": "a", "price": 1}, {"name": "b", "price": 2}]}"#;
        let c = FilterCriteria::new(&["items[*].name"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"items":[{"name":"a"},{"name":"b"}]}"#
        );
    }

    #[test]
    fn include_slice_first_n() {
        let json = r#"[{"n": "a"}, {"n": "b"}, {"n": "c"}, {"n": "d"}]"#;
        let c = FilterCriteria::new(&["[:2].n"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"[{"n":"a"},{"n":"b"}]"#);
    }

    #[test]
    fn include_slice_range() {
        let json = r#"[{"n": "a"}, {"n": "b"}, {"n": "c"}, {"n": "d"}]"#;
        let c = FilterCriteria::new(&["[1:3].n"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"[{"n":"b"},{"n":"c"}]"#);
    }

    #[test]
    fn out_of_bounds_index_returns_empty() {
        let json = r#"[1, 2, 3]"#;
        let c = FilterCriteria::new(&["[10]"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"[]"#);
    }

    // -- Exclusion --

    #[test]
    fn exclude_nested_key() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = FilterCriteria::new(&["customer.age"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn exclude_nested_all_match() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = FilterCriteria::new(&["customer.age", "customer.name"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), "{}");
    }

    #[test]
    fn exclude_top_level_key() {
        let json = r#"{"name": "Tom", "age": 24}"#;
        let c = FilterCriteria::new(&["age"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), r#"{"name":"Tom"}"#);
    }

    #[test]
    fn exclude_entire_subtree() {
        let json = r#"{"public": "yes", "private": {"secret": "shhh"}}"#;
        let c = FilterCriteria::new(&["private"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"{"public":"yes"}"#
        );
    }

    #[test]
    fn exclude_from_array_elements() {
        let json = r#"[{"id": 1, "secret": "x"}, {"id": 2, "secret": "y"}]"#;
        let c = FilterCriteria::new(&["[*].secret"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"[{"id":1},{"id":2}]"#
        );
    }

    #[test]
    fn exclude_preserves_all_when_no_match() {
        let json = r#"{"a": 1, "b": 2}"#;
        let c = FilterCriteria::new(&["z"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), r#"{"a":1,"b":2}"#);
    }

    // -- Exclusion: array selectors --

    #[test]
    fn exclude_single_index_element() {
        let json = r#"[1, 2, 3]"#;
        let c = FilterCriteria::new(&["[1]"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), r#"[1,3]"#);
    }

    #[test]
    fn exclude_field_from_indexed_element() {
        let json = r#"[{"name":"a","price":1},{"name":"b","price":2}]"#;
        let c = FilterCriteria::new(&["[1].price"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"[{"name":"a","price":1},{"name":"b"}]"#
        );
    }

    // -- Error handling --

    #[test]
    fn error_on_invalid_json() {
        let c = FilterCriteria::new(&["a"]);
        assert!(filter_json("not json", &c).is_err());
    }

    #[test]
    fn error_on_unexpected_eof() {
        let c = FilterCriteria::new(&["a"]);
        assert!(filter_json(r#"{"a": "#, &c).is_err());
    }
}
