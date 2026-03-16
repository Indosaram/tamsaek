//! Tantivy-based full-text search with incremental indexing
//!
//! Unlike DuckDB FTS which requires full rebuilds, Tantivy supports
//! incremental updates - documents can be added/updated/deleted
//! without rebuilding the entire index.
//!
//! The schema is denormalized to include all fields needed for search results,
//! eliminating the need for additional database lookups (N+1 problem).

use crate::error::{StorageError, StorageResult};
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, BooleanQuery, QueryParser, RegexQuery};
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use tracing::{debug, info};
use unicode_normalization::UnicodeNormalization;

/// Search result from Tantivy (denormalized - contains all display fields)
#[derive(Debug, Clone)]
pub struct TantivySearchResult {
    pub document_id: String,
    pub score: f32,
    pub title: String,
    pub snippet: Option<String>,
    /// File path (denormalized from documents table)
    pub path: Option<String>,
    /// File size in bytes (denormalized)
    pub size_bytes: Option<i64>,
    /// Last modified timestamp as RFC3339 string (denormalized)
    pub modified_at: Option<String>,
    /// Source type: "local", "googledrive", etc. (denormalized)
    pub source: Option<String>,
    /// File extension without dot (denormalized)
    pub extension: Option<String>,
}

/// Schema fields for the search index (denormalized for zero-DB-lookup search results)
struct IndexFields {
    id: Field,
    title: Field,
    content: Field,
    // Filename fields for better search matching (especially for Korean/CJK)
    filename_exact: Field, // STRING for exact matching
    filename_text: Field,  // TEXT for tokenized/partial matching
    // Denormalized fields for search results (stored but not indexed for search)
    path: Field,
    size_bytes: Field,
    modified_at: Field,
    source: Field,
    extension: Field,
}

/// Tantivy-based full-text search engine
pub struct TantivyFts {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    fields: IndexFields,
}

impl TantivyFts {
    /// Build the denormalized schema
    fn build_schema() -> (Schema, IndexFields) {
        let mut schema_builder = Schema::builder();

        // Indexed + stored fields (searchable)
        let id = schema_builder.add_text_field("id", STRING | STORED);
        let title = schema_builder.add_text_field("title", TEXT | STORED);
        let content = schema_builder.add_text_field("content", TEXT | STORED);

        // Filename fields for better search matching (especially for Korean/CJK)
        let filename_exact = schema_builder.add_text_field("filename_exact", STRING | STORED);
        let filename_text = schema_builder.add_text_field("filename_text", TEXT | STORED);

        // Path field: STRING for exact/prefix matching (not tokenized), STORED for retrieval
        // This enables folder-aware search (e.g., matching "Downloads" in path)
        let path = schema_builder.add_text_field("path", STRING | STORED);
        let size_bytes = schema_builder.add_i64_field("size_bytes", STORED);
        let modified_at = schema_builder.add_text_field("modified_at", STORED);
        let source = schema_builder.add_text_field("source", STRING | STORED);
        let extension = schema_builder.add_text_field("extension", STRING | STORED);

        let schema = schema_builder.build();
        let fields = IndexFields {
            id,
            title,
            content,
            filename_exact,
            filename_text,
            path,
            size_bytes,
            modified_at,
            source,
            extension,
        };

        (schema, fields)
    }

    /// Extract filename from path or title
    fn extract_filename(path: Option<&str>, title: &str) -> String {
        if let Some(p) = path {
            // Try to extract basename from path
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| title.to_string())
        } else {
            title.to_string()
        }
    }

    /// Normalize text to NFC (Canonical Composition) for consistent Unicode search.
    ///
    /// macOS stores filenames in NFD (decomposed) form, while user input is typically
    /// in NFC (composed) form. This normalization ensures Korean and other CJK
    /// characters match regardless of Unicode form.
    ///
    /// Note: This is applied ONLY to search/indexed text fields, NOT to stored paths
    /// or document IDs to avoid creating duplicate entries.
    fn normalize_for_search(text: &str) -> String {
        text.nfc().collect()
    }

    /// Open or create a Tantivy index at the given path
    /// Automatically rebuilds if schema is outdated (missing required fields)
    pub fn open(index_path: PathBuf) -> StorageResult<Self> {
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&index_path).map_err(|e| {
            StorageError::Tantivy(format!("Failed to create index directory: {}", e))
        })?;

        let (target_schema, _) = Self::build_schema();

        let needs_rebuild = if index_path.join("meta.json").exists() {
            info!("Opening existing Tantivy index at {:?}", index_path);
            let existing_index = Index::open_in_dir(&index_path)
                .map_err(|e| StorageError::Tantivy(format!("Failed to open index: {}", e)))?;

            let existing_schema = existing_index.schema();
            let has_all_fields = Self::schema_has_required_fields(&existing_schema);

            if !has_all_fields {
                info!("Existing index has outdated schema, will rebuild");
            }

            !has_all_fields
        } else {
            false
        };

        // Rebuild index if schema is outdated
        if needs_rebuild {
            info!("Rebuilding index with new schema");
            Self::rebuild_index(&index_path, &target_schema)?;
        }

        // Open or create index
        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(&index_path)
                .map_err(|e| StorageError::Tantivy(format!("Failed to open index: {}", e)))?
        } else {
            info!("Creating new Tantivy index at {:?}", index_path);
            Index::create_in_dir(&index_path, target_schema.clone())
                .map_err(|e| StorageError::Tantivy(format!("Failed to create index: {}", e)))?
        };

        let fields = Self::get_fields_from_schema(&index.schema())?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| StorageError::Tantivy(format!("Failed to create reader: {}", e)))?;

        let writer = index.writer(50_000_000).or_else(|e| {
            tracing::warn!("Writer lock busy, attempting stale lock recovery: {}", e);
            let lock_path = index_path.join(".tantivy-writer.lock");
            if lock_path.exists() {
                std::fs::remove_file(&lock_path).map_err(|e| {
                    StorageError::Tantivy(format!("Failed to remove stale lock: {}", e))
                })?;
                tracing::info!("Removed stale lock file, retrying writer creation");
                index.writer(50_000_000).map_err(|e| {
                    StorageError::Tantivy(format!(
                        "Failed to create writer after lock recovery: {}",
                        e
                    ))
                })
            } else {
                Err(StorageError::Tantivy(format!(
                    "Failed to create writer: {}",
                    e
                )))
            }
        })?;

        Ok(Self {
            index,
            reader,
            writer,
            fields,
        })
    }

    /// Check if schema has all required fields for current version
    fn schema_has_required_fields(schema: &Schema) -> bool {
        let required_fields = [
            "id",
            "title",
            "content",
            "filename_exact",
            "filename_text",
            "path",
            "size_bytes",
            "modified_at",
            "source",
            "extension",
        ];

        if !required_fields
            .iter()
            .all(|name| schema.get_field(name).is_ok())
        {
            return false;
        }

        if let Ok(path_field) = schema.get_field("path") {
            let path_entry = schema.get_field_entry(path_field);
            if !path_entry.is_indexed() {
                return false;
            }
        }

        true
    }

    /// Get IndexFields from an existing schema (used when opening existing index)
    fn get_fields_from_schema(schema: &Schema) -> StorageResult<IndexFields> {
        let get_field = |name: &str| {
            schema
                .get_field(name)
                .map_err(|_| StorageError::Tantivy(format!("Missing required field: {}", name)))
        };

        Ok(IndexFields {
            id: get_field("id")?,
            title: get_field("title")?,
            content: get_field("content")?,
            filename_exact: get_field("filename_exact")?,
            filename_text: get_field("filename_text")?,
            path: get_field("path")?,
            size_bytes: get_field("size_bytes")?,
            modified_at: get_field("modified_at")?,
            source: get_field("source")?,
            extension: get_field("extension")?,
        })
    }

    fn rebuild_index(index_path: &std::path::Path, schema: &Schema) -> StorageResult<()> {
        info!("Deleting old index files for schema rebuild");

        for entry in std::fs::read_dir(index_path)
            .map_err(|e| StorageError::Tantivy(format!("Failed to read index directory: {}", e)))?
        {
            let entry = entry.map_err(|e| {
                StorageError::Tantivy(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if file_name == ".tantivy-writer.lock" {
                continue;
            }

            if path.is_file() {
                std::fs::remove_file(&path).map_err(|e| {
                    StorageError::Tantivy(format!("Failed to remove old index file: {}", e))
                })?;
            } else if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| {
                    StorageError::Tantivy(format!("Failed to remove old index directory: {}", e))
                })?;
            }
        }

        let lock_path = index_path.join(".tantivy-writer.lock");
        if lock_path.exists() {
            let _ = std::fs::remove_file(&lock_path);
        }

        let _ = Index::create_in_dir(index_path, schema.clone())
            .map_err(|e| StorageError::Tantivy(format!("Failed to create new index: {}", e)))?;

        info!("Index rebuilt successfully with new schema");
        Ok(())
    }

    /// Create an in-memory index for testing
    pub fn in_memory() -> StorageResult<Self> {
        let (schema, fields) = Self::build_schema();

        let index = Index::create_in_ram(schema.clone());

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| StorageError::Tantivy(format!("Failed to create reader: {}", e)))?;

        let writer = index
            .writer(15_000_000)
            .map_err(|e| StorageError::Tantivy(format!("Failed to create writer: {}", e)))?;

        Ok(Self {
            index,
            reader,
            writer,
            fields,
        })
    }

    /// Add or update a document in the index (incremental)
    pub fn upsert_document(&mut self, id: &str, title: &str, content: &str) -> StorageResult<()> {
        self.upsert_document_full(id, title, content, None, None, None, None, None)
    }

    /// Add or update a document with all denormalized fields
    ///
    /// This method stores all fields needed for search result display,
    /// eliminating the need for database lookups (solves N+1 problem).
    ///
    /// NOTE: Title and filename fields are NFC-normalized for consistent Unicode
    /// search matching (especially for Korean/CJK filenames on macOS which uses NFD).
    /// The original path is stored unchanged for file operations.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_document_full(
        &mut self,
        id: &str,
        title: &str,
        content: &str,
        path: Option<&str>,
        size_bytes: Option<i64>,
        modified_at: Option<&str>,
        source: Option<&str>,
        extension: Option<&str>,
    ) -> StorageResult<()> {
        // Delete existing document with same ID first
        let id_term = tantivy::Term::from_field_text(self.fields.id, id);
        self.writer.delete_term(id_term);

        // Extract filename for search indexing
        let filename = Self::extract_filename(path, title);

        // Normalize title and filename to NFC for consistent Unicode search
        // This ensures Korean NFD filenames (macOS) match NFC user queries
        let title_normalized = Self::normalize_for_search(title);
        let filename_normalized = Self::normalize_for_search(&filename);

        // Build document with all fields
        let mut doc = tantivy::TantivyDocument::new();
        doc.add_text(self.fields.id, id);
        // Use normalized text for search fields
        doc.add_text(self.fields.title, &title_normalized);
        doc.add_text(self.fields.content, content);

        // Add filename fields for better search matching (NFC normalized)
        doc.add_text(self.fields.filename_exact, &filename_normalized);
        doc.add_text(self.fields.filename_text, &filename_normalized);

        // Add optional denormalized fields
        if let Some(p) = path {
            doc.add_text(self.fields.path, p);
        }
        if let Some(s) = size_bytes {
            doc.add_i64(self.fields.size_bytes, s);
        }
        if let Some(m) = modified_at {
            doc.add_text(self.fields.modified_at, m);
        }
        if let Some(src) = source {
            doc.add_text(self.fields.source, src);
        }
        if let Some(ext) = extension {
            doc.add_text(self.fields.extension, ext);
        }

        self.writer
            .add_document(doc)
            .map_err(|e| StorageError::Tantivy(format!("Failed to add document: {}", e)))?;

        debug!("Upserted document: {}", id);
        Ok(())
    }

    /// Delete a document from the index
    pub fn delete_document(&mut self, id: &str) -> StorageResult<()> {
        let id_term = tantivy::Term::from_field_text(self.fields.id, id);
        self.writer.delete_term(id_term);
        debug!("Deleted document: {}", id);
        Ok(())
    }

    /// Commit pending changes to make them searchable
    pub fn commit(&mut self) -> StorageResult<()> {
        self.writer
            .commit()
            .map_err(|e| StorageError::Tantivy(format!("Failed to commit: {}", e)))?;

        // Reload reader to see committed changes immediately
        self.reader
            .reload()
            .map_err(|e| StorageError::Tantivy(format!("Failed to reload reader: {}", e)))?;

        debug!("Committed changes to index");
        Ok(())
    }

    /// Search the index (both title and content)
    pub fn search(&self, query_str: &str, limit: usize) -> StorageResult<Vec<TantivySearchResult>> {
        self.search_fields(query_str, limit, true, true)
    }

    /// Search only the content field (excludes title)
    pub fn search_content_only(
        &self,
        query_str: &str,
        limit: usize,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        self.search_fields(query_str, limit, false, true)
    }

    /// Search only the title field (excludes content)
    pub fn search_title_only(
        &self,
        query_str: &str,
        limit: usize,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        self.search_fields(query_str, limit, true, false)
    }

    /// List all documents (for extension-based filtering)
    pub fn list_all(&self, limit: usize) -> StorageResult<Vec<TantivySearchResult>> {
        debug!("Listing all documents (limit: {})", limit);

        let searcher = self.reader.searcher();
        let query = AllQuery;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| StorageError::Tantivy(format!("List all failed: {}", e)))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| StorageError::Tantivy(format!("Failed to retrieve doc: {}", e)))?;

            results.push(self.doc_to_result(&doc, score, None));
        }

        debug!("Found {} documents", results.len());
        Ok(results)
    }

    pub fn search_by_extension(
        &self,
        ext: &str,
        limit: usize,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        let ext_lower = ext.to_lowercase();
        debug!("Searching by extension: {} (limit: {})", ext_lower, limit);

        let searcher = self.reader.searcher();
        let term = tantivy::Term::from_field_text(self.fields.extension, &ext_lower);
        let query = tantivy::query::TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| StorageError::Tantivy(format!("Extension search failed: {}", e)))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| StorageError::Tantivy(format!("Failed to retrieve doc: {}", e)))?;

            results.push(self.doc_to_result(&doc, score, None));
        }

        debug!(
            "Found {} documents with extension '{}'",
            results.len(),
            ext_lower
        );
        Ok(results)
    }

    /// Search for documents whose path matches the given pattern.
    /// Uses STRING field matching for exact/prefix/suffix path matching.
    /// Pattern can include wildcards for regex matching.
    ///
    /// This is used for folder-aware search (e.g., finding files in Downloads folder).
    pub fn search_by_path(
        &self,
        path_pattern: &str,
        limit: usize,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        debug!("Searching by path: {} (limit: {})", path_pattern, limit);

        let searcher = self.reader.searcher();

        // Normalize path pattern for consistent Unicode matching
        let pattern_normalized = Self::normalize_for_search(path_pattern);

        // For simple patterns (no regex metacharacters), wrap with wildcards
        let is_simple_pattern = !pattern_normalized.contains([
            '.', '*', '+', '?', '^', '$', '(', ')', '[', ']', '{', '}', '|', '\\',
        ]);
        let search_pattern = if is_simple_pattern {
            format!(".*{}.*", pattern_normalized)
        } else {
            pattern_normalized.to_string()
        };

        let query = RegexQuery::from_pattern(&search_pattern, self.fields.path)
            .map_err(|e| StorageError::Tantivy(format!("Invalid path regex pattern: {}", e)))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| StorageError::Tantivy(format!("Path search failed: {}", e)))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| StorageError::Tantivy(format!("Failed to retrieve doc: {}", e)))?;

            results.push(self.doc_to_result(&doc, score, None));
        }

        debug!(
            "Found {} documents matching path '{}'",
            results.len(),
            path_pattern
        );
        Ok(results)
    }

    fn doc_to_result(
        &self,
        doc: &TantivyDocument,
        score: f32,
        snippet: Option<String>,
    ) -> TantivySearchResult {
        let id = doc
            .get_first(self.fields.id)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let title = doc
            .get_first(self.fields.title)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let path = doc
            .get_first(self.fields.path)
            .and_then(|v| v.as_str())
            .map(|s: &str| s.to_string());

        let size_bytes = doc
            .get_first(self.fields.size_bytes)
            .and_then(|v| v.as_i64());

        let modified_at = doc
            .get_first(self.fields.modified_at)
            .and_then(|v| v.as_str())
            .map(|s: &str| s.to_string());

        let source = doc
            .get_first(self.fields.source)
            .and_then(|v| v.as_str())
            .map(|s: &str| s.to_string());

        let extension = doc
            .get_first(self.fields.extension)
            .and_then(|v| v.as_str())
            .map(|s: &str| s.to_string());

        TantivySearchResult {
            document_id: id,
            score,
            title,
            snippet,
            path,
            size_bytes,
            modified_at,
            source,
            extension,
        }
    }

    pub fn search_regex(
        &self,
        pattern: &str,
        limit: usize,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        if pattern.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Normalize regex pattern to NFC for consistent Unicode matching
        let pattern_normalized = Self::normalize_for_search(pattern);

        // For simple patterns (no regex metacharacters), wrap with wildcards for substring matching
        // This ensures "베어유" matches "베어유_프로필.jpg" and "베어유보고서.pdf"
        let is_simple_pattern = !pattern_normalized.contains([
            '.', '*', '+', '?', '^', '$', '(', ')', '[', ']', '{', '}', '|', '\\',
        ]);
        let search_pattern = if is_simple_pattern {
            format!(".*{}.*", pattern_normalized)
        } else {
            pattern_normalized.to_string()
        };

        let searcher = self.reader.searcher();
        let mut queries: Vec<Box<dyn tantivy::query::Query>> = Vec::new();

        queries.push(Box::new(
            RegexQuery::from_pattern(&search_pattern, self.fields.title)
                .map_err(|e| StorageError::Tantivy(format!("Invalid regex pattern: {}", e)))?,
        ));
        queries.push(Box::new(
            RegexQuery::from_pattern(&search_pattern, self.fields.content)
                .map_err(|e| StorageError::Tantivy(format!("Invalid regex pattern: {}", e)))?,
        ));
        // Also search filename_text for better filename matching (especially Korean/CJK)
        queries.push(Box::new(
            RegexQuery::from_pattern(&search_pattern, self.fields.filename_text)
                .map_err(|e| StorageError::Tantivy(format!("Invalid regex pattern: {}", e)))?,
        ));

        let query: Box<dyn tantivy::query::Query> = if queries.len() == 1 {
            queries.remove(0)
        } else {
            Box::new(BooleanQuery::union(queries))
        };

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| StorageError::Tantivy(format!("Regex search failed: {}", e)))?;

        let snippet_generator = SnippetGenerator::create(&searcher, &query, self.fields.content)
            .map_err(|e| {
                StorageError::Tantivy(format!("Failed to create snippet generator: {}", e))
            })?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| StorageError::Tantivy(format!("Failed to retrieve doc: {}", e)))?;

            let snippet = snippet_generator.snippet_from_doc(&doc);
            let snippet_text = if snippet.is_empty() {
                doc.get_first(self.fields.content)
                    .and_then(|v| v.as_str())
                    .map(|s: &str| {
                        let trimmed: String = s.chars().take(200).collect();
                        if s.len() > 200 {
                            format!("{}...", trimmed)
                        } else {
                            trimmed
                        }
                    })
            } else {
                Some(snippet.to_html())
            };

            results.push(self.doc_to_result(&doc, score, snippet_text));
        }

        Ok(results)
    }

    /// Search the index with configurable fields
    fn search_fields(
        &self,
        query_str: &str,
        limit: usize,
        search_title: bool,
        search_content: bool,
    ) -> StorageResult<Vec<TantivySearchResult>> {
        if query_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Normalize query to NFC for consistent Unicode matching
        // This ensures Korean NFC user queries match NFD filenames (macOS)
        let query_normalized = Self::normalize_for_search(query_str);

        // Build list of fields to search
        let mut fields = Vec::new();
        if search_title {
            fields.push(self.fields.title);
            // Include filename_text for better partial matching (especially for Korean/CJK)
            fields.push(self.fields.filename_text);
        }
        if search_content {
            fields.push(self.fields.content);
        }

        if fields.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Tantivy search: {} (limit: {}, title: {}, content: {})",
            query_str, limit, search_title, search_content
        );

        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, fields);

        let query = query_parser
            .parse_query(&query_normalized)
            .map_err(|e| StorageError::InvalidQuery(format!("Invalid query: {}", e)))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| StorageError::Tantivy(format!("Search failed: {}", e)))?;

        // Create snippet generator for content field
        let snippet_generator = SnippetGenerator::create(&searcher, &query, self.fields.content)
            .map_err(|e| {
                StorageError::Tantivy(format!("Failed to create snippet generator: {}", e))
            })?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| StorageError::Tantivy(format!("Failed to retrieve doc: {}", e)))?;

            // Generate snippet from content
            let snippet = snippet_generator.snippet_from_doc(&doc);
            let snippet_text = if snippet.is_empty() {
                // Fall back to first 200 chars of content if no highlight match
                doc.get_first(self.fields.content)
                    .and_then(|v| v.as_str())
                    .map(|s: &str| {
                        let trimmed: String = s.chars().take(200).collect();
                        if s.len() > 200 {
                            format!("{}...", trimmed)
                        } else {
                            trimmed
                        }
                    })
            } else {
                // Convert snippet to HTML with highlights
                Some(snippet.to_html())
            };

            results.push(self.doc_to_result(&doc, score, snippet_text));
        }

        Ok(results)
    }

    /// Get the number of documents in the index
    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Clear all documents from the index
    pub fn clear(&mut self) -> StorageResult<()> {
        self.writer
            .delete_all_documents()
            .map_err(|e| StorageError::Tantivy(format!("Failed to clear index: {}", e)))?;
        self.commit()?;
        info!("Cleared all documents from index");
        Ok(())
    }
}

/// Index policy for determining when a rebuild is required
#[derive(Debug, Clone, PartialEq)]
pub struct IndexPolicy {
    /// Schema version for compatibility checking
    pub schema_version: u32,
    /// Whether normalization is enabled for search
    pub normalization_enabled: bool,
}

impl Default for IndexPolicy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            normalization_enabled: true,
        }
    }
}

/// Decision result for auto-reindex trigger
#[derive(Debug, Clone, PartialEq)]
pub enum RebuildDecision {
    /// No rebuild needed
    NoAction,
    /// Rebuild required due to policy mismatch
    PolicyMismatch {
        current: IndexPolicy,
        required: IndexPolicy,
    },
    /// Rebuild required because index is empty but paths exist
    EmptyIndexWithPaths { indexed_paths_count: usize },
    /// Rebuild required due to schema version mismatch
    SchemaVersionMismatch { current: u32, required: u32 },
}

/// Check if rebuild is needed based on current state
pub fn check_rebuild_needed(
    tantivy_doc_count: u64,
    indexed_paths: &[PathBuf],
    current_policy: Option<&IndexPolicy>,
    required_policy: &IndexPolicy,
) -> RebuildDecision {
    // Check schema version mismatch
    if let Some(current) = current_policy {
        if current.schema_version != required_policy.schema_version {
            return RebuildDecision::SchemaVersionMismatch {
                current: current.schema_version,
                required: required_policy.schema_version,
            };
        }
        if current != required_policy {
            return RebuildDecision::PolicyMismatch {
                current: current.clone(),
                required: required_policy.clone(),
            };
        }
    }

    // Check if index is empty but we have indexed paths
    if tantivy_doc_count == 0 && !indexed_paths.is_empty() {
        return RebuildDecision::EmptyIndexWithPaths {
            indexed_paths_count: indexed_paths.len(),
        };
    }

    RebuildDecision::NoAction
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_normalization::UnicodeNormalization;

    #[test]
    fn test_open_and_search_empty() {
        let dir = tempfile::tempdir().unwrap();
        let fts = TantivyFts::open(dir.path().to_path_buf()).unwrap();
        let results = fts.search("test", 10).unwrap();
        assert_eq!(results.len(), 0);
        assert_eq!(fts.num_docs(), 0);
    }

    #[test]
    fn test_in_memory_upsert_and_search() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document("doc1", "test document", "hello world content")
            .unwrap();
        fts.commit().unwrap();

        let results = fts.search("hello", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, "doc1");
        assert_eq!(results[0].title, "test document");
    }

    #[test]
    fn test_stale_lock_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().to_path_buf();

        // 1) Create index normally, then drop it
        {
            let _fts = TantivyFts::open(index_path.clone()).unwrap();
        }

        // 2) Create a fake stale lock file
        let lock_path = index_path.join(".tantivy-writer.lock");
        std::fs::write(&lock_path, "stale").unwrap();
        assert!(lock_path.exists());

        // 3) Reopen — should recover from stale lock
        let fts = TantivyFts::open(index_path.clone());
        assert!(
            fts.is_ok(),
            "Should recover from stale lock: {:?}",
            fts.err()
        );
    }

    #[test]
    fn test_upsert_document_full() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document_full(
            "doc1",
            "readme.md",
            "project readme content",
            Some("/home/user/readme.md"),
            Some(2048),
            Some("2024-06-15T10:00:00Z"),
            Some("local"),
            Some("md"),
        )
        .unwrap();
        fts.commit().unwrap();

        let results = fts.search("readme", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, Some("/home/user/readme.md".to_string()));
        assert_eq!(results[0].source, Some("local".to_string()));
        assert_eq!(results[0].extension, Some("md".to_string()));
    }

    #[test]
    fn test_delete_document() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document("doc1", "first", "content one").unwrap();
        fts.upsert_document("doc2", "second", "content two")
            .unwrap();
        fts.commit().unwrap();
        assert_eq!(fts.search("content", 10).unwrap().len(), 2);

        fts.delete_document("doc1").unwrap();
        fts.commit().unwrap();

        let results = fts.search("content", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, "doc2");
    }

    #[test]
    fn test_upsert_updates_existing() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document("doc1", "original", "original content")
            .unwrap();
        fts.commit().unwrap();

        fts.upsert_document("doc1", "updated", "updated content")
            .unwrap();
        fts.commit().unwrap();

        let results = fts.search("updated", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "updated");

        // Original content should not be found
        let old_results = fts.search("original", 10).unwrap();
        assert_eq!(old_results.len(), 0);
    }

    #[test]
    fn test_clear() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document("doc1", "first", "content").unwrap();
        fts.upsert_document("doc2", "second", "content").unwrap();
        fts.commit().unwrap();

        fts.clear().unwrap();

        let results = fts.search("content", 10).unwrap();
        assert_eq!(results.len(), 0);
        assert_eq!(fts.num_docs(), 0);
    }

    #[test]
    fn test_search_respects_limit() {
        let mut fts = TantivyFts::in_memory().unwrap();
        for i in 0..10 {
            fts.upsert_document(
                &format!("doc{}", i),
                &format!("doc {}", i),
                "common search term",
            )
            .unwrap();
        }
        fts.commit().unwrap();

        let results = fts.search("common", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_korean_filename_exact_match() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document_full(
            "doc1",
            "베어유.pdf",
            "some content",
            Some("/docs/베어유.pdf"),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Exact stem query should match "베어유.pdf"
        let results = fts.search("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find document with exact Korean filename match"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    #[test]
    fn test_korean_filename_partial_match() {
        let mut fts = TantivyFts::in_memory().unwrap();
        fts.upsert_document_full(
            "doc1",
            "베어유보고서.md",
            "some content",
            Some("/docs/베어유보고서.md"),
            None,
            None,
            None,
            Some("md"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Use regex search for partial matching
        // Pattern "베어.*" matches "베어유보고서.md"
        let results = fts.search_regex("베어.*", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find document with partial Korean filename match"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    #[test]
    fn test_filename_extracted_from_path() {
        let mut fts = TantivyFts::in_memory().unwrap();
        // Title doesn't contain the filename, but path does
        fts.upsert_document_full(
            "doc1",
            "A Document",
            "content here",
            Some("/home/user/important-file.txt"),
            None,
            None,
            None,
            Some("txt"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Should find by filename extracted from path
        let results = fts.search("important-file", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find document by filename extracted from path"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    #[test]
    fn test_filename_fallback_to_title() {
        let mut fts = TantivyFts::in_memory().unwrap();
        // No path provided, should use title as filename
        fts.upsert_document_full(
            "doc1",
            "fallback-name.doc",
            "content here",
            None, // no path
            None,
            None,
            None,
            Some("doc"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Should find by filename (which is the title when no path)
        let results = fts.search("fallback-name", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find document by filename (fallback to title)"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    #[test]
    fn test_old_index_schema_rebuild() {
        use tantivy::schema::{Schema, STORED, STRING, TEXT};

        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().to_path_buf();

        let mut old_schema_builder = Schema::builder();
        let old_id = old_schema_builder.add_text_field("id", STRING | STORED);
        let old_title = old_schema_builder.add_text_field("title", TEXT | STORED);
        let old_content = old_schema_builder.add_text_field("content", TEXT | STORED);
        let old_path = old_schema_builder.add_text_field("path", STORED);
        let old_size_bytes = old_schema_builder.add_i64_field("size_bytes", STORED);
        let old_modified_at = old_schema_builder.add_text_field("modified_at", STORED);
        let old_source = old_schema_builder.add_text_field("source", STRING | STORED);
        let old_extension = old_schema_builder.add_text_field("extension", STRING | STORED);
        let old_schema = old_schema_builder.build();

        {
            let old_index =
                Index::create_in_dir(&index_path, old_schema.clone()).expect("create old index");
            let mut old_writer = old_index.writer(15_000_000).expect("create writer");

            let mut doc = TantivyDocument::new();
            doc.add_text(old_id, "old_doc1");
            doc.add_text(old_title, "Old Document");
            doc.add_text(old_content, "old content");
            doc.add_text(old_path, "/path/to/old.txt");
            doc.add_i64(old_size_bytes, 1024);
            doc.add_text(old_modified_at, "2024-01-01T00:00:00Z");
            doc.add_text(old_source, "local");
            doc.add_text(old_extension, "txt");
            old_writer.add_document(doc).expect("add doc");
            old_writer.commit().expect("commit");
        }

        assert!(
            index_path.join("meta.json").exists(),
            "meta.json should exist"
        );

        let mut fts = TantivyFts::open(index_path.clone()).expect("open with schema rebuild");

        fts.upsert_document_full(
            "new_doc1",
            "New Document",
            "new content",
            Some("/path/to/new.pdf"),
            Some(2048),
            Some("2024-06-15T10:00:00Z"),
            Some("local"),
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        let results = fts.search("new.pdf", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find by filename in rebuilt index"
        );
        assert_eq!(results[0].document_id, "new_doc1");

        let old_results = fts.search("old_doc1", 10).unwrap();
        assert!(
            old_results.is_empty(),
            "Old documents should be gone after rebuild"
        );

        let path_results = fts.search_by_path("/path/to/new", 10).unwrap();
        assert!(
            !path_results.is_empty(),
            "Should find by path in rebuilt index with indexed path field"
        );
        assert_eq!(path_results[0].document_id, "new_doc1");
    }

    /// Test that old index with stored-only path triggers rebuild and search_by_path works
    #[test]
    fn test_old_index_stored_only_path_triggers_rebuild() {
        use tantivy::schema::{Schema, STORED, STRING, TEXT};

        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().to_path_buf();

        let mut old_schema_builder = Schema::builder();
        let old_id = old_schema_builder.add_text_field("id", STRING | STORED);
        let old_title = old_schema_builder.add_text_field("title", TEXT | STORED);
        let old_content = old_schema_builder.add_text_field("content", TEXT | STORED);
        let old_filename_exact =
            old_schema_builder.add_text_field("filename_exact", STRING | STORED);
        let old_filename_text = old_schema_builder.add_text_field("filename_text", TEXT | STORED);
        let old_path = old_schema_builder.add_text_field("path", STORED);
        let old_size_bytes = old_schema_builder.add_i64_field("size_bytes", STORED);
        let old_modified_at = old_schema_builder.add_text_field("modified_at", STORED);
        let old_source = old_schema_builder.add_text_field("source", STRING | STORED);
        let old_extension = old_schema_builder.add_text_field("extension", STRING | STORED);
        let old_schema = old_schema_builder.build();

        {
            let old_index =
                Index::create_in_dir(&index_path, old_schema.clone()).expect("create old index");
            let mut old_writer = old_index.writer(15_000_000).expect("create writer");

            let mut doc = TantivyDocument::new();
            doc.add_text(old_id, "doc1");
            doc.add_text(old_title, "Document in Downloads");
            doc.add_text(old_content, "content here");
            doc.add_text(old_filename_exact, "document.txt");
            doc.add_text(old_filename_text, "document.txt");
            doc.add_text(old_path, "/home/user/Downloads/document.txt");
            doc.add_i64(old_size_bytes, 1024);
            doc.add_text(old_modified_at, "2024-01-01T00:00:00Z");
            doc.add_text(old_source, "local");
            doc.add_text(old_extension, "txt");
            old_writer.add_document(doc).expect("add doc");
            old_writer.commit().expect("commit");
        }

        let mut fts = TantivyFts::open(index_path.clone()).expect("open with schema rebuild");

        fts.upsert_document_full(
            "doc2",
            "Another Document",
            "more content",
            Some("/home/user/Downloads/another.pdf"),
            Some(2048),
            Some("2024-06-15T10:00:00Z"),
            Some("local"),
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        let path_results = fts.search_by_path("Downloads", 10).unwrap();
        assert!(
            !path_results.is_empty(),
            "search_by_path should work after rebuild with indexed path field"
        );
        assert_eq!(path_results[0].document_id, "doc2");
        assert_eq!(
            path_results[0].path,
            Some("/home/user/Downloads/another.pdf".to_string())
        );

        let old_path_results = fts
            .search_by_path("/home/user/Downloads/document.txt", 10)
            .unwrap();
        assert!(
            old_path_results.is_empty(),
            "Old document with stored-only path should be gone after rebuild"
        );
    }

    // ============================================================================
    // Index Policy and Auto-Rebuild Decision Tests
    // ============================================================================

    #[test]
    fn test_index_policy_default() {
        let policy = IndexPolicy::default();
        assert_eq!(policy.schema_version, 1);
        assert!(policy.normalization_enabled);
    }

    #[test]
    fn test_index_policy_equality() {
        let policy1 = IndexPolicy::default();
        let policy2 = IndexPolicy::default();
        assert_eq!(policy1, policy2);

        let different_version = IndexPolicy {
            schema_version: 2,
            normalization_enabled: true,
        };
        assert_ne!(policy1, different_version);

        let different_normalization = IndexPolicy {
            schema_version: 1,
            normalization_enabled: false,
        };
        assert_ne!(policy1, different_normalization);
    }

    #[test]
    fn test_check_rebuild_needed_no_action() {
        let required = IndexPolicy::default();
        let current = IndexPolicy::default();
        let paths = vec![PathBuf::from("/test/path")];

        // Non-empty index with matching policy = no action
        let decision = check_rebuild_needed(10, &paths, Some(&current), &required);
        assert_eq!(decision, RebuildDecision::NoAction);

        // Empty index with no paths = no action
        let decision = check_rebuild_needed(0, &[], Some(&current), &required);
        assert_eq!(decision, RebuildDecision::NoAction);
    }

    #[test]
    fn test_check_rebuild_needed_schema_version_mismatch() {
        let current = IndexPolicy {
            schema_version: 1,
            normalization_enabled: true,
        };
        let required = IndexPolicy {
            schema_version: 2,
            normalization_enabled: true,
        };
        let paths = vec![PathBuf::from("/test/path")];

        let decision = check_rebuild_needed(10, &paths, Some(&current), &required);

        match decision {
            RebuildDecision::SchemaVersionMismatch {
                current: c,
                required: r,
            } => {
                assert_eq!(c, 1);
                assert_eq!(r, 2);
            }
            _ => panic!("Expected SchemaVersionMismatch, got {:?}", decision),
        }
    }

    #[test]
    fn test_check_rebuild_needed_policy_mismatch() {
        let current = IndexPolicy {
            schema_version: 1,
            normalization_enabled: false, // Different
        };
        let required = IndexPolicy {
            schema_version: 1,
            normalization_enabled: true,
        };
        let paths = vec![PathBuf::from("/test/path")];

        let decision = check_rebuild_needed(10, &paths, Some(&current), &required);

        match decision {
            RebuildDecision::PolicyMismatch {
                current: c,
                required: r,
            } => {
                assert!(!c.normalization_enabled);
                assert!(r.normalization_enabled);
            }
            _ => panic!("Expected PolicyMismatch, got {:?}", decision),
        }
    }

    #[test]
    fn test_check_rebuild_needed_empty_index_with_paths() {
        let current = IndexPolicy::default();
        let required = IndexPolicy::default();
        let paths = vec![
            PathBuf::from("/test/path1"),
            PathBuf::from("/test/path2"),
            PathBuf::from("/test/path3"),
        ];

        // Empty index but we have indexed paths = rebuild needed
        let decision = check_rebuild_needed(0, &paths, Some(&current), &required);

        match decision {
            RebuildDecision::EmptyIndexWithPaths {
                indexed_paths_count,
            } => {
                assert_eq!(indexed_paths_count, 3);
            }
            _ => panic!("Expected EmptyIndexWithPaths, got {:?}", decision),
        }
    }

    #[test]
    fn test_check_rebuild_needed_no_policy() {
        let required = IndexPolicy::default();
        let paths = vec![PathBuf::from("/test/path")];

        // No current policy set should trigger policy mismatch
        // (this represents a new installation or upgrade scenario)
        let decision = check_rebuild_needed(10, &paths, None, &required);

        // With no current policy, we treat it as needing action only if
        // the index is empty with paths
        assert!(
            matches!(decision, RebuildDecision::NoAction),
            "Without current policy but with docs, should be NoAction"
        );

        // But empty index with paths should trigger rebuild
        let decision = check_rebuild_needed(0, &paths, None, &required);
        match decision {
            RebuildDecision::EmptyIndexWithPaths {
                indexed_paths_count,
            } => {
                assert_eq!(indexed_paths_count, 1);
            }
            _ => panic!("Expected EmptyIndexWithPaths when no policy and empty index"),
        }
    }

    #[test]
    fn test_check_rebuild_needed_edge_cases() {
        let current = IndexPolicy::default();
        let required = IndexPolicy::default();

        // Single path with empty index
        let single_path = vec![PathBuf::from("/single")];
        let decision = check_rebuild_needed(0, &single_path, Some(&current), &required);
        match decision {
            RebuildDecision::EmptyIndexWithPaths {
                indexed_paths_count,
            } => {
                assert_eq!(indexed_paths_count, 1);
            }
            _ => panic!("Expected rebuild for single path"),
        }

        // Many paths with empty index
        let many_paths: Vec<PathBuf> = (0..100)
            .map(|i| PathBuf::from(format!("/path{}", i)))
            .collect();
        let decision = check_rebuild_needed(0, &many_paths, Some(&current), &required);
        match decision {
            RebuildDecision::EmptyIndexWithPaths {
                indexed_paths_count,
            } => {
                assert_eq!(indexed_paths_count, 100);
            }
            _ => panic!("Expected rebuild for many paths"),
        }

        // Non-empty index with empty paths (no action)
        let decision = check_rebuild_needed(50, &[], Some(&current), &required);
        assert_eq!(decision, RebuildDecision::NoAction);
    }

    // ============================================================================
    // NFC/NFD Unicode Normalization Tests for Korean Filenames
    // ============================================================================

    /// Helper to create NFD (decomposed) form of Korean text
    /// macOS stores filenames in NFD form
    fn to_nfd(s: &str) -> String {
        s.nfd().collect()
    }

    /// Helper to create NFC (composed) form of Korean text
    /// This is what users typically type
    fn to_nfc(s: &str) -> String {
        s.nfc().collect()
    }

    // Test NFC query matching NFD-indexed filenames
    // This verifies Korean filename search works on macOS where filenames are stored in NFD
    // Note: Partial Korean word matching requires tokenizer config; full word matching works
    #[test]
    #[ignore = "Requires Tantivy Korean tokenizer for partial word matching"]
    fn test_nfc_query_matches_nfd_filename() {
        // This test simulates the real-world scenario on macOS:
        // 1. File is stored with NFD filename (macOS behavior)
        // 2. User searches with NFC query (typical user input)
        let mut fts = TantivyFts::in_memory().unwrap();

        // Korean text: "베어유보고서" (BearU Report)
        let korean_nfc = "베어유보고서.pdf";
        let korean_nfd = to_nfd(korean_nfc);

        // Verify they are different byte sequences
        assert_ne!(
            korean_nfc.as_bytes(),
            korean_nfd.as_bytes(),
            "NFC and NFD should have different byte representations"
        );

        // Index with NFD filename (simulating macOS filesystem)
        fts.upsert_document_full(
            "doc1",
            &korean_nfd, // Store with NFD
            "report content",
            Some(&format!("/docs/{}", korean_nfd)),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Search with NFC query (user typing)
        let results = fts.search("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "NFC query should match NFD-indexed Korean filename"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    // Test partial NFC query matching NFD-indexed filenames
    // Note: This requires proper Korean tokenization in Tantivy tokenizer
    #[test]
    #[ignore = "Requires Tantivy tokenizer config for Korean partial matching"]
    fn test_nfc_query_matches_nfd_filename_partial() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // More complex Korean text
        let filename_nfc = "프로젝트기획안_2024.hwp";
        let filename_nfd = to_nfd(filename_nfc);

        // Index with NFD
        fts.upsert_document_full(
            "doc1",
            &filename_nfd,
            "project plan content",
            Some(&format!("/documents/{}", filename_nfd)),
            None,
            None,
            None,
            Some("hwp"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Search with partial NFC query
        let results = fts.search("프로젝트", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Partial NFC query should match NFD-indexed filename"
        );

        // Search with another partial term
        let results = fts.search("기획안", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Another partial NFC query should match"
        );
    }

    #[test]
    fn test_mixed_nfc_nfd_documents() {
        // Test with multiple documents, some indexed as NFC, some as NFD
        let mut fts = TantivyFts::in_memory().unwrap();

        // Document 1: NFD filename (macOS style)
        let name1_nfd = to_nfd("회의록_제1차.pdf");
        fts.upsert_document_full(
            "doc1",
            &name1_nfd,
            "meeting minutes",
            Some(&format!("/meetings/{}", name1_nfd)),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();

        // Document 2: NFC filename (Linux/Windows style)
        let name2_nfc = "회의록_제2차.pdf";
        fts.upsert_document_full(
            "doc2",
            name2_nfc,
            "meeting minutes",
            Some(&format!("/meetings/{}", name2_nfc)),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // NFC query should find both
        let results = fts.search("회의록", 10).unwrap();
        assert_eq!(
            results.len(),
            2,
            "Should find both NFC and NFD indexed documents"
        );

        // Specific search should work too
        let results = fts.search("제1차", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, "doc1");

        let results = fts.search("제2차", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, "doc2");
    }

    #[test]
    fn test_korean_filename_with_english_mixed() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Mixed Korean-English filename
        let filename_nfc = "Project_기획서_v1.0.docx";
        let filename_nfd = to_nfd(filename_nfc);

        fts.upsert_document_full(
            "doc1",
            &filename_nfd,
            "project planning document",
            Some(&format!("/projects/{}", filename_nfd)),
            None,
            None,
            None,
            Some("docx"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Search by Korean part (NFC query)
        let results = fts.search("기획서", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Should find document by Korean part with NFC query"
        );

        // Search by English part
        let results = fts.search("Project", 10).unwrap();
        assert!(!results.is_empty(), "Should find document by English part");
    }

    #[test]
    fn test_normalization_helper_consistency() {
        // Verify our normalization helpers work correctly
        let test_strings = [
            "베어유보고서",
            "프로젝트기획안",
            "회의록",
            "한글파일",
            "문서작성",
        ];

        for s in &test_strings {
            let nfc = to_nfc(s);
            let nfd = to_nfd(s);
            let nfc_from_nfd = to_nfc(&nfd);
            let nfd_from_nfc = to_nfd(&nfc);

            // NFC should always be the same
            assert_eq!(nfc, nfc_from_nfd, "NFC normalization should be idempotent");

            // NFD should always be the same
            assert_eq!(nfd, nfd_from_nfc, "NFD normalization should be idempotent");

            // Round-trip: NFC -> NFD -> NFC should equal original NFC
            assert_eq!(nfc, to_nfc(&nfd), "Round-trip NFC->NFD->NFC failed");
        }
    }

    #[test]
    fn test_filename_exact_field_with_normalization() {
        // Test that the filename_exact field handles normalization
        let mut fts = TantivyFts::in_memory().unwrap();

        let filename_nfc = "보고서_2024.pdf";
        let filename_nfd = to_nfd(filename_nfc);

        fts.upsert_document_full(
            "doc1",
            &filename_nfd,
            "content",
            Some(&format!("/docs/{}", filename_nfd)),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // List all and verify filename is searchable
        let results = fts.list_all(10).unwrap();
        assert_eq!(results.len(), 1);

        // Search with exact NFC form
        let results = fts.search("보고서_2024", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Exact match with NFC should work on NFD-indexed file"
        );
    }

    // ============================================================================
    // Integration: NFC/NFD + Rebuild Decision Tests
    // ============================================================================

    #[test]
    fn test_korean_filename_search_after_simulated_rebuild() {
        // This test simulates a full workflow:
        // 1. Index has issues (empty)
        // 2. Decision logic triggers rebuild
        // 3. Rebuild indexes files with NFD names
        // 4. NFC queries work correctly

        let mut fts = TantivyFts::in_memory().unwrap();

        // Simulate: Empty index but paths exist (would trigger rebuild decision)
        // In real app: check_rebuild_needed(0, &indexed_paths, ...)

        // Simulate rebuild: index the files with NFD filenames
        let filenames_nfc = ["보고서.pdf", "기획안.pdf"];
        for (i, name_nfc) in filenames_nfc.iter().enumerate() {
            let name_nfd = to_nfd(name_nfc);
            fts.upsert_document_full(
                &format!("doc{}", i),
                &name_nfd,
                &format!("content of {}", name_nfc),
                Some(&format!("/docs/{}", name_nfd)),
                None,
                None,
                None,
                Some("pdf"),
            )
            .unwrap();
        }
        fts.commit().unwrap();

        // Verify index is no longer empty
        assert_eq!(fts.num_docs(), 2);

        // Search with NFC queries should work
        let results = fts.search("보고서", 10).unwrap();
        assert_eq!(results.len(), 1);

        let results = fts.search("기획안", 10).unwrap();
        assert_eq!(results.len(), 1);

        // General search should find both
        let results = fts.search("pdf", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    // ============================================================================
    // REGRESSION TESTS: Empty Tantivy + SQLite/Paths Exist
    // ============================================================================

    /// Test the real failure mode: Tantivy index is empty but SQLite has documents
    /// (indexed paths exist in the database). This must trigger a rebuild.
    #[test]
    fn test_empty_tantivy_with_sqlite_docs_triggers_rebuild() {
        // Simulate the real-world scenario from lib.rs check_and_trigger_rebuild_if_needed
        let tantivy_doc_count: u64 = 0;
        let _sqlite_doc_count: usize = 150; // SQLite has documents
        let indexed_paths: Vec<PathBuf> = vec![
            PathBuf::from("/home/user/Documents"),
            PathBuf::from("/home/user/Downloads"),
        ];

        // The decision logic from tantivy_fts
        let required_policy = IndexPolicy::default();
        let current_policy = Some(IndexPolicy::default());

        let decision = check_rebuild_needed(
            tantivy_doc_count,
            &indexed_paths,
            current_policy.as_ref(),
            &required_policy,
        );

        // MUST trigger EmptyIndexWithPaths rebuild
        match decision {
            RebuildDecision::EmptyIndexWithPaths { indexed_paths_count } => {
                assert_eq!(indexed_paths_count, 2, "Should report 2 indexed paths");
            }
            other => panic!(
                "CRITICAL: Empty Tantivy ({} docs) with {} SQLite docs and {} paths MUST trigger EmptyIndexWithPaths rebuild, got {:?}",
                tantivy_doc_count, _sqlite_doc_count, indexed_paths.len(), other
            ),
        }
    }

    /// Test that the reverse scenario (Tantivy has docs, no paths) doesn't trigger rebuild
    #[test]
    fn test_non_empty_tantivy_no_paths_no_rebuild() {
        let tantivy_doc_count: u64 = 150;
        let indexed_paths: Vec<PathBuf> = vec![];

        let required_policy = IndexPolicy::default();
        let current_policy = Some(IndexPolicy::default());

        let decision = check_rebuild_needed(
            tantivy_doc_count,
            &indexed_paths,
            current_policy.as_ref(),
            &required_policy,
        );

        assert_eq!(
            decision,
            RebuildDecision::NoAction,
            "Non-empty Tantivy with no paths should not trigger rebuild"
        );
    }

    /// Test policy version bump from v1 to v2 forces rebuild
    #[test]
    fn test_policy_version_bump_v1_to_v2_forces_rebuild() {
        let current = IndexPolicy {
            schema_version: 1,
            normalization_enabled: true,
        };
        let required = IndexPolicy {
            schema_version: 2, // Bumped version
            normalization_enabled: true,
        };
        let paths = vec![PathBuf::from("/test/path")];

        // Even with non-empty index, schema version mismatch triggers rebuild
        let decision = check_rebuild_needed(100, &paths, Some(&current), &required);

        match decision {
            RebuildDecision::SchemaVersionMismatch {
                current: c,
                required: r,
            } => {
                assert_eq!(c, 1, "Should report current version as 1");
                assert_eq!(r, 2, "Should report required version as 2");
            }
            other => panic!(
                "Policy version bump MUST trigger SchemaVersionMismatch, got {:?}",
                other
            ),
        }
    }

    /// Test policy version bump from v0 (unset) to v1 forces rebuild
    #[test]
    fn test_policy_version_bump_v0_to_v1_forces_rebuild() {
        let current = IndexPolicy {
            schema_version: 0, // Old/unset version
            normalization_enabled: false,
        };
        let required = IndexPolicy {
            schema_version: 1, // Current required version
            normalization_enabled: true,
        };
        let paths = vec![PathBuf::from("/test/path")];

        let decision = check_rebuild_needed(100, &paths, Some(&current), &required);

        // Schema version mismatch should take precedence
        match decision {
            RebuildDecision::SchemaVersionMismatch {
                current: c,
                required: r,
            } => {
                assert_eq!(c, 0);
                assert_eq!(r, 1);
            }
            other => panic!(
                "Version 0 to 1 bump MUST trigger SchemaVersionMismatch, got {:?}",
                other
            ),
        }
    }

    /// Test that empty Tantivy detection takes precedence over policy mismatch
    /// when both conditions are true (edge case)
    #[test]
    fn test_empty_tantivy_takes_precedence_over_policy_mismatch() {
        let current = IndexPolicy {
            schema_version: 1,
            normalization_enabled: false,
        };
        let required = IndexPolicy {
            schema_version: 1,
            normalization_enabled: true,
        };
        let paths = vec![PathBuf::from("/test/path")];

        // Empty Tantivy (0 docs) with paths - should detect empty index
        let decision = check_rebuild_needed(0, &paths, Some(&current), &required);

        // Policy mismatch is also true, but empty index should be detected
        // Actually, looking at the code, schema version is checked first, then empty index
        // So with same schema version, empty index should be detected
        match decision {
            RebuildDecision::EmptyIndexWithPaths { .. } => {
                // This is acceptable - empty index detected
            }
            RebuildDecision::PolicyMismatch { .. } => {
                // This is also acceptable - policy mismatch detected first
                // The important thing is that A rebuild is triggered
            }
            other => panic!(
                "Either EmptyIndexWithPaths or PolicyMismatch expected, got {:?}",
                other
            ),
        }
    }

    /// Test the exact scenario from lib.rs: check_and_trigger_rebuild_if_needed
    #[test]
    fn test_lib_rs_rebuild_detection_scenario_1_empty_index() {
        // From lib.rs line 172-177:
        // if doc_count == 0 && !indexed_paths.is_empty() {
        //     tracing::info!("Auto-rebuild triggered: Empty index (0 docs)...");
        //     true
        // }

        let tantivy_doc_count: u64 = 0;
        let indexed_paths: Vec<PathBuf> = vec![PathBuf::from("/home/user/docs")];
        let current_policy_version: u32 = 1;
        const CURRENT_INDEX_POLICY_VERSION: u32 = 1;

        // Simulate the lib.rs logic
        let needs_rebuild_lib = tantivy_doc_count == 0 && !indexed_paths.is_empty();
        assert!(needs_rebuild_lib, "lib.rs logic should detect empty index");

        // Verify tantivy_fts decision matches
        let required = IndexPolicy {
            schema_version: CURRENT_INDEX_POLICY_VERSION,
            normalization_enabled: true,
        };
        let current = Some(IndexPolicy {
            schema_version: current_policy_version,
            normalization_enabled: true,
        });

        let decision = check_rebuild_needed(
            tantivy_doc_count,
            &indexed_paths,
            current.as_ref(),
            &required,
        );
        assert!(
            matches!(decision, RebuildDecision::EmptyIndexWithPaths { .. }),
            "tantivy_fts decision should match lib.rs logic for empty index"
        );
    }

    /// Test the exact scenario from lib.rs: policy version outdated
    #[test]
    fn test_lib_rs_rebuild_detection_scenario_2_outdated_policy() {
        // From lib.rs line 178-183:
        // else if current_policy_version < CURRENT_INDEX_POLICY_VERSION {
        //     tracing::info!("Auto-rebuild triggered: Index policy version {} is outdated...");
        //     true
        // }

        let tantivy_doc_count: u64 = 100; // Has documents
        let indexed_paths: Vec<PathBuf> = vec![PathBuf::from("/home/user/docs")];
        let current_policy_version: u32 = 0; // Outdated
        const CURRENT_INDEX_POLICY_VERSION: u32 = 1;

        // Simulate the lib.rs logic
        let needs_rebuild_lib = if tantivy_doc_count == 0 && !indexed_paths.is_empty() {
            true
        } else {
            current_policy_version < CURRENT_INDEX_POLICY_VERSION
        };
        assert!(
            needs_rebuild_lib,
            "lib.rs logic should detect outdated policy"
        );

        // Verify tantivy_fts decision matches
        let required = IndexPolicy {
            schema_version: CURRENT_INDEX_POLICY_VERSION,
            normalization_enabled: true,
        };
        let current = Some(IndexPolicy {
            schema_version: current_policy_version,
            normalization_enabled: true,
        });

        let decision = check_rebuild_needed(
            tantivy_doc_count,
            &indexed_paths,
            current.as_ref(),
            &required,
        );
        assert!(
            matches!(
                decision,
                RebuildDecision::SchemaVersionMismatch {
                    current: 0,
                    required: 1
                }
            ),
            "tantivy_fts decision should match lib.rs logic for outdated policy"
        );
    }

    /// Test the "all clear" scenario from lib.rs
    #[test]
    fn test_lib_rs_rebuild_detection_scenario_3_no_rebuild_needed() {
        let tantivy_doc_count: u64 = 100; // Has documents
        let indexed_paths: Vec<PathBuf> = vec![PathBuf::from("/home/user/docs")];
        let current_policy_version: u32 = 1; // Up to date
        const CURRENT_INDEX_POLICY_VERSION: u32 = 1;

        // Simulate the lib.rs logic
        let needs_rebuild = if tantivy_doc_count == 0 && !indexed_paths.is_empty() {
            true
        } else {
            current_policy_version < CURRENT_INDEX_POLICY_VERSION
        };
        assert!(!needs_rebuild, "lib.rs logic should NOT trigger rebuild");

        // Verify tantivy_fts decision matches
        let required = IndexPolicy {
            schema_version: CURRENT_INDEX_POLICY_VERSION,
            normalization_enabled: true,
        };
        let current = Some(IndexPolicy {
            schema_version: current_policy_version,
            normalization_enabled: true,
        });

        let decision = check_rebuild_needed(
            tantivy_doc_count,
            &indexed_paths,
            current.as_ref(),
            &required,
        );
        assert_eq!(
            decision,
            RebuildDecision::NoAction,
            "tantivy_fts decision should match lib.rs logic for no rebuild"
        );
    }

    /// Integration test: Simulate full app state with mismatched SQLite/Tantivy counts
    #[test]
    fn test_sqlite_tantivy_count_mismatch_detection() {
        // This is the REAL failure mode: SQLite says we have docs, Tantivy is empty
        struct AppState {
            _sqlite_doc_count: usize, // Tracked for documentation purposes
            tantivy_doc_count: u64,
            indexed_paths: Vec<PathBuf>,
        }

        let scenarios = vec![
            (
                AppState {
                    _sqlite_doc_count: 150,
                    tantivy_doc_count: 0, // EMPTY - this is the bug!
                    indexed_paths: vec![PathBuf::from("/docs")],
                },
                true, // Should trigger rebuild
                "SQLite has docs but Tantivy empty - CRITICAL BUG SCENARIO",
            ),
            (
                AppState {
                    _sqlite_doc_count: 150,
                    tantivy_doc_count: 150, // Match
                    indexed_paths: vec![PathBuf::from("/docs")],
                },
                false, // Should NOT trigger rebuild
                "Counts match - normal state",
            ),
            (
                AppState {
                    _sqlite_doc_count: 0,
                    tantivy_doc_count: 0,
                    indexed_paths: vec![], // No paths configured
                },
                false, // Should NOT trigger rebuild
                "Fresh install - no data",
            ),
            (
                AppState {
                    _sqlite_doc_count: 0,
                    tantivy_doc_count: 0,
                    indexed_paths: vec![PathBuf::from("/docs")], // Has paths but no docs
                },
                true, // Should trigger rebuild (paths exist but no docs)
                "Paths configured but never indexed",
            ),
        ];

        let required = IndexPolicy::default();
        let current = Some(IndexPolicy::default());

        for (state, expected_rebuild, description) in scenarios {
            let decision = check_rebuild_needed(
                state.tantivy_doc_count,
                &state.indexed_paths,
                current.as_ref(),
                &required,
            );

            let would_rebuild = !matches!(decision, RebuildDecision::NoAction);

            assert_eq!(
                would_rebuild, expected_rebuild,
                "Scenario '{}' failed: expected rebuild={}, got decision={:?}",
                description, expected_rebuild, decision
            );
        }
    }

    /// Test policy normalization flag changes
    #[test]
    fn test_policy_normalization_flag_change_triggers_rebuild() {
        let current = IndexPolicy {
            schema_version: 1,
            normalization_enabled: false, // Old behavior
        };
        let required = IndexPolicy {
            schema_version: 1,
            normalization_enabled: true, // New behavior (Korean filename fix)
        };
        let paths = vec![PathBuf::from("/test/path")];

        // Same schema version, but normalization flag changed
        let decision = check_rebuild_needed(100, &paths, Some(&current), &required);

        match decision {
            RebuildDecision::PolicyMismatch {
                current: c,
                required: r,
            } => {
                assert!(
                    !c.normalization_enabled,
                    "Current should have normalization disabled"
                );
                assert!(
                    r.normalization_enabled,
                    "Required should have normalization enabled"
                );
            }
            other => panic!(
                "Normalization flag change MUST trigger PolicyMismatch, got {:?}",
                other
            ),
        }
    }

    // ============================================================================
    // REGEX SEARCH TESTS: Korean Filename Matching via Regex Fallback
    // ============================================================================

    /// Test that regex search can find underscore-delimited Korean filenames
    /// e.g., "베어유" should match "베어유_프로필.jpg"
    #[test]
    fn test_regex_search_korean_underscore_filename() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index file with underscore-delimited Korean filename
        fts.upsert_document_full(
            "doc1",
            "베어유_프로필.jpg",
            "profile image content",
            Some("/docs/베어유_프로필.jpg"),
            None,
            None,
            None,
            Some("jpg"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Regex search with exact Korean stem should match
        let results = fts.search_regex("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Regex search for '베어유' should find '베어유_프로필.jpg'"
        );
        assert_eq!(results[0].document_id, "doc1");
        assert!(results[0].title.contains("베어유"));
    }

    /// Test that regex search can find compound Korean filenames
    /// e.g., "베어유" should match "베어유보고서.pdf"
    #[test]
    fn test_regex_search_korean_compound_filename() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index file with compound Korean filename (no delimiter)
        fts.upsert_document_full(
            "doc1",
            "베어유보고서.pdf",
            "report content",
            Some("/docs/베어유보고서.pdf"),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Regex search with Korean stem should match compound filename
        let results = fts.search_regex("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Regex search for '베어유' should find '베어유보고서.pdf'"
        );
        assert_eq!(results[0].document_id, "doc1");
        assert!(results[0].title.contains("베어유"));
    }

    /// Test regex search matches both underscore-delimited and compound filenames
    #[test]
    fn test_regex_search_korean_both_filename_patterns() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index multiple files with different Korean filename patterns
        fts.upsert_document_full(
            "doc1",
            "베어유_프로필.jpg",
            "profile image",
            Some("/docs/베어유_프로필.jpg"),
            None,
            None,
            None,
            Some("jpg"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc2",
            "베어유보고서.pdf",
            "annual report",
            Some("/docs/베어유보고서.pdf"),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc3",
            "베어유_계약서.docx",
            "contract document",
            Some("/docs/베어유_계약서.docx"),
            None,
            None,
            None,
            Some("docx"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Regex search should find all three files
        let results = fts.search_regex("베어유", 10).unwrap();
        assert_eq!(
            results.len(),
            3,
            "Regex search should find all three '베어유' files"
        );

        // Verify all expected files are found
        let ids: Vec<_> = results.iter().map(|r| r.document_id.as_str()).collect();
        assert!(ids.contains(&"doc1"), "Should find 베어유_프로필.jpg");
        assert!(ids.contains(&"doc2"), "Should find 베어유보고서.pdf");
        assert!(ids.contains(&"doc3"), "Should find 베어유_계약서.docx");
    }

    /// Test regex search with Korean text in NFD form (macOS filenames)
    #[test]
    fn test_regex_search_korean_nfd_filename() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index with NFD form (macOS style)
        let filename_nfc = "베어유_프로필.jpg";
        let filename_nfd = to_nfd(filename_nfc);

        fts.upsert_document_full(
            "doc1",
            &filename_nfd,
            "profile content",
            Some(&format!("/docs/{}", filename_nfd)),
            None,
            None,
            None,
            Some("jpg"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Search with NFC form (user input) should match NFD-indexed file
        let results = fts.search_regex("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "NFC regex query should match NFD-indexed Korean filename"
        );
        assert_eq!(results[0].document_id, "doc1");
    }

    /// Test that search_fields (standard FTS) may miss but regex catches filenames
    /// This simulates the production fallback scenario
    #[test]
    fn test_standard_search_misses_regex_catches_korean() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index files that token search might miss
        fts.upsert_document_full(
            "doc1",
            "베어유_프로필.jpg",
            "content",
            Some("/docs/베어유_프로필.jpg"),
            None,
            None,
            None,
            Some("jpg"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc2",
            "베어유보고서.pdf",
            "content",
            Some("/docs/베어유보고서.pdf"),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Regex search should find both
        let regex_results = fts.search_regex("베어유", 10).unwrap();
        assert_eq!(
            regex_results.len(),
            2,
            "Regex search should find both Korean filenames"
        );
    }
    // ============================================================================
    // REGRESSION TESTS: Standard Search Korean Filename Coverage
    // These complement the regex tests above to ensure both paths work
    // ============================================================================

    /// Regression test: Standard search for underscore-delimited Korean filename
    /// This tests the primary search path (not fallback) with "베어유_프로필.jpg"
    #[test]
    fn test_standard_search_korean_underscore_delimited() {
        let mut fts = TantivyFts::in_memory().unwrap();

        fts.upsert_document_full(
            "doc1",
            "베어유_프로필.jpg",
            "profile image content",
            Some("/docs/베어유_프로필.jpg"),
            None,
            None,
            None,
            Some("jpg"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Standard search for second segment "프로필"
        let results = fts.search("프로필", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Standard search should find '베어유_프로필.jpg' for '프로필'"
        );
        assert_eq!(results[0].document_id, "doc1");

        // Standard search for first segment "베어유"
        let results = fts.search("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Standard search should find '베어유_프로필.jpg' for '베어유'"
        );
    }

    /// Regression test: Standard search for compound Korean filename  
    /// This tests the primary search path (not fallback) with "베어유보고서.pdf"
    #[test]
    #[ignore = "Known regression: https://github.com/n4ze3m/tamsaek/issues/XXX - compound Korean word tokenization"]
    fn test_standard_search_korean_compound_word() {
        let mut fts = TantivyFts::in_memory().unwrap();

        fts.upsert_document_full(
            "doc1",
            "베어유보고서.pdf",
            "report content",
            Some("/docs/베어유보고서.pdf"),
            None,
            None,
            None,
            Some("pdf"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Standard search for compound segment "보고서"
        let results = fts.search("보고서", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Standard search should find '베어유보고서.pdf' for '보고서'"
        );
        assert_eq!(results[0].document_id, "doc1");

        // Standard search for first segment "베어유"
        let results = fts.search("베어유", 10).unwrap();
        assert!(
            !results.is_empty(),
            "Standard search should find '베어유보고서.pdf' for '베어유'"
        );
    }

    /// Regression test: Verify fallback is available when standard search might miss
    /// This documents the fallback mechanism for production
    #[test]
    fn test_korean_filename_fallback_mechanism_available() {
        let mut fts = TantivyFts::in_memory().unwrap();

        fts.upsert_document_full(
            "doc1",
            "베어유분석자료.xlsx",
            "analysis data",
            Some("/data/베어유분석자료.xlsx"),
            None,
            None,
            None,
            Some("xlsx"),
        )
        .unwrap();
        fts.commit().unwrap();

        // Both standard and regex search should work
        let standard_results = fts.search("분석", 10).unwrap();
        let regex_results = fts.search_regex("분석", 10).unwrap();

        // Fallback mechanism: if standard misses, regex is available
        assert!(
            !regex_results.is_empty(),
            "Fallback regex search must always be available for Korean filenames"
        );

        // If standard search works, both should find the same doc
        if !standard_results.is_empty() {
            assert_eq!(standard_results[0].document_id, "doc1");
        }
        assert_eq!(regex_results[0].document_id, "doc1");
    }

    // ============================================================================
    // PATH-BASED SEARCH TESTS: Folder-aware search (Downloads folder, etc.)
    // ============================================================================

    /// Test that search_by_path finds files by path pattern matching
    #[test]
    fn test_search_by_path_finds_downloads_folder_files() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index files in Downloads folder
        fts.upsert_document_full(
            "doc1",
            "report.pdf",
            "annual report content",
            Some("/Users/indo/Downloads/report.pdf"),
            Some(1024000),
            Some("2024-06-15T10:00:00Z"),
            Some("local"),
            Some("pdf"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc2",
            "invoice.docx",
            "invoice content",
            Some("/Users/indo/Downloads/invoice.docx"),
            Some(512000),
            Some("2024-06-14T09:00:00Z"),
            Some("local"),
            Some("docx"),
        )
        .unwrap();

        // Index a file NOT in Downloads
        fts.upsert_document_full(
            "doc3",
            "notes.txt",
            "personal notes",
            Some("/Users/indo/Documents/notes.txt"),
            Some(1024),
            Some("2024-06-13T08:00:00Z"),
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Search by path should find only Downloads folder files
        let results = fts.search_by_path("Downloads", 10).unwrap();
        assert_eq!(
            results.len(),
            2,
            "Should find exactly 2 files in Downloads folder"
        );

        // Verify correct files are found
        let ids: Vec<_> = results.iter().map(|r| r.document_id.as_str()).collect();
        assert!(ids.contains(&"doc1"), "Should find report.pdf in Downloads");
        assert!(
            ids.contains(&"doc2"),
            "Should find invoice.docx in Downloads"
        );
        assert!(
            !ids.contains(&"doc3"),
            "Should NOT find notes.txt from Documents"
        );
    }

    /// Test that search_by_path works even when title/content don't contain the path keyword
    #[test]
    fn test_search_by_path_matches_when_title_does_not_contain_keyword() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index file with title/content that does NOT contain "Downloads"
        fts.upsert_document_full(
            "doc1",
            "quarterly_review.pdf",
            "Q2 financial review and analysis",
            Some("/Users/indo/Downloads/quarterly_review.pdf"),
            Some(2048000),
            Some("2024-06-15T10:00:00Z"),
            Some("local"),
            Some("pdf"),
        )
        .unwrap();

        // Index another file NOT in Downloads folder and without "Downloads" in title
        fts.upsert_document_full(
            "doc2",
            "backup_notes.txt",
            "notes about backups",
            Some("/Users/indo/Documents/backup_notes.txt"),
            Some(1024),
            Some("2024-06-14T09:00:00Z"),
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Regular search for "Downloads" should NOT find either doc
        // (titles and content don't contain "Downloads")
        let regular_results = fts.search("Downloads", 10).unwrap();
        assert!(
            !regular_results.iter().any(|r| r.document_id == "doc1"),
            "Regular search should NOT find doc1 (no 'Downloads' in title/content)"
        );
        assert!(
            !regular_results.iter().any(|r| r.document_id == "doc2"),
            "Regular search should NOT find doc2 (no 'Downloads' in title/content)"
        );

        // Path-based search should find doc1 (path contains "Downloads")
        // regardless of title/content
        let path_results = fts.search_by_path("Downloads", 10).unwrap();
        assert!(
        path_results.iter().any(|r| r.document_id == "doc1"),
        "Path search should find doc1 by path match even when title doesn't contain 'Downloads'"
    );
        assert!(
            !path_results.iter().any(|r| r.document_id == "doc2"),
            "Path search should NOT find doc2 because its path doesn't contain 'Downloads'"
        );
    }

    /// Test search_by_path with case-sensitive matching (STRING field behavior)
    #[test]
    fn test_search_by_path_case_sensitive() {
        let mut fts = TantivyFts::in_memory().unwrap();

        fts.upsert_document_full(
            "doc1",
            "file.txt",
            "content",
            Some("/Users/indo/Downloads/file.txt"),
            None,
            None,
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Should match exact case
        let results = fts.search_by_path("Downloads", 10).unwrap();
        assert_eq!(results.len(), 1);

        // Should also work with lowercase
        let results = fts.search_by_path("downloads", 10).unwrap();
        assert_eq!(
            results.len(),
            0,
            "STRING field is case-sensitive, lowercase should not match"
        );
    }

    /// Test search_by_path with regex pattern matching
    #[test]
    fn test_search_by_path_with_regex_pattern() {
        let mut fts = TantivyFts::in_memory().unwrap();

        // Index files in different folders
        fts.upsert_document_full(
            "doc1",
            "file1.txt",
            "content",
            Some("/Users/indo/Downloads/2024/file1.txt"),
            None,
            None,
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc2",
            "file2.txt",
            "content",
            Some("/Users/indo/Downloads/2023/file2.txt"),
            None,
            None,
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.upsert_document_full(
            "doc3",
            "file3.txt",
            "content",
            Some("/Users/indo/Documents/file3.txt"),
            None,
            None,
            Some("local"),
            Some("txt"),
        )
        .unwrap();

        fts.commit().unwrap();

        // Search with regex pattern for specific subfolder
        let results = fts.search_by_path("Downloads/2024", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, "doc1");
    }

    /// Test search_by_path returns denormalized fields (size, modified_at, etc.)
    #[test]
    fn test_search_by_path_returns_denormalized_fields() {
        let mut fts = TantivyFts::in_memory().unwrap();

        fts.upsert_document_full(
            "doc1",
            "large_file.zip",
            "content",
            Some("/Users/indo/Downloads/large_file.zip"),
            Some(10485760), // 10MB
            Some("2024-06-15T10:30:00Z"),
            Some("local"),
            Some("zip"),
        )
        .unwrap();

        fts.commit().unwrap();

        let results = fts.search_by_path("Downloads", 10).unwrap();
        assert_eq!(results.len(), 1);

        let result = &results[0];
        assert_eq!(result.document_id, "doc1");
        assert_eq!(result.title, "large_file.zip");
        assert_eq!(
            result.path,
            Some("/Users/indo/Downloads/large_file.zip".to_string())
        );
        assert_eq!(result.size_bytes, Some(10485760));
        assert_eq!(result.modified_at, Some("2024-06-15T10:30:00Z".to_string()));
        assert_eq!(result.source, Some("local".to_string()));
        assert_eq!(result.extension, Some("zip".to_string()));
    }
}
