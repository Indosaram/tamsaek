//! Query Parser - Natural Language to Search Filter Conversion
//!
//! This module uses a small LLM (Qwen 0.5B/1.5B) to parse natural language
//! queries into structured search filters.
//!
//! Supports multiple languages (Korean, English, Japanese, Chinese, etc.)

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::llm::{LlmClient, Message};
use super::AiError;

/// Parsed filter result from natural language query
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedFilter {
    /// File type categories to search
    #[serde(default, alias = "file_type", alias = "fileTypes", alias = "fileType")]
    pub file_types: Vec<FileTypeCategory>,

    /// Specific file extensions (e.g., ["pdf", "docx"])
    #[serde(default, alias = "extension", alias = "ext")]
    pub extensions: Vec<String>,

    /// Date range filter
    #[serde(default, alias = "dateRange", alias = "date")]
    pub date_range: Option<DateRange>,

    /// Source filters (local, google_drive, onedrive, sharepoint)
    #[serde(default, alias = "source")]
    pub sources: Vec<SourceFilter>,

    /// Additional keywords for text search
    #[serde(default, alias = "keyword", alias = "search", alias = "query")]
    pub keywords: Vec<String>,

    /// Sort preference
    #[serde(default, alias = "sortBy", alias = "sort")]
    pub sort_by: Option<SortPreference>,

    /// Original query for fallback
    #[serde(skip)]
    pub original_query: String,

    /// Confidence score (0.0 - 1.0)
    #[serde(default)]
    pub confidence: f32,
}

/// File type categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileTypeCategory {
    /// Documents (pdf, doc, docx, odt, rtf)
    Document,
    /// Spreadsheets (xls, xlsx, csv, ods)
    Spreadsheet,
    /// Presentations (ppt, pptx, key)
    Presentation,
    /// Source code files
    Code,
    /// Plain text and markdown
    Text,
    /// Images (jpg, png, gif, svg)
    Image,
    /// Audio files (mp3, wav, flac)
    Audio,
    /// Video files (mp4, mkv, avi)
    Video,
    /// Archive files (zip, tar, rar)
    Archive,
    /// E-books (epub, mobi)
    Ebook,
}

impl FileTypeCategory {
    /// Get file extensions for this category
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Document => &["pdf", "doc", "docx", "odt", "rtf", "hwp", "hwpx"],
            Self::Spreadsheet => &["xls", "xlsx", "csv", "ods", "numbers"],
            Self::Presentation => &["ppt", "pptx", "key", "odp"],
            Self::Code => &[
                "rs", "py", "js", "ts", "jsx", "tsx", "go", "c", "cpp", "h", "hpp", "java", "rb",
                "swift", "kt", "scala", "php", "cs", "fs", "hs", "ml", "ex", "exs", "clj", "vue",
                "svelte",
            ],
            Self::Text => &["txt", "md", "markdown", "rst", "org", "tex", "log"],
            Self::Image => &[
                "jpg", "jpeg", "png", "gif", "svg", "webp", "bmp", "tiff", "ico", "heic",
            ],
            Self::Audio => &["mp3", "wav", "flac", "m4a", "ogg", "aac", "wma", "aiff"],
            Self::Video => &["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v"],
            Self::Archive => &["zip", "tar", "gz", "rar", "7z", "bz2", "xz"],
            Self::Ebook => &["epub", "mobi", "azw", "azw3", "fb2"],
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Document => "Documents",
            Self::Spreadsheet => "Spreadsheets",
            Self::Presentation => "Presentations",
            Self::Code => "Code",
            Self::Text => "Text Files",
            Self::Image => "Images",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archive => "Archives",
            Self::Ebook => "E-books",
        }
    }
}

/// Date range filter
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateRange {
    /// Today
    Today,
    /// Yesterday
    Yesterday,
    /// This week (current week)
    ThisWeek,
    /// Last 7 days
    LastWeek,
    /// Last 30 days
    LastMonth,
    /// Last 3 months
    LastQuarter,
    /// Last 365 days
    LastYear,
    /// Custom range with ISO dates
    Custom {
        from: Option<String>,
        to: Option<String>,
    },
}

impl DateRange {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This week",
            Self::LastWeek => "Last week",
            Self::LastMonth => "Last month",
            Self::LastQuarter => "Last 3 months",
            Self::LastYear => "Last year",
            Self::Custom { .. } => "Custom range",
        }
    }
}

/// Source filter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFilter {
    /// Local files
    Local,
    /// Google Drive
    GoogleDrive,
    /// Microsoft OneDrive
    OneDrive,
    /// Microsoft SharePoint
    SharePoint,
}

impl SourceFilter {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::GoogleDrive => "Google Drive",
            Self::OneDrive => "OneDrive",
            Self::SharePoint => "SharePoint",
        }
    }
}

/// Sort preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortPreference {
    /// Most relevant first
    Relevance,
    /// Newest first
    Newest,
    /// Oldest first
    Oldest,
    /// Largest first
    Largest,
    /// Smallest first
    Smallest,
    /// Alphabetical A-Z
    NameAsc,
    /// Alphabetical Z-A
    NameDesc,
}

impl SortPreference {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Relevance => "Relevance",
            Self::Newest => "Newest first",
            Self::Oldest => "Oldest first",
            Self::Largest => "Largest first",
            Self::Smallest => "Smallest first",
            Self::NameAsc => "Name (A-Z)",
            Self::NameDesc => "Name (Z-A)",
        }
    }
}

/// System prompt for query parsing - simple format works best with small models
const SYSTEM_PROMPT: &str = r#"Query to JSON. Output JSON only.

date_range: yesterday, today, last_week, this_week, last_month
file_types: document, spreadsheet, presentation, code, image, audio, video
extensions: pdf, docx, xlsx, pptx, jpg, png

yesterday → {"date_range":"yesterday"}
today → {"date_range":"today"}
last week → {"date_range":"last_week"}
this week → {"date_range":"this_week"}
PDF → {"extensions":["pdf"]}
excel → {"file_types":["spreadsheet"]}
code → {"file_types":["code"]}
image → {"file_types":["image"]}
presentation → {"file_types":["presentation"]}
어제 → {"date_range":"yesterday"}
오늘 → {"date_range":"today"}
지난주 → {"date_range":"last_week"}
이번주 → {"date_range":"this_week"}
사진 → {"file_types":["image"]}
문서 → {"file_types":["document"]}
발표자료 → {"file_types":["presentation"]}
어제 파일 → {"date_range":"yesterday"}
오늘 문서 → {"date_range":"today","file_types":["document"]}
지난주 발표 → {"date_range":"last_week","file_types":["presentation"]}

Query:"#;

/// System prompt for conversational filter refinement
const SYSTEM_PROMPT_CONVERSATION: &str = r#"Update JSON filters based on user message. Output JSON only.

date_range: yesterday, today, last_week, this_week, last_month
file_types: document, spreadsheet, presentation, code, image, audio, video

{} → "오늘" → {"date_range":"today"}
{} → "어제" → {"date_range":"yesterday"}
{} → "사진" → {"file_types":["image"]}
{"date_range":"last_week"} → "PDF만" → {"date_range":"last_week","extensions":["pdf"]}
{"date_range":"yesterday"} → "사진" → {"date_range":"yesterday","file_types":["image"]}

Current:"#;

/// Query parser using LLM
pub struct QueryParser<L: LlmClient> {
    llm: L,
}

impl<L: LlmClient> QueryParser<L> {
    /// Create a new query parser
    pub fn new(llm: L) -> Self {
        Self { llm }
    }

    /// Parse a natural language query into filters
    pub async fn parse(&self, query: &str) -> Result<ParsedFilter, AiError> {
        self.parse_with_context(query, None).await
    }

    /// Parse with conversation context (previous filter)
    /// If previous_filter is provided, the AI will modify/extend it based on new query
    pub async fn parse_with_context(
        &self,
        query: &str,
        previous_filter: Option<&ParsedFilter>,
    ) -> Result<ParsedFilter, AiError> {
        if query.trim().is_empty() {
            return Ok(previous_filter.cloned().unwrap_or_default());
        }

        let messages = if let Some(prev) = previous_filter {
            // Build context from previous filter
            let prev_json = serde_json::to_string(prev).unwrap_or_default();
            vec![
                Message::system(SYSTEM_PROMPT_CONVERSATION),
                Message::user(format!(
                    "Current filters: {}\nUser says: \"{}\"",
                    prev_json, query
                )),
            ]
        } else {
            vec![
                Message::system(SYSTEM_PROMPT),
                Message::user(format!("\"{}\"", query)),
            ]
        };

        debug!(
            "Parsing query: {} (with context: {})",
            query,
            previous_filter.is_some()
        );

        let response = self.llm.chat(&messages).await?;
        let content = response.content.trim();

        info!("LLM response for query '{}': {}", query, content);

        // Try to parse JSON from response
        let mut filter = self.parse_json_response(content)?;
        filter.original_query = query.to_string();

        Ok(filter)
    }

    /// Parse JSON response, handling potential formatting issues
    fn parse_json_response(&self, content: &str) -> Result<ParsedFilter, AiError> {
        // Extract JSON from content (handles markdown code blocks, etc.)
        let json_str = Self::extract_json(content);

        // Normalize the JSON to handle model variations
        let normalized = Self::normalize_json(&json_str);

        // Try parsing normalized JSON
        if let Ok(filter) = serde_json::from_str::<ParsedFilter>(&normalized) {
            return Ok(filter);
        }

        // Try parsing original extracted JSON
        if let Ok(filter) = serde_json::from_str::<ParsedFilter>(&json_str) {
            return Ok(filter);
        }

        // Log the full response content for debugging
        warn!("Failed to parse LLM response as JSON");
        warn!("Response content ({} chars): {}", content.len(), content);
        warn!("Extracted JSON: {}", json_str);

        // Return empty filter with low confidence
        Ok(ParsedFilter {
            confidence: 0.0,
            ..Default::default()
        })
    }

    /// Extract JSON from content (handles markdown code blocks)
    fn extract_json(content: &str) -> String {
        // Try to extract from markdown code block
        if content.contains("```") {
            if let Some(json_part) = content.split("```").nth(1) {
                let trimmed = json_part.trim_start_matches("json").trim();
                if trimmed.starts_with('{') {
                    return trimmed.to_string();
                }
            }
        }

        // Try to find JSON object in the response
        if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                if start <= end {
                    return content[start..=end].to_string();
                }
            }
        }

        content.to_string()
    }

    /// Normalize JSON to handle common model output variations
    fn normalize_json(json_str: &str) -> String {
        // First, try to fix common JSON syntax errors
        let fixed = Self::fix_json_syntax(json_str);

        // Normalize field names before parsing (handle spaces and case variations)
        let normalized = fixed
            // Field names (various formats -> standard)
            .replace("\"date Range\"", "\"date_range\"")
            .replace("\"date range\"", "\"date_range\"")
            .replace("\"Date Range\"", "\"date_range\"")
            .replace("\"dateRange\"", "\"date_range\"")
            .replace("\"query_date\"", "\"date_range\"")
            .replace("\" dateRange \"", "\"date_range\"")
            .replace("\" dateRange\"", "\"date_range\"")
            .replace("\"dateRange \"", "\"date_range\"")
            .replace("\"data_type\"", "\"file_types\"")
            .replace("\"date_ranges\"", "\"date_range\"")
            .replace("\"month_end_date\"", "\"date_range\"")
            .replace("\"File Types\"", "\"file_types\"")
            .replace("\"file Types\"", "\"file_types\"")
            .replace("\"File types\"", "\"file_types\"")
            .replace("\"fileTypes\"", "\"file_types\"")
            .replace("\" fileTypes\"", "\"file_types\"")
            .replace("\"fileTypes \"", "\"file_types\"")
            .replace("\" fileTypes \"", "\"file_types\"")
            .replace("\"document_type\"", "\"file_types\"")
            .replace("\"types\"", "\"file_types\"")
            .replace("\"type\"", "\"file_types\"")
            .replace("\"Type\"", "\"file_types\"")
            .replace("\"keyword\"", "\"keywords\"")
            // date_range values (case + space + underscore variations + typos)
            .replace("\"yesteraday\"", "\"yesterday\"")
            .replace("\"Yesterday\"", "\"yesterday\"")
            .replace("\"Today\"", "\"today\"")
            // Chinese date values
            .replace("\"今天\"", "\"today\"")
            .replace("\"昨天\"", "\"yesterday\"")
            .replace("\"上周\"", "\"last_week\"")
            .replace("\"本周\"", "\"this_week\"")
            .replace("\"上个月\"", "\"last_month\"")
            .replace("\"本月\"", "\"last_month\"")
            // Korean date values
            .replace("\"오늘\"", "\"today\"")
            .replace("\"어제\"", "\"yesterday\"")
            .replace("\"지난주\"", "\"last_week\"")
            .replace("\"이번주\"", "\"this_week\"")
            .replace("\"지난달\"", "\"last_month\"")
            .replace("\"이번달\"", "\"last_month\"")
            // Japanese date values
            .replace("\"今日\"", "\"today\"")
            .replace("\"昨日\"", "\"yesterday\"")
            .replace("\"先週\"", "\"last_week\"")
            .replace("\"今週\"", "\"this_week\"")
            .replace("\"先月\"", "\"last_month\"")
            .replace("\"今月\"", "\"last_month\"")
            .replace("\"Last Week\"", "\"last_week\"")
            .replace("\"last week\"", "\"last_week\"")
            .replace("\"Last_Week\"", "\"last_week\"")
            .replace("\"last_Week\"", "\"last_week\"")
            .replace("\"Last_week\"", "\"last_week\"")
            .replace("\"LastWeek\"", "\"last_week\"")
            .replace("\"week\"", "\"last_week\"")
            .replace("\"week_ago\"", "\"last_week\"")
            .replace("\"two_days_ago\"", "\"yesterday\"")
            .replace("\"2_days_ago\"", "\"yesterday\"")
            .replace("\"Last Month\"", "\"last_month\"")
            .replace("\"last month\"", "\"last_month\"")
            .replace("\"Last_Month\"", "\"last_month\"")
            .replace("\"last_Month\"", "\"last_month\"")
            .replace("\"LastMonth\"", "\"last_month\"")
            .replace("\"this_month\"", "\"last_month\"")
            .replace("\"this month\"", "\"last_month\"")
            // this_week variations
            .replace("\"This Week\"", "\"this_week\"")
            .replace("\"this week\"", "\"this_week\"")
            .replace("\"This_Week\"", "\"this_week\"")
            .replace("\"ThisWeek\"", "\"this_week\"")
            // file_types values (case variations + synonyms)
            .replace("\"Document\"", "\"document\"")
            .replace("\"report\"", "\"document\"")
            .replace("\"reports\"", "\"document\"")
            .replace("\"Report\"", "\"document\"")
            .replace("\"Reports\"", "\"document\"")
            .replace("\"data\"", "\"document\"")
            .replace("\"file\"", "\"document\"")
            .replace("\"File\"", "\"document\"")
            .replace("\"file_type\"", "\"document\"")
            .replace("\"Spreadsheet\"", "\"spreadsheet\"")
            .replace("\"table\"", "\"spreadsheet\"")
            .replace("\"Table\"", "\"spreadsheet\"")
            .replace("\"Presentation\"", "\"presentation\"")
            .replace("\"slides\"", "\"presentation\"")
            .replace("\"Slides\"", "\"presentation\"")
            .replace("\"ppt\"", "\"presentation\"")
            .replace("\"Code\"", "\"code\"")
            // Programming languages → code
            .replace("\"typescript\"", "\"code\"")
            .replace("\"Typescript\"", "\"code\"")
            .replace("\"TypeScript\"", "\"code\"")
            .replace("\"javascript\"", "\"code\"")
            .replace("\"Javascript\"", "\"code\"")
            .replace("\"JavaScript\"", "\"code\"")
            .replace("\"python\"", "\"code\"")
            .replace("\"Python\"", "\"code\"")
            .replace("\"rust\"", "\"code\"")
            .replace("\"Rust\"", "\"code\"")
            .replace("\"java\"", "\"code\"")
            .replace("\"Java\"", "\"code\"")
            .replace("\"script\"", "\"code\"")
            .replace("\"Script\"", "\"code\"")
            .replace("\"scripts\"", "\"code\"")
            .replace("\"Scripts\"", "\"code\"")
            .replace("\"source\"", "\"code\"")
            .replace("\"Source\"", "\"code\"")
            .replace("\"programming\"", "\"code\"")
            .replace("\"excel\"", "\"spreadsheet\"")
            .replace("\"Excel\"", "\"spreadsheet\"")
            .replace("\"xls\"", "\"spreadsheet\"")
            .replace("\"xlsx\"", "\"spreadsheet\"")
            .replace("\"Image\"", "\"image\"")
            .replace("\"Audio\"", "\"audio\"")
            .replace("\"Video\"", "\"video\"")
            // Multilingual file type terms
            // Korean
            .replace("\"문서\"", "\"document\"")
            .replace("\"보고서\"", "\"document\"")
            .replace("\"리포트\"", "\"document\"")
            .replace("\"발표\"", "\"presentation\"")
            .replace("\"발표자료\"", "\"presentation\"")
            .replace("\"슬라이드\"", "\"presentation\"")
            .replace("\"프레젠테이션\"", "\"presentation\"")
            .replace("\"엑셀\"", "\"spreadsheet\"")
            .replace("\"스프레드시트\"", "\"spreadsheet\"")
            .replace("\"코드\"", "\"code\"")
            .replace("\"소스\"", "\"code\"")
            .replace("\"소스코드\"", "\"code\"")
            // Korean - Image/Photo
            .replace("\"그림\"", "\"image\"")
            .replace("\"그림파일\"", "\"image\"")
            .replace("\"사진\"", "\"image\"")
            .replace("\"이미지\"", "\"image\"")
            .replace("\"사진파일\"", "\"image\"")
            .replace("\"이미지파일\"", "\"image\"")
            .replace("\"스크린샷\"", "\"image\"")
            .replace("\"캡처\"", "\"image\"")
            // Korean - Video
            .replace("\"동영상\"", "\"video\"")
            .replace("\"영상\"", "\"video\"")
            .replace("\"비디오\"", "\"video\"")
            .replace("\"영상파일\"", "\"video\"")
            .replace("\"동영상파일\"", "\"video\"")
            .replace("\"무비\"", "\"video\"")
            // Korean - Audio
            .replace("\"음악\"", "\"audio\"")
            .replace("\"음악파일\"", "\"audio\"")
            .replace("\"오디오\"", "\"audio\"")
            .replace("\"음성\"", "\"audio\"")
            .replace("\"녹음\"", "\"audio\"")
            .replace("\"노래\"", "\"audio\"")
            // Japanese
            .replace("\"ドキュメント\"", "\"document\"")
            .replace("\"文書\"", "\"document\"")
            .replace("\"報告\"", "\"document\"")
            .replace("\"プレゼン\"", "\"presentation\"")
            .replace("\"資料\"", "\"presentation\"")
            .replace("\"スライド\"", "\"presentation\"")
            .replace("\"表計算\"", "\"spreadsheet\"")
            .replace("\"コード\"", "\"code\"")
            // Japanese - Image/Video/Audio
            .replace("\"画像\"", "\"image\"")
            .replace("\"写真\"", "\"image\"")
            .replace("\"動画\"", "\"video\"")
            .replace("\"ビデオ\"", "\"video\"")
            .replace("\"音楽\"", "\"audio\"")
            .replace("\"音声\"", "\"audio\"")
            // Chinese
            .replace("\"文档\"", "\"document\"")
            .replace("\"报告\"", "\"document\"")
            .replace("\"文件\"", "\"document\"")
            .replace("\"演示\"", "\"presentation\"")
            .replace("\"幻灯片\"", "\"presentation\"")
            .replace("\"表格\"", "\"spreadsheet\"")
            .replace("\"代码\"", "\"code\"")
            // Chinese - Image/Video/Audio
            .replace("\"图片\"", "\"image\"")
            .replace("\"照片\"", "\"image\"")
            .replace("\"图像\"", "\"image\"")
            .replace("\"视频\"", "\"video\"")
            .replace("\"影片\"", "\"video\"")
            .replace("\"音乐\"", "\"audio\"")
            .replace("\"音频\"", "\"audio\"");

        // Parse as generic JSON Value to normalize
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&normalized) else {
            return json_str.to_string();
        };

        if let Some(obj) = value.as_object_mut() {
            // Handle date_range as array -> extract first element
            if let Some(date_val) = obj.get("date_range").cloned() {
                if let Some(arr) = date_val.as_array() {
                    if let Some(first) = arr.first() {
                        obj.insert("date_range".to_string(), first.clone());
                    }
                }
            }

            // Handle "files.type" -> "file_types"
            if let Some(files) = obj.remove("files") {
                if let Some(files_obj) = files.as_object() {
                    if let Some(file_type) = files_obj.get("type").or(files_obj.get("file_types")) {
                        let types = if file_type.is_string() {
                            vec![file_type.clone()]
                        } else if let Some(arr) = file_type.as_array() {
                            arr.clone()
                        } else {
                            vec![]
                        };
                        obj.insert("file_types".to_string(), serde_json::Value::Array(types));
                    }
                }
            }

            // Handle "file_types" as string -> array, normalize values, and move extensions
            let extension_values = [
                "pdf", "docx", "xlsx", "pptx", "doc", "xls", "ppt", "txt", "md", "ts", "js", "py",
                "rs", "java", "cpp", "c", "h", "go", "rb", "php",
            ];
            if let Some(ft) = obj.remove("file_types") {
                let mut types = Vec::new();
                let mut exts = Vec::new();

                let items = if ft.is_string() {
                    vec![ft]
                } else if let Some(arr) = ft.as_array() {
                    arr.clone()
                } else {
                    vec![]
                };

                for item in items {
                    if let Some(s) = item.as_str() {
                        let lower = s.to_lowercase();
                        // Check if it's an extension
                        if extension_values.contains(&lower.as_str()) {
                            exts.push(serde_json::Value::String(lower));
                            continue;
                        }
                        // Normalize file type value
                        let normalized_type = Self::normalize_file_type(&lower);
                        if !normalized_type.is_empty() {
                            types.push(serde_json::Value::String(normalized_type));
                        }
                    }
                }

                if !types.is_empty() {
                    obj.insert("file_types".to_string(), serde_json::Value::Array(types));
                }
                if !exts.is_empty() {
                    // Merge with existing extensions
                    let existing = obj
                        .remove("extensions")
                        .and_then(|e| e.as_array().cloned())
                        .unwrap_or_default();
                    let mut all_exts = existing;
                    all_exts.extend(exts);
                    obj.insert("extensions".to_string(), serde_json::Value::Array(all_exts));
                }
            }

            // Handle "extension" (string) -> "extensions" (array)
            if let Some(ext) = obj.remove("extension") {
                if ext.is_string() {
                    let existing = obj
                        .remove("extensions")
                        .and_then(|e| e.as_array().cloned())
                        .unwrap_or_default();
                    let mut all_exts = existing;
                    all_exts.push(ext);
                    obj.insert("extensions".to_string(), serde_json::Value::Array(all_exts));
                }
            }

            // Handle keywords as string -> array, and filter generic keywords
            if let Some(kw) = obj.remove("keywords") {
                let items = if kw.is_string() {
                    vec![kw]
                } else if let Some(arr) = kw.as_array() {
                    arr.clone()
                } else {
                    vec![]
                };

                let filtered: Vec<_> = items
                    .into_iter()
                    .filter(|v| {
                        if let Some(s) = v.as_str() {
                            !["자료", "파일", "문서"].contains(&s)
                        } else {
                            true
                        }
                    })
                    .collect();

                if !filtered.is_empty() {
                    obj.insert("keywords".to_string(), serde_json::Value::Array(filtered));
                }
            }
        }

        serde_json::to_string(&value).unwrap_or_else(|_| json_str.to_string())
    }

    /// Fix common JSON syntax errors from LLM output
    fn fix_json_syntax(json_str: &str) -> String {
        let mut fixed = json_str.to_string();

        // Remove trailing commas before } or ]
        loop {
            let prev = fixed.clone();
            fixed = fixed.replace(",}", "}").replace(",]", "]");
            if fixed == prev {
                break;
            }
        }

        // Fix unbalanced braces (too many closing braces)
        let open_braces = fixed.chars().filter(|&c| c == '{').count();
        let close_braces = fixed.chars().filter(|&c| c == '}').count();
        if close_braces > open_braces {
            // Remove extra closing braces from end
            let extra = close_braces - open_braces;
            for _ in 0..extra {
                if let Some(pos) = fixed.rfind('}') {
                    fixed.remove(pos);
                }
            }
        }

        fixed
    }

    /// Normalize file type value to standard types: document, spreadsheet, presentation, code, image, audio, video
    fn normalize_file_type(value: &str) -> String {
        match value {
            // Already valid
            "document" | "spreadsheet" | "presentation" | "code" | "image" | "audio" | "video" => {
                value.to_string()
            }
            // Document synonyms (English)
            "report" | "reports" | "doc" | "docs" | "text" | "file" | "files" | "data" | "word" => {
                "document".to_string()
            }
            // Document synonyms (Korean)
            "문서" | "보고서" | "리포트" | "텍스트" | "파일" => "document".to_string(),
            // Document synonyms (Japanese)
            "ドキュメント" | "文書" | "報告" | "レポート" | "テキスト" => {
                "document".to_string()
            }
            // Document synonyms (Chinese)
            "文档" | "报告" | "文件" => "document".to_string(),

            // Spreadsheet synonyms (English)
            "excel" | "sheet" | "sheets" | "table" | "tables" | "csv" => "spreadsheet".to_string(),
            // Spreadsheet synonyms (Korean)
            "엑셀" | "시트" | "스프레드시트" | "표" => "spreadsheet".to_string(),
            // Spreadsheet synonyms (Japanese)
            "表計算" | "シート" | "エクセル" => "spreadsheet".to_string(),
            // Spreadsheet synonyms (Chinese)
            "表格" | "电子表格" => "spreadsheet".to_string(),

            // Presentation synonyms (English)
            "ppt" | "pptx" | "slides" | "slide" | "powerpoint" | "keynote" => {
                "presentation".to_string()
            }
            // Presentation synonyms (Korean)
            "발표" | "발표자료" | "슬라이드" | "프레젠테이션" | "피피티" => {
                "presentation".to_string()
            }
            // Presentation synonyms (Japanese)
            "プレゼン" | "プレゼンテーション" | "スライド" | "資料" => {
                "presentation".to_string()
            }
            // Presentation synonyms (Chinese)
            "演示" | "幻灯片" | "演示文稿" => "presentation".to_string(),

            // Code synonyms (English)
            "source" | "script" | "scripts" | "programming" | "program" | "programs" => {
                "code".to_string()
            }
            // Programming languages -> code
            "typescript" | "javascript" | "python" | "rust" | "java" | "cpp" | "c++" | "golang"
            | "ruby" | "php" | "swift" | "kotlin" | "scala" | "html" | "css" | "csharp" | "c#" => {
                "code".to_string()
            }
            // Code synonyms (Korean)
            "코드" | "소스" | "소스코드" | "프로그램" | "스크립트" => {
                "code".to_string()
            }
            // Code synonyms (Japanese)
            "コード" | "ソース" | "ソースコード" | "プログラム" | "スクリプト" => {
                "code".to_string()
            }
            // Code synonyms (Chinese)
            "代码" | "源码" | "源代码" | "程序" | "脚本" => "code".to_string(),

            // Image synonyms
            "photo" | "photos" | "picture" | "pictures" | "img" | "png" | "jpg" | "jpeg"
            | "gif" | "사진" | "이미지" | "그림" | "写真" | "画像" | "图片" | "照片" => {
                "image".to_string()
            }

            // Audio synonyms
            "music" | "sound" | "mp3" | "wav" | "음악" | "오디오" | "音楽" | "音声" | "音频"
            | "音乐" => "audio".to_string(),

            // Video synonyms
            "movie" | "movies" | "mp4" | "avi" | "영상" | "동영상" | "비디오" | "動画"
            | "ビデオ" | "视频" | "影片" => "video".to_string(),

            // Unknown - return as-is (will be filtered or cause parse error)
            _ => value.to_string(),
        }
    }

    /// Check if the LLM is available
    pub async fn is_available(&self) -> bool {
        self.llm.is_available().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== FileTypeCategory Tests ====================

    #[test]
    fn test_file_type_extensions() {
        let doc = FileTypeCategory::Document;
        assert!(doc.extensions().contains(&"pdf"));
        assert!(doc.extensions().contains(&"docx"));

        let pres = FileTypeCategory::Presentation;
        assert!(pres.extensions().contains(&"pptx"));
        assert!(pres.extensions().contains(&"key"));
    }

    #[test]
    fn test_file_type_all_categories() {
        // Test all file type categories have extensions
        let categories = [
            FileTypeCategory::Document,
            FileTypeCategory::Spreadsheet,
            FileTypeCategory::Presentation,
            FileTypeCategory::Code,
            FileTypeCategory::Text,
            FileTypeCategory::Image,
            FileTypeCategory::Audio,
            FileTypeCategory::Video,
            FileTypeCategory::Archive,
            FileTypeCategory::Ebook,
        ];

        for category in categories {
            assert!(
                !category.extensions().is_empty(),
                "{:?} should have extensions",
                category
            );
            assert!(
                !category.display_name().is_empty(),
                "{:?} should have display name",
                category
            );
        }
    }

    #[test]
    fn test_file_type_code_extensions() {
        let code = FileTypeCategory::Code;
        let exts = code.extensions();
        assert!(exts.contains(&"rs"));
        assert!(exts.contains(&"py"));
        assert!(exts.contains(&"js"));
        assert!(exts.contains(&"ts"));
        assert!(exts.contains(&"go"));
    }

    #[test]
    fn test_file_type_image_extensions() {
        let image = FileTypeCategory::Image;
        let exts = image.extensions();
        assert!(exts.contains(&"jpg"));
        assert!(exts.contains(&"jpeg"));
        assert!(exts.contains(&"png"));
        assert!(exts.contains(&"gif"));
        assert!(exts.contains(&"webp"));
    }

    // ==================== DateRange Tests ====================

    #[test]
    fn test_date_range_display() {
        assert_eq!(DateRange::Today.display_name(), "Today");
        assert_eq!(DateRange::LastWeek.display_name(), "Last week");
    }

    #[test]
    fn test_source_filter_display() {
        assert_eq!(SourceFilter::Local.display_name(), "Local");
        assert_eq!(SourceFilter::GoogleDrive.display_name(), "Google Drive");
    }

    #[test]
    fn test_sort_preference_display() {
        assert_eq!(SortPreference::Relevance.display_name(), "Relevance");
        assert_eq!(SortPreference::Newest.display_name(), "Newest first");
    }

    #[test]
    fn test_parsed_filter_default() {
        let filter = ParsedFilter::default();
        assert!(filter.file_types.is_empty());
        assert!(filter.extensions.is_empty());
        assert!(filter.date_range.is_none());
        assert!(filter.sources.is_empty());
        assert!(filter.keywords.is_empty());
        assert!(filter.sort_by.is_none());
        assert_eq!(filter.confidence, 0.0);
    }
}
