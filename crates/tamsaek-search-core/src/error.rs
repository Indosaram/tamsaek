use thiserror::Error;

pub type SearchResult<T> = Result<T, SearchError>;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Storage error: {0}")]
    Storage(#[from] tamsaek_storage::StorageError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Document not found: {0}")]
    NotFound(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Index error: {0}")]
    Index(String),
}

#[derive(Error, Debug, Clone)]
pub enum ParseError {
    #[error("Unexpected token at position {position}: {message}")]
    UnexpectedToken { position: usize, message: String },

    #[error("Unclosed quote at position {position}")]
    UnclosedQuote { position: usize },

    #[error("Invalid filter: {filter}")]
    InvalidFilter { filter: String },

    #[error("Invalid date expression: {expr}")]
    InvalidDate { expr: String },

    #[error("Invalid regex: {pattern}")]
    InvalidRegex { pattern: String },

    #[error("Empty query")]
    EmptyQuery,
}
