//! Search-related MCP tools.
//!
//! This module contains:
//! - `search`: Full-text search across indexed documents
//! - `search_regex`: Regex-based pattern search
//! - `filter`: Filter documents by extension or source

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request parameters for full-text search.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchRequest {
    /// The search query string.
    #[schemars(description = "The search query to find in indexed documents")]
    pub query: String,

    /// Maximum number of results to return.
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,
}

/// Request parameters for regex search.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchRegexRequest {
    /// The regex pattern to search for.
    #[schemars(description = "Regular expression pattern to search for")]
    pub pattern: String,

    /// Maximum number of results to return.
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,
}

/// Request parameters for filtering documents.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FilterRequest {
    /// Filter by file extension (e.g., "rs", "md").
    #[schemars(description = "File extension to filter by (e.g., 'rs', 'md')")]
    pub extension: Option<String>,

    /// Filter by source/origin.
    #[schemars(description = "Source or origin to filter by")]
    pub source: Option<String>,

    /// Maximum number of results to return.
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,
}

/// Response containing search results.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    /// Total number of results found.
    pub total: usize,
    /// The search results.
    pub results: Vec<SearchResultItem>,
}

/// A single search result item.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultItem {
    /// Document ID.
    pub id: String,
    /// Document title or filename.
    pub title: String,
    /// Document path.
    pub path: String,
    /// Relevance score.
    pub score: f32,
    /// Snippet showing the match context.
    pub snippet: Option<String>,
}
