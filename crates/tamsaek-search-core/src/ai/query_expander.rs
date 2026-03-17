//! Query expansion for improving search recall
//!
//! Uses an LLM to generate related queries and variations of the
//! original user query to capture more relevant documents.

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, warn};

use super::llm::{LlmClient, Message};
use super::AiError;

/// Service for expanding search queries using an LLM
#[derive(Clone)]
pub struct LlmQueryExpander<L: LlmClient + Clone> {
    llm: L,
    max_expansions: usize,
}

#[async_trait]
impl<L: LlmClient + Clone + 'static> super::QueryExpander for LlmQueryExpander<L> {
    async fn expand(&self, query: &str) -> Result<Vec<String>, String> {
        LlmQueryExpander::expand(self, query)
            .await
            .map_err(|e| e.to_string())
    }
}

impl<L: LlmClient + Clone> LlmQueryExpander<L> {
    /// Create a new query expander with the given LLM client
    pub fn new(llm: L) -> Self {
        Self {
            llm,
            max_expansions: 2, // Default to 2 variations + original
        }
    }

    /// Set the maximum number of expansions to generate
    pub fn with_max_expansions(mut self, max: usize) -> Self {
        self.max_expansions = max;
        self
    }

    /// Expand a user query into multiple variations
    pub async fn expand(&self, query: &str) -> Result<Vec<String>, AiError> {
        if query.trim().is_empty() {
            return Ok(vec![query.to_string()]);
        }

        let system_prompt = format!(
            "You are a search query expansion assistant. Your goal is to generate {0} related search queries \
            that help find more relevant documents for the user's input. \
            The original query might be in English, Korean, or Japanese. Generate variations in the same language. \
            Output ONLY a JSON array of strings. No other text.",
            self.max_expansions
        );

        let messages = vec![
            Message::system(system_prompt),
            Message::user(format!("Expand this search query: {0}", query)),
        ];

        match self.llm.chat(&messages).await {
            Ok(response) => {
                let mut expanded = vec![query.to_string()];
                match self.parse_json_array(&response.content) {
                    Ok(mut variations) => {
                        variations.truncate(self.max_expansions);
                        for v in variations {
                            if !expanded.contains(&v) {
                                expanded.push(v);
                            }
                        }
                        debug!("Expanded query '{}' into: {:?}", query, expanded);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse expanded queries: {0}. Response: {1}",
                            e, response.content
                        );
                    }
                }
                Ok(expanded)
            }
            Err(e) => {
                warn!("Query expansion failed: {0}. Using original query only.", e);
                Ok(vec![query.to_string()])
            }
        }
    }

    fn parse_json_array(&self, text: &str) -> Result<Vec<String>, AiError> {
        let trimmed = text.trim();
        // Handle cases where model might wrap JSON in backticks
        let json_str = if trimmed.starts_with("```json") && trimmed.ends_with("```") {
            trimmed[7..trimmed.len() - 3].trim()
        } else if trimmed.starts_with("```") && trimmed.ends_with("```") {
            trimmed[3..trimmed.len() - 3].trim()
        } else {
            trimmed
        };

        let v: Value = serde_json::from_str(json_str)?;
        if let Some(array) = v.as_array() {
            let strings: Vec<String> = array
                .iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect();
            Ok(strings)
        } else {
            Err(AiError::QueryParsing("Expected JSON array".into()))
        }
    }
}
