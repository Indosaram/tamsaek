//! Configuration for the hybrid search pipeline

use serde::{Deserialize, Serialize};

/// Configuration for the hybrid search pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchConfig {
    /// Whether to use query expansion (requires LLM)
    pub enable_query_expansion: bool,
    /// Weight for FTS results in RRF fusion [0.0 - 1.0]
    pub fts_weight: f32,
    /// Weight for vector results in RRF fusion [0.0 - 1.0]
    pub vector_weight: f32,
    /// Maximum results to fetch from each source
    pub fetch_limit: usize,
    /// RRF constant k
    pub rrf_k: f32,
    /// Whether to apply filename and recency bonuses
    pub enable_bonuses: bool,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            enable_query_expansion: false,
            fts_weight: 0.6,
            vector_weight: 0.4,
            fetch_limit: 50,
            rrf_k: 60.0,
            enable_bonuses: true,
        }
    }
}
