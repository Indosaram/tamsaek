//! Tamsaek MCP Server
//!
//! An MCP (Model Context Protocol) server for local file search.
//! This server provides 8 tools for indexing, searching, and managing documents.

mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use tamsaek_core::{Document, TamsaekIndex};
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::tools::{
    index::{
        ClearIndexResponse, IndexDirectoryRequest, IndexDirectoryResponse, IndexError,
        RemoveDocumentRequest, RemoveDocumentResponse,
    },
    search::{FilterRequest, SearchRegexRequest, SearchRequest, SearchResponse, SearchResultItem},
    stats::{DocumentResponse, GetDocumentRequest, StatsResponse},
};

/// Server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default index path.
    pub index_path: PathBuf,
    /// Default search result limit.
    pub default_limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        let index_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tamsaek")
            .join("index");

        Self {
            index_path,
            default_limit: 10,
        }
    }
}

/// The main Tamsaek MCP server.
#[derive(Clone)]
pub struct TamsaekServer {
    tool_router: ToolRouter<Self>,
    index: Arc<RwLock<TamsaekIndex>>,
    config: Config,
}

/// Create an error tool result.
fn tool_error(code: &str, message: &str) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult {
        content: vec![Content::text(format!("{}: {}", code, message))],
        structured_content: None,
        is_error: Some(true),
        meta: None,
    })
}

/// Create a success tool result.
fn tool_success<T: serde::Serialize>(data: T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::json(data)?]))
}

#[tool_router]
impl TamsaekServer {
    /// Creates a new TamsaekServer instance.
    pub fn new(index: TamsaekIndex, config: Config) -> Self {
        Self {
            tool_router: Self::tool_router(),
            index: Arc::new(RwLock::new(index)),
            config,
        }
    }

    // =========================================================================
    // SEARCH TOOLS
    // =========================================================================

    /// Full-text search across indexed documents.
    #[tool(
        name = "search",
        description = "Full-text search across indexed documents. Returns matching documents with relevance scores and snippets.",
        annotations(
            title = "Search Documents",
            read_only_hint = true,
            destructive_hint = false
        )
    )]
    async fn search(
        &self,
        Parameters(req): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(self.config.default_limit);

        let results = {
            let index = self.index.read();
            match index.search(&req.query, limit) {
                Ok(results) => results,
                Err(e) => {
                    return tool_error("SEARCH_ERROR", &format!("Search failed: {}", e));
                }
            }
        };

        let response = SearchResponse {
            total: results.len(),
            results: results
                .into_iter()
                .map(|r| SearchResultItem {
                    id: r.id,
                    title: r.title,
                    path: r.path.unwrap_or_default(),
                    score: r.score,
                    snippet: r.snippet,
                })
                .collect(),
        };

        tool_success(response)
    }

    /// Search using regular expression patterns.
    #[tool(
        name = "search-regex",
        description = "Search documents using regular expression patterns. Supports full regex syntax.",
        annotations(
            title = "Regex Search",
            read_only_hint = true,
            destructive_hint = false
        )
    )]
    async fn search_regex(
        &self,
        Parameters(req): Parameters<SearchRegexRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(self.config.default_limit);

        let results = {
            let index = self.index.read();
            match index.search_regex(&req.pattern, limit) {
                Ok(results) => results,
                Err(e) => {
                    return tool_error("REGEX_ERROR", &format!("Regex search failed: {}", e));
                }
            }
        };

        let response = SearchResponse {
            total: results.len(),
            results: results
                .into_iter()
                .map(|r| SearchResultItem {
                    id: r.id,
                    title: r.title,
                    path: r.path.unwrap_or_default(),
                    score: r.score,
                    snippet: r.snippet,
                })
                .collect(),
        };

        tool_success(response)
    }

    /// Filter documents by extension or source.
    #[tool(
        name = "filter",
        description = "Filter indexed documents by file extension or source. Useful for narrowing down results to specific file types.",
        annotations(
            title = "Filter Documents",
            read_only_hint = true,
            destructive_hint = false
        )
    )]
    async fn filter(
        &self,
        Parameters(req): Parameters<FilterRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(self.config.default_limit);

        // If extension filter is specified, use search_by_extension
        // Otherwise, list all documents
        let results = {
            let index = self.index.read();
            if let Some(ref ext) = req.extension {
                match index.search_by_extension(ext, limit) {
                    Ok(results) => results,
                    Err(e) => {
                        return tool_error("FILTER_ERROR", &format!("Filter failed: {}", e));
                    }
                }
            } else {
                // If no extension filter, list all documents
                match index.list_all(limit) {
                    Ok(results) => results,
                    Err(e) => {
                        return tool_error("FILTER_ERROR", &format!("Filter failed: {}", e));
                    }
                }
            }
        };

        // Post-filter by source if specified
        let filtered_results: Vec<_> = if let Some(ref source) = req.source {
            results
                .into_iter()
                .filter(|r| r.source.as_deref() == Some(source.as_str()))
                .collect()
        } else {
            results
        };

        let response = SearchResponse {
            total: filtered_results.len(),
            results: filtered_results
                .into_iter()
                .map(|r| SearchResultItem {
                    id: r.id,
                    title: r.title,
                    path: r.path.unwrap_or_default(),
                    score: r.score,
                    snippet: r.snippet,
                })
                .collect(),
        };

        tool_success(response)
    }

    // =========================================================================
    // INDEX TOOLS
    // =========================================================================

    /// Index files from a directory.
    #[tool(
        name = "index-directory",
        description = "Index files from a directory. Supports filtering by file extension and recursive indexing.",
        annotations(
            title = "Index Directory",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    async fn index_directory(
        &self,
        Parameters(req): Parameters<IndexDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let path = PathBuf::from(&req.path);
        if !path.exists() {
            return tool_error("PATH_NOT_FOUND", &format!("Path does not exist: {}", req.path));
        }

        let recursive = req.recursive.unwrap_or(true);
        let extensions: Option<Vec<String>> = req.extensions.map(|exts| {
            exts.into_iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect()
        });

        let mut indexed = 0;
        let mut failed = 0;
        let mut errors: Vec<IndexError> = Vec::new();

        let walker = if recursive {
            WalkDir::new(&path)
        } else {
            WalkDir::new(&path).max_depth(1)
        };

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());

            // Filter by extension if specified
            if let Some(ref allowed_exts) = extensions {
                if let Some(ref file_ext) = ext {
                    if !allowed_exts.contains(file_ext) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Read file content
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read file {}: {}", file_path.display(), e);
                    errors.push(IndexError {
                        path: file_path.display().to_string(),
                        error: e.to_string(),
                    });
                    failed += 1;
                    continue;
                }
            };

            // Create document using builder pattern
            let mut doc = Document::new(
                file_path.display().to_string(),
                file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                content,
            )
            .with_path(file_path.display().to_string())
            .with_source("local");

            // Add extension if available
            if let Some(ref e) = ext {
                doc = doc.with_extension(e);
            }

            // Add to index
            {
                let index = self.index.write();
                if let Err(e) = index.add_document(&doc) {
                    warn!("Failed to index file {}: {}", file_path.display(), e);
                    errors.push(IndexError {
                        path: file_path.display().to_string(),
                        error: e.to_string(),
                    });
                    failed += 1;
                    continue;
                }
                // Commit after each document (could be optimized for batch commits)
                if let Err(e) = index.commit() {
                    warn!("Failed to commit after indexing {}: {}", file_path.display(), e);
                }
            }

            indexed += 1;
        }

        info!(
            "Indexed {} files from {}, {} failed",
            indexed,
            req.path,
            failed
        );

        let response = IndexDirectoryResponse {
            indexed,
            failed,
            total: indexed + failed,
            errors,
        };

        tool_success(response)
    }

    /// Remove a document from the index.
    #[tool(
        name = "remove-document",
        description = "Remove a document from the index by its ID.",
        annotations(
            title = "Remove Document",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    async fn remove_document(
        &self,
        Parameters(req): Parameters<RemoveDocumentRequest>,
    ) -> Result<CallToolResult, McpError> {
        // First check if document exists
        let exists = {
            let index = self.index.read();
            match index.get_document(&req.id) {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(e) => {
                    return tool_error(
                        "GET_ERROR",
                        &format!("Failed to check document: {}", e),
                    );
                }
            }
        };

        if !exists {
            return tool_error("NOT_FOUND", &format!("Document not found: {}", req.id));
        }

        // Delete the document
        {
            let index = self.index.write();
            if let Err(e) = index.delete_document(&req.id) {
                return tool_error(
                    "REMOVE_ERROR",
                    &format!("Failed to remove document: {}", e),
                );
            }
            if let Err(e) = index.commit() {
                return tool_error(
                    "COMMIT_ERROR",
                    &format!("Failed to commit after removal: {}", e),
                );
            }
        }

        info!("Removed document: {}", req.id);
        tool_success(RemoveDocumentResponse {
            success: true,
            id: req.id,
        })
    }

    /// Clear all documents from the index.
    #[tool(
        name = "clear-index",
        description = "Clear all documents from the index. This operation is destructive and cannot be undone.",
        annotations(
            title = "Clear Index",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    async fn clear_index(&self) -> Result<CallToolResult, McpError> {
        // Get count before clearing
        let count = {
            let index = self.index.read();
            index.num_docs() as usize
        };

        // Clear the index
        {
            let index = self.index.write();
            if let Err(e) = index.clear() {
                return tool_error("CLEAR_ERROR", &format!("Failed to clear index: {}", e));
            }
        }

        info!("Cleared index, removed {} documents", count);

        tool_success(ClearIndexResponse {
            removed: count,
            message: format!("Successfully cleared {} documents from the index", count),
        })
    }

    // =========================================================================
    // STATS TOOLS
    // =========================================================================

    /// Get index statistics.
    #[tool(
        name = "get-stats",
        description = "Get statistics about the index including document count, total size, and breakdown by file extension.",
        annotations(
            title = "Get Statistics",
            read_only_hint = true,
            destructive_hint = false
        )
    )]
    async fn get_stats(&self) -> Result<CallToolResult, McpError> {
        let doc_count = {
            let index = self.index.read();
            index.num_docs() as usize
        };

        // For now, return basic stats. Extension breakdown would require iterating all docs.
        let response = StatsResponse {
            document_count: doc_count,
            total_size_bytes: 0, // Would need to track this separately
            created_at: None,
            updated_at: None,
            by_extension: vec![], // Would need additional index methods to compute
        };

        tool_success(response)
    }

    /// Get a specific document by ID.
    #[tool(
        name = "get-document",
        description = "Retrieve a specific document from the index by its ID. Returns the full document content.",
        annotations(
            title = "Get Document",
            read_only_hint = true,
            destructive_hint = false
        )
    )]
    async fn get_document(
        &self,
        Parameters(req): Parameters<GetDocumentRequest>,
    ) -> Result<CallToolResult, McpError> {
        let doc = {
            let index = self.index.read();
            match index.get_document(&req.id) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    return tool_error("NOT_FOUND", &format!("Document not found: {}", req.id));
                }
                Err(e) => {
                    return tool_error(
                        "GET_ERROR",
                        &format!("Failed to get document: {}", e),
                    );
                }
            }
        };

        let response = DocumentResponse {
            id: doc.id.clone(),
            title: doc.title.clone(),
            path: doc.path.clone().unwrap_or_default(),
            content: doc.content.clone(),
            extension: doc.extension.clone(),
            source: Some(doc.source.clone()),
            size_bytes: doc.content.len() as u64,
            modified_at: doc.modified_at.map(|t| t.to_rfc3339()),
        };

        tool_success(response)
    }
}

#[tool_handler]
impl ServerHandler for TamsaekServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Tamsaek MCP Server - A local file search server.\n\n\
                 Available tools:\n\
                 - search: Full-text search across indexed documents\n\
                 - search-regex: Search using regular expression patterns\n\
                 - filter: Filter documents by extension or source\n\
                 - get-document: Get a specific document by ID\n\
                 - index-directory: Index files from a directory\n\
                 - remove-document: Remove a document from the index\n\
                 - get-stats: Get index statistics\n\
                 - clear-index: Clear all documents from the index"
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing to stderr (MCP uses stdout for communication)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Starting Tamsaek MCP Server");

    // Load configuration
    let config = Config::default();

    // Ensure index directory exists
    if let Some(parent) = config.index_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Open or create the index
    let index = TamsaekIndex::open(config.index_path.clone())?;

    // Create and run the server
    let server = TamsaekServer::new(index, config);
    let service = server.serve(stdio()).await?;

    info!("Tamsaek MCP Server running on stdio");

    // Wait for the server to finish
    service.waiting().await?;

    info!("Tamsaek MCP Server shutting down");
    Ok(())
}
