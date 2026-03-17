//! Embedding models for semantic search
//!
//! This module provides traits and configurations for generating
//! dense vector embeddings from text.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Available embedding model sizes/types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EmbeddingModelSize {
    /// multilingual-e5-small (384 dimensions, ~470MB)
    #[default]
    E5Small,
    /// bge-small-en-v1.5 (384 dimensions)
    BgeSmall,
    /// bge-m3 (1024 dimensions, multilingual)
    BgeM3,
}

impl EmbeddingModelSize {
    /// Get the number of dimensions for this model
    pub fn dimensions(&self) -> usize {
        match self {
            Self::E5Small => 384,
            Self::BgeSmall => 384,
            Self::BgeM3 => 1024,
        }
    }

    /// Get HuggingFace repo ID for the model
    pub fn repo_id(&self) -> &'static str {
        match self {
            Self::E5Small => "intfloat/multilingual-e5-small",
            Self::BgeSmall => "BAAI/bge-small-en-v1.5",
            Self::BgeM3 => "BAAI/bge-m3",
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::E5Small => "Multilingual-E5-Small",
            Self::BgeSmall => "BGE-Small-EN-v1.5",
            Self::BgeM3 => "BGE-M3",
        }
    }
}

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model size to use
    pub model_size: EmbeddingModelSize,
    /// Batch size for processing multiple texts
    pub batch_size: usize,
    /// Preferred device (cpu, cuda, metal)
    pub device: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_size: EmbeddingModelSize::default(),
            batch_size: 32,
            device: None,
        }
    }
}

/// Trait for embedding generation clients
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, String>;

    /// Generate embeddings for a batch of texts
    async fn embed_batch(&self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, String>;

    /// Get the number of dimensions
    fn dimensions(&self) -> usize;
}
