//! Search scoring and result fusion
//!
//! This module provides scoring utilities for ranking and merging search results:
//! - RRF (Reciprocal Rank Fusion) for combining multiple ranked lists
//! - Bonus scoring for filename matches and recency boosts

pub mod bonuses;
pub mod rrf;

pub use bonuses::{BonusConfig, BonusScorer};
pub use rrf::RRFScorer;

use chrono::{DateTime, Utc};

/// Trait for types that can be scored and ranked
pub trait Scorable {
    /// Get the unique identifier
    fn id(&self) -> &str;

    /// Get the current score
    fn score(&self) -> f32;

    /// Set a new score
    fn set_score(&mut self, score: f32);

    /// Get the title (for filename matching)
    fn title(&self) -> Option<&str>;

    /// Get the modification time (for recency scoring)
    fn modified_at(&self) -> Option<DateTime<Utc>>;
}

/// Implementation of Scorable for SearchResult
impl Scorable for crate::SearchResult {
    fn id(&self) -> &str {
        &self.id
    }

    fn score(&self) -> f32 {
        self.score
    }

    fn set_score(&mut self, score: f32) {
        self.score = score;
    }

    fn title(&self) -> Option<&str> {
        Some(&self.title)
    }

    fn modified_at(&self) -> Option<DateTime<Utc>> {
        self.modified_at.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
    }
}
