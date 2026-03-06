//! Index management MCP tools.
//!
//! This module contains:
//! - `index_directory`: Index files from a directory
//! - `remove_document`: Remove a document from the index
//! - `clear_index`: Clear all documents from the index

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request parameters for indexing a directory.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IndexDirectoryRequest {
    /// The directory path to index.
    #[schemars(description = "Absolute or relative path to the directory to index")]
    pub path: String,

    /// File extensions to include (e.g., ["rs", "md", "txt"]).
    #[schemars(
        description = "List of file extensions to include (e.g., ['rs', 'md']). If empty, indexes all files."
    )]
    pub extensions: Option<Vec<String>>,

    /// Whether to recursively index subdirectories.
    #[schemars(description = "Whether to recursively index subdirectories (default: true)")]
    pub recursive: Option<bool>,
}

/// Request parameters for removing a document.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RemoveDocumentRequest {
    /// The document ID to remove.
    #[schemars(description = "The unique identifier of the document to remove")]
    pub id: String,
}

/// Response from index directory operation.
#[derive(Debug, Clone, Serialize)]
pub struct IndexDirectoryResponse {
    /// Number of files successfully indexed.
    pub indexed: usize,
    /// Number of files that failed to index.
    pub failed: usize,
    /// Total number of files processed.
    pub total: usize,
    /// Details of any failures.
    pub errors: Vec<IndexError>,
}

/// Details of an indexing error.
#[derive(Debug, Clone, Serialize)]
pub struct IndexError {
    /// Path of the file that failed.
    pub path: String,
    /// Error message.
    pub error: String,
}

/// Response from remove document operation.
#[derive(Debug, Clone, Serialize)]
pub struct RemoveDocumentResponse {
    /// Whether the document was successfully removed.
    pub success: bool,
    /// The ID of the removed document.
    pub id: String,
}

/// Response from clear index operation.
#[derive(Debug, Clone, Serialize)]
pub struct ClearIndexResponse {
    /// Number of documents removed.
    pub removed: usize,
    /// Success message.
    pub message: String,
}
