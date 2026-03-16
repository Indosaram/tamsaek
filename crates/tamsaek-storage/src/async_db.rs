//! Async database layer with dedicated worker thread
//!
//! This module provides a non-blocking async interface to SQLite by running
//! all database operations on a dedicated thread and communicating via channels.
//!
//! Architecture:
//! ```text
//! ┌─────────────────────────────────┐
//! │ AsyncDatabase (Send + Sync)     │
//! │ - Exposes async methods         │
//! │ - Sends DbRequest via channel   │
//! └──────────────┬──────────────────┘
//!                │ mpsc channel
//! ┌──────────────▼──────────────────┐
//! │ DbWorker (dedicated thread)     │
//! │ - Owns rusqlite Connection      │
//! │ - Processes requests serially   │
//! │ - Sends responses via oneshot   │
//! └─────────────────────────────────┘
//! ```

use crate::error::{StorageError, StorageResult};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

/// Configuration for async database
#[derive(Debug, Clone)]
pub struct AsyncDatabaseConfig {
    pub path: PathBuf,
    /// Channel buffer size for pending requests
    pub channel_buffer: usize,
}

impl Default for AsyncDatabaseConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("tamsaek.db"),
            channel_buffer: 256,
        }
    }
}

impl AsyncDatabaseConfig {
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::from(":memory:"),
            channel_buffer: 256,
        }
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            channel_buffer: 256,
        }
    }
}

/// Request types for database operations
enum DbRequest {
    /// Execute SQL with no return value
    Execute {
        sql: String,
        params: Vec<SqlValue>,
        response: oneshot::Sender<StorageResult<usize>>,
    },
    /// Query returning multiple rows
    Query {
        sql: String,
        params: Vec<SqlValue>,
        response: oneshot::Sender<StorageResult<Vec<Vec<SqlValue>>>>,
    },
    /// Query returning single row
    QueryOne {
        sql: String,
        params: Vec<SqlValue>,
        response: oneshot::Sender<StorageResult<Option<Vec<SqlValue>>>>,
    },
    /// Execute multiple statements in a transaction
    Transaction {
        statements: Vec<(String, Vec<SqlValue>)>,
        response: oneshot::Sender<StorageResult<()>>,
    },
    /// Batch execute (multiple inserts/updates)
    BatchExecute {
        sql: String,
        params_list: Vec<Vec<SqlValue>>,
        response: oneshot::Sender<StorageResult<usize>>,
    },
    /// Shutdown the worker
    Shutdown,
}

/// SQL value that can be sent across threads
#[derive(Debug, Clone)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl SqlValue {
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            SqlValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            SqlValue::Text(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_real(&self) -> Option<f64> {
        match self {
            SqlValue::Real(v) => Some(*v),
            _ => None,
        }
    }

    pub fn into_text(self) -> Option<String> {
        match self {
            SqlValue::Text(v) => Some(v),
            _ => None,
        }
    }

    pub fn into_integer(self) -> Option<i64> {
        match self {
            SqlValue::Integer(v) => Some(v),
            _ => None,
        }
    }
}

impl rusqlite::ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            SqlValue::Null => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Null,
            )),
            SqlValue::Integer(v) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Integer(*v),
            )),
            SqlValue::Real(v) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Real(*v),
            )),
            SqlValue::Text(v) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Text(v.clone()),
            )),
            SqlValue::Blob(v) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Blob(v.clone()),
            )),
        }
    }
}

impl From<rusqlite::types::Value> for SqlValue {
    fn from(value: rusqlite::types::Value) -> Self {
        match value {
            rusqlite::types::Value::Null => SqlValue::Null,
            rusqlite::types::Value::Integer(v) => SqlValue::Integer(v),
            rusqlite::types::Value::Real(v) => SqlValue::Real(v),
            rusqlite::types::Value::Text(v) => SqlValue::Text(v),
            rusqlite::types::Value::Blob(v) => SqlValue::Blob(v),
        }
    }
}

// Convenience conversions
impl From<&str> for SqlValue {
    fn from(s: &str) -> Self {
        SqlValue::Text(s.to_string())
    }
}

impl From<String> for SqlValue {
    fn from(s: String) -> Self {
        SqlValue::Text(s)
    }
}

impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        SqlValue::Integer(v)
    }
}

impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        SqlValue::Integer(v as i64)
    }
}

impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        SqlValue::Real(v)
    }
}

impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => SqlValue::Null,
        }
    }
}

/// Database worker that runs on a dedicated thread
struct DbWorker {
    conn: Connection,
}

impl DbWorker {
    fn new(config: &AsyncDatabaseConfig) -> StorageResult<Self> {
        let conn = if config.path.to_string_lossy() == ":memory:" {
            Connection::open_in_memory()?
        } else {
            if let Some(parent) = config.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let conn = Connection::open(&config.path)?;

            // Set restrictive file permissions (owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) =
                    std::fs::set_permissions(&config.path, std::fs::Permissions::from_mode(0o600))
                {
                    tracing::warn!("Failed to set database file permissions: {}", e);
                }
            }

            conn
        };

        // Configure SQLite for performance
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -64000;
             PRAGMA temp_store = MEMORY;",
        )?;

        debug!("Database worker initialized with WAL mode");

        Ok(Self { conn })
    }

    fn run(mut self, mut rx: mpsc::Receiver<DbRequest>) {
        debug!("Database worker thread started");

        while let Some(request) = rx.blocking_recv() {
            match request {
                DbRequest::Execute {
                    sql,
                    params,
                    response,
                } => {
                    let result = self.execute(&sql, &params);
                    let _ = response.send(result);
                }
                DbRequest::Query {
                    sql,
                    params,
                    response,
                } => {
                    let result = self.query(&sql, &params);
                    let _ = response.send(result);
                }
                DbRequest::QueryOne {
                    sql,
                    params,
                    response,
                } => {
                    let result = self.query_one(&sql, &params);
                    let _ = response.send(result);
                }
                DbRequest::Transaction {
                    statements,
                    response,
                } => {
                    let result = self.transaction(&statements);
                    let _ = response.send(result);
                }
                DbRequest::BatchExecute {
                    sql,
                    params_list,
                    response,
                } => {
                    let result = self.batch_execute(&sql, &params_list);
                    let _ = response.send(result);
                }
                DbRequest::Shutdown => {
                    debug!("Database worker received shutdown signal");
                    break;
                }
            }
        }

        debug!("Database worker thread exiting");
    }

    fn execute(&self, sql: &str, params: &[SqlValue]) -> StorageResult<usize> {
        let params_ref: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let count = self.conn.execute(sql, params_ref.as_slice())?;
        Ok(count)
    }

    fn query(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Vec<Vec<SqlValue>>> {
        let params_ref: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

        let mut stmt = self.conn.prepare(sql)?;
        let column_count = stmt.column_count();

        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            let mut values = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let value: rusqlite::types::Value = row.get(i)?;
                values.push(SqlValue::from(value));
            }
            Ok(values)
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    fn query_one(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Option<Vec<SqlValue>>> {
        let results = self.query(sql, params)?;
        Ok(results.into_iter().next())
    }

    fn transaction(&mut self, statements: &[(String, Vec<SqlValue>)]) -> StorageResult<()> {
        let tx = self.conn.transaction()?;

        for (sql, params) in statements {
            let params_ref: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            tx.execute(sql, params_ref.as_slice())?;
        }

        tx.commit()?;
        Ok(())
    }

    fn batch_execute(&mut self, sql: &str, params_list: &[Vec<SqlValue>]) -> StorageResult<usize> {
        let tx = self.conn.transaction()?;
        let mut total = 0;

        {
            let mut stmt = tx.prepare(sql)?;
            for params in params_list {
                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
                total += stmt.execute(params_ref.as_slice())?;
            }
        }

        tx.commit()?;
        Ok(total)
    }
}

/// Async database handle
///
/// This is the main interface for async database operations.
/// It's `Send + Sync` and can be safely shared across tasks.
pub struct AsyncDatabase {
    tx: mpsc::Sender<DbRequest>,
    config: AsyncDatabaseConfig,
    _worker_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl AsyncDatabase {
    /// Open a new async database connection
    pub fn open(config: AsyncDatabaseConfig) -> StorageResult<Self> {
        info!("Opening async database at {:?}", config.path);

        let (tx, rx) = mpsc::channel(config.channel_buffer);

        // Create worker on dedicated thread
        let worker_config = config.clone();
        let worker_handle = thread::Builder::new()
            .name("db-worker".to_string())
            .spawn(move || match DbWorker::new(&worker_config) {
                Ok(worker) => worker.run(rx),
                Err(e) => {
                    error!("Failed to initialize database worker: {}", e);
                }
            })
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;

        Ok(Self {
            tx,
            config,
            _worker_handle: Arc::new(Mutex::new(Some(worker_handle))),
        })
    }

    /// Initialize the database schema
    pub async fn initialize_schema(&self) -> StorageResult<()> {
        // Create tables
        self.execute(
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
            vec![],
        )
        .await?;

        self.execute(
            r#"
            CREATE TABLE IF NOT EXISTS document_tags (
                document_id TEXT NOT NULL REFERENCES documents(id),
                tag TEXT NOT NULL,
                PRIMARY KEY (document_id, tag)
            )
            "#,
            vec![],
        )
        .await?;

        self.execute(
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
            vec![],
        )
        .await?;

        self.execute(
            r#"
            CREATE TABLE IF NOT EXISTS search_history (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                searched_at TEXT DEFAULT (datetime('now')),
                result_count INTEGER
            )
            "#,
            vec![],
        )
        .await?;

        self.execute(
            r#"
            CREATE TABLE IF NOT EXISTS indexed_directories (
                path TEXT PRIMARY KEY,
                added_at TEXT DEFAULT (datetime('now'))
            )
            "#,
            vec![],
        )
        .await?;

        // Create indexes
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_source ON documents(source)",
            vec![],
        )
        .await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_modified ON documents(modified_at DESC)",
            vec![],
        )
        .await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_mime_type ON documents(mime_type)",
            vec![],
        )
        .await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_content_hash ON documents(content_hash)",
            vec![],
        )
        .await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_document_tags_tag ON document_tags(tag)",
            vec![],
        )
        .await?;

        info!("Async database schema initialized");
        Ok(())
    }

    /// Execute SQL statement
    pub async fn execute(&self, sql: &str, params: Vec<SqlValue>) -> StorageResult<usize> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(DbRequest::Execute {
                sql: sql.to_string(),
                params,
                response: response_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;

        response_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }

    /// Query returning multiple rows
    pub async fn query(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
    ) -> StorageResult<Vec<Vec<SqlValue>>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(DbRequest::Query {
                sql: sql.to_string(),
                params,
                response: response_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;

        response_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }

    /// Query returning single row
    pub async fn query_one(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
    ) -> StorageResult<Option<Vec<SqlValue>>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(DbRequest::QueryOne {
                sql: sql.to_string(),
                params,
                response: response_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;

        response_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }

    /// Execute multiple statements in a transaction
    pub async fn transaction(&self, statements: Vec<(String, Vec<SqlValue>)>) -> StorageResult<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(DbRequest::Transaction {
                statements,
                response: response_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;

        response_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }

    /// Batch execute (multiple inserts/updates with same SQL)
    pub async fn batch_execute(
        &self,
        sql: &str,
        params_list: Vec<Vec<SqlValue>>,
    ) -> StorageResult<usize> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(DbRequest::BatchExecute {
                sql: sql.to_string(),
                params_list,
                response: response_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;

        response_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }

    /// Get the database path
    pub fn path(&self) -> &Path {
        &self.config.path
    }

    /// Shutdown the database worker
    pub async fn shutdown(&self) -> StorageResult<()> {
        let _ = self.tx.send(DbRequest::Shutdown).await;
        Ok(())
    }
}

impl Clone for AsyncDatabase {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            config: self.config.clone(),
            _worker_handle: Arc::clone(&self._worker_handle),
        }
    }
}

impl Drop for AsyncDatabase {
    fn drop(&mut self) {
        // Only attempt shutdown if this is the last reference
        if Arc::strong_count(&self._worker_handle) == 1 {
            // Try to send shutdown signal (non-blocking)
            let _ = self.tx.try_send(DbRequest::Shutdown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_async_database_basic() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();

        // Insert a document
        db.execute(
            "INSERT INTO documents (id, source, external_id, title) VALUES (?1, ?2, ?3, ?4)",
            vec![
                "test|1".into(),
                "test".into(),
                "1".into(),
                "Test Doc".into(),
            ],
        )
        .await
        .unwrap();

        // Query it back
        let rows = db
            .query(
                "SELECT id, title FROM documents WHERE id = ?1",
                vec!["test|1".into()],
            )
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].as_text(), Some("test|1"));
        assert_eq!(rows[0][1].as_text(), Some("Test Doc"));
    }

    #[tokio::test]
    async fn test_batch_execute() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();

        let params_list: Vec<Vec<SqlValue>> = (0..100)
            .map(|i| {
                vec![
                    format!("test|{}", i).into(),
                    "test".into(),
                    format!("{}", i).into(),
                    format!("Doc {}", i).into(),
                ]
            })
            .collect();

        let count = db
            .batch_execute(
                "INSERT INTO documents (id, source, external_id, title) VALUES (?1, ?2, ?3, ?4)",
                params_list,
            )
            .await
            .unwrap();

        assert_eq!(count, 100);

        // Verify count
        let rows = db
            .query_one("SELECT COUNT(*) FROM documents", vec![])
            .await
            .unwrap()
            .unwrap();

        assert_eq!(rows[0].as_integer(), Some(100));
    }

    #[tokio::test]
    async fn test_transaction() {
        let db = AsyncDatabase::open(AsyncDatabaseConfig::in_memory()).unwrap();
        db.initialize_schema().await.unwrap();

        db.transaction(vec![
            (
                "INSERT INTO documents (id, source, external_id, title) VALUES (?1, ?2, ?3, ?4)"
                    .to_string(),
                vec!["test|1".into(), "test".into(), "1".into(), "Doc 1".into()],
            ),
            (
                "INSERT INTO documents (id, source, external_id, title) VALUES (?1, ?2, ?3, ?4)"
                    .to_string(),
                vec!["test|2".into(), "test".into(), "2".into(), "Doc 2".into()],
            ),
        ])
        .await
        .unwrap();

        let rows = db
            .query_one("SELECT COUNT(*) FROM documents", vec![])
            .await
            .unwrap()
            .unwrap();

        assert_eq!(rows[0].as_integer(), Some(2));
    }
}
