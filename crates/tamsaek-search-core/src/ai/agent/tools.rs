//! Agent Tool Definitions
//!
//! Tools that the Search Agent can invoke during execution.

use serde::{Deserialize, Serialize};

use super::super::query_parser::{DateRange, FileTypeCategory, SortPreference, SourceFilter};

/// Tool invocation from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "arguments")]
pub enum ToolCall {
    /// Parse natural language query into structured filters
    #[serde(rename = "parse_query")]
    ParseQuery(ParseQueryArgs),

    /// Execute a search with optional filters
    #[serde(rename = "search")]
    Search(SearchArgs),

    /// Read document content for RAG
    #[serde(rename = "read_document")]
    ReadDocument(ReadDocumentArgs),

    /// Ask user for clarification
    #[serde(rename = "ask_user")]
    AskUser(AskUserArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseQueryArgs {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub filters: Option<SearchFilters>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    #[serde(default)]
    pub file_types: Vec<FileTypeCategory>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub date_range: Option<DateRange>,
    #[serde(default)]
    pub sources: Vec<SourceFilter>,
    #[serde(default)]
    pub sort_by: Option<SortPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDocumentArgs {
    pub document_id: String,
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserArgs {
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// Result from tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ToolResult {
    #[serde(rename = "success")]
    Success { data: serde_json::Value },
    #[serde(rename = "error")]
    Error { message: String },
}

impl ToolResult {
    pub fn success<T: Serialize>(data: T) -> Self {
        Self::Success {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// Tool definitions for LLM system prompt
pub const TOOL_DEFINITIONS: &str = r#"
You have access to the following tools:

1. parse_query
   - Purpose: Parse natural language query into structured filters
   - Arguments: { "query": "natural language query" }
   - Returns: { "file_types": [...], "date_range": "...", "keywords": [...], ... }

2. search
   - Purpose: Search files with query and optional filters
   - Arguments: { "query": "search terms", "filters": {...}, "limit": 10 }
   - Returns: Array of { "id": "...", "title": "...", "score": 0.85, "snippet": "..." }

3. read_document
   - Purpose: Read document content for analysis
   - Arguments: { "document_id": "...", "max_chars": 2000 }
   - Returns: { "content": "document text...", "metadata": {...} }

4. ask_user
   - Purpose: Ask user for clarification when query is ambiguous
   - Arguments: { "question": "What do you mean by...?", "options": ["A", "B"] }
   - Returns: User's response

To use a tool, respond with JSON:
{"name": "tool_name", "arguments": {...}}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_parse() {
        let json = r#"{"name": "search", "arguments": {"query": "test", "limit": 5}}"#;
        let tool: ToolCall = serde_json::from_str(json).unwrap();

        if let ToolCall::Search(args) = tool {
            assert_eq!(args.query, "test");
            assert_eq!(args.limit, 5);
        } else {
            panic!("Expected Search tool");
        }
    }

    #[test]
    fn test_tool_call_with_filters() {
        let json = r#"{
            "name": "search",
            "arguments": {
                "query": "report",
                "filters": {
                    "file_types": ["document"],
                    "date_range": "last_week"
                },
                "limit": 20
            }
        }"#;
        let tool: ToolCall = serde_json::from_str(json).unwrap();

        if let ToolCall::Search(args) = tool {
            assert_eq!(args.query, "report");
            assert!(args.filters.is_some());
            let filters = args.filters.unwrap();
            assert_eq!(filters.file_types.len(), 1);
        } else {
            panic!("Expected Search tool");
        }
    }

    #[test]
    fn test_ask_user() {
        let json = r#"{"name": "ask_user", "arguments": {"question": "Which project?", "options": ["A", "B"]}}"#;
        let tool: ToolCall = serde_json::from_str(json).unwrap();

        if let ToolCall::AskUser(args) = tool {
            assert_eq!(args.question, "Which project?");
            assert_eq!(args.options.len(), 2);
        } else {
            panic!("Expected AskUser tool");
        }
    }

    #[test]
    fn test_tool_result() {
        let success = ToolResult::success(vec!["a", "b", "c"]);
        assert!(success.is_success());

        let error = ToolResult::error("Not found");
        assert!(!error.is_success());
    }
}
