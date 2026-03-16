//! Vector storage using sqlite-vec
//!
//! Provides semantic search capabilities by storing and querying
//! dense vector embeddings in SQLite.

use crate::db::Database;
use crate::error::StorageResult;
use async_trait::async_trait;

/// Result of a vector search
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    pub id: String,
    pub distance: f32,
}

/// Trait for vector storage and retrieval
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Add or update an embedding for a document
    async fn upsert(&self, id: &str, embedding: &[f32]) -> StorageResult<()>;

    /// Delete an embedding for a document
    async fn delete(&self, id: &str) -> StorageResult<bool>;

    /// Find documents with embeddings similar to the query
    async fn search(&self, query: &[f32], limit: usize) -> StorageResult<Vec<VectorSearchResult>>;
}

/// Implementation of VectorStore using sqlite-vec
#[derive(Clone)]
pub struct SqliteVectorStore {
    db: Database,
}

impl SqliteVectorStore {
    /// Create a new sqlite-vec vector store
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl VectorStore for SqliteVectorStore {
    async fn upsert(&self, id: &str, embedding: &[f32]) -> StorageResult<()> {
        let mut embedding_bytes = Vec::with_capacity(embedding.len() * 4);
        for &f in embedding {
            embedding_bytes.extend_from_slice(&f.to_le_bytes());
        }

        self.db.execute(
            "INSERT OR REPLACE INTO vec_documents(id, embedding) VALUES (?, ?)",
            &[&id, &embedding_bytes],
        )?;

        Ok(())
    }

    async fn delete(&self, id: &str) -> StorageResult<bool> {
        let count = self
            .db
            .execute("DELETE FROM vec_documents WHERE id = ?", &[&id])?;
        Ok(count > 0)
    }

    async fn search(&self, query: &[f32], limit: usize) -> StorageResult<Vec<VectorSearchResult>> {
        let mut query_bytes = Vec::with_capacity(query.len() * 4);
        for &f in query {
            query_bytes.extend_from_slice(&f.to_le_bytes());
        }

        let limit_i64 = limit as i64;

        let results = self.db.query(
            "SELECT id, distance FROM vec_documents WHERE embedding MATCH ? AND k = ? ORDER BY distance",
            &[&query_bytes, &limit_i64],
            |row| {
                Ok(VectorSearchResult {
                    id: row.get(0)?,
                    distance: row.get(1)?,
                })
            },
        )?;

        Ok(results)
    }
}
