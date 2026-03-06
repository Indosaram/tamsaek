//! Search result types.

use serde::{Deserialize, Serialize};

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Document ID
    pub id: String,

    /// Document title
    pub title: String,

    /// Search relevance score
    pub score: f32,

    /// Text snippet with search term highlighted
    pub snippet: Option<String>,

    /// File path (if applicable)
    pub path: Option<String>,

    /// File extension
    pub extension: Option<String>,

    /// File size in bytes
    pub size_bytes: Option<i64>,

    /// Last modification time (ISO 8601 string)
    pub modified_at: Option<String>,

    /// Document source
    pub source: Option<String>,
}
