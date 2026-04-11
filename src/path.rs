use crate::error::FilterError;

// ─── Segment ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Segment<'a> {
    Key(&'a str),
    Index(usize),
    All,
    Slice {
        start: Option<usize>,
        end: Option<usize>,
    },
}

fn parse_selector<'a>(s: &str, path: &'a str) -> Result<Segment<'a>, FilterError> {
    if s.is_empty() {
        return Err(FilterError::InvalidCriteria(format!(
            "array selector in '{path}' must not be empty"
        )));
    }
    if s == "*" {
        return Ok(Segment::All);
    }
    if let Ok(n) = s.parse::<usize>() {
        return Ok(Segment::Index(n));
    }
    if let Some(colon_pos) = s.find(':') {
        let start_str = &s[..colon_pos];
        let end_str = &s[colon_pos + 1..];
        let start = if start_str.is_empty() {
            None
        } else {
            Some(start_str.parse::<usize>().map_err(|_| {
                FilterError::InvalidCriteria(format!(
                    "invalid slice start '{start_str}' in '{path}': expected an integer"
                ))
            })?)
        };
        let end = if end_str.is_empty() {
            None
        } else {
            Some(end_str.parse::<usize>().map_err(|_| {
                FilterError::InvalidCriteria(format!(
                    "invalid slice end '{end_str}' in '{path}': expected an integer"
                ))
            })?)
        };
        if (start, end) == (None, None) {
            return Ok(Segment::All);
        }
        return Ok(Segment::Slice { start, end });
    }
    Err(FilterError::InvalidCriteria(format!(
        "invalid array selector '[{s}]' in '{path}': \
         expected an integer index, '*', or a slice 'a:b'"
    )))
}

pub(crate) fn parse_path<'a>(path: &'a str) -> Result<Vec<Segment<'a>>, FilterError> {
    if path.is_empty() {
        return Err(FilterError::InvalidCriteria(
            "path must not be empty".to_string(),
        ));
    }
    let mut segments = Vec::new();
    for dot_segment in path.split('.') {
        if dot_segment.is_empty() {
            return Err(FilterError::InvalidCriteria(format!(
                "path '{path}' contains an empty segment \
                 (check for leading, trailing, or consecutive dots)"
            )));
        }
        if let Some(bracket_pos) = dot_segment.find('[') {
            let key_part = &dot_segment[..bracket_pos];
            if !key_part.is_empty() {
                segments.push(Segment::Key(key_part));
            }
            let mut remaining = &dot_segment[bracket_pos..];
            while remaining.starts_with('[') {
                if let Some(close) = remaining.find(']') {
                    let selector = &remaining[1..close];
                    segments.push(parse_selector(selector, path)?);
                    remaining = &remaining[close + 1..];
                } else {
                    return Err(FilterError::InvalidCriteria(format!(
                        "unclosed '[' in path '{path}'"
                    )));
                }
            }
            if !remaining.is_empty() {
                return Err(FilterError::InvalidCriteria(format!(
                    "unexpected characters '{remaining}' after ']' in path '{path}'"
                )));
            }
        } else {
            segments.push(Segment::Key(dot_segment));
        }
    }
    Ok(segments)
}

// ─── Criteria ──────────────────────────────────────────────────────────────

/// A set of key paths used to filter a JSON value.
///
/// Paths are dot-separated key names, optionally with array selectors
/// (`[n]`, `[*]`, `[:n]`, `[a:b]`, `[a:]`).
///
/// Pass to [`filter_json`] to include only matching keys, or to
/// [`filter_json_exclude`] to remove matching keys.
///
/// ```
/// # use filter_json::FilterCriteria;
/// let c = FilterCriteria::new(&["customer.name"]).unwrap();
/// let c2 = FilterCriteria::new(&["items[*].price"]).unwrap();
/// ```
pub struct FilterCriteria<'a> {
    pub(crate) paths: Vec<Vec<Segment<'a>>>,
}

impl<'a> FilterCriteria<'a> {
    pub fn new(paths: &[&'a str]) -> Result<Self, FilterError> {
        let parsed = paths
            .iter()
            .map(|p| parse_path(p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { paths: parsed })
    }
}

impl<'a> TryFrom<Vec<&'a str>> for FilterCriteria<'a> {
    type Error = FilterError;

    fn try_from(paths: Vec<&'a str>) -> Result<Self, FilterError> {
        Self::new(&paths)
    }
}

// ─── Segment matching ──────────────────────────────────────────────────────

pub(crate) fn segment_matches(criterion: &Segment, runtime: &Segment) -> bool {
    match (criterion, runtime) {
        (Segment::Key(a), Segment::Key(b)) => a == b,
        (Segment::Index(a), Segment::Index(b)) => a == b,
        (Segment::All, Segment::Index(_)) => true,
        (Segment::Slice { start, end }, Segment::Index(i)) => {
            start.is_none_or(|e| *i >= e) && end.is_none_or(|e| *i < e)
        }
        _ => false,
    }
}

pub(crate) fn criterion_matches_path(criterion: &[Segment], path: &PathNode) -> bool {
    criterion.len() == path.len()
        && criterion
            .iter()
            .rev()
            .zip(path.iter())
            .all(|(c, pi)| segment_matches(c, &pi.segment))
}

pub(crate) fn criterion_is_prefix_of(criterion: &[Segment], path: &PathNode) -> bool {
    criterion.len() > path.len()
        && criterion[..path.len()]
            .iter()
            .rev()
            .zip(path.iter())
            .all(|(c, pi)| segment_matches(c, &pi.segment))
}

// ─── Status types ──────────────────────────────────────────────────────────

#[derive(PartialEq)]
pub(crate) enum InclusionStatus {
    /// `path` exactly equals a criterion → copy the value verbatim.
    Keep,
    /// `path` is a strict prefix of some criterion → recurse into the value.
    Recurse,
    /// No criterion matches or has `path` as a prefix → skip the value.
    Skip,
}

pub(crate) fn inclusion_status(path: &PathNode, criteria: &FilterCriteria) -> InclusionStatus {
    let mut found_prefix = false;
    for criterion in criteria.paths.as_slice() {
        if criterion_matches_path(criterion, path) {
            return InclusionStatus::Keep;
        }
        if criterion_is_prefix_of(criterion, path) {
            // Cannot immediately return because there might be another criterion that is
            // an exact match, e.g. someone requests ["customer.name", "customer"]
            found_prefix = true;
        }
    }
    if found_prefix {
        InclusionStatus::Recurse
    } else {
        InclusionStatus::Skip
    }
}

pub(crate) fn exclusion_status(path: &PathNode, criteria: &FilterCriteria) -> InclusionStatus {
    let mut found_prefix = false;
    for criterion in criteria.paths.as_slice() {
        if criterion_matches_path(criterion, path) {
            return InclusionStatus::Skip;
        }
        if criterion_is_prefix_of(criterion, path) {
            // Cannot immediately return because there might be another criterion that is
            // an exact match, e.g. someone requests ["customer.name", "customer"]
            found_prefix = true;
        }
    }
    if found_prefix {
        InclusionStatus::Recurse
    } else {
        InclusionStatus::Keep
    }
}

// ─── PathNode ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum PathNode<'a> {
    Root,
    Child(PathItem<'a>),
}

#[derive(Debug)]
pub(crate) struct PathItem<'a> {
    pub segment: Segment<'a>,
    pub parent: &'a PathNode<'a>,
    pub depth: usize,
}

impl<'a> PathNode<'a> {
    pub fn create_child(segment: Segment<'a>, parent: &'a PathNode) -> PathNode<'a> {
        match parent {
            PathNode::Child(pi) => PathNode::Child(PathItem {
                segment: segment,
                parent: parent,
                depth: pi.depth + 1,
            }),
            PathNode::Root => PathNode::Child(PathItem {
                segment: segment,
                parent: parent,
                depth: 1,
            }),
        }
    }

    fn len(&self) -> usize {
        match self {
            PathNode::Child(pi) => pi.depth,
            PathNode::Root => 0,
        }
    }

    fn iter(&'a self) -> PathNodeIterator<'a> {
        PathNodeIterator { current_node: self }
    }
}

struct PathNodeIterator<'a> {
    current_node: &'a PathNode<'a>,
}

impl<'a> Iterator for PathNodeIterator<'a> {
    type Item = &'a PathItem<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.current_node {
            PathNode::Child(pi) => {
                self.current_node = pi.parent;
                Some(pi)
            }
            PathNode::Root => return None,
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FilterError;

    fn parsed<'a>(path: &'a str) -> Vec<Segment<'a>> {
        parse_path(path).unwrap()
    }

    fn is_invalid_criteria(result: Result<FilterCriteria, FilterError>) -> bool {
        matches!(result, Err(FilterError::InvalidCriteria(_)))
    }

    // -- Valid paths: check parsed segments --

    #[test]
    fn valid_simple_key() {
        assert_eq!(parsed("name"), vec![Segment::Key("name".into())]);
    }

    #[test]
    fn valid_nested_key() {
        assert_eq!(
            parsed("customer.name"),
            vec![Segment::Key("customer".into()), Segment::Key("name".into())]
        );
    }

    #[test]
    fn valid_wildcard_selector() {
        assert_eq!(
            parsed("items[*].price"),
            vec![
                Segment::Key("items".into()),
                Segment::All,
                Segment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn valid_index_selector() {
        assert_eq!(
            parsed("items[0].price"),
            vec![
                Segment::Key("items".into()),
                Segment::Index(0),
                Segment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn valid_open_slice() {
        assert_eq!(
            parsed("items[:3].price"),
            vec![
                Segment::Key("items".into()),
                Segment::Slice {
                    start: None,
                    end: Some(3)
                },
                Segment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn valid_closed_slice() {
        assert_eq!(
            parsed("items[1:3].price"),
            vec![
                Segment::Key("items".into()),
                Segment::Slice {
                    start: Some(1),
                    end: Some(3)
                },
                Segment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn valid_open_ended_slice() {
        assert_eq!(
            parsed("items[2:].price"),
            vec![
                Segment::Key("items".into()),
                Segment::Slice {
                    start: Some(2),
                    end: None
                },
                Segment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn valid_top_level_array_selector() {
        assert_eq!(
            parsed("[*].name"),
            vec![Segment::All, Segment::Key("name".into())]
        );
    }

    #[test]
    fn valid_slice_to_all() {
        assert_eq!(
            parsed("name[:]"),
            vec![Segment::Key("name".into()), Segment::All]
        );
    }

    // -- Invalid paths: empty segments --

    #[test]
    fn error_on_empty_path() {
        assert!(is_invalid_criteria(FilterCriteria::new(&[""])));
    }

    #[test]
    fn error_on_dot_only() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["."])));
    }

    #[test]
    fn error_on_leading_dot() {
        assert!(is_invalid_criteria(FilterCriteria::new(&[".name"])));
    }

    #[test]
    fn error_on_trailing_dot() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["name."])));
    }

    #[test]
    fn error_on_consecutive_dots() {
        assert!(is_invalid_criteria(FilterCriteria::new(&[
            "customer..name"
        ])));
    }

    // -- Invalid paths: malformed bracket selectors --

    #[test]
    fn error_on_empty_bracket_selector() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[]"])));
    }

    #[test]
    fn error_on_unclosed_bracket() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items["])));
    }

    #[test]
    fn error_on_non_numeric_bracket_selector() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[abc]"])));
    }

    #[test]
    fn error_on_invalid_slice_start() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[abc:2]"])));
    }

    #[test]
    fn error_on_invalid_slice_end() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[1:xyz]"])));
    }

    #[test]
    fn error_on_trailing_chars_after_bracket() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[0]extra"])));
    }

    #[test]
    fn error_on_trailing_close_brackets() {
        assert!(is_invalid_criteria(FilterCriteria::new(&["items[0]]]"])));
    }

    // -- Error propagates across all paths --

    #[test]
    fn error_reported_for_second_path_in_list() {
        assert!(is_invalid_criteria(FilterCriteria::new(&[
            "valid.path",
            "bad..path"
        ])));
    }
}
