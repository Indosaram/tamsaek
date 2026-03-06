//! Tool modules for the tamsaek-mcp server.
//!
//! This module organizes MCP tools into logical groups:
//! - `search`: Full-text search, regex search, and filter tools
//! - `index`: Directory indexing, document removal, and index clearing
//! - `stats`: Statistics and document retrieval

pub mod index;
pub mod search;
pub mod stats;
