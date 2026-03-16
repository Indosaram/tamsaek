use std::path::PathBuf;

use parking_lot::Mutex;

use crate::document::Document;
use crate::error::{Result, TamsaekError};
use crate::search::SearchResult;

#[derive(Clone, Default)]
pub struct SchemaFields;

pub struct TamsaekIndex {
    store: tamsaek_storage::DocumentStore,
    fts: Mutex<tamsaek_storage::TantivyFts>,
}

impl TamsaekIndex {
    pub fn open(path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&path)?;

        let db = tamsaek_storage::Database::open(tamsaek_storage::DatabaseConfig::with_path(
            path.join("metadata.db"),
        ))?;
        db.initialize_schema()?;

        let fts = tamsaek_storage::TantivyFts::open(path.join("fts"))?;

        Ok(Self {
            store: tamsaek_storage::DocumentStore::new(db),
            fts: Mutex::new(fts),
        })
    }

    pub fn in_memory() -> Result<Self> {
        let db = tamsaek_storage::Database::open(tamsaek_storage::DatabaseConfig::in_memory())?;
        db.initialize_schema()?;

        let fts = tamsaek_storage::TantivyFts::in_memory()?;

        Ok(Self {
            store: tamsaek_storage::DocumentStore::new(db),
            fts: Mutex::new(fts),
        })
    }

    pub fn add_document(&self, doc: &Document) -> Result<()> {
        let stored = self.compat_to_stored(doc);
        self.store.upsert(&stored)?;

        self.fts.lock().upsert_document_full(
            &stored.id,
            &stored.title,
            stored.content.as_deref().unwrap_or_default(),
            stored.path.as_deref(),
            stored.size_bytes,
            stored.modified_at.map(|dt| dt.to_rfc3339()).as_deref(),
            Some(&stored.source),
            doc.extension.as_deref(),
        )?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_document_full(
        &self,
        id: &str,
        title: &str,
        content: &str,
        path: Option<&str>,
        size_bytes: Option<i64>,
        modified_at: Option<&str>,
        source: Option<&str>,
        extension: Option<&str>,
    ) -> Result<()> {
        let source = source.unwrap_or("local").to_string();
        let external_id = id.to_string();
        let parsed_modified = modified_at
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let mut stored = tamsaek_storage::StoredDocument {
            id: id.to_string(),
            source,
            external_id,
            title: title.to_string(),
            content: Some(content.to_string()),
            mime_type: None,
            path: path.map(|value| value.to_string()),
            author: None,
            size_bytes,
            created_at: None,
            modified_at: parsed_modified,
            indexed_at: None,
            content_hash: None,
            metadata: None,
            tags: Vec::new(),
        };
        stored.compute_content_hash();

        self.store.upsert(&stored)?;
        self.fts.lock().upsert_document_full(
            id,
            title,
            content,
            path,
            size_bytes,
            modified_at,
            Some(&stored.source),
            extension,
        )?;
        Ok(())
    }

    pub fn delete_document(&self, id: &str) -> Result<()> {
        self.store.delete(id)?;
        self.fts.lock().delete_document(id)?;
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.fts.lock().commit()?;
        Ok(())
    }

    pub fn num_docs(&self) -> u64 {
        self.fts.lock().num_docs()
    }

    pub fn clear(&self) -> Result<()> {
        let total = usize::try_from(self.store.count(None)?).unwrap_or(0);
        let docs = self.store.list(None, total, 0)?;
        for doc in docs {
            self.store.delete(&doc.id)?;
        }
        self.fts.lock().clear()?;
        Ok(())
    }

    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .search(query_str, limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn search_content_only(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .search_content_only(query_str, limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn search_title_only(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .search_title_only(query_str, limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn search_regex(&self, pattern: &str, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .search_regex(pattern, limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn search_by_extension(&self, ext: &str, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .search_by_extension(ext, limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn list_all(&self, limit: usize) -> Result<Vec<SearchResult>> {
        Ok(self
            .fts
            .lock()
            .list_all(limit)?
            .into_iter()
            .map(map_search_result)
            .collect())
    }

    pub fn get_document(&self, id: &str) -> Result<Option<Document>> {
        Ok(self.store.get(id)?.map(stored_to_compat))
    }

    fn compat_to_stored(&self, doc: &Document) -> tamsaek_storage::StoredDocument {
        let mut stored = tamsaek_storage::StoredDocument {
            id: doc.id.clone(),
            source: doc.source.clone(),
            external_id: doc.external_id.clone().unwrap_or_else(|| doc.id.clone()),
            title: doc.title.clone(),
            content: Some(doc.content.clone()),
            mime_type: doc.mime_type.clone(),
            path: doc.path.clone(),
            author: doc.author.clone(),
            size_bytes: doc.size_bytes,
            created_at: doc.created_at,
            modified_at: doc.modified_at,
            indexed_at: doc.indexed_at,
            content_hash: doc.content_hash.clone(),
            metadata: doc.metadata.clone(),
            tags: doc.tags.clone(),
        };

        if stored.content_hash.is_none() {
            stored.compute_content_hash();
        }

        stored
    }
}

fn map_search_result(result: tamsaek_storage::TantivySearchResult) -> SearchResult {
    SearchResult {
        id: result.document_id,
        title: result.title,
        score: result.score,
        snippet: result.snippet,
        path: result.path,
        extension: result.extension,
        size_bytes: result.size_bytes,
        modified_at: result.modified_at,
        source: result.source,
    }
}

fn stored_to_compat(stored: tamsaek_storage::StoredDocument) -> Document {
    let extension = stored.path.as_ref().and_then(|path| {
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_string())
    });

    let external_id = if stored.external_id == stored.id && stored.source == "local" {
        None
    } else {
        Some(stored.external_id)
    };

    Document {
        id: stored.id,
        title: stored.title,
        content: stored.content.unwrap_or_default(),
        path: stored.path,
        extension,
        size_bytes: stored.size_bytes,
        modified_at: stored.modified_at,
        source: stored.source,
        external_id,
        mime_type: stored.mime_type,
        author: stored.author,
        created_at: stored.created_at,
        indexed_at: stored.indexed_at,
        content_hash: stored.content_hash,
        metadata: stored.metadata,
        tags: stored.tags,
    }
}

impl From<std::num::TryFromIntError> for TamsaekError {
    fn from(err: std::num::TryFromIntError) -> Self {
        TamsaekError::Document(err.to_string())
    }
}
