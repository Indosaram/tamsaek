//! Query Intent Classification
//!
//! Classifies user queries to determine the appropriate processing pipeline:
//! - Search: Standard keyword/semantic search
//! - Question: Requires RAG pipeline with answer generation
//! - Summarize: Summarize selected documents
//! - Compare: Compare multiple documents

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// The intent behind a user query
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum QueryIntent {
    /// Standard search - find relevant documents
    #[default]
    Search,

    /// Question answering - requires RAG pipeline
    Question,

    /// Summarize one or more documents
    Summarize,

    /// Compare multiple documents
    Compare,

    /// Navigate/open a specific file
    Navigate,

    /// List or filter by criteria
    List,
}

impl QueryIntent {
    /// Whether this intent requires LLM generation
    pub fn requires_llm(&self) -> bool {
        matches!(self, Self::Question | Self::Summarize | Self::Compare)
    }

    /// Whether this intent uses semantic search
    pub fn uses_semantic_search(&self) -> bool {
        matches!(self, Self::Search | Self::Question | Self::Compare)
    }

    /// Suggested hybrid search weight (keyword vs semantic)
    /// Returns (keyword_weight, semantic_weight)
    pub fn search_weights(&self) -> (f32, f32) {
        match self {
            Self::Search => (0.7, 0.3),    // Keyword dominant
            Self::Question => (0.5, 0.5),  // Balanced
            Self::Summarize => (0.3, 0.7), // Semantic dominant
            Self::Compare => (0.4, 0.6),   // Slightly semantic
            Self::Navigate => (1.0, 0.0),  // Exact match
            Self::List => (0.8, 0.2),      // Filter-heavy
        }
    }
}

/// Classification result with confidence score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentClassification {
    /// Classified intent
    pub intent: QueryIntent,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,

    /// Alternative intents with lower confidence
    pub alternatives: Vec<(QueryIntent, f32)>,

    /// Detected question words (if any)
    pub question_words: Vec<String>,

    /// Detected action keywords (if any)
    pub action_keywords: Vec<String>,
}

impl IntentClassification {
    /// Create a new classification result
    pub fn new(intent: QueryIntent, confidence: f32) -> Self {
        Self {
            intent,
            confidence,
            alternatives: Vec::new(),
            question_words: Vec::new(),
            action_keywords: Vec::new(),
        }
    }

    /// Check if the classification is confident (> 0.7)
    pub fn is_confident(&self) -> bool {
        self.confidence > 0.7
    }
}

/// Query intent classifier using rule-based pattern matching
pub struct QueryIntentClassifier {
    // Question patterns
    question_words_en: Vec<&'static str>,
    question_words_ko: Vec<&'static str>,

    // Action patterns
    summarize_keywords: Vec<&'static str>,
    compare_keywords: Vec<&'static str>,
    navigate_keywords: Vec<&'static str>,
    list_keywords: Vec<&'static str>,
}

// Static regex patterns
static QUESTION_MARK_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?$").expect("Invalid regex"));

static KOREAN_QUESTION_ENDING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(뭐|뭔가|무엇|어떤|어떻게|왜|언제|어디|누가|몇|얼마)[가는요야죠까니]?\s*[\?]?$")
        .expect("Invalid regex")
});

static FILTER_HEAVY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(from:|type:|ext:|date:|author:|path:|tag:|size:)").expect("Invalid regex")
});

static PHRASE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""[^"]+""#).expect("Invalid regex"));

static REGEX_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/[^/]+/").expect("Invalid regex"));

impl Default for QueryIntentClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryIntentClassifier {
    /// Create a new classifier with default patterns
    pub fn new() -> Self {
        Self {
            // English question words
            question_words_en: vec![
                "what", "who", "where", "when", "why", "how", "which", "whose", "whom", "is",
                "are", "was", "were", "do", "does", "did", "can", "could", "would", "should",
                "tell me", "explain", "describe",
            ],

            // Korean question words/endings
            question_words_ko: vec![
                "뭐",
                "뭔가",
                "무엇",
                "무슨",
                "어떤",
                "어떻게",
                "어디",
                "언제",
                "왜",
                "누가",
                "누구",
                "몇",
                "얼마",
                "알려줘",
                "설명해",
                "말해줘",
            ],

            // Summarize action keywords
            summarize_keywords: vec![
                "summarize",
                "summary",
                "요약",
                "요약해",
                "tl;dr",
                "tldr",
                "brief",
                "overview",
                "핵심",
                "주요 내용",
                "간략히",
            ],

            // Compare action keywords
            compare_keywords: vec![
                "compare",
                "comparison",
                "비교",
                "비교해",
                "difference",
                "differ",
                "차이",
                "다른점",
                "vs",
                "versus",
                "and",
                "or",
            ],

            // Navigate action keywords
            navigate_keywords: vec![
                "open",
                "열어",
                "go to",
                "이동",
                "show",
                "보여줘",
                "find file",
                "파일 찾아",
            ],

            // List action keywords
            list_keywords: vec![
                "list",
                "show all",
                "전부",
                "모두",
                "find all",
                "목록",
                "리스트",
            ],
        }
    }

    /// Classify the intent of a query
    pub fn classify(&self, query: &str) -> IntentClassification {
        let query_lower = query.to_lowercase();
        let query_trimmed = query_lower.trim();

        // Track scores for each intent
        let mut scores: Vec<(QueryIntent, f32)> = vec![
            (QueryIntent::Search, 0.0),
            (QueryIntent::Question, 0.0),
            (QueryIntent::Summarize, 0.0),
            (QueryIntent::Compare, 0.0),
            (QueryIntent::Navigate, 0.0),
            (QueryIntent::List, 0.0),
        ];

        let mut detected_question_words = Vec::new();
        let mut detected_action_keywords = Vec::new();

        // Check for explicit question patterns
        if QUESTION_MARK_PATTERN.is_match(query_trimmed) {
            scores[1].1 += 0.4; // Question
        }

        if KOREAN_QUESTION_ENDING.is_match(query_trimmed) {
            scores[1].1 += 0.5; // Question
        }

        // Check English question words
        for word in &self.question_words_en {
            if query_lower.starts_with(word) || query_lower.contains(&format!(" {} ", word)) {
                scores[1].1 += 0.3; // Question
                detected_question_words.push(word.to_string());
            }
        }

        // Check Korean question words
        for word in &self.question_words_ko {
            if query_lower.contains(word) {
                scores[1].1 += 0.3; // Question
                detected_question_words.push(word.to_string());
            }
        }

        // Check summarize keywords
        for keyword in &self.summarize_keywords {
            if query_lower.contains(keyword) {
                scores[2].1 += 0.5; // Summarize
                detected_action_keywords.push(keyword.to_string());
            }
        }

        // Check compare keywords
        for keyword in &self.compare_keywords {
            if query_lower.contains(keyword) {
                scores[3].1 += 0.4; // Compare
                detected_action_keywords.push(keyword.to_string());
            }
        }

        // Check navigate keywords
        for keyword in &self.navigate_keywords {
            if query_lower.contains(keyword) {
                scores[4].1 += 0.5; // Navigate
                detected_action_keywords.push(keyword.to_string());
            }
        }

        // Check list keywords
        for keyword in &self.list_keywords {
            if query_lower.contains(keyword) {
                scores[5].1 += 0.4; // List
                detected_action_keywords.push(keyword.to_string());
            }
        }

        // Check for filter-heavy queries (likely Search or List)
        let filter_count = FILTER_HEAVY_PATTERN.find_iter(query_trimmed).count();
        if filter_count > 0 {
            scores[0].1 += 0.2 * filter_count as f32; // Search
            scores[5].1 += 0.1 * filter_count as f32; // List
        }

        // Check for exact phrase or regex (likely Search)
        if PHRASE_PATTERN.is_match(query_trimmed) || REGEX_PATTERN.is_match(query_trimmed) {
            scores[0].1 += 0.4; // Search
        }

        // Short queries are likely Search
        let word_count = query_trimmed.split_whitespace().count();
        if word_count <= 3
            && detected_question_words.is_empty()
            && detected_action_keywords.is_empty()
        {
            scores[0].1 += 0.3; // Search
        }

        // Normalize scores and find best match
        let total_score: f32 = scores.iter().map(|(_, s)| s).sum();

        if total_score > 0.0 {
            for (_, score) in &mut scores {
                *score /= total_score;
            }
        } else {
            // Default to Search if no signals
            scores[0].1 = 1.0;
        }

        // Sort by score descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (best_intent, best_score) = scores[0];

        // Build alternatives (exclude the best one)
        let alternatives: Vec<(QueryIntent, f32)> = scores
            .into_iter()
            .skip(1)
            .filter(|(_, score)| *score > 0.1)
            .collect();

        IntentClassification {
            intent: best_intent,
            confidence: best_score.min(1.0),
            alternatives,
            question_words: detected_question_words,
            action_keywords: detected_action_keywords,
        }
    }

    /// Quick check if a query is likely a question
    pub fn is_question(&self, query: &str) -> bool {
        let classification = self.classify(query);
        classification.intent == QueryIntent::Question && classification.confidence > 0.5
    }

    /// Quick check if a query requires LLM
    pub fn requires_llm(&self, query: &str) -> bool {
        let classification = self.classify(query);
        classification.intent.requires_llm() && classification.confidence > 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier() -> QueryIntentClassifier {
        QueryIntentClassifier::new()
    }

    #[test]
    fn test_simple_search_query() {
        let c = classifier();

        // Simple keyword search
        let result = c.classify("rust async programming");
        assert_eq!(result.intent, QueryIntent::Search);

        // Filter query
        let result = c.classify("from:drive type:pdf");
        assert_eq!(result.intent, QueryIntent::Search);
    }

    #[test]
    fn test_question_detection_english() {
        let c = classifier();

        let result = c.classify("What is the revenue for Q3?");
        assert_eq!(result.intent, QueryIntent::Question);
        assert!(result.confidence > 0.5);

        let result = c.classify("How do I configure the settings?");
        assert_eq!(result.intent, QueryIntent::Question);

        let result = c.classify("When was the last meeting?");
        assert_eq!(result.intent, QueryIntent::Question);
    }

    #[test]
    fn test_question_detection_korean() {
        let c = classifier();

        let result = c.classify("Q3 매출이 얼마야?");
        assert_eq!(result.intent, QueryIntent::Question);

        let result = c.classify("설정은 어떻게 해?");
        assert_eq!(result.intent, QueryIntent::Question);

        let result = c.classify("회의가 언제였어?");
        assert_eq!(result.intent, QueryIntent::Question);
    }

    #[test]
    fn test_summarize_detection() {
        let c = classifier();

        let result = c.classify("summarize this document");
        assert_eq!(result.intent, QueryIntent::Summarize);

        let result = c.classify("이 문서 요약해줘");
        assert_eq!(result.intent, QueryIntent::Summarize);

        let result = c.classify("give me a tldr");
        assert_eq!(result.intent, QueryIntent::Summarize);
    }

    #[test]
    fn test_compare_detection() {
        let c = classifier();

        let result = c.classify("compare Q2 and Q3 reports");
        assert_eq!(result.intent, QueryIntent::Compare);

        let result = c.classify("두 문서의 차이점");
        assert_eq!(result.intent, QueryIntent::Compare);
    }

    #[test]
    fn test_navigate_detection() {
        let c = classifier();

        let result = c.classify("open config.json");
        assert_eq!(result.intent, QueryIntent::Navigate);

        let result = c.classify("settings.rs 파일 열어");
        assert_eq!(result.intent, QueryIntent::Navigate);
    }

    #[test]
    fn test_list_detection() {
        let c = classifier();

        let result = c.classify("list all pdf files");
        assert_eq!(result.intent, QueryIntent::List);

        // "show all" triggers both List and Navigate, but List should win due to "all"
        let result = c.classify("전부 목록 보여줘");
        assert_eq!(result.intent, QueryIntent::List);
    }

    #[test]
    fn test_filter_heavy_query() {
        let c = classifier();

        let result = c.classify("from:drive type:pdf date:thisweek author:john");
        assert_eq!(result.intent, QueryIntent::Search);
    }

    #[test]
    fn test_phrase_query() {
        let c = classifier();

        let result = c.classify("\"quarterly report\"");
        assert_eq!(result.intent, QueryIntent::Search);
    }

    #[test]
    fn test_search_weights() {
        assert_eq!(QueryIntent::Search.search_weights(), (0.7, 0.3));
        assert_eq!(QueryIntent::Question.search_weights(), (0.5, 0.5));
        assert_eq!(QueryIntent::Summarize.search_weights(), (0.3, 0.7));
    }

    #[test]
    fn test_requires_llm() {
        assert!(!QueryIntent::Search.requires_llm());
        assert!(QueryIntent::Question.requires_llm());
        assert!(QueryIntent::Summarize.requires_llm());
        assert!(QueryIntent::Compare.requires_llm());
        assert!(!QueryIntent::Navigate.requires_llm());
        assert!(!QueryIntent::List.requires_llm());
    }

    #[test]
    fn test_is_question_helper() {
        let c = classifier();

        assert!(c.is_question("What is the revenue?"));
        assert!(!c.is_question("revenue report"));
    }

    #[test]
    fn test_requires_llm_helper() {
        let c = classifier();

        assert!(c.requires_llm("What is the revenue?"));
        assert!(c.requires_llm("summarize this"));
        // Short keyword queries typically classified as Search which doesn't require LLM
        assert!(!c.requires_llm("meeting notes"));
    }

    #[test]
    fn test_classification_alternatives() {
        let c = classifier();

        // A query that could be both question and compare
        let result = c.classify("what is the difference between Q2 and Q3?");
        assert!(!result.alternatives.is_empty());
    }

    #[test]
    fn test_default_intent() {
        assert_eq!(QueryIntent::default(), QueryIntent::Search);
    }

    #[test]
    fn test_empty_query() {
        let c = classifier();
        let result = c.classify("");
        // Empty query defaults to search
        assert_eq!(result.intent, QueryIntent::Search);
    }
}
