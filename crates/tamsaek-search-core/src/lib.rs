pub mod ai;
pub mod document;
pub mod error;
pub mod index;
pub mod pipeline;
pub mod query;
pub mod scoring;

pub use document::{Document, DocumentId, DocumentMetadata, SourceType};
pub use error::{ParseError, SearchError, SearchResult};
pub use index::{QueryType, ScoreBreakdown, SearchBackend, SearchHit, SearchResults};
pub use query::{Query, QueryParser};
