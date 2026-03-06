use crate::error::Result;
use crate::metadata::Schema;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub path: PathBuf,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("tamsaek.db"),
        }
    }
}

impl DatabaseConfig {
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::from(":memory:"),
        }
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
    config: DatabaseConfig,
}

impl Database {
    pub fn open(config: DatabaseConfig) -> Result<Self> {
        info!("Opening database at {:?}", config.path);

        #[cfg(feature = "vector")]
        Self::register_vector_extension();

        let conn = if config.path.to_string_lossy() == ":memory:" {
            Connection::open_in_memory()?
        } else {
            if let Some(parent) = config.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let conn = Connection::open(&config.path)?;

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

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            config,
        };

        db.configure()?;

        Ok(db)
    }

    #[cfg(feature = "vector")]
    fn register_vector_extension() {
        use rusqlite::ffi::sqlite3_auto_extension;
        use sqlite_vec::sqlite3_vec_init;
        use std::sync::Once;

        static REGISTER: Once = Once::new();
        REGISTER.call_once(|| {
            unsafe {
                type ExtensionFn = unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *const i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32;
                sqlite3_auto_extension(Some(std::mem::transmute::<*const (), ExtensionFn>(
                    sqlite3_vec_init as *const (),
                )));
            }
            debug!("sqlite-vec extension registered");
        });
    }

    fn configure(&self) -> Result<()> {
        let conn = self.conn.lock();

        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        debug!("Database configured with WAL mode");

        Ok(())
    }

    pub fn initialize_schema(&self) -> Result<()> {
        Schema::create_tables(self)?;
        Schema::create_indexes(self)?;
        info!("Database schema initialized");
        Ok(())
    }

    pub fn execute(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<usize> {
        let conn = self.conn.lock();
        let count = conn.execute(sql, params)?;
        Ok(count)
    }

    pub fn query<T, F>(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
        mut f: F,
    ) -> Result<Vec<T>>
    where
        F: FnMut(&rusqlite::Row<'_>) -> Result<T>,
    {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| Ok(f(row)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row??);
        }
        Ok(results)
    }

    pub fn query_one<T, F>(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
        f: F,
    ) -> Result<Option<T>>
    where
        F: FnMut(&rusqlite::Row<'_>) -> Result<T>,
    {
        let results = self.query(sql, params, f)?;
        Ok(results.into_iter().next())
    }

    pub fn transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn path(&self) -> &Path {
        &self.config.path
    }

    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            config: self.config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        assert_eq!(db.path().to_string_lossy(), ":memory:");
    }

    #[test]
    fn test_execute_query() {
        let db = Database::open(DatabaseConfig::in_memory()).unwrap();
        db.execute("CREATE TABLE test (id INTEGER, name TEXT)", &[])
            .unwrap();
        db.execute("INSERT INTO test VALUES (1, 'hello')", &[])
            .unwrap();

        let results: Vec<(i32, String)> = db
            .query("SELECT id, name FROM test", &[], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], (1, "hello".to_string()));
    }
}
