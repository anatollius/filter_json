// ─── Segment ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Segment {
    Key(String),
    Index(usize),
    All,
    Slice {
        start: Option<usize>,
        end: Option<usize>,
    },
}

fn parse_selector(s: &str) -> Segment {
    if s == "*" {
        return Segment::All;
    }
    if let Ok(n) = s.parse::<usize>() {
        return Segment::Index(n);
    }
    if let Some(colon_pos) = s.find(':') {
        let start_str = &s[..colon_pos];
        let end_str = &s[colon_pos + 1..];
        let start = if start_str.is_empty() {
            None
        } else {
            start_str.parse::<usize>().ok()
        };
        let end = if end_str.is_empty() {
            None
        } else {
            end_str.parse::<usize>().ok()
        };
        return Segment::Slice { start, end };
    }
    Segment::Key(s.to_string())
}

pub(crate) fn parse_path(path: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for dot_segment in path.split('.') {
        if dot_segment.is_empty() {
            continue;
        }
        if let Some(bracket_pos) = dot_segment.find('[') {
            let key_part = &dot_segment[..bracket_pos];
            if !key_part.is_empty() {
                segments.push(Segment::Key(key_part.to_string()));
            }
            let mut remaining = &dot_segment[bracket_pos..];
            while remaining.starts_with('[') {
                if let Some(close) = remaining.find(']') {
                    let selector = &remaining[1..close];
                    segments.push(parse_selector(selector));
                    remaining = &remaining[close + 1..];
                } else {
                    break;
                }
            }
        } else {
            segments.push(Segment::Key(dot_segment.to_string()));
        }
    }
    segments
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
/// let c = FilterCriteria::new(&["customer.name"]);
/// let c2 = FilterCriteria::new(&["items[*].price"]);
/// ```
pub struct FilterCriteria {
    pub(crate) paths: Vec<Vec<Segment>>,
}

impl FilterCriteria {
    pub fn new(paths: &[&str]) -> Self {
        Self {
            paths: paths.iter().map(|p| parse_path(p)).collect(),
        }
    }
}

impl<'a> From<Vec<&'a str>> for FilterCriteria {
    fn from(paths: Vec<&'a str>) -> Self {
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

pub(crate) fn criterion_matches_path(criterion: &[Segment], path: &[Segment]) -> bool {
    criterion.len() == path.len()
        && criterion
            .iter()
            .zip(path.iter())
            .all(|(c, r)| segment_matches(c, r))
}

pub(crate) fn criterion_is_prefix_of(criterion: &[Segment], path: &[Segment]) -> bool {
    criterion.len() > path.len()
        && criterion[..path.len()]
            .iter()
            .zip(path.iter())
            .all(|(c, r)| segment_matches(c, r))
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

pub(crate) fn inclusion_status(path: &[Segment], criteria: &FilterCriteria) -> InclusionStatus {
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

pub(crate) fn exclusion_status(path: &[Segment], criteria: &FilterCriteria) -> InclusionStatus {
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
