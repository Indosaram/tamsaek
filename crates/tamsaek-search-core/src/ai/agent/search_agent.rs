//! Search Agent - Agentic file search with tool use

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::super::llm::{LlmClient, Message, MessageRole};
use super::super::query_parser::QueryParser;
use super::super::AiError;

use super::tools::{ToolCall, ToolResult, TOOL_DEFINITIONS};

const AGENT_SYSTEM_PROMPT: &str = r#"You are a file search assistant. Help users find files on their computer.

Your goal: Find the files the user is looking for, analyze results, and provide helpful answers.

Strategy:
1. First, understand what the user wants
2. Use parse_query to extract filters from natural language
3. Use search to find matching files
4. If results are unclear, use read_document to check content
5. If query is ambiguous, use ask_user to clarify
6. When confident, provide final answer with found files

Always respond in JSON format:
- Tool call: {"name": "tool_name", "arguments": {...}}
- Final answer: {"answer": "Your response to user", "files": [...]}

Be concise. Focus on finding the right files.
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub score: f32,
    pub snippet: Option<String>,
    pub path: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnswer {
    pub answer: String,
    pub files: Vec<SearchResult>,
}

#[derive(Debug, Clone)]
pub enum AgentResponse {
    Answer(AgentAnswer),
    Clarification {
        question: String,
        options: Vec<String>,
    },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub iteration: usize,
    pub last_tool: Option<String>,
    pub search_results: Vec<SearchResult>,
}

#[async_trait]
pub trait SearchBackend: Send + Sync {
    async fn search(
        &self,
        query: &str,
        filters: Option<&super::tools::SearchFilters>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, AiError>;
    async fn read_document(&self, id: &str, max_chars: Option<usize>) -> Result<String, AiError>;
}

#[async_trait]
pub trait DocumentReader: Send + Sync {
    async fn read_content(&self, id: &str, max_chars: Option<usize>) -> Result<String, AiError>;
}

pub struct SearchAgent<L: LlmClient + ?Sized, B: SearchBackend> {
    llm: Arc<L>,
    backend: Arc<B>,
    max_iterations: usize,
}

impl<L: LlmClient + ?Sized + 'static, B: SearchBackend> SearchAgent<L, B> {
    pub fn new(llm: Arc<L>, backend: Arc<B>) -> Self {
        Self {
            llm,
            backend,
            max_iterations: 10,
        }
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub async fn run(&self, user_message: &str) -> Result<AgentResponse, AiError> {
        let mut messages = vec![
            Message::system(format!("{}\n\n{}", AGENT_SYSTEM_PROMPT, TOOL_DEFINITIONS)),
            Message::user(user_message),
        ];

        let mut state = AgentState {
            iteration: 0,
            last_tool: None,
            search_results: Vec::new(),
        };

        for iteration in 0..self.max_iterations {
            state.iteration = iteration;
            debug!("Agent iteration {}", iteration);

            let response = self.llm.chat(&messages).await?;
            let content = response.content.trim();

            info!("Agent response: {}", content);

            let json_str = Self::extract_json(content);

            if let Ok(answer) = serde_json::from_str::<AgentAnswer>(&json_str) {
                return Ok(AgentResponse::Answer(answer));
            }

            if let Ok(tool_call) = serde_json::from_str::<ToolCall>(&json_str) {
                let (tool_name, result) = self.execute_tool(&tool_call, &mut state).await;

                if let ToolCall::AskUser(args) = &tool_call {
                    return Ok(AgentResponse::Clarification {
                        question: args.question.clone(),
                        options: args.options.clone(),
                    });
                }

                messages.push(Message::assistant(content));
                messages.push(Message {
                    role: MessageRole::User,
                    content: format!(
                        "Tool '{}' result:\n{}",
                        tool_name,
                        serde_json::to_string_pretty(&result).unwrap_or_default()
                    ),
                });

                state.last_tool = Some(tool_name);
                continue;
            }

            warn!("Could not parse agent response as JSON, treating as answer");
            return Ok(AgentResponse::Answer(AgentAnswer {
                answer: content.to_string(),
                files: state.search_results.clone(),
            }));
        }

        Ok(AgentResponse::Error(
            "Max iterations reached without answer".into(),
        ))
    }

    pub async fn continue_with_answer(
        &self,
        previous_messages: Vec<Message>,
        user_answer: &str,
    ) -> Result<AgentResponse, AiError> {
        let mut messages = previous_messages;
        messages.push(Message::user(user_answer));

        let mut state = AgentState {
            iteration: 0,
            last_tool: None,
            search_results: Vec::new(),
        };

        for iteration in 0..self.max_iterations {
            state.iteration = iteration;

            let response = self.llm.chat(&messages).await?;
            let content = response.content.trim();
            let json_str = Self::extract_json(content);

            if let Ok(answer) = serde_json::from_str::<AgentAnswer>(&json_str) {
                return Ok(AgentResponse::Answer(answer));
            }

            if let Ok(tool_call) = serde_json::from_str::<ToolCall>(&json_str) {
                let (tool_name, result) = self.execute_tool(&tool_call, &mut state).await;

                if let ToolCall::AskUser(args) = &tool_call {
                    return Ok(AgentResponse::Clarification {
                        question: args.question.clone(),
                        options: args.options.clone(),
                    });
                }

                messages.push(Message::assistant(content));
                messages.push(Message {
                    role: MessageRole::User,
                    content: format!(
                        "Tool '{}' result:\n{}",
                        tool_name,
                        serde_json::to_string_pretty(&result).unwrap_or_default()
                    ),
                });

                state.last_tool = Some(tool_name);
                continue;
            }

            return Ok(AgentResponse::Answer(AgentAnswer {
                answer: content.to_string(),
                files: state.search_results.clone(),
            }));
        }

        Ok(AgentResponse::Error("Max iterations reached".into()))
    }

    async fn execute_tool(&self, tool: &ToolCall, state: &mut AgentState) -> (String, ToolResult) {
        match tool {
            ToolCall::ParseQuery(args) => {
                let name = "parse_query".to_string();
                let parser = QueryParser::new(self.llm.clone());
                match parser.parse(&args.query).await {
                    Ok(filter) => (name, ToolResult::success(filter)),
                    Err(e) => (name, ToolResult::error(e.to_string())),
                }
            }

            ToolCall::Search(args) => {
                let name = "search".to_string();
                match self
                    .backend
                    .search(&args.query, args.filters.as_ref(), args.limit)
                    .await
                {
                    Ok(results) => {
                        state.search_results = results.clone();
                        (name, ToolResult::success(results))
                    }
                    Err(e) => (name, ToolResult::error(e.to_string())),
                }
            }

            ToolCall::ReadDocument(args) => {
                let name = "read_document".to_string();
                match self
                    .backend
                    .read_document(&args.document_id, args.max_chars)
                    .await
                {
                    Ok(content) => (
                        name,
                        ToolResult::success(serde_json::json!({ "content": content })),
                    ),
                    Err(e) => (name, ToolResult::error(e.to_string())),
                }
            }

            ToolCall::AskUser(_) => (
                "ask_user".to_string(),
                ToolResult::success(serde_json::json!({})),
            ),
        }
    }

    fn extract_json(content: &str) -> String {
        if content.contains("```") {
            if let Some(json_part) = content.split("```").nth(1) {
                let trimmed = json_part.trim_start_matches("json").trim();
                if trimmed.starts_with('{') {
                    return trimmed.to_string();
                }
            }
        }

        if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                if start <= end {
                    return content[start..=end].to_string();
                }
            }
        }

        content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tools::SearchFilters;
    use crate::ai::llm::{LlmConfig, LlmResponse, LlmStream};

    struct MockLlm;

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn generate(
            &self,
            _prompt: &str,
        ) -> Result<LlmResponse, AiError> {
            unimplemented!()
        }

        async fn chat(
            &self,
            _messages: &[Message],
        ) -> Result<LlmResponse, AiError> {
            unimplemented!()
        }

        fn stream(&self, _prompt: &str) -> LlmStream {
            unimplemented!()
        }

        fn stream_chat(&self, _messages: &[Message]) -> LlmStream {
            unimplemented!()
        }

        async fn is_available(&self) -> bool {
            false
        }

        async fn list_models(&self) -> Result<Vec<String>, AiError> {
            Ok(vec![])
        }

        fn model(&self) -> &str {
            "mock"
        }

        fn config(&self) -> &LlmConfig {
            static CONFIG: std::sync::OnceLock<LlmConfig> = std::sync::OnceLock::new();
            CONFIG.get_or_init(LlmConfig::default)
        }
    }

    #[test]
    fn test_extract_json() {
        let cases = [
            (r#"{"name": "search"}"#, r#"{"name": "search"}"#),
            (
                "Here is the result:\n```json\n{\"a\": 1}\n```",
                "{\"a\": 1}",
            ),
            ("Some text {\"x\": 2} more text", "{\"x\": 2}"),
        ];

        for (input, expected) in cases {
            let result = SearchAgent::<MockLlm, MockBackend>::extract_json(input);
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    struct MockBackend;

    #[async_trait]
    impl SearchBackend for MockBackend {
        async fn search(
            &self,
            _query: &str,
            _filters: Option<&SearchFilters>,
            _limit: usize,
        ) -> Result<Vec<SearchResult>, AiError> {
            Ok(vec![])
        }
        async fn read_document(
            &self,
            _id: &str,
            _max_chars: Option<usize>,
        ) -> Result<String, AiError> {
            Ok(String::new())
        }
    }
}
