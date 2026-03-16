//! Bonus scoring for search results
//!
//! Applies additional points for filename matches, extension matches,
//! and recency to improve relevance beyond pure content matches.

use crate::index::SearchHit;
use chrono::Utc;

/// Configuration for bonus scoring
pub struct BonusConfig {
    /// Bonus for exact or partial filename match [0.0 - 1.0]
    pub filename_match_bonus: f32,
    /// Bonus for matching requested extension [0.0 - 1.0]
    pub extension_match_bonus: f32,
    /// Half-life in days for recency decay (default 30 days)
    pub recency_half_life_days: f32,
}

impl Default for BonusConfig {
    fn default() -> Self {
        Self {
            filename_match_bonus: 0.1,
            extension_match_bonus: 0.05,
            recency_half_life_days: 30.0,
        }
    }
}

/// Scorer for applying various relevance bonuses
pub struct BonusScorer {
    config: BonusConfig,
}

impl BonusScorer {
    /// Create a new bonus scorer with given config
    pub fn new(config: BonusConfig) -> Self {
        Self { config }
    }

    /// Apply all bonuses to a search hit
    pub fn apply(&self, hit: &mut SearchHit, query: &str) {
        let filename_bonus = self.calculate_filename_bonus(hit, query);
        let recency_bonus = self.calculate_recency_bonus(hit);

        hit.score += filename_bonus + recency_bonus;

        if let Some(ref mut breakdown) = hit.score_breakdown {
            breakdown.filename_bonus = filename_bonus;
            breakdown.recency_boost = recency_bonus;
        }
    }

    fn calculate_filename_bonus(&self, hit: &SearchHit, query: &str) -> f32 {
        if query.is_empty() {
            return 0.0;
        }

        let title_lower = hit.title.to_lowercase();
        let query_lower = query.to_lowercase();

        if title_lower == query_lower {
            self.config.filename_match_bonus
        } else if title_lower.contains(&query_lower) {
            self.config.filename_match_bonus * 0.6
        } else {
            0.0
        }
    }

    fn calculate_recency_bonus(&self, hit: &SearchHit) -> f32 {
        let Some(modified_at) = hit.modified_at else {
            return 0.0;
        };

        let now = Utc::now();
        let duration = now.signed_duration_since(modified_at);
        let days = duration.num_days() as f32;

        if days <= 0.0 {
            return 0.05; // Maximum small boost for today
        }

        // Exponential decay: score = initial * 2^(-days/half_life)
        // We use a small maximum bonus (e.g. 0.05)
        0.05 * (2.0f32.powf(-days / self.config.recency_half_life_days))
    }
}
