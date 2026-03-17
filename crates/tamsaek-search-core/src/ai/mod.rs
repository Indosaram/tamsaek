//! AI module for search integration
//!
//! Provides traits and framework-agnostic implementations for:
//! - LLM integration (trait + types)
//! - Embedding generation (trait + config)
//! - Reranking (trait + config)
//! - Query expansion (trait + LLM-based implementation)
//! - Query intent classification (rule-based)
//! - Natural language query parsing (LLM-based)
//! - Agentic search (agent framework)

pub mod agent;
pub mod embedding;
pub mod intent;
pub mod llm;
pub mod query_expander;
pub mod query_parser;
pub mod reranker;

pub use agent::{
    AgentAnswer, AgentResponse, SearchAgent, SearchBackend, SearchMode,
    SearchResult as AgentSearchResult, ToolCall, ToolResult,
};
pub use embedding::{EmbeddingClient, EmbeddingConfig, EmbeddingModelSize};
pub use intent::{IntentClassification, QueryIntent, QueryIntentClassifier};
pub use llm::{LlmClient, LlmConfig, LlmResponse, LlmStream, Message, MessageRole, StreamChunk};
pub use query_expander::LlmQueryExpander;
pub use query_parser::{
    DateRange, FileTypeCategory, ParsedFilter, QueryParser, SortPreference, SourceFilter,
};
pub use reranker::{Reranker, RerankerConfig, RerankerModelSize};

/// Default model name for query parsing
pub const DEFAULT_MODEL: &str = "Qwen2.5-3B-Instruct";

/// AI module errors
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Query parsing error: {0}")]
    QueryParsing(String),

    #[error("Model load error: {0}")]
    ModelLoadError(String),

    #[error("Model is loading: {0}")]
    ModelLoading(String),

    #[error("Generation error: {0}")]
    GenerationError(String),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AiError>;

/// Trait for expanding queries (used by pipeline)
#[async_trait::async_trait]
pub trait QueryExpander: Send + Sync {
    async fn expand(&self, query: &str) -> std::result::Result<Vec<String>, String>;
}
