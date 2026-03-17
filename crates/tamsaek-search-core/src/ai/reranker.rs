//! Cross-encoder reranking for improving search precision
//!
//! Rerankers take a list of candidate documents and re-score them
//! by directly comparing the query and document text using a transformer model.

use super::AiError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Available reranker model sizes/types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RerankerModelSize {
    /// ms-marco-MiniLM-L-6-v2 (Very fast, ~80MB)
    #[default]
    MiniLM,
    /// bge-reranker-v2-m3 (Large, multilingual)
    BgeM3,
}

impl RerankerModelSize {
    /// Get HuggingFace repo ID for the model
    pub fn repo_id(&self) -> &'static str {
        match self {
            Self::MiniLM => "cross-encoder/ms-marco-MiniLM-L-6-v2",
            Self::BgeM3 => "BAAI/bge-reranker-v2-m3",
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::MiniLM => "ms-marco-MiniLM-L-6-v2",
            Self::BgeM3 => "BGE-Reranker-V2-M3",
        }
    }
}

/// Configuration for reranking
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RerankerConfig {
    /// Model to use
    pub model_size: RerankerModelSize,
    /// Preferred device
    pub device: Option<String>,
}

/// Trait for search result rerankers
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Rerank a list of documents for a given query
    /// Returns a score for each document [0.0 - 1.0]
    async fn rerank(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>, AiError>;

    /// Get the model identifier
    fn model(&self) -> &str;
}
