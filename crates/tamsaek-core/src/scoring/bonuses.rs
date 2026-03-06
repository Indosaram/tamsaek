//! Bonus scoring for search results
//!
//! Applies additional points for filename matches, extension matches,
//! and recency to improve relevance beyond pure content matches.

use super::Scorable;
use chrono::Utc;

/// Configuration for bonus scoring
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct BonusScorer {
    config: BonusConfig,
}

impl Default for BonusScorer {
    fn default() -> Self {
        Self::new(BonusConfig::default())
    }
}

impl BonusScorer {
    /// Create a new bonus scorer with given config
    pub fn new(config: BonusConfig) -> Self {
        Self { config }
    }

    /// Get the config
    pub fn config(&self) -> &BonusConfig {
        &self.config
    }

    /// Apply all bonuses to a scorable item
    ///
    /// # Arguments
    /// * `item` - The item to apply bonuses to
    /// * `query` - The search query for filename matching
    pub fn apply<T: Scorable>(&self, item: &mut T, query: &str) {
        let filename_bonus = self.calculate_filename_bonus(item, query);
        let recency_bonus = self.calculate_recency_bonus(item);

        let new_score = item.score() + filename_bonus + recency_bonus;
        item.set_score(new_score);
    }

    /// Apply bonuses to multiple items
    pub fn apply_all<T: Scorable>(&self, items: &mut [T], query: &str) {
        for item in items {
            self.apply(item, query);
        }
    }

    /// Calculate filename match bonus
    ///
    /// Returns:
    /// - Full bonus (0.1) for exact title match
    /// - Partial bonus (0.06) for substring match
    /// - 0.0 for no match
    fn calculate_filename_bonus<T: Scorable>(&self, item: &T, query: &str) -> f32 {
        if query.is_empty() {
            return 0.0;
        }

        let Some(title) = item.title() else {
            return 0.0;
        };

        let title_lower = title.to_lowercase();
        let query_lower = query.to_lowercase();

        if title_lower == query_lower {
            self.config.filename_match_bonus
        } else if title_lower.contains(&query_lower) {
            self.config.filename_match_bonus * 0.6
        } else {
            0.0
        }
    }

    /// Calculate recency bonus using exponential decay
    ///
    /// Returns a small boost (max 0.05) for recent documents,
    /// decaying by half every `recency_half_life_days` days.
    fn calculate_recency_bonus<T: Scorable>(&self, item: &T) -> f32 {
        let Some(modified_at) = item.modified_at() else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchResult;

    fn make_result(id: &str, title: &str, modified_at: Option<&str>) -> SearchResult {
        SearchResult {
            id: id.to_string(),
            title: title.to_string(),
            score: 1.0,
            snippet: None,
            path: None,
            extension: None,
            size_bytes: None,
            modified_at: modified_at.map(String::from),
            source: None,
        }
    }

    #[test]
    fn test_default_config() {
        let config = BonusConfig::default();
        assert!((config.filename_match_bonus - 0.1).abs() < f32::EPSILON);
        assert!((config.extension_match_bonus - 0.05).abs() < f32::EPSILON);
        assert!((config.recency_half_life_days - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_filename_exact_match() {
        let scorer = BonusScorer::default();
        let mut result = make_result("1", "readme", None);

        scorer.apply(&mut result, "readme");

        // Should get full filename bonus
        assert!(result.score > 1.0);
        assert!((result.score - 1.1).abs() < 0.001);
    }

    #[test]
    fn test_filename_partial_match() {
        let scorer = BonusScorer::default();
        let mut result = make_result("1", "readme.md", None);

        scorer.apply(&mut result, "read");

        // Should get partial filename bonus (0.1 * 0.6 = 0.06)
        assert!(result.score > 1.0);
        assert!((result.score - 1.06).abs() < 0.001);
    }

    #[test]
    fn test_filename_no_match() {
        let scorer = BonusScorer::default();
        let mut result = make_result("1", "config.toml", None);

        scorer.apply(&mut result, "readme");

        // Should get no bonus
        assert!((result.score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_filename_case_insensitive() {
        let scorer = BonusScorer::default();
        let mut result = make_result("1", "README.MD", None);

        scorer.apply(&mut result, "readme");

        // Should match case-insensitively
        assert!(result.score > 1.0);
    }

    #[test]
    fn test_recency_today() {
        let scorer = BonusScorer::default();
        let today = Utc::now().to_rfc3339();
        let mut result = make_result("1", "doc", Some(&today));

        scorer.apply(&mut result, "");

        // Should get max recency bonus (0.05)
        assert!((result.score - 1.05).abs() < 0.001);
    }

    #[test]
    fn test_recency_30_days_ago() {
        let scorer = BonusScorer::default();
        let thirty_days_ago = (Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let mut result = make_result("1", "doc", Some(&thirty_days_ago));

        scorer.apply(&mut result, "");

        // Should get half of max bonus (0.05 / 2 = 0.025)
        assert!((result.score - 1.025).abs() < 0.005);
    }

    #[test]
    fn test_recency_no_date() {
        let scorer = BonusScorer::default();
        let mut result = make_result("1", "doc", None);

        scorer.apply(&mut result, "");

        // Should get no bonus
        assert!((result.score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_apply_all() {
        let scorer = BonusScorer::default();
        let mut results = vec![
            make_result("1", "readme", None),
            make_result("2", "config", None),
        ];

        scorer.apply_all(&mut results, "readme");

        // First should have bonus, second should not
        assert!(results[0].score > results[1].score);
    }
}
