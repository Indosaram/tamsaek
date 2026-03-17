//! LLM (Large Language Model) integration module

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use super::AiError;

/// Configuration for LLM client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Model name
    pub model: String,

    /// Temperature for generation (0.0 - 1.0)
    pub temperature: f32,

    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,

    /// Top-p sampling
    pub top_p: Option<f32>,

    /// System prompt
    pub system_prompt: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: super::DEFAULT_MODEL.to_string(),
            temperature: 0.7,
            max_tokens: Some(2048),
            top_p: Some(0.9),
            system_prompt: None,
        }
    }
}

/// Response from LLM generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// Generated text content
    pub content: String,

    /// Model used for generation
    pub model: String,

    /// Number of tokens in prompt
    pub prompt_tokens: Option<u32>,

    /// Number of tokens generated
    pub completion_tokens: Option<u32>,

    /// Total generation time in milliseconds
    pub duration_ms: Option<u64>,

    /// Whether generation was stopped early
    pub stopped_early: bool,
}

/// Message role in conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

/// Stream chunk from LLM generation
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Partial content
    pub content: String,

    /// Whether this is the final chunk
    pub done: bool,
}

/// Type alias for streaming response
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, AiError>> + Send>>;

/// Trait for LLM clients
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate a response for a single prompt
    async fn generate(&self, prompt: &str) -> Result<LlmResponse, AiError>;

    /// Generate a response with conversation history
    async fn chat(&self, messages: &[Message]) -> Result<LlmResponse, AiError>;

    /// Stream a response for a single prompt
    fn stream(&self, prompt: &str) -> LlmStream;

    /// Stream a response with conversation history
    fn stream_chat(&self, messages: &[Message]) -> LlmStream;

    /// Check if the LLM backend is available
    async fn is_available(&self) -> bool;

    /// List available models
    async fn list_models(&self) -> Result<Vec<String>, AiError>;

    /// Get the current model name
    fn model(&self) -> &str;

    /// Get the configuration
    fn config(&self) -> &LlmConfig;
}

/// Blanket implementation for Arc<L> where L: LlmClient
#[async_trait]
impl<L: LlmClient + ?Sized> LlmClient for std::sync::Arc<L> {
    async fn generate(&self, prompt: &str) -> Result<LlmResponse, AiError> {
        (**self).generate(prompt).await
    }

    async fn chat(&self, messages: &[Message]) -> Result<LlmResponse, AiError> {
        (**self).chat(messages).await
    }

    fn stream(&self, prompt: &str) -> LlmStream {
        (**self).stream(prompt)
    }

    fn stream_chat(&self, messages: &[Message]) -> LlmStream {
        (**self).stream_chat(messages)
    }

    async fn is_available(&self) -> bool {
        (**self).is_available().await
    }

    async fn list_models(&self) -> Result<Vec<String>, AiError> {
        (**self).list_models().await
    }

    fn model(&self) -> &str {
        (**self).model()
    }

    fn config(&self) -> &LlmConfig {
        (**self).config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LlmConfig::default();
        assert_eq!(config.model, "Qwen2.5-3B-Instruct");
        assert!((config.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_message_creation() {
        let system = Message::system("You are a helpful assistant");
        assert_eq!(system.role, MessageRole::System);

        let user = Message::user("Hello!");
        assert_eq!(user.role, MessageRole::User);

        let assistant = Message::assistant("Hi there!");
        assert_eq!(assistant.role, MessageRole::Assistant);
    }
}
