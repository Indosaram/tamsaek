//! Hybrid search pipeline
//!
//! Orchestrates the multi-stage search process:
//! 1. Query Expansion (optional)
//! 2. Parallel FTS and Vector retrieval
//! 3. RRF Fusion
//! 4. Bonus scoring

use std::sync::Arc;
use tokio::sync::RwLock;

use super::config::HybridSearchConfig;
use crate::ai::QueryExpander;
use crate::error::SearchResult;
use crate::index::{QueryType, ScoreBreakdown, SearchHit, SearchResults};
use crate::scoring::{BonusConfig, BonusScorer, RRFScorer};

/// Hybrid search pipeline orchestrator
pub struct HybridSearchPipeline<Q: QueryExpander + 'static> {
    fts: Arc<RwLock<Option<tamsaek_storage::TantivyFts>>>,
    vector_store: Arc<dyn tamsaek_storage::VectorStore>,
    embedding_client: Arc<dyn crate::ai::EmbeddingClient>,
    query_expander: Option<Q>,
    config: Arc<RwLock<HybridSearchConfig>>,
}

impl<Q: QueryExpander + 'static> Clone for HybridSearchPipeline<Q> {
    fn clone(&self) -> Self {
        Self {
            fts: self.fts.clone(),
            vector_store: self.vector_store.clone(),
            embedding_client: self.embedding_client.clone(),
            query_expander: None,
            config: self.config.clone(),
        }
    }
}

impl<Q: QueryExpander + 'static> HybridSearchPipeline<Q> {
    /// Create a new hybrid search pipeline
    pub fn new(
        fts: Arc<RwLock<Option<tamsaek_storage::TantivyFts>>>,
        vector_store: Arc<dyn tamsaek_storage::VectorStore>,
        embedding_client: Arc<dyn crate::ai::EmbeddingClient>,
        query_expander: Option<Q>,
    ) -> Self {
        Self {
            fts,
            vector_store,
            embedding_client,
            query_expander,
            config: Arc::new(RwLock::new(HybridSearchConfig::default())),
        }
    }

    /// Set the pipeline configuration
    pub async fn set_config(&self, config: HybridSearchConfig) {
        let mut guard = self.config.write().await;
        *guard = config;
    }

    /// Get the current pipeline configuration
    pub async fn get_config(&self) -> HybridSearchConfig {
        self.config.read().await.clone()
    }

    /// Execute a hybrid search for the given query
    pub async fn search(&self, query: &str, limit: usize) -> SearchResult<SearchResults> {
        let start = std::time::Instant::now();
        let config = self.config.read().await.clone();

        // 1. Query Expansion
        let mut queries = vec![query.to_string()];
        if config.enable_query_expansion {
            if let Some(ref expander) = self.query_expander {
                if let Ok(expanded) = expander.expand(query).await {
                    queries = expanded;
                }
            }
        }

        let primary_query = queries[0].clone();

        let fts_handle = {
            let fts_clone = self.fts.clone();
            let query_clone = primary_query.clone();
            let limit_clone = config.fetch_limit;
            tokio::spawn(async move {
                let guard = fts_clone.read().await;
                if let Some(ref fts) = *guard {
                    fts.search(&query_clone, limit_clone)
                } else {
                    Ok(Vec::new())
                }
            })
        };

        let vec_handle = {
            let client = self.embedding_client.clone();
            let store = self.vector_store.clone();
            let query_clone = primary_query.clone();
            let limit_clone = config.fetch_limit;
            tokio::spawn(async move {
                // Try to get embedding, if fails, return empty instead of erroring entire pipeline
                let embedding = match client.embed(&query_clone).await {
                    Ok(e) => e,
                    Err(_e) => return Ok(Vec::new()),
                };
                match store.search(&embedding, limit_clone).await {
                    Ok(r) => Ok(r),
                    Err(_e) => Ok(Vec::new()),
                }
            })
        };

        let fts_results = match fts_handle.await {
            Ok(r) => match r {
                Ok(hits) => hits,
                Err(e) => return Err(crate::error::SearchError::Storage(e)),
            },
            Err(e) => return Err(crate::error::SearchError::Index(e.to_string())),
        };

        let vec_results = match vec_handle.await {
            Ok(r) => match r {
                Ok(hits) => hits,
                Err(e) => return Err(e),
            },
            Err(e) => return Err(crate::error::SearchError::Index(e.to_string())),
        };

        let fts_hits: Vec<SearchHit> = fts_results
            .into_iter()
            .map(Self::convert_fts_result)
            .collect();
        let vec_hits: Vec<SearchHit> = vec_results
            .into_iter()
            .map(Self::convert_vec_result)
            .collect();

        // 3. Fusion
        let rrf = RRFScorer::new(config.rrf_k);
        let mut fused_hits = rrf.fuse(&[
            (&fts_hits, config.fts_weight),
            (&vec_hits, config.vector_weight),
        ]);

        // 4. Bonuses
        if config.enable_bonuses {
            let bonus_scorer = BonusScorer::new(BonusConfig::default());
            for hit in fused_hits.iter_mut() {
                if hit.score_breakdown.is_none() {
                    hit.score_breakdown = Some(ScoreBreakdown::default());
                }
                bonus_scorer.apply(hit, query);
            }
            fused_hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        fused_hits.truncate(limit);

        Ok(SearchResults {
            hits: fused_hits,
            total_count: 0,
            took_ms: start.elapsed().as_millis() as u64,
            query_type: QueryType::Hybrid,
        })
    }

    fn convert_fts_result(res: tamsaek_storage::TantivySearchResult) -> SearchHit {
        let mut hit = SearchHit::new(
            document_id_from_storage(&res.document_id),
            res.score,
            res.title,
        );
        hit.snippet = res.snippet;
        hit.modified_at = res.modified_at.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        });
        hit.score_breakdown = Some(ScoreBreakdown {
            keyword_score: res.score,
            ..Default::default()
        });
        hit
    }

    fn convert_vec_result(res: tamsaek_storage::VectorSearchResult) -> SearchHit {
        let mut hit = SearchHit::new(
            document_id_from_storage(&res.id),
            1.0 - res.distance,
            "Semantic Match".to_string(),
        );
        hit.score_breakdown = Some(ScoreBreakdown {
            semantic_score: 1.0 - res.distance,
            ..Default::default()
        });
        hit
    }
}

fn document_id_from_storage(id: &str) -> crate::document::DocumentId {
    if let Some(idx) = id.find('|') {
        let source_str = &id[..idx];
        let external_id = &id[idx + 1..];
        let source = match source_str {
            "googledrive" => crate::document::SourceType::GoogleDrive,
            "onedrive" => crate::document::SourceType::OneDrive,
            "sharepoint" => crate::document::SourceType::SharePoint,
            _ => crate::document::SourceType::Local,
        };
        crate::document::DocumentId::new(source, external_id)
    } else {
        crate::document::DocumentId::new(crate::document::SourceType::Local, id)
    }
}
