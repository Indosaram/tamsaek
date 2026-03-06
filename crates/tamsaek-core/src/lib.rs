//! Tamsaek Core - Full-text search library built on Tantivy.
//!
//! This crate provides a high-level API for indexing and searching documents
//! using the Tantivy search engine.
//!
//! # Example
//!
//! ```rust
//! use tamsaek_core::{TamsaekIndex, Document};
//!
//! # fn main() -> Result<(), tamsaek_core::TamsaekError> {
//! // Create an in-memory index
//! let index = TamsaekIndex::in_memory()?;
//!
//! // Add a document
//! let doc = Document::new("1", "Hello World", "This is the content of my document.");
//! index.add_document(&doc)?;
//! index.commit()?;
//!
//! // Search for documents
//! let results = index.search("content", 10)?;
//! println!("Found {} results", results.len());
//! # Ok(())
//! # }
//! ```

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

#[cfg(feature = "metadata")]
pub use metadata::{
    Database, DatabaseConfig, DocumentStore, FileMetadataInfo, Schema, StoredDocument,
};

#[cfg(feature = "scoring")]
pub use scoring::{BonusConfig, BonusScorer, RRFScorer, Scorable};

#[cfg(feature = "query-dsl")]
pub use query::{DateOp, Filter, Query, QueryParser, SizeOp};

#[cfg(feature = "vector")]
pub use vector::{SqliteVectorStore, VectorSearchResult, VectorStore, EMBEDDING_DIM};
