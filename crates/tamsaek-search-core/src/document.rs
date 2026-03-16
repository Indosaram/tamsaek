use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId {
    pub source: SourceType,
    pub external_id: String,
}

impl DocumentId {
    pub fn new(source: SourceType, external_id: impl Into<String>) -> Self {
        Self {
            source,
            external_id: external_id.into(),
        }
    }

    pub fn to_storage_id(&self) -> String {
        format!("{}|{}", self.source.as_str(), self.external_id)
    }

    pub fn from_storage_id(id: &str) -> Option<Self> {
        let parts: Vec<&str> = id.splitn(2, '|').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Self {
            source: SourceType::parse(parts[0])?,
            external_id: parts[1].to_string(),
        })
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}|{}", self.source.as_str(), self.external_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Local,
    GoogleDrive,
    SharePoint,
    OneDrive,
    Dropbox,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::GoogleDrive => "googledrive",
            Self::SharePoint => "sharepoint",
            Self::OneDrive => "onedrive",
            Self::Dropbox => "dropbox",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "local" => Some(Self::Local),
            "googledrive" | "gdrive" | "drive" | "google" => Some(Self::GoogleDrive),
            "sharepoint" | "sp" => Some(Self::SharePoint),
            "onedrive" | "od" => Some(Self::OneDrive),
            "dropbox" | "db" => Some(Self::Dropbox),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::GoogleDrive => "Google Drive",
            Self::SharePoint => "SharePoint",
            Self::OneDrive => "OneDrive",
            Self::Dropbox => "Dropbox",
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub created_at: Option<DateTime<Utc>>,
    pub modified_at: Option<DateTime<Utc>>,
    pub author: Option<String>,
    pub path: Option<String>,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub web_url: Option<String>,
    #[serde(default)]
    pub custom: HashMap<String, serde_json::Value>,
}

impl DocumentMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size_bytes = Some(size);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_dates(
        mut self,
        created: Option<DateTime<Utc>>,
        modified: Option<DateTime<Utc>>,
    ) -> Self {
        self.created_at = created;
        self.modified_at = modified;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub title: String,
    pub content: String,
    pub metadata: DocumentMetadata,
}

impl Document {
    pub fn new(id: DocumentId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            content: String::new(),
            metadata: DocumentMetadata::default(),
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_metadata(mut self, metadata: DocumentMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn storage_id(&self) -> String {
        self.id.to_storage_id()
    }

    pub fn to_stored(&self) -> tamsaek_storage::StoredDocument {
        let mut stored = tamsaek_storage::StoredDocument::new(
            self.id.source.as_str(),
            &self.id.external_id,
            &self.title,
        )
        .with_tags(self.metadata.tags.clone());

        if let Some(ref mime_type) = self.metadata.mime_type {
            stored = stored.with_mime_type(mime_type);
        }
        if let Some(ref path) = self.metadata.path {
            stored = stored.with_path(path);
        }
        if let Some(ref author) = self.metadata.author {
            stored = stored.with_author(author);
        }

        stored.size_bytes = self.metadata.size_bytes.map(|s| s as i64);
        stored.created_at = self.metadata.created_at;
        stored.modified_at = self.metadata.modified_at;

        stored
    }

    pub fn from_stored(stored: tamsaek_storage::StoredDocument) -> Option<Self> {
        let id = DocumentId::from_storage_id(&stored.id)?;

        let metadata = DocumentMetadata {
            created_at: stored.created_at,
            modified_at: stored.modified_at,
            author: stored.author,
            path: stored.path,
            size_bytes: stored.size_bytes.map(|s| s as u64),
            mime_type: stored.mime_type,
            tags: stored.tags,
            web_url: None,
            custom: HashMap::new(),
        };

        Some(Self {
            id,
            title: stored.title,
            content: stored.content.unwrap_or_default(),
            metadata,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Text,
    Markdown,
    Code,
    Pdf,
    Document,     // DOCX, DOC
    Spreadsheet,  // XLSX, XLS
    Presentation, // PPTX, PPT
    Image,
    Audio,
    Video,
    Archive,
    Other,
}

impl FileType {
    pub fn from_mime_type(mime: &str) -> Self {
        match mime.to_lowercase().as_str() {
            "text/plain" => Self::Text,
            "text/markdown" | "text/x-markdown" => Self::Markdown,
            "application/pdf" => Self::Pdf,
            "application/msword"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Self::Document
            }
            "application/vnd.ms-excel"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                Self::Spreadsheet
            }
            "application/vnd.ms-powerpoint"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                Self::Presentation
            }
            m if m.starts_with("text/x-") || m.starts_with("application/x-") => Self::Code,
            m if m.starts_with("image/") => Self::Image,
            m if m.starts_with("audio/") => Self::Audio,
            m if m.starts_with("video/") => Self::Video,
            "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed" => Self::Archive,
            _ => Self::Other,
        }
    }

    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" => Self::Text,
            "md" | "markdown" => Self::Markdown,
            "pdf" => Self::Pdf,
            "doc" | "docx" | "odt" | "rtf" => Self::Document,
            "xls" | "xlsx" | "ods" | "csv" => Self::Spreadsheet,
            "ppt" | "pptx" | "odp" => Self::Presentation,
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" => Self::Image,
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => Self::Audio,
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "webm" => Self::Video,
            "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" => Self::Archive,
            "rs" | "py" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "h" | "hpp" | "cs" | "rb"
            | "php" | "swift" | "kt" | "scala" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "sql"
            | "html" | "css" | "scss" | "sass" | "less" | "json" | "yaml" | "yml" | "toml"
            | "xml" | "ini" | "conf" | "cfg" => Self::Code,
            _ => Self::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Code => "code",
            Self::Pdf => "pdf",
            Self::Document => "document",
            Self::Spreadsheet => "spreadsheet",
            Self::Presentation => "presentation",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Archive => "archive",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_id_roundtrip() {
        let id = DocumentId::new(SourceType::GoogleDrive, "abc123");
        let storage_id = id.to_storage_id();
        assert_eq!(storage_id, "googledrive|abc123");

        let restored = DocumentId::from_storage_id(&storage_id).unwrap();
        assert_eq!(restored, id);
    }

    #[test]
    fn test_source_type_parse() {
        assert_eq!(SourceType::parse("local"), Some(SourceType::Local));
        assert_eq!(
            SourceType::parse("googledrive"),
            Some(SourceType::GoogleDrive)
        );
        assert_eq!(
            SourceType::parse("sharepoint"),
            Some(SourceType::SharePoint)
        );
        assert_eq!(SourceType::parse("onedrive"), Some(SourceType::OneDrive));
        assert_eq!(SourceType::parse("dropbox"), Some(SourceType::Dropbox));
        assert_eq!(SourceType::parse("invalid"), None);
    }

    #[test]
    fn test_file_type_detection() {
        assert_eq!(FileType::from_extension("rs"), FileType::Code);
        assert_eq!(FileType::from_extension("pdf"), FileType::Pdf);
        assert_eq!(FileType::from_extension("docx"), FileType::Document);
        assert_eq!(FileType::from_extension("xlsx"), FileType::Spreadsheet);
        assert_eq!(FileType::from_extension("jpg"), FileType::Image);
    }
}
