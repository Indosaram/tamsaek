//! Statistics and document retrieval MCP tools.
//!
//! This module contains:
//! - `get_stats`: Get index statistics
//! - `get_document`: Get a specific document by ID

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request parameters for getting a document.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetDocumentRequest {
    /// The document ID to retrieve.
    #[schemars(description = "The unique identifier of the document to retrieve")]
    pub id: String,
}

/// Response containing index statistics.
#[derive(Debug, Clone, Serialize)]
pub struct StatsResponse {
    /// Total number of indexed documents.
    pub document_count: usize,
    /// Total size of all indexed content in bytes.
    pub total_size_bytes: u64,
    /// Index creation timestamp.
    pub created_at: Option<String>,
    /// Last update timestamp.
    pub updated_at: Option<String>,
    /// Breakdown by file extension.
    pub by_extension: Vec<ExtensionStats>,
}

/// Statistics for a specific file extension.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionStats {
    /// The file extension.
    pub extension: String,
    /// Number of documents with this extension.
    pub count: usize,
    /// Total size in bytes.
    pub size_bytes: u64,
}

/// Response containing a single document.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    /// Document ID.
    pub id: String,
    /// Document title or filename.
    pub title: String,
    /// Document path.
    pub path: String,
    /// Document content.
    pub content: String,
    /// File extension.
    pub extension: Option<String>,
    /// Document source.
    pub source: Option<String>,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Last modified timestamp.
    pub modified_at: Option<String>,
}
