use crate::db::Database;
use crate::error::StorageResult;
use tracing::debug;

pub struct Schema;

impl Schema {
    pub fn create_tables(db: &Database) -> StorageResult<()> {
        debug!("Creating database tables");

        // Documents table
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                external_id TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT,
                mime_type TEXT,
                path TEXT,
                author TEXT,
                size_bytes INTEGER,
                created_at TEXT,
                modified_at TEXT,
                indexed_at TEXT DEFAULT (datetime('now')),
                content_hash TEXT,
                metadata TEXT,
                UNIQUE(source, external_id)
            )
            "#,
            &[],
        )?;

        // Tags table for document tagging
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS document_tags (
                document_id TEXT NOT NULL REFERENCES documents(id),
                tag TEXT NOT NULL,
                PRIMARY KEY (document_id, tag)
            )
            "#,
            &[],
        )?;

        // Sync state for connectors
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS sync_state (
                connector_id TEXT PRIMARY KEY,
                delta_token TEXT,
                last_sync_at TEXT,
                items_synced INTEGER DEFAULT 0,
                sync_status TEXT DEFAULT 'idle',
                error_message TEXT
            )
            "#,
            &[],
        )?;

        // Search history for autocomplete
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS search_history (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                searched_at TEXT DEFAULT (datetime('now')),
                result_count INTEGER
            )
            "#,
            &[],
        )?;

        // Indexed directories for tracking which paths are indexed
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS indexed_directories (
                path TEXT PRIMARY KEY,
                added_at TEXT DEFAULT (datetime('now'))
            )
            "#,
            &[],
        )?;

        // Vector table for semantic search
        db.execute(
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

    pub fn create_indexes(db: &Database) -> StorageResult<()> {
        debug!("Creating database indexes");

        // Document indexes
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_source ON documents(source)",
            &[],
        )?;
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_modified ON documents(modified_at DESC)",
            &[],
        )?;
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_mime_type ON documents(mime_type)",
            &[],
        )?;
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_author ON documents(author)",
            &[],
        )?;

        // Tag index
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_document_tags_tag ON document_tags(tag)",
            &[],
        )?;

        // Search history index
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_search_history_query ON search_history(query)",
            &[],
        )?;

        Ok(())
    }

    pub fn drop_all(db: &Database) -> StorageResult<()> {
        debug!("Dropping all tables");

        db.execute("DROP TABLE IF EXISTS indexed_directories", &[])?;
        db.execute("DROP TABLE IF EXISTS search_history", &[])?;
        db.execute("DROP TABLE IF EXISTS sync_state", &[])?;
        db.execute("DROP TABLE IF EXISTS document_tags", &[])?;
        db.execute("DROP TABLE IF EXISTS documents", &[])?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DatabaseConfig;

    #[test]
    fn test_create_schema() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        Schema::create_tables(&db).unwrap();
        Schema::create_indexes(&db).unwrap();

        // Verify tables exist
        let tables: Vec<String> = db
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                &[],
                |row| Ok(row.get(0)?),
            )
            .unwrap();

        assert!(tables.contains(&"documents".to_string()));
        assert!(tables.contains(&"document_tags".to_string()));
        assert!(tables.contains(&"sync_state".to_string()));
        assert!(tables.contains(&"search_history".to_string()));
        assert!(tables.contains(&"indexed_directories".to_string()));
    }
}
