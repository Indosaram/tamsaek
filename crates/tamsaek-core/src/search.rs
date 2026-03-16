use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub score: f32,
    pub snippet: Option<String>,
    pub path: Option<String>,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<String>,
    pub source: Option<String>,
}
