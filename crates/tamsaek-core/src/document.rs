use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub content: String,
    pub path: Option<String>,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub source: String,
    pub external_id: Option<String>,
    pub mime_type: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub indexed_at: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Document {
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

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    pub fn with_size(mut self, size_bytes: i64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    pub fn with_modified_at(mut self, modified_at: DateTime<Utc>) -> Self {
        self.modified_at = Some(modified_at);
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn with_external_id(mut self, external_id: impl Into<String>) -> Self {
        self.external_id = Some(external_id.into());
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub fn with_indexed_at(mut self, indexed_at: DateTime<Utc>) -> Self {
        self.indexed_at = Some(indexed_at);
        self
    }

    pub fn with_content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = Some(content_hash.into());
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn compute_content_hash(&mut self) {
        self.content_hash = Some(blake3::hash(self.content.as_bytes()).to_hex().to_string());
    }
}
