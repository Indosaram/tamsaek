//! Error types for the tamsaek-core library.

use thiserror::Error;

/// The main error type for tamsaek operations.
#[derive(Error, Debug)]
pub enum TamsaekError {
    /// Error during index operations (create, open, commit, etc.)
    #[error("Index error: {0}")]
    Index(String),

    /// Error during search operations
    #[error("Search error: {0}")]
    Search(String),

    /// Invalid query syntax
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Error during document operations (add, delete, retrieve)
    #[error("Document error: {0}")]
    Document(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(feature = "metadata")]
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[cfg(feature = "query-dsl")]
    #[error("Query parse error: {0}")]
    Parse(String),

    #[cfg(feature = "vector")]
    #[error("Vector operation failed: {0}")]
    Vector(String),
}

/// A convenience Result type for tamsaek operations.
pub type Result<T> = std::result::Result<T, TamsaekError>;

impl From<tantivy::TantivyError> for TamsaekError {
    fn from(err: tantivy::TantivyError) -> Self {
        TamsaekError::Index(err.to_string())
    }
}

impl From<tantivy::query::QueryParserError> for TamsaekError {
    fn from(err: tantivy::query::QueryParserError) -> Self {
        TamsaekError::Search(err.to_string())
    }
}

impl From<tantivy::directory::error::OpenDirectoryError> for TamsaekError {
    fn from(err: tantivy::directory::error::OpenDirectoryError) -> Self {
        TamsaekError::Index(err.to_string())
    }
}
