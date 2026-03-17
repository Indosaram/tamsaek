//! Search Mode Configuration
//!
//! Defines the 4 search modes that control which components are active.

use serde::{Deserialize, Serialize};

/// Search mode determining which AI components are active
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Full AI capabilities: Agent + Hybrid (FTS + Vector)
    /// Best quality, highest resource usage
    FullSpeed,

    /// Agent + FTS only (no embedding/vector search)
    /// Agent judgment with keyword search
    LlmOnly,

    /// Hybrid search only (FTS + Vector), no Agent
    /// Fast semantic search without conversation
    LocalOnly,

    /// FTS only, minimal resources
    /// Simple keyword matching
    #[default]
    FtsOnly,
}

impl SearchMode {
    /// Whether this mode uses the LLM Agent
    pub fn use_agent(&self) -> bool {
        matches!(self, Self::FullSpeed | Self::LlmOnly)
    }

    /// Whether this mode uses embedding/vector search
    pub fn use_embedding(&self) -> bool {
        matches!(self, Self::FullSpeed | Self::LocalOnly)
    }

    /// Whether this mode uses FTS (always true)
    pub fn use_fts(&self) -> bool {
        true
    }

    /// Display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::FullSpeed => "Full Speed",
            Self::LlmOnly => "LLM Only",
            Self::LocalOnly => "Local Only",
            Self::FtsOnly => "FTS Only",
        }
    }

    /// Description for UI tooltips
    pub fn description(&self) -> &'static str {
        match self {
            Self::FullSpeed => "AI Agent + Semantic + Keyword search (highest quality)",
            Self::LlmOnly => "AI Agent + Keyword search (no embedding)",
            Self::LocalOnly => "Semantic + Keyword search (no agent)",
            Self::FtsOnly => "Keyword search only (fastest, minimal resources)",
        }
    }

    /// Estimated relative resource usage (1-4)
    pub fn resource_level(&self) -> u8 {
        match self {
            Self::FullSpeed => 4,
            Self::LlmOnly => 3,
            Self::LocalOnly => 2,
            Self::FtsOnly => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_capabilities() {
        assert!(SearchMode::FullSpeed.use_agent());
        assert!(SearchMode::FullSpeed.use_embedding());
        assert!(SearchMode::FullSpeed.use_fts());

        assert!(SearchMode::LlmOnly.use_agent());
        assert!(!SearchMode::LlmOnly.use_embedding());
        assert!(SearchMode::LlmOnly.use_fts());

        assert!(!SearchMode::LocalOnly.use_agent());
        assert!(SearchMode::LocalOnly.use_embedding());
        assert!(SearchMode::LocalOnly.use_fts());

        assert!(!SearchMode::FtsOnly.use_agent());
        assert!(!SearchMode::FtsOnly.use_embedding());
        assert!(SearchMode::FtsOnly.use_fts());
    }

    #[test]
    fn test_resource_levels() {
        assert!(SearchMode::FullSpeed.resource_level() > SearchMode::LlmOnly.resource_level());
        assert!(SearchMode::LlmOnly.resource_level() > SearchMode::LocalOnly.resource_level());
        assert!(SearchMode::LocalOnly.resource_level() > SearchMode::FtsOnly.resource_level());
    }

    #[test]
    fn test_serde() {
        let mode = SearchMode::FullSpeed;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"full_speed\"");

        let parsed: SearchMode = serde_json::from_str("\"llm_only\"").unwrap();
        assert_eq!(parsed, SearchMode::LlmOnly);
    }
}
