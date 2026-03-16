//! Reciprocal Rank Fusion (RRF) for merging ranked search results
//!
//! RRF is a simple but effective algorithm for combining multiple
//! ranked lists into a single unified ranking.

use crate::index::SearchHit;
use std::collections::HashMap;

/// Scorer for Reciprocal Rank Fusion
pub struct RRFScorer {
    /// RRF constant (standard value is 60.0)
    pub k: f32,
}

impl Default for RRFScorer {
    fn default() -> Self {
        Self { k: 60.0 }
    }
}

impl RRFScorer {
    /// Create a new RRF scorer with custom constant k
    pub fn new(k: f32) -> Self {
        Self { k }
    }

    /// Calculate RRF score for a given rank (0-indexed)
    pub fn score_rank(&self, rank: usize) -> f32 {
        1.0 / (self.k + (rank + 1) as f32)
    }

    /// Fuse multiple ranked lists into a single list of results
    ///
    /// Each input list should be sorted by its native score (descending).
    /// Weights can be applied to each source.
    pub fn fuse(
        &self,
        ranked_lists: &[(&[SearchHit], f32)], // (results, weight)
    ) -> Vec<SearchHit> {
        let mut combined_scores: HashMap<String, (f32, SearchHit)> = HashMap::new();

        for (list, weight) in ranked_lists {
            for (rank, hit) in list.iter().enumerate() {
                let id = hit.document_id.to_storage_id();
                let rrf_score = self.score_rank(rank) * weight;

                let entry = combined_scores
                    .entry(id)
                    .or_insert_with(|| (0.0, hit.clone()));
                entry.0 += rrf_score;

                // Track which rank contributed from which source if needed in future
            }
        }

        let mut fused_results: Vec<SearchHit> = combined_scores
            .into_values()
            .map(|(score, mut hit)| {
                hit.score = score;
                hit
            })
            .collect();

        // Sort by final fused score descending
        fused_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        fused_results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentId, SourceType};

    #[test]
    fn test_rrf_scoring() {
        let scorer = RRFScorer::new(60.0);
        // Rank 0: 1 / (60 + 1) = 1/61 ≈ 0.01639
        assert!((scorer.score_rank(0) - 0.01639).abs() < 0.0001);
        // Rank 1: 1 / (60 + 2) = 1/62 ≈ 0.01613
        assert!((scorer.score_rank(1) - 0.01613).abs() < 0.0001);
    }

    #[test]
    fn test_rrf_fusion() {
        let scorer = RRFScorer::default();

        let id1 = DocumentId::new(SourceType::Local, "doc1");
        let id2 = DocumentId::new(SourceType::Local, "doc2");

        let list1 = vec![
            SearchHit::new(id1.clone(), 1.0, "Doc 1"),
            SearchHit::new(id2.clone(), 0.8, "Doc 2"),
        ];

        let list2 = vec![
            SearchHit::new(id2.clone(), 0.9, "Doc 2"),
            SearchHit::new(id1.clone(), 0.7, "Doc 1"),
        ];

        let fused = scorer.fuse(&[(&list1, 1.0), (&list2, 1.0)]);

        assert_eq!(fused.len(), 2);
        // Both should have same score if ranks are swapped and weights equal
        assert!((fused[0].score - fused[1].score).abs() < 0.0001);
    }
}
