//! Agent module for agentic file search

mod mode;
mod search_agent;
mod tools;

pub use mode::SearchMode;
pub use search_agent::{
    AgentAnswer, AgentResponse, AgentState, DocumentReader, SearchAgent, SearchBackend,
    SearchResult,
};
pub use tools::{
    AskUserArgs, ParseQueryArgs, ReadDocumentArgs, SearchArgs, SearchFilters, ToolCall, ToolResult,
    TOOL_DEFINITIONS,
};
