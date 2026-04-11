use crate::error::FilterError;
use crate::path::{FilterCriteria, InclusionStatus, Segment, exclusion_status, inclusion_status};

// ─── Helpers ───────────────────────────────────────────────────────────────

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

fn is_empty_container(s: &str) -> bool {
    s == "{}" || s == "[]"
}

// ─── Parser ────────────────────────────────────────────────────────────────

pub(crate) struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
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
                    )));
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
        let mut depth = 1;
        loop {
            match self.peek() {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => {
                    self.skip_string()?;
                }
                Some(b'{') => {
                    self.advance();
                    depth += 1;
                }
                Some(b'}') => {
                    self.advance();
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn skip_array(&mut self) -> Result<(), FilterError> {
        self.expect(b'[')?;
        let mut depth = 1;
        loop {
            match self.peek() {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => {
                    self.skip_string()?;
                }
                Some(b'[') => {
                    self.advance();
                    depth += 1;
                }
                Some(b']') => {
                    self.advance();
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {
                    self.advance();
                }
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
                        )));
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
    pub(crate) fn filter_value_include(
        &mut self,
        path: &[Segment],
        criteria: &FilterCriteria,
        out: &mut String,
    ) -> Result<(), FilterError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.filter_object_include(path, criteria, out),
            Some(b'[') => self.filter_array_include(path, criteria, out),
            _ => {
                // Non-container where we expected to recurse deeper: no path can match.
                self.skip_value_inner()?;
                Ok(())
            }
        }
    }

    fn filter_object_include(
        &mut self,
        path: &[Segment],
        criteria: &FilterCriteria,
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
            child_path.push(Segment::Key(&key));

            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();

            match inclusion_status(&child_path, criteria) {
                InclusionStatus::Keep => {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    self.copy_value(out)?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = String::new();
                    self.filter_value_include(&child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
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
                    )));
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    fn filter_array_include(
        &mut self,
        path: &[Segment],
        criteria: &FilterCriteria,
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
        let mut index: usize = 0;
        loop {
            self.skip_whitespace();
            let mut child_path = path.to_vec();
            child_path.push(Segment::Index(index));
            index += 1;

            match inclusion_status(&child_path, criteria) {
                InclusionStatus::Keep => {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    self.copy_value(out)?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = String::new();
                    self.filter_value_include(&child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first {
                            out.push(',');
                        }
                        first = false;
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
                Some(b']') => {
                    out.push(']');
                    break;
                }
                Some(b) => {
                    return Err(FilterError::InvalidJson(format!(
                        "expected ',' or ']' but got '{}'",
                        b as char
                    )));
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    // ── Exclusion filter ─────────────────────────────────────────────────

    pub(crate) fn filter_value_exclude(
        &mut self,
        path: &[Segment],
        criteria: &FilterCriteria,
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
        path: &[Segment],
        criteria: &FilterCriteria,
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
            child_path.push(Segment::Key(&key));

            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();

            match exclusion_status(&child_path, criteria) {
                InclusionStatus::Skip => {
                    self.skip_value_inner()?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = String::new();
                    self.filter_value_exclude(&child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        push_json_key(out, &key);
                        out.push_str(&child_out);
                    }
                }
                InclusionStatus::Keep => {
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
                    )));
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }

    fn filter_array_exclude(
        &mut self,
        path: &[Segment],
        criteria: &FilterCriteria,
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

        let mut first_out = true;
        let mut index: usize = 0;
        loop {
            self.skip_whitespace();
            let mut child_path = path.to_vec();
            child_path.push(Segment::Index(index));
            index += 1;

            match exclusion_status(&child_path, criteria) {
                InclusionStatus::Skip => {
                    self.skip_value_inner()?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = String::new();
                    self.filter_value_exclude(&child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first_out {
                            out.push(',');
                        }
                        first_out = false;
                        out.push_str(&child_out);
                    }
                }
                InclusionStatus::Keep => {
                    if !first_out {
                        out.push(',');
                    }
                    first_out = false;
                    self.copy_value(out)?;
                }
            }

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
                    )));
                }
                None => return Err(FilterError::UnexpectedEof),
            }
        }
        Ok(())
    }
}
