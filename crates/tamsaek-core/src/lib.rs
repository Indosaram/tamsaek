mod document;
mod error;
mod index;
mod search;

#[cfg(feature = "metadata")]
pub mod metadata;

#[cfg(feature = "scoring")]
pub mod scoring;

#[cfg(feature = "query-dsl")]
pub mod query;

#[cfg(feature = "vector")]
pub mod vector;

pub use document::Document;
pub use error::{Result, TamsaekError};
pub use index::{SchemaFields, TamsaekIndex};
pub use search::SearchResult;

pub use tamsaek_search_core::{
    Document as SearchDocument, DocumentId, DocumentMetadata, ParseError, QueryType,
    ScoreBreakdown, SearchBackend, SearchError, SearchHit, SearchResults, SourceType,
};
pub use tamsaek_storage::{
    check_rebuild_needed, Database, DatabaseConfig, DocumentStore, FileMetadataInfo, IndexPolicy,
    PathPrefixSortBy, RebuildDecision, Schema, SqliteVectorStore, StorageError, StorageResult,
    StoredDocument, TantivyFts, TantivySearchResult, VectorSearchResult, VectorStore,
};

pub use tamsaek_storage::rusqlite;

pub mod storage {
    pub use tamsaek_storage::*;
}

pub mod search_core {
    pub use tamsaek_search_core::*;
}
