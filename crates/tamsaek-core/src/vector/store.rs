use crate::error::{Result, TamsaekError};
use crate::metadata::Database;

pub const EMBEDDING_DIM: usize = 384;

#[derive(Debug, Clone, PartialEq)]
pub struct VectorSearchResult {
    pub id: String,
    pub distance: f32,
}

pub trait VectorStore: Send + Sync {
    fn upsert(&self, id: &str, embedding: &[f32]) -> Result<()>;
    fn delete(&self, id: &str) -> Result<bool>;
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorSearchResult>>;
}

#[derive(Clone)]
pub struct SqliteVectorStore {
    db: Database,
}

impl SqliteVectorStore {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn initialize(&self) -> Result<()> {
        self.db.execute(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_documents USING vec0(
                id TEXT PRIMARY KEY,
                embedding FLOAT[384]
            )
            "#,
            &[],
        )?;
        Ok(())
    }
}

impl VectorStore for SqliteVectorStore {
    fn upsert(&self, id: &str, embedding: &[f32]) -> Result<()> {
        if embedding.len() != EMBEDDING_DIM {
            return Err(TamsaekError::Vector(format!(
                "Expected {} dimensions, got {}",
                EMBEDDING_DIM,
                embedding.len()
            )));
        }

        let embedding_bytes = embedding_to_bytes(embedding);

        self.db.execute(
            "INSERT OR REPLACE INTO vec_documents(id, embedding) VALUES (?, ?)",
            &[&id, &embedding_bytes],
        )?;

        Ok(())
    }

    fn delete(&self, id: &str) -> Result<bool> {
        let count = self
            .db
            .execute("DELETE FROM vec_documents WHERE id = ?", &[&id])?;
        Ok(count > 0)
    }

    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorSearchResult>> {
        if query.len() != EMBEDDING_DIM {
            return Err(TamsaekError::Vector(format!(
                "Expected {} dimensions, got {}",
                EMBEDDING_DIM,
                query.len()
            )));
        }

        let query_bytes = embedding_to_bytes(query);
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

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &f in embedding {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::DatabaseConfig;

    #[test]
    fn test_vector_upsert_and_search() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().unwrap();

        let store = SqliteVectorStore::new(db);
        store.initialize().unwrap();

        let embedding = vec![0.1f32; 384];
        store.upsert("doc1", &embedding).unwrap();

        let results = store.search(&embedding, 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc1");
    }

    #[test]
    fn test_vector_delete() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().unwrap();

        let store = SqliteVectorStore::new(db);
        store.initialize().unwrap();

        let embedding = vec![0.1f32; 384];
        store.upsert("doc1", &embedding).unwrap();

        let deleted = store.delete("doc1").unwrap();
        assert!(deleted);

        let deleted_again = store.delete("doc1").unwrap();
        assert!(!deleted_again);
    }

    #[test]
    fn test_vector_wrong_dimension() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().unwrap();

        let store = SqliteVectorStore::new(db);
        store.initialize().unwrap();

        let wrong_embedding = vec![0.1f32; 100];
        let result = store.upsert("doc1", &wrong_embedding);
        assert!(result.is_err());

        let wrong_query = vec![0.1f32; 512];
        let result = store.search(&wrong_query, 5);
        assert!(result.is_err());
    }
}
