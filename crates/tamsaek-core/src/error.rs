use thiserror::Error;

#[derive(Error, Debug)]
pub enum TamsaekError {
    #[error("Index error: {0}")]
    Index(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Document error: {0}")]
    Document(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Vector operation failed: {0}")]
    Vector(String),
}

pub type Result<T> = std::result::Result<T, TamsaekError>;

impl From<tamsaek_storage::StorageError> for TamsaekError {
    fn from(err: tamsaek_storage::StorageError) -> Self {
        match err {
            tamsaek_storage::StorageError::Database(e) => Self::Database(e.to_string()),
            tamsaek_storage::StorageError::InvalidQuery(e) => Self::InvalidQuery(e),
            tamsaek_storage::StorageError::Io(e) => Self::Io(e),
            tamsaek_storage::StorageError::Tantivy(e) => Self::Search(e),
            other => Self::Document(other.to_string()),
        }
    }
}

impl From<tamsaek_search_core::SearchError> for TamsaekError {
    fn from(err: tamsaek_search_core::SearchError) -> Self {
        match err {
            tamsaek_search_core::SearchError::Storage(e) => Self::from(e),
            tamsaek_search_core::SearchError::Parse(e) => Self::Parse(e.to_string()),
            tamsaek_search_core::SearchError::InvalidQuery(e) => Self::InvalidQuery(e),
            tamsaek_search_core::SearchError::NotFound(e) => Self::Document(e),
            tamsaek_search_core::SearchError::Embedding(e) => Self::Vector(e),
            tamsaek_search_core::SearchError::Index(e) => Self::Index(e),
        }
    }
}
