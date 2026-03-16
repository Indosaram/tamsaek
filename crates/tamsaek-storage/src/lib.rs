mod async_db;
mod async_document;
mod db;
mod document;
mod error;
mod schema;
mod tantivy_fts;
mod vector_store;

pub use async_db::{AsyncDatabase, AsyncDatabaseConfig, SqlValue};
pub use async_document::AsyncDocumentStore;
pub use db::{Database, DatabaseConfig};
pub use document::{DocumentStore, FileMetadataInfo, PathPrefixSortBy, StoredDocument};
pub use error::{StorageError, StorageResult};
pub use schema::Schema;
pub use tantivy_fts::{check_rebuild_needed, IndexPolicy, RebuildDecision, TantivyFts, TantivySearchResult};
pub use vector_store::{SqliteVectorStore, VectorSearchResult, VectorStore};

// Re-export rusqlite for use by consumers
pub use rusqlite;
