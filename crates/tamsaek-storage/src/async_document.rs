//! Async document store using the async database layer
//!
//! Provides async document CRUD operations that don't block the UI thread.

use crate::async_db::{AsyncDatabase, SqlValue};
use crate::document::StoredDocument;
use crate::error::StorageResult;
use chrono::{DateTime, Utc};
use tracing::debug;

/// Async document store
#[derive(Clone)]
pub struct AsyncDocumentStore {
    db: AsyncDatabase,
}

impl AsyncDocumentStore {
    pub fn new(db: AsyncDatabase) -> Self {
        Self { db }
    }

    /// Insert a new document
    pub async fn insert(&self, doc: &StoredDocument) -> StorageResult<()> {
        debug!("Inserting document: {}", doc.id);

        self.db
            .execute(
                r#"
                INSERT INTO documents (
                    id, source, external_id, title, content, mime_type, path,
                    author, size_bytes, created_at, modified_at, content_hash, metadata
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                vec![
                    doc.id.clone().into(),
                    doc.source.clone().into(),
                    doc.external_id.clone().into(),
                    doc.title.clone().into(),
                    doc.content
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.mime_type
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.path
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.author
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.size_bytes.map(SqlValue::from).unwrap_or(SqlValue::Null),
                    doc.created_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.modified_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.content_hash
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.metadata
                        .as_ref()
                        .map(|m| m.to_string())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                ],
            )
            .await?;

        // Insert tags
        for tag in &doc.tags {
            self.db
                .execute(
                    "INSERT INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                    vec![doc.id.clone().into(), tag.clone().into()],
                )
                .await?;
        }

        Ok(())
    }

    /// Upsert a document (insert or update)
    pub async fn upsert(&self, doc: &StoredDocument) -> StorageResult<()> {
        debug!("Upserting document: {}", doc.id);

        self.db
            .execute(
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
                vec![
                    doc.id.clone().into(),
                    doc.source.clone().into(),
                    doc.external_id.clone().into(),
                    doc.title.clone().into(),
                    doc.content
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.mime_type
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.path
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.author
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.size_bytes.map(SqlValue::from).unwrap_or(SqlValue::Null),
                    doc.created_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.modified_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.content_hash
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.metadata
                        .as_ref()
                        .map(|m| m.to_string())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                ],
            )
            .await?;

        // Update tags: delete existing and re-insert
        self.db
            .execute(
                "DELETE FROM document_tags WHERE document_id = ?1",
                vec![doc.id.clone().into()],
            )
            .await?;

        for tag in &doc.tags {
            self.db
                .execute(
                    "INSERT INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                    vec![doc.id.clone().into(), tag.clone().into()],
                )
                .await?;
        }

        Ok(())
    }

    /// Get a document by ID
    pub async fn get(&self, id: &str) -> StorageResult<Option<StoredDocument>> {
        let row = self
            .db
            .query_one(
                r#"
                SELECT id, source, external_id, title, content, mime_type, path,
                       author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
                FROM documents
                WHERE id = ?1
                "#,
                vec![id.into()],
            )
            .await?;

        if let Some(row) = row {
            let mut doc = Self::row_to_document(&row)?;
            doc.tags = self.get_tags(&doc.id).await?;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Get multiple documents by IDs (batch fetch - solves N+1 problem)
    pub async fn get_batch(&self, ids: &[&str]) -> StorageResult<Vec<StoredDocument>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build IN clause with placeholders
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            r#"
            SELECT id, source, external_id, title, content, mime_type, path,
                   author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
            FROM documents
            WHERE id IN ({})
            "#,
            placeholders.join(", ")
        );

        let params: Vec<SqlValue> = ids.iter().map(|&id| id.into()).collect();
        let rows = self.db.query(&sql, params).await?;

        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            let doc = Self::row_to_document(&row)?;
            docs.push(doc);
        }

        // Batch fetch tags
        if !docs.is_empty() {
            let doc_ids: Vec<&str> = docs.iter().map(|d| d.id.as_str()).collect();
            let tags_map = self.get_tags_batch(&doc_ids).await?;

            for doc in &mut docs {
                if let Some(tags) = tags_map.get(&doc.id) {
                    doc.tags = tags.clone();
                }
            }
        }

        Ok(docs)
    }

    /// Delete a document
    pub async fn delete(&self, id: &str) -> StorageResult<bool> {
        debug!("Deleting document: {}", id);
        let count = self
            .db
            .execute("DELETE FROM documents WHERE id = ?1", vec![id.into()])
            .await?;
        Ok(count > 0)
    }

    /// Count documents
    pub async fn count(&self, source: Option<&str>) -> StorageResult<i64> {
        let row = if let Some(src) = source {
            self.db
                .query_one(
                    "SELECT COUNT(*) FROM documents WHERE source = ?1",
                    vec![src.into()],
                )
                .await?
        } else {
            self.db
                .query_one("SELECT COUNT(*) FROM documents", vec![])
                .await?
        };

        row.and_then(|r| r.first().and_then(|v| v.as_integer()))
            .ok_or_else(|| crate::error::StorageError::InvalidQuery("Count failed".into()))
    }

    /// Check if document exists
    pub async fn exists(&self, id: &str) -> StorageResult<bool> {
        let row = self
            .db
            .query_one("SELECT 1 FROM documents WHERE id = ?1", vec![id.into()])
            .await?;
        Ok(row.is_some())
    }

    /// Get document by content hash (for deduplication)
    pub async fn get_by_content_hash(&self, hash: &str) -> StorageResult<Option<StoredDocument>> {
        let row = self
            .db
            .query_one(
                r#"
                SELECT id, source, external_id, title, content, mime_type, path,
                       author, size_bytes, created_at, modified_at, indexed_at, content_hash, metadata
                FROM documents
                WHERE content_hash = ?1
                "#,
                vec![hash.into()],
            )
            .await?;

        if let Some(row) = row {
            let mut doc = Self::row_to_document(&row)?;
            doc.tags = self.get_tags(&doc.id).await?;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Check if content hash exists (for incremental indexing)
    pub async fn content_hash_exists(&self, hash: &str) -> StorageResult<bool> {
        let row = self
            .db
            .query_one(
                "SELECT 1 FROM documents WHERE content_hash = ?1",
                vec![hash.into()],
            )
            .await?;
        Ok(row.is_some())
    }

    /// Get content hash for a document path (for incremental indexing)
    pub async fn get_content_hash_by_path(&self, path: &str) -> StorageResult<Option<String>> {
        let row = self
            .db
            .query_one(
                "SELECT content_hash FROM documents WHERE path = ?1",
                vec![path.into()],
            )
            .await?;

        Ok(row
            .and_then(|r| r.first().cloned())
            .and_then(|v| v.into_text()))
    }

    /// Batch upsert documents (for efficient indexing)
    pub async fn batch_upsert(&self, docs: &[StoredDocument]) -> StorageResult<usize> {
        if docs.is_empty() {
            return Ok(0);
        }

        let params_list: Vec<Vec<SqlValue>> = docs
            .iter()
            .map(|doc| {
                vec![
                    doc.id.clone().into(),
                    doc.source.clone().into(),
                    doc.external_id.clone().into(),
                    doc.title.clone().into(),
                    doc.content
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.mime_type
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.path
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.author
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.size_bytes.map(SqlValue::from).unwrap_or(SqlValue::Null),
                    doc.created_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.modified_at
                        .map(|dt| dt.to_rfc3339())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.content_hash
                        .clone()
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                    doc.metadata
                        .as_ref()
                        .map(|m| m.to_string())
                        .map(SqlValue::from)
                        .unwrap_or(SqlValue::Null),
                ]
            })
            .collect();

        let count = self
            .db
            .batch_execute(
                r#"
                INSERT INTO documents (
                    id, source, external_id, title, content, mime_type, path,
                    author, size_bytes, created_at, modified_at, content_hash, metadata
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
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
                params_list,
            )
            .await?;

        Ok(count)
    }

    /// Get tags for a document
    async fn get_tags(&self, document_id: &str) -> StorageResult<Vec<String>> {
        let rows = self
            .db
            .query(
                "SELECT tag FROM document_tags WHERE document_id = ?1 ORDER BY tag",
                vec![document_id.into()],
            )
            .await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| row.first().cloned().and_then(|v| v.into_text()))
            .collect())
    }

    /// Get tags for multiple documents (batch)
    async fn get_tags_batch(
        &self,
        doc_ids: &[&str],
    ) -> StorageResult<std::collections::HashMap<String, Vec<String>>> {
        use std::collections::HashMap;

        if doc_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders: Vec<String> = (1..=doc_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT document_id, tag FROM document_tags WHERE document_id IN ({}) ORDER BY document_id, tag",
            placeholders.join(", ")
        );

        let params: Vec<SqlValue> = doc_ids.iter().map(|&id| id.into()).collect();
        let rows = self.db.query(&sql, params).await?;

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            if let (Some(doc_id), Some(tag)) = (
                row.first().cloned().and_then(|v| v.into_text()),
                row.get(1).cloned().and_then(|v| v.into_text()),
            ) {
                result.entry(doc_id).or_default().push(tag);
            }
        }

        Ok(result)
    }

    /// Convert a database row to StoredDocument
    fn row_to_document(row: &[SqlValue]) -> StorageResult<StoredDocument> {
        fn parse_datetime(s: Option<String>) -> Option<DateTime<Utc>> {
            s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc))
        }

        Ok(StoredDocument {
            id: row
                .first()
                .cloned()
                .and_then(|v| v.into_text())
                .unwrap_or_default(),
            source: row
                .get(1)
                .cloned()
                .and_then(|v| v.into_text())
                .unwrap_or_default(),
            external_id: row
                .get(2)
                .cloned()
                .and_then(|v| v.into_text())
                .unwrap_or_default(),
            title: row
                .get(3)
                .cloned()
                .and_then(|v| v.into_text())
                .unwrap_or_default(),
            content: row.get(4).cloned().and_then(|v| v.into_text()),
            mime_type: row.get(5).cloned().and_then(|v| v.into_text()),
            path: row.get(6).cloned().and_then(|v| v.into_text()),
            author: row.get(7).cloned().and_then(|v| v.into_text()),
            size_bytes: row.get(8).and_then(|v| v.as_integer()),
            created_at: parse_datetime(row.get(9).cloned().and_then(|v| v.into_text())),
            modified_at: parse_datetime(row.get(10).cloned().and_then(|v| v.into_text())),
            indexed_at: parse_datetime(row.get(11).cloned().and_then(|v| v.into_text())),
            content_hash: row.get(12).cloned().and_then(|v| v.into_text()),
            metadata: row
                .get(13)
                .cloned()
                .and_then(|v| v.into_text())
                .and_then(|s| serde_json::from_str(&s).ok()),
            tags: Vec::new(), // Loaded separately
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_db::AsyncDatabaseConfig;

    #[tokio::test]
    async fn test_async_document_store() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();
        let store = AsyncDocumentStore::new(db);

        // Insert
        let doc =
            StoredDocument::new("local", "test.txt", "Test Document").with_content("Hello, world!");
        store.insert(&doc).await.unwrap();

        // Get
        let retrieved = store.get(&doc.id).await.unwrap().unwrap();
        assert_eq!(retrieved.title, "Test Document");
        assert_eq!(retrieved.content, Some("Hello, world!".to_string()));

        // Count
        assert_eq!(store.count(None).await.unwrap(), 1);
        assert_eq!(store.count(Some("local")).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_batch_get() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();
        let store = AsyncDocumentStore::new(db);

        // Insert multiple docs
        for i in 0..10 {
            let doc = StoredDocument::new("local", format!("{}.txt", i), format!("Doc {}", i));
            store.insert(&doc).await.unwrap();
        }

        // Batch get
        let ids: Vec<&str> = (0..5)
            .map(|i| Box::leak(format!("local|{}.txt", i).into_boxed_str()) as &str)
            .collect();

        let docs = store.get_batch(&ids).await.unwrap();
        assert_eq!(docs.len(), 5);
    }

    #[tokio::test]
    async fn test_batch_upsert() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();
        let store = AsyncDocumentStore::new(db);

        let docs: Vec<StoredDocument> = (0..100)
            .map(|i| StoredDocument::new("local", format!("{}.txt", i), format!("Doc {}", i)))
            .collect();

        let count = store.batch_upsert(&docs).await.unwrap();
        assert_eq!(count, 100);
        assert_eq!(store.count(None).await.unwrap(), 100);
    }
}
