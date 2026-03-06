//! Reciprocal Rank Fusion (RRF) for merging ranked search results
//!
//! RRF is a simple but effective algorithm for combining multiple
//! ranked lists into a single unified ranking.
//!
//! Formula: RRF_score(rank) = 1 / (k + rank)
//! where k = 60.0 is the standard constant

use super::Scorable;
use std::collections::HashMap;

/// Scorer for Reciprocal Rank Fusion
#[derive(Debug, Clone)]
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
    ///
    /// # Example
    /// ```
    /// use tamsaek_core::RRFScorer;
    ///
    /// let scorer = RRFScorer::new(60.0);
    /// // Rank 0: 1 / (60 + 1) = 1/61 ≈ 0.01639
    /// assert!((scorer.score_rank(0) - 0.01639).abs() < 0.0001);
    /// ```
    pub fn score_rank(&self, rank: usize) -> f32 {
        1.0 / (self.k + (rank + 1) as f32)
    }

    /// Fuse multiple ranked lists into a single list of results
    ///
    /// Each input list should be sorted by its native score (descending).
    /// Weights can be applied to each source.
    ///
    /// # Arguments
    /// * `ranked_lists` - Slice of (results, weight) tuples
    ///
    /// # Returns
    /// A new vector of results sorted by fused RRF score
    pub fn fuse<T>(&self, ranked_lists: &[(&[T], f32)]) -> Vec<T>
    where
        T: Scorable + Clone,
    {
        let mut combined_scores: HashMap<String, (f32, T)> = HashMap::new();

        for (list, weight) in ranked_lists {
            for (rank, item) in list.iter().enumerate() {
                let id = item.id().to_string();
                let rrf_score = self.score_rank(rank) * weight;

                let entry = combined_scores
                    .entry(id)
                    .or_insert_with(|| (0.0, item.clone()));
                entry.0 += rrf_score;
            }
        }

        let mut fused_results: Vec<T> = combined_scores
            .into_values()
            .map(|(score, mut item)| {
                item.set_score(score);
                item
            })
            .collect();

        // Sort by final fused score descending
        fused_results.sort_by(|a, b| {
            b.score()
                .partial_cmp(&a.score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        fused_results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_scoring() {
        let scorer = RRFScorer::new(60.0);
        // Rank 0: 1 / (60 + 1) = 1/61 ≈ 0.01639
        assert!((scorer.score_rank(0) - 0.01639).abs() < 0.0001);
        // Rank 1: 1 / (60 + 2) = 1/62 ≈ 0.01613
        assert!((scorer.score_rank(1) - 0.01613).abs() < 0.0001);
    }

    #[test]
    fn test_rrf_default() {
        let scorer = RRFScorer::default();
        assert!((scorer.k - 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_rrf_fusion_with_search_results() {
        use crate::SearchResult;

        let scorer = RRFScorer::default();

        let list1 = vec![
            SearchResult {
                id: "doc1".to_string(),
                title: "Doc 1".to_string(),
                score: 1.0,
                snippet: None,
                path: None,
                extension: None,
                size_bytes: None,
                modified_at: None,
                source: None,
            },
            SearchResult {
                id: "doc2".to_string(),
                title: "Doc 2".to_string(),
                score: 0.8,
                snippet: None,
                path: None,
                extension: None,
                size_bytes: None,
                modified_at: None,
                source: None,
            },
        ];

        let list2 = vec![
            SearchResult {
                id: "doc2".to_string(),
                title: "Doc 2".to_string(),
                score: 0.9,
                snippet: None,
                path: None,
                extension: None,
                size_bytes: None,
                modified_at: None,
                source: None,
            },
            SearchResult {
                id: "doc1".to_string(),
                title: "Doc 1".to_string(),
                score: 0.7,
                snippet: None,
                path: None,
                extension: None,
                size_bytes: None,
                modified_at: None,
                source: None,
            },
        ];

        let fused = scorer.fuse(&[(&list1, 1.0), (&list2, 1.0)]);

        assert_eq!(fused.len(), 2);
        // Both should have same score if ranks are swapped and weights equal
        // doc1: rank 0 in list1 + rank 1 in list2 = 1/61 + 1/62
        // doc2: rank 1 in list1 + rank 0 in list2 = 1/62 + 1/61
        assert!((fused[0].score - fused[1].score).abs() < 0.0001);
    }

    #[test]
    fn test_rrf_fusion_weighted() {
        use crate::SearchResult;

        let scorer = RRFScorer::default();

        let list1 = vec![SearchResult {
            id: "doc1".to_string(),
            title: "Doc 1".to_string(),
            score: 1.0,
            snippet: None,
            path: None,
            extension: None,
            size_bytes: None,
            modified_at: None,
            source: None,
        }];

        let list2 = vec![SearchResult {
            id: "doc2".to_string(),
            title: "Doc 2".to_string(),
            score: 1.0,
            snippet: None,
            path: None,
            extension: None,
            size_bytes: None,
            modified_at: None,
            source: None,
        }];

        // Weight list1 higher
        let fused = scorer.fuse(&[(&list1, 2.0), (&list2, 1.0)]);

        assert_eq!(fused.len(), 2);
        // doc1 should rank higher due to weight
        assert_eq!(fused[0].id, "doc1");
        assert!(fused[0].score > fused[1].score);
    }
}
