use std::io::Read;

use crate::error::FilterError;
use crate::path::{
    FilterCriteria, InclusionStatus, PathNode, Segment, exclusion_status, inclusion_status,
};

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Write a JSON key with colon to `out`, escaping special characters.
fn push_json_key(out: &mut Vec<u8>, key: &str) {
    out.push(b'"');
    for c in key.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.extend_from_slice(b"\":");
}

fn is_empty_container(s: &[u8]) -> bool {
    s == b"{}" || s == b"[]"
}

// ─── InputSource trait ─────────────────────────────────────────────────────

pub(crate) trait InputSource {
    fn peek(&mut self) -> Option<u8>;
    fn advance(&mut self) -> Option<u8>;
    /// Copy the current JSON value verbatim to `out`.
    ///
    /// The cursor must be positioned at the first byte of the value (no
    /// leading whitespace).
    fn copy_value(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError>;

    // ── Default implementations ─────────────────────────────────────────

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
        let mut depth = 1usize;
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
        let mut depth = 1usize;
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
}

// ─── StrInput ──────────────────────────────────────────────────────────────

pub(crate) struct StrInput<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> StrInput<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }
}

impl<'a> InputSource for StrInput<'a> {
    fn peek(&mut self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn copy_value(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        self.skip_whitespace();
        let start = self.pos;
        self.skip_value_inner()?;
        out.extend_from_slice(&self.bytes[start..self.pos]);
        Ok(())
    }
}

// ─── ReaderInput ───────────────────────────────────────────────────────────

pub(crate) struct ReaderInput<R> {
    reader: R,
    peeked: Option<u8>,
}

impl<R: Read> ReaderInput<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader,
            peeked: None,
        }
    }

    fn read_byte(&mut self) -> Option<u8> {
        let mut buf = [0u8; 1];
        match self.reader.read(&mut buf) {
            Ok(1) => Some(buf[0]),
            _ => None,
        }
    }

    fn copy_byte(&mut self, out: &mut Vec<u8>) -> Option<u8> {
        let b = self.peeked.take().or_else(|| self.read_byte())?;
        out.push(b);
        Some(b)
    }

    fn copy_raw_string(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        self.copy_byte(out).ok_or(FilterError::UnexpectedEof)?; // opening `"`
        loop {
            match self.copy_byte(out) {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => return Ok(()),
                Some(b'\\') => {
                    self.copy_byte(out).ok_or(FilterError::UnexpectedEof)?;
                }
                _ => {}
            }
        }
    }

    fn copy_number_raw(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        if self.peek() == Some(b'-') {
            self.copy_byte(out);
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.copy_byte(out);
        }
        if self.peek() == Some(b'.') {
            self.copy_byte(out);
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.copy_byte(out);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.copy_byte(out);
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.copy_byte(out);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.copy_byte(out);
            }
        }
        Ok(())
    }

    fn copy_keyword_raw(&mut self, keyword: &[u8], out: &mut Vec<u8>) -> Result<(), FilterError> {
        for &expected in keyword {
            match self.copy_byte(out) {
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

    fn copy_container(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        let open = self.copy_byte(out).ok_or(FilterError::UnexpectedEof)?;
        let close = if open == b'{' { b'}' } else { b']' };
        let mut depth = 1usize;
        loop {
            match self.peek() {
                None => return Err(FilterError::UnexpectedEof),
                Some(b'"') => self.copy_raw_string(out)?,
                Some(b) if b == open => {
                    self.copy_byte(out);
                    depth += 1;
                }
                Some(b) if b == close => {
                    self.copy_byte(out);
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {
                    self.copy_byte(out);
                }
            }
        }
    }

    fn copy_value_inner(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        match self.peek() {
            Some(b'{') | Some(b'[') => self.copy_container(out),
            Some(b'"') => self.copy_raw_string(out),
            Some(b't') => self.copy_keyword_raw(b"true", out),
            Some(b'f') => self.copy_keyword_raw(b"false", out),
            Some(b'n') => self.copy_keyword_raw(b"null", out),
            Some(b'-' | b'0'..=b'9') => self.copy_number_raw(out),
            Some(b) => Err(FilterError::InvalidJson(format!(
                "unexpected '{}' at start of value",
                b as char
            ))),
            None => Err(FilterError::UnexpectedEof),
        }
    }
}

impl<R: Read> InputSource for ReaderInput<R> {
    fn peek(&mut self) -> Option<u8> {
        if self.peeked.is_none() {
            self.peeked = self.read_byte();
        }
        self.peeked
    }

    fn advance(&mut self) -> Option<u8> {
        self.peeked.take().or_else(|| self.read_byte())
    }

    fn copy_value(&mut self, out: &mut Vec<u8>) -> Result<(), FilterError> {
        self.skip_whitespace();
        self.copy_value_inner(out)
    }
}

// ─── Parser ────────────────────────────────────────────────────────────────

pub(crate) struct Parser<I> {
    input: I,
}

impl<I: InputSource> Parser<I> {
    pub(crate) fn new(input: I) -> Self {
        Self { input }
    }

    // ── Inclusion filter ─────────────────────────────────────────────────

    pub(crate) fn filter_value_include(
        &mut self,
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.skip_whitespace();
        match self.input.peek() {
            Some(b'{') => self.filter_object_include(path, criteria, out),
            Some(b'[') => self.filter_array_include(path, criteria, out),
            _ => {
                self.input.skip_value_inner()?;
                Ok(())
            }
        }
    }

    fn filter_object_include(
        &mut self,
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.expect(b'{')?;
        out.push(b'{');
        self.input.skip_whitespace();

        if self.input.peek() == Some(b'}') {
            self.input.advance();
            out.push(b'}');
            return Ok(());
        }

        let mut first = true;
        loop {
            self.input.skip_whitespace();
            let key = self.input.parse_string()?;
            let child_path = PathNode::create_child(Segment::Key(&key), &path);

            self.input.skip_whitespace();
            self.input.expect(b':')?;
            self.input.skip_whitespace();

            match inclusion_status(&child_path, criteria) {
                InclusionStatus::Keep => {
                    if !first {
                        out.push(b',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    self.input.copy_value(out)?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = Vec::new();
                    self.filter_value_include(child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first {
                            out.push(b',');
                        }
                        first = false;
                        push_json_key(out, &key);
                        out.extend_from_slice(&child_out);
                    }
                }
                InclusionStatus::Skip => {
                    self.input.skip_value_inner()?;
                }
            }

            self.input.skip_whitespace();
            match self.input.advance() {
                Some(b',') => {}
                Some(b'}') => {
                    out.push(b'}');
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
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.expect(b'[')?;
        out.push(b'[');
        self.input.skip_whitespace();

        if self.input.peek() == Some(b']') {
            self.input.advance();
            out.push(b']');
            return Ok(());
        }

        let mut first = true;
        let mut index: usize = 0;
        loop {
            self.input.skip_whitespace();
            let child_path = PathNode::create_child(Segment::Index(index), &path);
            index += 1;

            match inclusion_status(&child_path, criteria) {
                InclusionStatus::Keep => {
                    if !first {
                        out.push(b',');
                    }
                    first = false;
                    self.input.copy_value(out)?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = Vec::new();
                    self.filter_value_include(child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first {
                            out.push(b',');
                        }
                        first = false;
                        out.extend_from_slice(&child_out);
                    }
                }
                InclusionStatus::Skip => {
                    self.input.skip_value_inner()?;
                }
            }

            self.input.skip_whitespace();
            match self.input.advance() {
                Some(b',') => {}
                Some(b']') => {
                    out.push(b']');
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
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.skip_whitespace();
        match self.input.peek() {
            Some(b'{') => self.filter_object_exclude(path, criteria, out),
            Some(b'[') => self.filter_array_exclude(path, criteria, out),
            _ => self.input.copy_value(out),
        }
    }

    fn filter_object_exclude(
        &mut self,
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.expect(b'{')?;
        out.push(b'{');
        self.input.skip_whitespace();

        if self.input.peek() == Some(b'}') {
            self.input.advance();
            out.push(b'}');
            return Ok(());
        }

        let mut first = true;
        loop {
            self.input.skip_whitespace();
            let key = self.input.parse_string()?;
            let child_path = PathNode::create_child(Segment::Key(&key), &path);

            self.input.skip_whitespace();
            self.input.expect(b':')?;
            self.input.skip_whitespace();

            match exclusion_status(&child_path, criteria) {
                InclusionStatus::Skip => {
                    self.input.skip_value_inner()?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = Vec::new();
                    self.filter_value_exclude(child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first {
                            out.push(b',');
                        }
                        first = false;
                        push_json_key(out, &key);
                        out.extend_from_slice(&child_out);
                    }
                }
                InclusionStatus::Keep => {
                    if !first {
                        out.push(b',');
                    }
                    first = false;
                    push_json_key(out, &key);
                    self.input.copy_value(out)?;
                }
            }

            self.input.skip_whitespace();
            match self.input.advance() {
                Some(b',') => {}
                Some(b'}') => {
                    out.push(b'}');
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
        path: PathNode,
        criteria: &FilterCriteria,
        out: &mut Vec<u8>,
    ) -> Result<(), FilterError> {
        self.input.expect(b'[')?;
        out.push(b'[');
        self.input.skip_whitespace();

        if self.input.peek() == Some(b']') {
            self.input.advance();
            out.push(b']');
            return Ok(());
        }

        let mut first_out = true;
        let mut index: usize = 0;
        loop {
            self.input.skip_whitespace();
            let child_path = PathNode::create_child(Segment::Index(index), &path);
            index += 1;

            match exclusion_status(&child_path, criteria) {
                InclusionStatus::Skip => {
                    self.input.skip_value_inner()?;
                }
                InclusionStatus::Recurse => {
                    let mut child_out = Vec::new();
                    self.filter_value_exclude(child_path, criteria, &mut child_out)?;
                    if !is_empty_container(&child_out) {
                        if !first_out {
                            out.push(b',');
                        }
                        first_out = false;
                        out.extend_from_slice(&child_out);
                    }
                }
                InclusionStatus::Keep => {
                    if !first_out {
                        out.push(b',');
                    }
                    first_out = false;
                    self.input.copy_value(out)?;
                }
            }

            self.input.skip_whitespace();
            match self.input.advance() {
                Some(b',') => {}
                Some(b']') => {
                    out.push(b']');
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
