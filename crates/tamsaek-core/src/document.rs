//! Document model for indexed content.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A document that can be indexed and searched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique identifier for this document
    pub id: String,

    /// Title of the document (e.g., filename)
    pub title: String,

    /// Full text content of the document
    pub content: String,

    /// File path (if applicable)
    pub path: Option<String>,

    /// File extension (e.g., "rs", "md", "txt")
    pub extension: Option<String>,

    /// File size in bytes
    pub size_bytes: Option<i64>,

    /// Last modification time
    pub modified_at: Option<DateTime<Utc>>,

    /// Source of the document (e.g., "local", "google_drive")
    pub source: String,

    /// External identifier for cloud sources (e.g., Google Drive file ID)
    pub external_id: Option<String>,

    /// MIME type of the document (e.g., "text/plain", "application/pdf")
    pub mime_type: Option<String>,

    /// Document author
    pub author: Option<String>,

    /// Document creation time
    pub created_at: Option<DateTime<Utc>>,

    /// When this document was indexed
    pub indexed_at: Option<DateTime<Utc>>,

    /// Blake3 hash of content for change detection
    pub content_hash: Option<String>,

    /// Arbitrary JSON metadata
    pub metadata: Option<serde_json::Value>,

    /// User-defined tags
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Document {
    /// Creates a new document with the required fields.
    ///
    /// All optional fields are initialized to `None` or empty defaults.
    /// Use builder methods to set additional fields.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            content: content.into(),
            path: None,
            extension: None,
            size_bytes: None,
            modified_at: None,
            source: "local".to_string(),
            external_id: None,
            mime_type: None,
            author: None,
            created_at: None,
            indexed_at: None,
            content_hash: None,
            metadata: None,
            tags: Vec::new(),
        }
    }

    /// Sets the file path.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Sets the file extension.
    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    /// Sets the file size in bytes.
    pub fn with_size(mut self, size_bytes: i64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    /// Sets the modification time.
    pub fn with_modified_at(mut self, modified_at: DateTime<Utc>) -> Self {
        self.modified_at = Some(modified_at);
        self
    }

    /// Sets the source.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Sets the external ID (e.g., Google Drive file ID).
    pub fn with_external_id(mut self, external_id: impl Into<String>) -> Self {
        self.external_id = Some(external_id.into());
        self
    }

    /// Sets the MIME type.
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Sets the document author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Sets the creation time.
    pub fn with_created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    /// Sets the indexed time.
    pub fn with_indexed_at(mut self, indexed_at: DateTime<Utc>) -> Self {
        self.indexed_at = Some(indexed_at);
        self
    }

    /// Sets the content hash directly.
    pub fn with_content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = Some(content_hash.into());
        self
    }

    /// Sets arbitrary JSON metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Sets user-defined tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Computes and sets the content hash using blake3.
    ///
    /// This method hashes the document's content and stores the result
    /// in the `content_hash` field for change detection.
    pub fn compute_content_hash(&mut self) {
        self.content_hash = Some(blake3::hash(self.content.as_bytes()).to_hex().to_string());
    }
}
