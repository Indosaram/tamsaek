use crate::error::{Result, TamsaekError};
use crate::metadata::Database;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

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

    pub fn insert(&self, doc: &StoredDocument) -> Result<()> {
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

        for tag in &doc.tags {
            self.db.execute(
                "INSERT INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                &[&doc.id as &dyn rusqlite::ToSql, tag],
            )?;
        }

        Ok(())
    }

    pub fn upsert(&self, doc: &StoredDocument) -> Result<()> {
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

    pub fn get(&self, id: &str) -> Result<Option<StoredDocument>> {
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

    pub fn get_by_source(&self, source: &str, external_id: &str) -> Result<Option<StoredDocument>> {
        let id = format!("{}|{}", source, external_id);
        self.get(&id)
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
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
    ) -> Result<Vec<StoredDocument>> {
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

        let mut result = Vec::with_capacity(docs.len());
        for mut doc in docs {
            doc.tags = self.get_tags(&doc.id)?;
            result.push(doc);
        }

        Ok(result)
    }

    pub fn count(&self, source: Option<&str>) -> Result<i64> {
        if let Some(src) = source {
            self.db
                .query_one(
                    "SELECT COUNT(*) FROM documents WHERE source = ?1",
                    &[&src as &dyn rusqlite::ToSql],
                    |row| Ok(row.get::<_, i64>(0)?),
                )?
                .ok_or_else(|| TamsaekError::Document("Count query failed".to_string()))
        } else {
            self.db
                .query_one("SELECT COUNT(*) FROM documents", &[], |row| {
                    Ok(row.get::<_, i64>(0)?)
                })?
                .ok_or_else(|| TamsaekError::Document("Count query failed".to_string()))
        }
    }

    pub fn list_for_fts_migration(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, String, Option<String>)>> {
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

    pub fn exists(&self, id: &str) -> Result<bool> {
        let count: Option<i64> = self.db.query_one(
            "SELECT 1 FROM documents WHERE id = ?1",
            &[&id as &dyn rusqlite::ToSql],
            |row| Ok(row.get(0)?),
        )?;
        Ok(count.is_some())
    }

    pub fn get_by_content_hash(&self, hash: &str) -> Result<Option<StoredDocument>> {
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

    pub fn get_file_metadata_by_path(&self, path: &str) -> Result<Option<FileMetadataInfo>> {
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

    pub fn get_file_metadata_batch(
        &self,
        paths: &[&str],
    ) -> Result<std::collections::HashMap<String, FileMetadataInfo>> {
        if paths.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

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

    fn get_tags(&self, document_id: &str) -> Result<Vec<String>> {
        self.db.query(
            "SELECT tag FROM document_tags WHERE document_id = ?1 ORDER BY tag",
            &[&document_id as &dyn rusqlite::ToSql],
            |row| Ok(row.get(0)?),
        )
    }

    fn row_to_document(&self, row: &rusqlite::Row<'_>) -> Result<StoredDocument> {
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
            tags: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::DatabaseConfig;

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
        assert_eq!(retrieved.tags, vec!["example", "test"]);
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
}
