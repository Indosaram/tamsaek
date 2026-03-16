//! AI-related traits for search integration

use async_trait::async_trait;

/// Trait for embedding generation
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>;
    fn dimensions(&self) -> usize;
}

/// Trait for LLM interaction
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, prompt: &str) -> Result<String, String>;
}

/// Trait for reranking search results
#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>, String>;
}

/// Trait for expanding queries
#[async_trait]
pub trait QueryExpander: Send + Sync {
    async fn expand(&self, query: &str) -> Result<Vec<String>, String>;
}
