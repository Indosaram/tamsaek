use crate::document::{Document, DocumentId};
use crate::error::SearchResult;
use crate::query::Query;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;

/// A single search result hit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub document_id: DocumentId,
    pub score: f32,
    pub title: String,
    pub snippet: Option<String>,
    pub highlights: HashMap<String, Vec<Range<usize>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_breakdown: Option<ScoreBreakdown>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl SearchHit {
    pub fn new(document_id: DocumentId, score: f32, title: impl Into<String>) -> Self {
        Self {
            document_id,
            score,
            title: title.into(),
            snippet: None,
            highlights: HashMap::new(),
            score_breakdown: None,
            modified_at: None,
        }
    }

    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }

    pub fn with_highlights(mut self, field: impl Into<String>, ranges: Vec<Range<usize>>) -> Self {
        self.highlights.insert(field.into(), ranges);
        self
    }

    pub fn with_breakdown(mut self, breakdown: ScoreBreakdown) -> Self {
        self.score_breakdown = Some(breakdown);
        self
    }

    pub fn with_modified_at(mut self, modified_at: chrono::DateTime<chrono::Utc>) -> Self {
        self.modified_at = Some(modified_at);
        self
    }
}

/// Breakdown of score components for transparency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub keyword_score: f32,
    pub semantic_score: f32,
    pub filename_bonus: f32,
    pub recency_boost: f32,
    pub source_boost: f32,
}

impl Default for ScoreBreakdown {
    fn default() -> Self {
        Self {
            keyword_score: 0.0,
            semantic_score: 0.0,
            filename_bonus: 0.0,
            recency_boost: 0.0,
            source_boost: 0.0,
        }
    }
}

/// Collection of search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub total_count: u64,
    pub took_ms: u64,
    pub query_type: QueryType,
}

impl SearchResults {
    pub fn empty() -> Self {
        Self {
            hits: Vec::new(),
            total_count: 0,
            took_ms: 0,
            query_type: QueryType::Keyword,
        }
    }

    pub fn new(hits: Vec<SearchHit>, total_count: u64, took_ms: u64) -> Self {
        Self {
            hits,
            total_count,
            took_ms,
            query_type: QueryType::Keyword,
        }
    }

    pub fn with_query_type(mut self, query_type: QueryType) -> Self {
        self.query_type = query_type;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryType {
    Keyword,
    Regex,
    Semantic,
    Hybrid,
}

/// Result of bulk indexing operation
#[derive(Debug, Clone)]
pub struct BulkIndexResult {
    pub indexed: usize,
    pub failed: usize,
    pub errors: Vec<(DocumentId, String)>,
}

impl BulkIndexResult {
    pub fn success(indexed: usize) -> Self {
        Self {
            indexed,
            failed: 0,
            errors: Vec::new(),
        }
    }
}

/// Statistics about the search index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub document_count: u64,
    pub index_size_bytes: u64,
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
}

/// Core search backend trait
#[async_trait]
pub trait SearchBackend: Send + Sync {
    /// Backend identifier
    fn id(&self) -> &str;

    /// Index a single document
    async fn index(&self, doc: &Document) -> SearchResult<()>;

    /// Bulk index multiple documents
    async fn bulk_index(&self, docs: &[Document]) -> SearchResult<BulkIndexResult>;

    /// Remove a document from the index
    async fn remove(&self, id: &DocumentId) -> SearchResult<bool>;

    /// Search with a parsed query
    async fn search(
        &self,
        query: &Query,
        limit: usize,
        offset: usize,
    ) -> SearchResult<SearchResults>;

    /// Get index statistics
    async fn stats(&self) -> SearchResult<IndexStats>;

    /// Clear all indexed data
    async fn clear(&self) -> SearchResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::SourceType;

    #[test]
    fn test_search_hit_builder() {
        let id = DocumentId::new(SourceType::Local, "test.txt");
        let hit = SearchHit::new(id.clone(), 0.95, "Test Document")
            .with_snippet("This is a test...")
            .with_highlights("content", vec![0..4, 10..14]);

        assert_eq!(hit.score, 0.95);
        assert_eq!(hit.title, "Test Document");
        assert!(hit.snippet.is_some());
        assert!(hit.highlights.contains_key("content"));
    }

    #[test]
    fn test_search_results() {
        let results = SearchResults::empty();
        assert!(results.is_empty());
        assert_eq!(results.len(), 0);
    }
}
