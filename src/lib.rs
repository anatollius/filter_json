#[cfg(feature = "extension-module")]
use pyo3::prelude::*;

// ─── Error ─────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum FilterError {
    InvalidJson(String),
    UnexpectedEof,
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterError::InvalidJson(msg) => write!(f, "Invalid JSON: {msg}"),
            FilterError::UnexpectedEof => write!(f, "Unexpected end of input"),
        }
    }
}

impl std::error::Error for FilterError {}

// ─── Criteria ──────────────────────────────────────────────────────────────

/// Keep only the keys at these dot-separated paths; all others are removed.
///
/// ```
/// # use filter_json::InclusionCriteria;
/// let c = InclusionCriteria::from(vec!["customer.name"]);
/// ```
pub struct InclusionCriteria {
    paths: Vec<Vec<String>>,
}

impl InclusionCriteria {
    pub fn new(paths: &[&str]) -> Self {
        Self {
            paths: paths
                .iter()
                .map(|p| p.split('.').map(String::from).collect())
                .collect(),
        }
    }
}

impl<'a> From<Vec<&'a str>> for InclusionCriteria {
    fn from(paths: Vec<&'a str>) -> Self {
        Self::new(&paths)
    }
}

/// Remove any keys at these dot-separated paths; all others are kept.
pub struct ExclusionCriteria {
    paths: Vec<Vec<String>>,
}

impl ExclusionCriteria {
    pub fn new(paths: &[&str]) -> Self {
        Self {
            paths: paths
                .iter()
                .map(|p| p.split('.').map(String::from).collect())
                .collect(),
        }
    }
}

impl<'a> From<Vec<&'a str>> for ExclusionCriteria {
    fn from(paths: Vec<&'a str>) -> Self {
        Self::new(&paths)
    }
}

// ─── Path-match helpers ────────────────────────────────────────────────────

#[derive(PartialEq)]
enum InclusionStatus {
    /// `path` exactly equals a criterion → copy the value verbatim.
    Exact,
    /// `path` is a strict prefix of some criterion → recurse into the value.
    Prefix,
    /// No criterion matches or has `path` as a prefix → skip the value.
    Skip,
}

fn inclusion_status(path: &[String], criteria: &InclusionCriteria) -> InclusionStatus {
    let mut found_prefix = false;
    for criterion in &criteria.paths {
        if criterion.as_slice() == path {
            return InclusionStatus::Exact;
        }
        if criterion.len() > path.len() && criterion.starts_with(path) {
            found_prefix = true;
        }
    }
    if found_prefix {
        InclusionStatus::Prefix
    } else {
        InclusionStatus::Skip
    }
}

#[derive(PartialEq)]
enum ExclusionStatus {
    /// `path` exactly equals a criterion → skip the value.
    Skip,
    /// `path` is a strict prefix of some criterion → recurse into the value.
    Recurse,
    /// No criterion matches → copy the value verbatim.
    Keep,
}

fn exclusion_status(path: &[String], criteria: &ExclusionCriteria) -> ExclusionStatus {
    let mut found_prefix = false;
    for criterion in &criteria.paths {
        if criterion.as_slice() == path {
            return ExclusionStatus::Skip;
        }
        if criterion.len() > path.len() && criterion.starts_with(path) {
            found_prefix = true;
        }
    }
    if found_prefix {
        ExclusionStatus::Recurse
    } else {
        ExclusionStatus::Keep
    }
}

// ─── Parser ────────────────────────────────────────────────────────────────

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.advance();
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), FilterError> {
        match self.advance() {
            Some(b) if b == expected => Ok(()),
            Some(b) => Err(FilterError::InvalidJson(format!(
                "expected '{}' but got '{}'",
                expected as char, b as char
            ))),
            None => Err(FilterError::UnexpectedEof),
        }
    }

    // ── Skip methods (advance pos, no output) ───────────────────────────

    fn skip_string(&mut self) -> Result<(), FilterError> {
        self.expect(b'"')?;
        loop {
            match self.advance() {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => return Ok(()),
                Some(b'\\') => {
                    self.advance();
                }
                Some(_) => {}
            }
        }
    }

    fn skip_number(&mut self) -> Result<(), FilterError> {
        if self.peek() == Some(b'-') {
            self.advance();
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.advance();
        }
        if self.peek() == Some(b'.') {
            self.advance();
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.advance();
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        Ok(())
    }

    fn skip_keyword(&mut self, keyword: &[u8]) -> Result<(), FilterError> {
        for &expected in keyword {
            match self.advance() {
                Some(b) if b == expected => {}
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected '{}' in keyword but got '{}'",
                        expected as char, b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    /// Skip a JSON value; `pos` must already point at the first byte of the value.
    fn skip_value_inner(&mut self) -> Result<(), FilterError> {
        match self.peek() {
            Some(b'{') => self.skip_object(),
            Some(b'[') => self.skip_array(),
            Some(b'"') => self.skip_string(),
            Some(b't') => self.skip_keyword(b"true"),
            Some(b'f') => self.skip_keyword(b"false"),
            Some(b'n') => self.skip_keyword(b"null"),
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            Some(b) => Err(FilterError::InvalidJson(format!(
                "unexpected '{}' at start of value",
                b as char
            ))),
            None => Err(FilterError::UnexpectedEof),
        }
    }

    fn skip_object(&mut self) -> Result<(), FilterError> {
        self.expect(b'{')?;
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok(());
        }
        loop {
            self.skip_whitespace();
            self.skip_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            self.skip_value_inner()?;
            self.skip_whitespace();
            match self.advance() {
                Some(b',') => {}
                Some(b'}') => return Ok(()),
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or '}}' but got '{}'",
                        b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
    }

    fn skip_array(&mut self) -> Result<(), FilterError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.advance();
            return Ok(());
        }
        loop {
            self.skip_whitespace();
            self.skip_value_inner()?;
            self.skip_whitespace();
            match self.advance() {
                Some(b',') => {}
                Some(b']') => return Ok(()),
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or ']' but got '{}'",
                        b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
    }

    // ── Copy method (advance pos, emit to out) ───────────────────────────

    /// Copy the current JSON value verbatim (leading whitespace stripped).
    fn copy_value(&mut self, out: &mut String) -> Result<(), FilterError> {
        self.skip_whitespace();
        let start = self.pos;
        self.skip_value_inner()?;
        out.push_str(&self.input[start..self.pos]);
        Ok(())
    }

    // ── Parse method (advance pos, return decoded value) ─────────────────

    /// Decode a JSON string key at the current position.
    fn parse_string(&mut self) -> Result<String, FilterError> {
        self.expect(b'"')?;
        let mut result: Vec<u8> = Vec::new();
        loop {
            match self.advance() {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => break,
                Some(b'\\') => match self.advance() {
                    None => return Err(FilterError::UnexpectedEof),
                    Some(b'"') => result.push(b'"'),
                    Some(b'\\') => result.push(b'\\'),
                    Some(b'/') => result.push(b'/'),
                    Some(b'n') => result.push(b'\n'),
                    Some(b'r') => result.push(b'\r'),
                    Some(b't') => result.push(b'\t'),
                    Some(b'b') => result.push(0x08),
                    Some(b'f') => result.push(0x0C),
                    Some(b'u') => {
                        let mut hex = [0u8; 4];
                        for slot in &mut hex {
                            *slot = self.advance().ok_or(FilterError::UnexpectedEof)?;
                        }
                        let hex_str = std::str::from_utf8(&hex).map_err(|_| {
                            FilterError::InvalidJson("invalid \\u escape".to_string())
                        })?;
                        let code = u32::from_str_radix(hex_str, 16).map_err(|_| {
                            FilterError::InvalidJson(format!("invalid \\u{hex_str} escape"))
                        })?;
                        let ch = char::from_u32(code).ok_or_else(|| {
                            FilterError::InvalidJson(format!("invalid codepoint U+{code:04X}"))
                        })?;
                        let mut buf = [0u8; 4];
                        result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    Some(b) => {
                        return Err(FilterError::InvalidJson(format!(
                            "invalid escape \\{}",
                            b as char
                        )))
                    }
                },
                Some(b) => result.push(b),
            }
        }
        String::from_utf8(result)
            .map_err(|e| FilterError::InvalidJson(format!("invalid UTF-8 in string: {e}")))
    }

    // ── Inclusion filter ─────────────────────────────────────────────────

    /// Filter a value for inclusion. Writes to `out` only if there is something
    /// to include; caller must check whether `out` grew before emitting the key.
    fn filter_value_include(
        &mut self,
        path: &[String],
        criteria: &InclusionCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.skip_whitespace();
        if self.peek() == Some(b'{') {
            self.filter_object_include(path, criteria, out)
        } else {
            // Non-object where we expected to recurse deeper: no path can match.
            self.skip_value_inner()?;
            Ok(())
        }
    }

    fn filter_object_include(
        &mut self,
        path: &[String],
        criteria: &InclusionCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.expect(b'{')?;
        out.push('{');
        self.skip_whitespace();

        if self.peek() == Some(b'}') {
            self.advance();
            out.push('}');
            return Ok(());
        }

        let mut first = true;
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            let mut child_path = path.to_vec();
            child_path.push(key.clone());

            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();

            match inclusion_status(&child_path, criteria) {
                InclusionStatus::Exact => {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    self.copy_value(out)?;
                }
                InclusionStatus::Prefix => {
                    let mut child_out = String::new();
                    self.filter_value_include(&child_path, criteria, &mut child_out)?;
                    if !child_out.is_empty() {
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        push_json_key(out, &key);
                        out.push_str(&child_out);
                    }
                }
                InclusionStatus::Skip => {
                    self.skip_value_inner()?;
                }
            }

            self.skip_whitespace();
            match self.advance() {
                Some(b',') => {}
                Some(b'}') => {
                    out.push('}');
                    break;
                }
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or '}}' but got '{}'",
                        b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    // ── Exclusion filter ─────────────────────────────────────────────────

    fn filter_value_exclude(
        &mut self,
        path: &[String],
        criteria: &ExclusionCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.filter_object_exclude(path, criteria, out),
            Some(b'[') => self.filter_array_exclude(path, criteria, out),
            _ => self.copy_value(out),
        }
    }

    fn filter_object_exclude(
        &mut self,
        path: &[String],
        criteria: &ExclusionCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.expect(b'{')?;
        out.push('{');
        self.skip_whitespace();

        if self.peek() == Some(b'}') {
            self.advance();
            out.push('}');
            return Ok(());
        }

        let mut first = true;
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            let mut child_path = path.to_vec();
            child_path.push(key.clone());

            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();

            match exclusion_status(&child_path, criteria) {
                ExclusionStatus::Skip => {
                    self.skip_value_inner()?;
                }
                ExclusionStatus::Recurse => {
                    let mut child_out = String::new();
                    self.filter_value_exclude(&child_path, criteria, &mut child_out)?;
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    out.push_str(&child_out);
                }
                ExclusionStatus::Keep => {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    self.copy_value(out)?;
                }
            }

            self.skip_whitespace();
            match self.advance() {
                Some(b',') => {}
                Some(b'}') => {
                    out.push('}');
                    break;
                }
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or '}}' but got '{}'",
                        b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    fn filter_array_exclude(
        &mut self,
        path: &[String],
        criteria: &ExclusionCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.expect(b'[')?;
        out.push('[');
        self.skip_whitespace();

        if self.peek() == Some(b']') {
            self.advance();
            out.push(']');
            return Ok(());
        }

        let mut first = true;
        loop {
            self.skip_whitespace();
            if !first {
                out.push(',');
            }
            first = false;
            self.filter_value_exclude(path, criteria, out)?;
            self.skip_whitespace();
            match self.advance() {
                Some(b',') => {}
                Some(b']') => {
                    out.push(']');
                    break;
                }
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or ']' but got '{}'",
                        b as char
                    )))
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }
}

// ─── Helper ────────────────────────────────────────────────────────────────

/// Write a JSON key with colon to `out`, escaping special characters.
fn push_json_key(out: &mut String, key: &str) {
    out.push('"');
    for c in key.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push_str("\":");
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Filter `input` JSON, retaining only the keys that match the inclusion criteria.
///
/// Output is compact (no extra whitespace). Returns an error on malformed JSON.
pub fn filter_json(input: &str, criteria: &InclusionCriteria) -> Result<String, FilterError> {
    let mut parser = Parser::new(input);
    let mut out = String::new();
    parser.filter_value_include(&[], criteria, &mut out)?;
    Ok(out)
}

/// Filter `input` JSON, removing any keys that match the exclusion criteria.
///
/// Output is compact (no extra whitespace). Returns an error on malformed JSON.
pub fn filter_json_exclude(
    input: &str,
    criteria: &ExclusionCriteria,
) -> Result<String, FilterError> {
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
        let c = InclusionCriteria::new(&["customer.name"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn include_top_level_key() {
        let json = r#"{"name": "Tom", "age": 24}"#;
        let c = InclusionCriteria::new(&["name"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"name":"Tom"}"#);
    }

    #[test]
    fn include_multiple_criteria() {
        let json = r#"{"name": "Tom", "age": 24, "city": "London"}"#;
        let c = InclusionCriteria::new(&["name", "city"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"name":"Tom","city":"London"}"#
        );
    }

    #[test]
    fn include_missing_key_returns_empty_object() {
        let json = r#"{"customer": {"other": 1}}"#;
        let c = InclusionCriteria::new(&["customer.name"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"customer":{}}"#);
    }

    #[test]
    fn include_empty_input_object() {
        let c = InclusionCriteria::new(&["name"]);
        assert_eq!(filter_json("{}", &c).unwrap(), "{}");
    }

    #[test]
    fn include_from_vec_str() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = InclusionCriteria::from(vec!["customer.name"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn include_numeric_value() {
        let json = r#"{"x": 3.14, "y": 2}"#;
        let c = InclusionCriteria::new(&["x"]);
        assert_eq!(filter_json(json, &c).unwrap(), r#"{"x":3.14}"#);
    }

    #[test]
    fn include_boolean_and_null() {
        let json = r#"{"a": true, "b": false, "c": null, "d": 1}"#;
        let c = InclusionCriteria::new(&["a", "b", "c"]);
        assert_eq!(
            filter_json(json, &c).unwrap(),
            r#"{"a":true,"b":false,"c":null}"#
        );
    }

    // -- Exclusion --

    #[test]
    fn exclude_nested_key() {
        let json = r#"{"customer": {"name": "Tom", "age": 24}}"#;
        let c = ExclusionCriteria::new(&["customer.age"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"{"customer":{"name":"Tom"}}"#
        );
    }

    #[test]
    fn exclude_top_level_key() {
        let json = r#"{"name": "Tom", "age": 24}"#;
        let c = ExclusionCriteria::new(&["age"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), r#"{"name":"Tom"}"#);
    }

    #[test]
    fn exclude_entire_subtree() {
        let json = r#"{"public": "yes", "private": {"secret": "shhh"}}"#;
        let c = ExclusionCriteria::new(&["private"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"{"public":"yes"}"#
        );
    }

    #[test]
    fn exclude_from_array_elements() {
        let json = r#"[{"id": 1, "secret": "x"}, {"id": 2, "secret": "y"}]"#;
        let c = ExclusionCriteria::new(&["secret"]);
        assert_eq!(
            filter_json_exclude(json, &c).unwrap(),
            r#"[{"id":1},{"id":2}]"#
        );
    }

    #[test]
    fn exclude_preserves_all_when_no_match() {
        let json = r#"{"a": 1, "b": 2}"#;
        let c = ExclusionCriteria::new(&["z"]);
        assert_eq!(filter_json_exclude(json, &c).unwrap(), r#"{"a":1,"b":2}"#);
    }

    // -- Error handling --

    #[test]
    fn error_on_invalid_json() {
        let c = InclusionCriteria::new(&["a"]);
        assert!(filter_json("not json", &c).is_err());
    }

    #[test]
    fn error_on_unexpected_eof() {
        let c = InclusionCriteria::new(&["a"]);
        assert!(filter_json(r#"{"a": "#, &c).is_err());
    }
}
