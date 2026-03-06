//! Tantivy-based full-text search with incremental indexing and snippet extraction.

use std::path::PathBuf;

use parking_lot::RwLock;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{AllQuery, BooleanQuery, QueryParser, RegexQuery, TermQuery};
use tantivy::schema::document::Value;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, STORED, STRING,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};
use tracing::debug;

use crate::document::Document;
use crate::error::{Result, TamsaekError};
use crate::search::SearchResult;

/// Schema field handles for the index.
#[derive(Clone)]
pub struct SchemaFields {
    /// Document unique identifier
    pub id: Field,
    /// Document title
    pub title: Field,
    /// Document content
    pub content: Field,
    /// File path
    pub path: Field,
    /// File extension
    pub extension: Field,
    /// File size in bytes
    pub size_bytes: Field,
    /// Last modification time (ISO 8601 string)
    pub modified_at: Field,
    /// Document source
    pub source: Field,
}

/// A full-text search index backed by Tantivy.
pub struct TamsaekIndex {
    index: Index,
    writer: RwLock<IndexWriter>,
    reader: IndexReader,
    fields: SchemaFields,
    #[allow(dead_code)]
    schema: Schema,
}

impl TamsaekIndex {
    /// Creates the schema for the index.
    fn build_schema() -> (Schema, SchemaFields) {
        let mut schema_builder = Schema::builder();

        // Text field options with indexing
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        let id = schema_builder.add_text_field("id", STRING | STORED);
        let title = schema_builder.add_text_field("title", text_options.clone());
        let content = schema_builder.add_text_field("content", text_options);
        let path = schema_builder.add_text_field("path", STORED);
        let extension = schema_builder.add_text_field("extension", STRING | STORED);
        let size_bytes = schema_builder.add_i64_field("size_bytes", STORED);
        let modified_at = schema_builder.add_text_field("modified_at", STRING | STORED);
        let source = schema_builder.add_text_field("source", STRING | STORED);

        let schema = schema_builder.build();
        let fields = SchemaFields {
            id,
            title,
            content,
            path,
            extension,
            size_bytes,
            modified_at,
            source,
        };

        (schema, fields)
    }

    /// Opens an index at the specified path, creating it if it doesn't exist.
    ///
    /// # Arguments
    /// * `path` - Directory path where the index will be stored
    ///
    /// # Errors
    /// Returns `TamsaekError::Index` if the index cannot be opened or created.
    pub fn open(path: PathBuf) -> Result<Self> {
        debug!("Opening index at {:?}", path);

        std::fs::create_dir_all(&path)?;
        let (schema, fields) = Self::build_schema();

        let dir = MmapDirectory::open(&path)?;
        let index = Index::open_or_create(dir, schema.clone())?;

        let writer = index.writer(50_000_000)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        debug!("Index opened successfully");

        Ok(Self {
            index,
            writer: RwLock::new(writer),
            reader,
            fields,
            schema,
        })
    }

    /// Creates an in-memory index (useful for testing).
    ///
    /// # Errors
    /// Returns `TamsaekError::Index` if the index cannot be created.
    pub fn in_memory() -> Result<Self> {
        debug!("Creating in-memory index");

        let (schema, fields) = Self::build_schema();
        let index = Index::create_in_ram(schema.clone());

        let writer = index.writer(50_000_000)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        debug!("In-memory index created successfully");

        Ok(Self {
            index,
            writer: RwLock::new(writer),
            reader,
            fields,
            schema,
        })
    }

    /// Adds a document to the index.
    ///
    /// Note: Changes are not visible until `commit()` is called.
    ///
    /// # Arguments
    /// * `doc` - The document to add
    ///
    /// # Errors
    /// Returns `TamsaekError::Document` if the document cannot be added.
    pub fn add_document(&self, doc: &Document) -> Result<()> {
        debug!("Adding document: {}", doc.id);

        // First delete any existing document with the same ID
        self.delete_document(&doc.id)?;

        let mut tantivy_doc = TantivyDocument::default();
        tantivy_doc.add_text(self.fields.id, &doc.id);
        tantivy_doc.add_text(self.fields.title, &doc.title);
        tantivy_doc.add_text(self.fields.content, &doc.content);

        if let Some(ref path) = doc.path {
            tantivy_doc.add_text(self.fields.path, path);
        }
        if let Some(ref ext) = doc.extension {
            tantivy_doc.add_text(self.fields.extension, ext);
        }
        if let Some(size) = doc.size_bytes {
            tantivy_doc.add_i64(self.fields.size_bytes, size);
        }
        if let Some(modified) = doc.modified_at {
            tantivy_doc.add_text(self.fields.modified_at, modified.to_rfc3339());
        }
        tantivy_doc.add_text(self.fields.source, &doc.source);

        self.writer
            .write()
            .add_document(tantivy_doc)
            .map_err(|e| TamsaekError::Document(e.to_string()))?;

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
        self.delete_document(id)?;

        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.id, id);
        doc.add_text(self.fields.title, title);
        doc.add_text(self.fields.content, content);

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
            .write()
            .add_document(doc)
            .map_err(|e| TamsaekError::Document(e.to_string()))?;

        debug!("Upserted document: {}", id);
        Ok(())
    }

    /// Deletes a document by its ID.
    ///
    /// # Arguments
    /// * `id` - The document ID to delete
    ///
    /// # Errors
    /// Returns `TamsaekError::Document` if the operation fails.
    pub fn delete_document(&self, id: &str) -> Result<()> {
        debug!("Deleting document: {}", id);

        let term = Term::from_field_text(self.fields.id, id);
        self.writer.write().delete_term(term);

        Ok(())
    }

    /// Commits all pending changes to the index.
    ///
    /// # Errors
    /// Returns `TamsaekError::Index` if the commit fails.
    pub fn commit(&self) -> Result<()> {
        debug!("Committing index changes");

        self.writer
            .write()
            .commit()
            .map_err(|e| TamsaekError::Index(e.to_string()))?;

        // Reload the reader to make changes visible immediately
        self.reader
            .reload()
            .map_err(|e| TamsaekError::Index(e.to_string()))?;

        Ok(())
    }

    /// Returns the number of documents in the index.
    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Clears all documents from the index.
    ///
    /// # Errors
    /// Returns `TamsaekError::Index` if the operation fails.
    pub fn clear(&self) -> Result<()> {
        debug!("Clearing all documents from index");

        self.writer
            .write()
            .delete_all_documents()
            .map_err(|e| TamsaekError::Index(e.to_string()))?;
        self.commit()?;

        Ok(())
    }

    /// Searches for documents matching the query string.
    ///
    /// # Arguments
    /// * `query` - The search query
    /// * `limit` - Maximum number of results to return
    ///
    /// # Errors
    /// Returns `TamsaekError::Search` if the search fails.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_fields(query_str, limit, true, true)
    }

    pub fn search_content_only(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_fields(query_str, limit, false, true)
    }

    pub fn search_title_only(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_fields(query_str, limit, true, false)
    }

    fn search_fields(
        &self,
        query_str: &str,
        limit: usize,
        search_title: bool,
        search_content: bool,
    ) -> Result<Vec<SearchResult>> {
        if query_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut fields = Vec::new();
        if search_title {
            fields.push(self.fields.title);
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
            .parse_query(query_str)
            .map_err(|e| TamsaekError::InvalidQuery(format!("Invalid query: {}", e)))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| TamsaekError::Search(format!("Search failed: {}", e)))?;

        let snippet_generator = SnippetGenerator::create(&searcher, &query, self.fields.content)
            .map_err(|e| {
                TamsaekError::Search(format!("Failed to create snippet generator: {}", e))
            })?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| TamsaekError::Search(format!("Failed to retrieve doc: {}", e)))?;

            let snippet = snippet_generator.snippet_from_doc(&doc);
            let snippet_text = if snippet.is_empty() {
                self.get_text_field(&doc, self.fields.content).map(|s| {
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

            results.push(self.doc_to_search_result(&doc, score, snippet_text));
        }

        debug!("Found {} results", results.len());
        Ok(results)
    }

    /// Searches for documents matching a regex pattern in the content field.
    ///
    /// # Arguments
    /// * `pattern` - The regex pattern
    /// * `limit` - Maximum number of results to return
    ///
    /// # Errors
    /// Returns `TamsaekError::Search` if the search fails or pattern is invalid.
    pub fn search_regex(&self, pattern: &str, limit: usize) -> Result<Vec<SearchResult>> {
        if pattern.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Tantivy's RegexQuery uses FST-based matching which requires patterns
        // that can match complete tokens. We wrap the pattern to ensure it matches
        // tokens containing the pattern anywhere (substring matching).
        let wrapped_pattern = if pattern.starts_with(".*") || pattern.ends_with(".*") {
            pattern.to_string()
        } else {
            format!(".*{}.*", pattern)
        };

        debug!(
            "Regex search for: {} (wrapped: {}, limit: {})",
            pattern, wrapped_pattern, limit
        );

        let searcher = self.reader.searcher();
        let mut queries: Vec<Box<dyn tantivy::query::Query>> = Vec::new();

        queries.push(Box::new(
            RegexQuery::from_pattern(&wrapped_pattern, self.fields.title)
                .map_err(|e| TamsaekError::InvalidQuery(format!("Invalid regex pattern: {}", e)))?,
        ));
        queries.push(Box::new(
            RegexQuery::from_pattern(&wrapped_pattern, self.fields.content)
                .map_err(|e| TamsaekError::InvalidQuery(format!("Invalid regex pattern: {}", e)))?,
        ));

        let query: Box<dyn tantivy::query::Query> = if queries.len() == 1 {
            queries.remove(0)
        } else {
            Box::new(BooleanQuery::union(queries))
        };

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| TamsaekError::Search(format!("Regex search failed: {}", e)))?;

        let snippet_generator = SnippetGenerator::create(&searcher, &query, self.fields.content)
            .map_err(|e| {
                TamsaekError::Search(format!("Failed to create snippet generator: {}", e))
            })?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| TamsaekError::Search(format!("Failed to retrieve doc: {}", e)))?;

            let snippet = snippet_generator.snippet_from_doc(&doc);
            let snippet_text = if snippet.is_empty() {
                self.get_text_field(&doc, self.fields.content).map(|s| {
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

            results.push(self.doc_to_search_result(&doc, score, snippet_text));
        }

        debug!("Found {} regex results", results.len());
        Ok(results)
    }

    /// Searches for documents with a specific file extension.
    ///
    /// # Arguments
    /// * `ext` - The file extension (without dot, e.g., "rs")
    /// * `limit` - Maximum number of results to return
    ///
    /// # Errors
    /// Returns `TamsaekError::Search` if the search fails.
    pub fn search_by_extension(&self, ext: &str, limit: usize) -> Result<Vec<SearchResult>> {
        debug!("Searching by extension: {} (limit: {})", ext, limit);

        let searcher = self.reader.searcher();
        let term = Term::from_field_text(self.fields.extension, ext);
        let term_query = TermQuery::new(term, IndexRecordOption::Basic);

        let top_docs = searcher.search(&term_query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
            results.push(self.doc_to_search_result(&retrieved_doc, score, None));
        }

        debug!("Found {} results with extension {}", results.len(), ext);
        Ok(results)
    }

    /// Lists all documents in the index.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of results to return
    ///
    /// # Errors
    /// Returns `TamsaekError::Search` if the operation fails.
    pub fn list_all(&self, limit: usize) -> Result<Vec<SearchResult>> {
        debug!("Listing all documents (limit: {})", limit);

        let searcher = self.reader.searcher();
        let all_query = AllQuery;

        let top_docs = searcher.search(&all_query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
            results.push(self.doc_to_search_result(&retrieved_doc, score, None));
        }

        debug!("Listed {} documents", results.len());
        Ok(results)
    }

    /// Retrieves a document by its ID.
    ///
    /// # Arguments
    /// * `id` - The document ID
    ///
    /// # Errors
    /// Returns `TamsaekError::Search` if the operation fails.
    pub fn get_document(&self, id: &str) -> Result<Option<Document>> {
        debug!("Getting document by id: {}", id);

        let searcher = self.reader.searcher();
        let term = Term::from_field_text(self.fields.id, id);
        let term_query = TermQuery::new(term, IndexRecordOption::Basic);

        let top_docs = searcher.search(&term_query, &TopDocs::with_limit(1))?;

        if let Some((_score, doc_address)) = top_docs.first() {
            let retrieved_doc: TantivyDocument = searcher.doc(*doc_address)?;
            Ok(Some(self.tantivy_doc_to_document(&retrieved_doc)))
        } else {
            Ok(None)
        }
    }

    fn doc_to_search_result(
        &self,
        doc: &TantivyDocument,
        score: f32,
        snippet: Option<String>,
    ) -> SearchResult {
        SearchResult {
            id: self.get_text_field(doc, self.fields.id).unwrap_or_default(),
            title: self
                .get_text_field(doc, self.fields.title)
                .unwrap_or_default(),
            score,
            snippet,
            path: self.get_text_field(doc, self.fields.path),
            extension: self.get_text_field(doc, self.fields.extension),
            size_bytes: self.get_i64_field(doc, self.fields.size_bytes),
            modified_at: self.get_text_field(doc, self.fields.modified_at),
            source: self.get_text_field(doc, self.fields.source),
        }
    }

    /// Converts a Tantivy document to a Document.
    fn tantivy_doc_to_document(&self, doc: &TantivyDocument) -> Document {
        use chrono::DateTime;

        let modified_at = self
            .get_text_field(doc, self.fields.modified_at)
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        Document {
            id: self.get_text_field(doc, self.fields.id).unwrap_or_default(),
            title: self
                .get_text_field(doc, self.fields.title)
                .unwrap_or_default(),
            content: self
                .get_text_field(doc, self.fields.content)
                .unwrap_or_default(),
            path: self.get_text_field(doc, self.fields.path),
            extension: self.get_text_field(doc, self.fields.extension),
            size_bytes: self.get_i64_field(doc, self.fields.size_bytes),
            modified_at,
            source: self
                .get_text_field(doc, self.fields.source)
                .unwrap_or_else(|| "local".to_string()),
            external_id: None,
            mime_type: None,
            author: None,
            created_at: None,
            indexed_at: None,
            content_hash: None,
            metadata: None,
            tags: Vec::new(),
        }
    }

    /// Helper to extract a text field value from a Tantivy document.
    fn get_text_field(&self, doc: &TantivyDocument, field: Field) -> Option<String> {
        doc.get_first(field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Helper to extract an i64 field value from a Tantivy document.
    fn get_i64_field(&self, doc: &TantivyDocument, field: Field) -> Option<i64> {
        doc.get_first(field).and_then(|v| v.as_i64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_index() {
        let index = TamsaekIndex::in_memory().unwrap();
        assert_eq!(index.num_docs(), 0);
    }

    #[test]
    fn test_add_and_search_document() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc = Document::new(
            "1",
            "Test Document",
            "This is some test content for searching.",
        );
        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        // Need to reload reader after commit
        let results = index.search("test", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn test_delete_document() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc = Document::new("1", "Test", "Content");
        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        index.delete_document("1").unwrap();
        index.commit().unwrap();

        let result = index.get_document("1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_clear_index() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc1 = Document::new("1", "Test 1", "Content 1");
        let doc2 = Document::new("2", "Test 2", "Content 2");
        index.add_document(&doc1).unwrap();
        index.add_document(&doc2).unwrap();
        index.commit().unwrap();

        index.clear().unwrap();
        assert_eq!(index.num_docs(), 0);
    }

    #[test]
    fn test_search_with_snippet() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc = Document::new(
            "1",
            "Rust Programming",
            "Rust is a systems programming language focused on safety and performance.",
        );
        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        let results = index.search("safety", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].snippet.is_some());
    }

    #[test]
    fn test_search_content_only() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc = Document::new("1", "Title with rust", "Content without the keyword.");
        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        let results = index.search_content_only("rust", 10).unwrap();
        assert!(results.is_empty());

        let results = index.search_title_only("rust", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_regex() {
        let index = TamsaekIndex::in_memory().unwrap();

        let doc = Document::new("1", "Test Doc", "The error code is ERR404 here.");
        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        // Tantivy regex matches against lowercased tokens
        let results = index.search_regex("err.*", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_upsert_document_full() {
        let index = TamsaekIndex::in_memory().unwrap();

        index
            .upsert_document_full(
                "doc1",
                "My Title",
                "My Content",
                Some("/path/to/file.txt"),
                Some(1024),
                Some("2024-01-01T00:00:00Z"),
                Some("local"),
                Some("txt"),
            )
            .unwrap();
        index.commit().unwrap();

        let doc = index.get_document("doc1").unwrap();
        assert!(doc.is_some());
        let doc = doc.unwrap();
        assert_eq!(doc.title, "My Title");
        assert_eq!(doc.path, Some("/path/to/file.txt".to_string()));
    }
}
