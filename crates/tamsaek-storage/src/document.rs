use crate::db::Database;
use crate::error::{StorageError, StorageResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// File metadata for incremental indexing change detection
#[derive(Debug, Clone)]
pub struct FileMetadataInfo {
    pub content_hash: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDocument {
    pub id: String,
    pub source: String,
    pub external_id: String,
    pub title: String,
    pub content: Option<String>,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub author: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub modified_at: Option<DateTime<Utc>>,
    pub indexed_at: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub tags: Vec<String>,
}

impl StoredDocument {
    pub fn new(
        source: impl Into<String>,
        external_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        let source = source.into();
        let external_id = external_id.into();
        Self {
            id: format!("{}|{}", source, external_id),
            source,
            external_id,
            title: title.into(),
            content: None,
            mime_type: None,
            path: None,
            author: None,
            size_bytes: None,
            created_at: None,
            modified_at: None,
            indexed_at: None,
            content_hash: None,
            metadata: None,
            tags: Vec::new(),
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn compute_content_hash(&mut self) {
        if let Some(ref content) = self.content {
            self.content_hash = Some(blake3::hash(content.as_bytes()).to_hex().to_string());
        }
    }
}

pub struct DocumentStore {
    db: Database,
}

impl DocumentStore {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, doc: &StoredDocument) -> StorageResult<()> {
        debug!("Inserting document: {}", doc.id);

        self.db.execute(
            r#"
            INSERT INTO documents (
                id, source, external_id, title, content, mime_type, path,
                author, size_bytes, created_at, modified_at, content_hash, metadata
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            &[
                &doc.id as &dyn rusqlite::ToSql,
                &doc.source,
                &doc.external_id,
                &doc.title,
                &doc.content,
                &doc.mime_type,
                &doc.path,
                &doc.author,
                &doc.size_bytes,
                &doc.created_at.map(|dt| dt.to_rfc3339()),
                &doc.modified_at.map(|dt| dt.to_rfc3339()),
                &doc.content_hash,
                &doc.metadata.as_ref().map(|m| m.to_string()),
            ],
        )?;

        // Insert tags
        for tag in &doc.tags {
            self.db.execute(
                "INSERT INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                &[&doc.id as &dyn rusqlite::ToSql, tag],
            )?;
        }

        Ok(())
    }

    pub fn upsert(&self, doc: &StoredDocument) -> StorageResult<()> {
        debug!("Upserting document: {}", doc.id);

        self.db.execute(
            r#"
            INSERT INTO documents (
                id, source, external_id, title, content, mime_type, path,
                author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'), ?12, ?13)
            ON CONFLICT (id) DO UPDATE SET
                title = excluded.title,
                content = excluded.content,
                mime_type = excluded.mime_type,
                path = excluded.path,
                author = excluded.author,
                size_bytes = excluded.size_bytes,
                created_at = excluded.created_at,
                modified_at = excluded.modified_at,
                indexed_at = datetime('now'),
                content_hash = excluded.content_hash,
                metadata = excluded.metadata
            "#,
            &[
                &doc.id as &dyn rusqlite::ToSql,
                &doc.source,
                &doc.external_id,
                &doc.title,
                &doc.content,
                &doc.mime_type,
                &doc.path,
                &doc.author,
                &doc.size_bytes,
                &doc.created_at.map(|dt| dt.to_rfc3339()),
                &doc.modified_at.map(|dt| dt.to_rfc3339()),
                &doc.content_hash,
                &doc.metadata.as_ref().map(|m| m.to_string()),
            ],
        )?;

        // Update tags
        self.db.execute(
            "DELETE FROM document_tags WHERE document_id = ?1",
            &[&doc.id as &dyn rusqlite::ToSql],
        )?;

        for tag in &doc.tags {
            self.db.execute(
                "INSERT INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                &[&doc.id as &dyn rusqlite::ToSql, tag],
            )?;
        }

        Ok(())
    }

    pub fn get(&self, id: &str) -> StorageResult<Option<StoredDocument>> {
        let doc = self.db.query_one(
            r#"
            SELECT id, source, external_id, title, content, mime_type, path,
                   author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
            FROM documents
            WHERE id = ?1
            "#,
            &[&id as &dyn rusqlite::ToSql],
            |row| self.row_to_document(row),
        )?;

        if let Some(mut doc) = doc {
            doc.tags = self.get_tags(&doc.id)?;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_source(
        &self,
        source: &str,
        external_id: &str,
    ) -> StorageResult<Option<StoredDocument>> {
        let id = format!("{}|{}", source, external_id);
        self.get(&id)
    }

    pub fn delete(&self, id: &str) -> StorageResult<bool> {
        debug!("Deleting document: {}", id);
        let count = self.db.execute(
            "DELETE FROM documents WHERE id = ?1",
            &[&id as &dyn rusqlite::ToSql],
        )?;
        Ok(count > 0)
    }

    pub fn list(
        &self,
        source: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> StorageResult<Vec<StoredDocument>> {
        let limit_val = limit as i64;
        let offset_val = offset as i64;

        let docs = if let Some(src) = source {
            self.db.query(
                r#"
                SELECT id, source, external_id, title, content, mime_type, path,
                       author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
                FROM documents
                WHERE source = ?1
                ORDER BY modified_at DESC
                LIMIT ?2 OFFSET ?3
                "#,
                &[&src as &dyn rusqlite::ToSql, &limit_val, &offset_val],
                |row| self.row_to_document(row),
            )?
        } else {
            self.db.query(
                r#"
                SELECT id, source, external_id, title, content, mime_type, path,
                       author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
                FROM documents
                ORDER BY modified_at DESC
                LIMIT ?1 OFFSET ?2
                "#,
                &[&limit_val as &dyn rusqlite::ToSql, &offset_val],
                |row| self.row_to_document(row),
            )?
        };

        // Load tags for each document
        let mut result = Vec::with_capacity(docs.len());
        for mut doc in docs {
            doc.tags = self.get_tags(&doc.id)?;
            result.push(doc);
        }

        Ok(result)
    }

    pub fn count(&self, source: Option<&str>) -> StorageResult<i64> {
        if let Some(src) = source {
            self.db
                .query_one(
                    "SELECT COUNT(*) FROM documents WHERE source = ?1",
                    &[&src as &dyn rusqlite::ToSql],
                    |row| Ok(row.get::<_, i64>(0)?),
                )?
                .ok_or_else(|| StorageError::InvalidQuery("Count query failed".to_string()))
        } else {
            self.db
                .query_one("SELECT COUNT(*) FROM documents", &[], |row| {
                    Ok(row.get::<_, i64>(0)?)
                })?
                .ok_or_else(|| StorageError::InvalidQuery("Count query failed".to_string()))
        }
    }

    /// List only id, title, and content for FTS migration (memory efficient)
    pub fn list_for_fts_migration(
        &self,
        limit: usize,
        offset: usize,
    ) -> StorageResult<Vec<(String, String, Option<String>)>> {
        let limit_val = limit as i64;
        let offset_val = offset as i64;

        self.db.query(
            r#"
            SELECT id, title, content
            FROM documents
            ORDER BY id
            LIMIT ?1 OFFSET ?2
            "#,
            &[&limit_val as &dyn rusqlite::ToSql, &offset_val],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
    }

    pub fn exists(&self, id: &str) -> StorageResult<bool> {
        let count: Option<i64> = self.db.query_one(
            "SELECT 1 FROM documents WHERE id = ?1",
            &[&id as &dyn rusqlite::ToSql],
            |row| Ok(row.get(0)?),
        )?;
        Ok(count.is_some())
    }

    pub fn get_by_content_hash(&self, hash: &str) -> StorageResult<Option<StoredDocument>> {
        let doc = self.db.query_one(
            r#"
            SELECT id, source, external_id, title, content, mime_type, path,
                   author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
            FROM documents
            WHERE content_hash = ?1
            "#,
            &[&hash as &dyn rusqlite::ToSql],
            |row| self.row_to_document(row),
        )?;

        if let Some(mut doc) = doc {
            doc.tags = self.get_tags(&doc.id)?;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Get document metadata by path (for incremental indexing)
    /// Returns (content_hash, size_bytes, modified_at) if document exists
    pub fn get_file_metadata_by_path(&self, path: &str) -> StorageResult<Option<FileMetadataInfo>> {
        self.db.query_one(
            r#"
            SELECT content_hash, size_bytes, modified_at
            FROM documents
            WHERE path = ?1
            "#,
            &[&path as &dyn rusqlite::ToSql],
            |row| {
                Ok(FileMetadataInfo {
                    content_hash: row.get(0)?,
                    size_bytes: row.get(1)?,
                    modified_at: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            },
        )
    }

    /// Batch get file metadata by paths (for efficient incremental indexing)
    pub fn get_file_metadata_batch(
        &self,
        paths: &[&str],
    ) -> StorageResult<std::collections::HashMap<String, FileMetadataInfo>> {
        if paths.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = (1..=paths.len()).map(|i| format!("?{}", i)).collect();
        let query = format!(
            r#"
            SELECT path, content_hash, size_bytes, modified_at
            FROM documents
            WHERE path IN ({})
            "#,
            placeholders.join(", ")
        );

        let params: Vec<&dyn rusqlite::ToSql> =
            paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

        let results = self.db.query(&query, &params, |row| {
            let path: String = row.get(0)?;
            let info = FileMetadataInfo {
                content_hash: row.get(1)?,
                size_bytes: row.get(2)?,
                modified_at: row
                    .get::<_, Option<String>>(3)?
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
            };
            Ok((path, info))
        })?;

        Ok(results.into_iter().collect())
    }

    /// List documents by path prefix with optional filtering and sorting.
    /// This is used for deterministic folder queries (e.g., Downloads folder).
    /// Returns documents with metadata needed for search results.
    pub fn list_by_path_prefix(
        &self,
        path_prefix: &str,
        source: Option<&str>,
        extensions: Option<&[&str]>,
        modified_after: Option<DateTime<Utc>>,
        modified_before: Option<DateTime<Utc>>,
        sort_by: PathPrefixSortBy,
        sort_ascending: bool,
        limit: usize,
    ) -> StorageResult<Vec<StoredDocument>> {
        debug!(
            "Listing documents by path prefix: {}, sort: {:?}, asc: {}",
            path_prefix, sort_by, sort_ascending
        );

        let limit_val = limit as i64;
        let mut param_idx = 2; // Start at 2 because path_prefix is ?1

        // Build conditions dynamically
        let mut conditions = vec!["path LIKE ?1".to_string()];

        // Source filter
        let source_condition = source.map(|_| {
            let cond = format!("source = ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(ref cond) = source_condition {
            conditions.push(cond.clone());
        }

        // Extension filter (case-insensitive) - use OR with LOWER(path) LIKE '%.ext'
        let ext_conditions: Vec<String> = extensions
            .map(|exts| {
                exts.iter()
                    .map(|_| {
                        let cond = format!("LOWER(path) LIKE '%.' || ?{}", param_idx);
                        param_idx += 1;
                        cond
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !ext_conditions.is_empty() {
            conditions.push(format!("({})", ext_conditions.join(" OR ")));
        }

        // Date range filters
        let modified_after_condition = modified_after.map(|_| {
            let cond = format!("modified_at >= ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(ref cond) = modified_after_condition {
            conditions.push(cond.clone());
        }

        let modified_before_condition = modified_before.map(|_| {
            let cond = format!("modified_at <= ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(ref cond) = modified_before_condition {
            conditions.push(cond.clone());
        }

        // Build ORDER BY clause
        let order_by = match sort_by {
            PathPrefixSortBy::Size => format!(
                "size_bytes {}",
                if sort_ascending { "ASC" } else { "DESC" }
            ),
            PathPrefixSortBy::ModifiedAt => format!(
                "modified_at {}",
                if sort_ascending { "ASC" } else { "DESC" }
            ),
            PathPrefixSortBy::Title => format!(
                "title {}",
                if sort_ascending { "ASC" } else { "DESC" }
            ),
        };

        // Build final query
        let query = format!(
            r#"
            SELECT id, source, external_id, title, content, mime_type, path,
                   author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
            FROM documents
            WHERE {}
            ORDER BY {}
            LIMIT ?{}
            "#,
            conditions.join(" AND "),
            order_by,
            param_idx
        );

        // Build params array - must keep values alive until query execution
        let modified_after_str = modified_after.map(|d| d.to_rfc3339());
        let modified_before_str = modified_before.map(|d| d.to_rfc3339());

        // Build dynamic params using a boxed approach
        // Note: path_prefix should already end with '/' or other separator for proper prefix matching
        let like_pattern = format!("{}%", path_prefix);
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        params.push(Box::new(like_pattern));
        
        if let Some(src) = source {
            params.push(Box::new(src.to_string()));
        }
        
        if let Some(exts) = extensions {
            for ext in exts {
                params.push(Box::new(ext.to_lowercase()));
            }
        }
        
        if let Some(ref after_str) = modified_after_str {
            params.push(Box::new(after_str.clone()));
        }
        
        if let Some(ref before_str) = modified_before_str {
            params.push(Box::new(before_str.clone()));
        }
        
        params.push(Box::new(limit_val));

        // Convert to refs for query
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let docs = self.db.query(&query, &param_refs, |row| self.row_to_document(row))?;

        // Load tags for each document
        let mut result = Vec::with_capacity(docs.len());
        for mut doc in docs {
            doc.tags = self.get_tags(&doc.id)?;
            result.push(doc);
        }

        Ok(result)
    }

    fn get_tags(&self, document_id: &str) -> StorageResult<Vec<String>> {
        self.db.query(
            "SELECT tag FROM document_tags WHERE document_id = ?1 ORDER BY tag",
            &[&document_id as &dyn rusqlite::ToSql],
            |row| Ok(row.get(0)?),
        )
    }

    fn row_to_document(&self, row: &rusqlite::Row<'_>) -> StorageResult<StoredDocument> {
        Ok(StoredDocument {
            id: row.get(0)?,
            source: row.get(1)?,
            external_id: row.get(2)?,
            title: row.get(3)?,
            content: row.get(4)?,
            mime_type: row.get(5)?,
            path: row.get(6)?,
            author: row.get(7)?,
            size_bytes: row.get(8)?,
            created_at: row
                .get::<_, Option<String>>(9)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            modified_at: row
                .get::<_, Option<String>>(10)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            indexed_at: row
                .get::<_, Option<String>>(11)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            content_hash: row.get(12)?,
            metadata: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            tags: Vec::new(), // Loaded separately
        })
    }
}

/// Sort options for path prefix queries
#[derive(Debug, Clone, Copy)]
pub enum PathPrefixSortBy {
    Size,
    ModifiedAt,
    Title,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DatabaseConfig;

    fn setup() -> DocumentStore {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().unwrap();
        DocumentStore::new(db)
    }

    #[test]
    fn test_insert_and_get() {
        let store = setup();

        let doc = StoredDocument::new("local", "test.txt", "Test Document")
            .with_content("Hello, world!")
            .with_tags(vec!["test".to_string(), "example".to_string()]);

        store.insert(&doc).unwrap();

        let retrieved = store.get(&doc.id).unwrap().unwrap();
        assert_eq!(retrieved.title, "Test Document");
        assert_eq!(retrieved.content, Some("Hello, world!".to_string()));
        assert_eq!(retrieved.tags, vec!["example", "test"]); // Sorted
    }

    #[test]
    fn test_upsert() {
        let store = setup();

        let mut doc = StoredDocument::new("local", "test.txt", "Original Title");
        store.insert(&doc).unwrap();

        doc.title = "Updated Title".to_string();
        store.upsert(&doc).unwrap();

        let retrieved = store.get(&doc.id).unwrap().unwrap();
        assert_eq!(retrieved.title, "Updated Title");
    }

    #[test]
    fn test_delete() {
        let store = setup();

        let doc = StoredDocument::new("local", "test.txt", "Test");
        store.insert(&doc).unwrap();

        assert!(store.exists(&doc.id).unwrap());
        assert!(store.delete(&doc.id).unwrap());
        assert!(!store.exists(&doc.id).unwrap());
    }

    #[test]
    fn test_count() {
        let store = setup();

        store
            .insert(&StoredDocument::new("local", "1.txt", "Doc 1"))
            .unwrap();
        store
            .insert(&StoredDocument::new("local", "2.txt", "Doc 2"))
            .unwrap();
        store
            .insert(&StoredDocument::new("drive", "3.txt", "Doc 3"))
            .unwrap();

        assert_eq!(store.count(None).unwrap(), 3);
        assert_eq!(store.count(Some("local")).unwrap(), 2);
        assert_eq!(store.count(Some("drive")).unwrap(), 1);
    }

    #[test]
    fn test_list_by_path_prefix_basic() {
        let store = setup();

        // Insert documents with different paths
        let downloads_doc1 = StoredDocument::new("local", "doc1.pdf", "Downloads PDF")
            .with_path("/Users/test/Downloads/document1.pdf")
            .with_content("content1")
            .with_mime_type("application/pdf");
        store.insert(&downloads_doc1).unwrap();

        let downloads_doc2 = StoredDocument::new("local", "doc2.txt", "Downloads TXT")
            .with_path("/Users/test/Downloads/document2.txt")
            .with_content("content2");
        store.insert(&downloads_doc2).unwrap();

        let documents_doc = StoredDocument::new("local", "doc3.pdf", "Documents PDF")
            .with_path("/Users/test/Documents/document3.pdf")
            .with_content("content3");
        store.insert(&documents_doc).unwrap();

        // Query Downloads folder
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                None,
                None,
                None,
                None,
                PathPrefixSortBy::Title,
                true,
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 2, "Should find 2 documents in Downloads folder");
        assert!(results.iter().any(|d| d.external_id == "doc1.pdf"));
        assert!(results.iter().any(|d| d.external_id == "doc2.txt"));
        assert!(!results.iter().any(|d| d.external_id == "doc3.pdf"));
    }

    #[test]
    fn test_list_by_path_prefix_with_extension_filter() {
        let store = setup();

        // Insert documents with different extensions
        let pdf_doc = StoredDocument::new("local", "doc1.pdf", "PDF Document")
            .with_path("/Users/test/Downloads/document1.pdf");
        store.insert(&pdf_doc).unwrap();

        let txt_doc = StoredDocument::new("local", "doc2.txt", "TXT Document")
            .with_path("/Users/test/Downloads/document2.txt");
        store.insert(&txt_doc).unwrap();

        let docx_doc = StoredDocument::new("local", "doc3.docx", "DOCX Document")
            .with_path("/Users/test/Downloads/document3.docx");
        store.insert(&docx_doc).unwrap();

        // Query with PDF extension filter
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                None,
                Some(&["pdf"]),
                None,
                None,
                PathPrefixSortBy::Title,
                true,
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 1, "Should find only PDF document");
        assert_eq!(results[0].external_id, "doc1.pdf");
    }

    #[test]
    fn test_list_by_path_prefix_sort_by_size() {
        let store = setup();

        // Insert documents with different sizes
        let small_doc = StoredDocument::new("local", "small.txt", "Small File")
            .with_path("/Users/test/Downloads/small.txt")
            .with_content("small content");
        store.insert(&small_doc).unwrap();
        // Manually update size (would normally be done by ingestion)
        store.db.execute(
            "UPDATE documents SET size_bytes = ?1 WHERE id = ?2",
            &[&100i64 as &dyn rusqlite::ToSql, &small_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        let large_doc = StoredDocument::new("local", "large.zip", "Large File")
            .with_path("/Users/test/Downloads/large.zip")
            .with_content("large content");
        store.insert(&large_doc).unwrap();
        store.db.execute(
            "UPDATE documents SET size_bytes = ?1 WHERE id = ?2",
            &[&10000i64 as &dyn rusqlite::ToSql, &large_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        let medium_doc = StoredDocument::new("local", "medium.pdf", "Medium File")
            .with_path("/Users/test/Downloads/medium.pdf")
            .with_content("medium content");
        store.insert(&medium_doc).unwrap();
        store.db.execute(
            "UPDATE documents SET size_bytes = ?1 WHERE id = ?2",
            &[&1000i64 as &dyn rusqlite::ToSql, &medium_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        // Query sorted by size descending (largest first)
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                None,
                None,
                None,
                None,
                PathPrefixSortBy::Size,
                false, // descending
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].external_id, "large.zip", "Largest file should be first");
        assert_eq!(results[1].external_id, "medium.pdf");
        assert_eq!(results[2].external_id, "small.txt");
    }

    #[test]
    fn test_list_by_path_prefix_with_date_filter() {
        let store = setup();
        use chrono::{Duration, Utc};

        let now = Utc::now();
        let yesterday = now - Duration::days(1);
        let last_week = now - Duration::days(7);

        // Insert documents with different dates
        let recent_doc = StoredDocument::new("local", "recent.txt", "Recent File")
            .with_path("/Users/test/Downloads/recent.txt");
        store.insert(&recent_doc).unwrap();
        store.db.execute(
            "UPDATE documents SET modified_at = ?1 WHERE id = ?2",
            &[&now.to_rfc3339() as &dyn rusqlite::ToSql, &recent_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        let yesterday_doc = StoredDocument::new("local", "yesterday.txt", "Yesterday File")
            .with_path("/Users/test/Downloads/yesterday.txt");
        store.insert(&yesterday_doc).unwrap();
        store.db.execute(
            "UPDATE documents SET modified_at = ?1 WHERE id = ?2",
            &[&yesterday.to_rfc3339() as &dyn rusqlite::ToSql, &yesterday_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        let old_doc = StoredDocument::new("local", "old.txt", "Old File")
            .with_path("/Users/test/Downloads/old.txt");
        store.insert(&old_doc).unwrap();
        store.db.execute(
            "UPDATE documents SET modified_at = ?1 WHERE id = ?2",
            &[&last_week.to_rfc3339() as &dyn rusqlite::ToSql, &old_doc.id as &dyn rusqlite::ToSql],
        ).unwrap();

        // Query with date filter (last 2 days)
        let two_days_ago = now - Duration::days(2);
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                None,
                None,
                Some(two_days_ago),
                None,
                PathPrefixSortBy::ModifiedAt,
                false,
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 2, "Should find only recent documents");
        assert!(results.iter().any(|d| d.external_id == "recent.txt"));
        assert!(results.iter().any(|d| d.external_id == "yesterday.txt"));
        assert!(!results.iter().any(|d| d.external_id == "old.txt"));
    }

    #[test]
    fn test_list_by_path_prefix_source_filter() {
        let store = setup();

        // Insert documents from different sources
        let local_doc = StoredDocument::new("local", "local.pdf", "Local File")
            .with_path("/Users/test/Downloads/local.pdf");
        store.insert(&local_doc).unwrap();

        let drive_doc = StoredDocument::new("googledrive", "drive.pdf", "Drive File")
            .with_path("/Users/test/Downloads/drive.pdf");
        store.insert(&drive_doc).unwrap();

        // Query with source filter
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                Some("local"),
                None,
                None,
                None,
                PathPrefixSortBy::Title,
                true,
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].external_id, "local.pdf");
    }

    #[test]
    fn test_list_by_path_prefix_matches_even_without_title_content() {
        let store = setup();

        // Insert a document whose title and content do NOT contain "Downloads"
        // but whose path IS in the Downloads folder
        let doc = StoredDocument::new("local", "quarterly_report.pdf", "Q3 Financial Report")
            .with_path("/Users/test/Downloads/quarterly_report.pdf")
            .with_content("This is a financial report with no mention of Downloads");
        store.insert(&doc).unwrap();

        // Query Downloads folder
        let results = store
            .list_by_path_prefix(
                "/Users/test/Downloads/",
                None,
                None,
                None,
                None,
                PathPrefixSortBy::Title,
                true,
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 1, "Should find document by path even when title/content don't mention Downloads");
        assert_eq!(results[0].external_id, "quarterly_report.pdf");
    }
}
