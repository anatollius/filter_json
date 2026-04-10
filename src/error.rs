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
