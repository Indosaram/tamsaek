//! Query DSL types for structured search queries.
//!
//! This module provides a rich query language that supports:
//! - Simple terms and phrases
//! - Regular expressions
//! - Wildcards
//! - Boolean operators (AND, OR, NOT)
//! - Field-specific filters (date, size, extension, etc.)

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a parsed search query
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Query {
    /// Simple term search
    Term(String),
    /// Exact phrase match
    Phrase(String),
    /// Regular expression pattern
    Regex(String),
    /// Wildcard pattern (e.g., "project*")
    Wildcard(String),
    /// Filter expression
    Filter(Filter),
    /// Boolean AND
    And(Vec<Query>),
    /// Boolean OR
    Or(Vec<Query>),
    /// Boolean NOT
    Not(Box<Query>),
    /// Empty query (matches all)
    All,
}

impl Query {
    pub fn term(s: impl Into<String>) -> Self {
        Self::Term(s.into())
    }

    pub fn phrase(s: impl Into<String>) -> Self {
        Self::Phrase(s.into())
    }

    pub fn regex(s: impl Into<String>) -> Self {
        Self::Regex(s.into())
    }

    pub fn and(queries: Vec<Query>) -> Self {
        if queries.is_empty() {
            Self::All
        } else if queries.len() == 1 {
            queries.into_iter().next().unwrap()
        } else {
            Self::And(queries)
        }
    }

    pub fn or(queries: Vec<Query>) -> Self {
        if queries.is_empty() {
            Self::All
        } else if queries.len() == 1 {
            queries.into_iter().next().unwrap()
        } else {
            Self::Or(queries)
        }
    }

    pub fn negate(query: Query) -> Self {
        Self::Not(Box::new(query))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Extract all filters from the query
    pub fn extract_filters(&self) -> Vec<&Filter> {
        let mut filters = Vec::new();
        self.collect_filters(&mut filters);
        filters
    }

    fn collect_filters<'a>(&'a self, filters: &mut Vec<&'a Filter>) {
        match self {
            Self::Filter(f) => filters.push(f),
            Self::And(qs) | Self::Or(qs) => {
                for q in qs {
                    q.collect_filters(filters);
                }
            }
            Self::Not(q) => q.collect_filters(filters),
            _ => {}
        }
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Term(t) => write!(f, "{}", t),
            Self::Phrase(p) => write!(f, "\"{}\"", p),
            Self::Regex(r) => write!(f, "/{}/", r),
            Self::Wildcard(w) => write!(f, "{}", w),
            Self::Filter(filter) => write!(f, "{}", filter),
            Self::And(qs) => {
                let parts: Vec<_> = qs.iter().map(|q| format!("{}", q)).collect();
                write!(f, "({})", parts.join(" AND "))
            }
            Self::Or(qs) => {
                let parts: Vec<_> = qs.iter().map(|q| format!("{}", q)).collect();
                write!(f, "({})", parts.join(" OR "))
            }
            Self::Not(q) => write!(f, "NOT {}", q),
            Self::All => write!(f, "*"),
        }
    }
}

/// Source types for documents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Local,
    GoogleDrive,
    SharePoint,
    OneDrive,
    Dropbox,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::GoogleDrive => "googledrive",
            Self::SharePoint => "sharepoint",
            Self::OneDrive => "onedrive",
            Self::Dropbox => "dropbox",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "local" => Some(Self::Local),
            "googledrive" | "gdrive" | "drive" | "google" => Some(Self::GoogleDrive),
            "sharepoint" | "sp" => Some(Self::SharePoint),
            "onedrive" | "od" => Some(Self::OneDrive),
            "dropbox" | "db" => Some(Self::Dropbox),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::GoogleDrive => "Google Drive",
            Self::SharePoint => "SharePoint",
            Self::OneDrive => "OneDrive",
            Self::Dropbox => "Dropbox",
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Filter expressions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    /// Source filter: from:drive
    Source(SourceType),
    /// File type filter: type:pdf
    FileType(String),
    /// Extension filter: ext:rs
    Extension(String),
    /// Date filter: date:>7d, modified:<2024-01-01
    Date(DateField, DateOp),
    /// Author filter: author:john
    Author(String),
    /// Path filter: path:projects/
    Path(String),
    /// Tag filter: tag:important
    Tag(String),
    /// Size filter: size:>10mb
    Size(SizeOp),
    /// Field existence: has:author
    HasField(String),
    /// MIME type filter: mime:application/pdf
    MimeType(String),
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(s) => write!(f, "from:{}", s.as_str()),
            Self::FileType(t) => write!(f, "type:{}", t),
            Self::Extension(e) => write!(f, "ext:{}", e),
            Self::Date(field, op) => write!(f, "{}:{}", field.as_str(), op),
            Self::Author(a) => write!(f, "author:{}", a),
            Self::Path(p) => write!(f, "path:{}", p),
            Self::Tag(t) => write!(f, "tag:{}", t),
            Self::Size(op) => write!(f, "size:{}", op),
            Self::HasField(field) => write!(f, "has:{}", field),
            Self::MimeType(m) => write!(f, "mime:{}", m),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateField {
    Created,
    Modified,
    Indexed,
    Any, // Match any date field
}

impl DateField {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Indexed => "indexed",
            Self::Any => "date",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "created" | "created_at" => Some(Self::Created),
            "modified" | "modified_at" | "updated" | "updated_at" => Some(Self::Modified),
            "indexed" | "indexed_at" => Some(Self::Indexed),
            "date" => Some(Self::Any),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DateOp {
    Before(DateValue),
    After(DateValue),
    Between(DateValue, DateValue),
    Equals(DateValue),
}

impl fmt::Display for DateOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before(v) => write!(f, "<{}", v),
            Self::After(v) => write!(f, ">{}", v),
            Self::Between(start, end) => write!(f, "{}..{}", start, end),
            Self::Equals(v) => write!(f, "{}", v),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DateValue {
    Absolute(DateTime<Utc>),
    Relative(RelativeDate),
    Preset(DatePreset),
}

impl DateValue {
    pub fn to_datetime(&self) -> DateTime<Utc> {
        match self {
            Self::Absolute(dt) => *dt,
            Self::Relative(rel) => rel.to_datetime(),
            Self::Preset(preset) => preset.to_datetime(),
        }
    }
}

impl fmt::Display for DateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(dt) => write!(f, "{}", dt.format("%Y-%m-%d")),
            Self::Relative(rel) => write!(f, "{}", rel),
            Self::Preset(preset) => write!(f, "{}", preset.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelativeDate {
    pub amount: i64,
    pub unit: TimeUnit,
}

impl RelativeDate {
    pub fn new(amount: i64, unit: TimeUnit) -> Self {
        Self { amount, unit }
    }

    pub fn days(amount: i64) -> Self {
        Self::new(amount, TimeUnit::Days)
    }

    pub fn weeks(amount: i64) -> Self {
        Self::new(amount, TimeUnit::Weeks)
    }

    pub fn months(amount: i64) -> Self {
        Self::new(amount, TimeUnit::Months)
    }

    pub fn years(amount: i64) -> Self {
        Self::new(amount, TimeUnit::Years)
    }

    pub fn to_datetime(&self) -> DateTime<Utc> {
        let now = Utc::now();
        match self.unit {
            TimeUnit::Days => now - Duration::days(self.amount),
            TimeUnit::Weeks => now - Duration::weeks(self.amount),
            TimeUnit::Months => now - Duration::days(self.amount * 30),
            TimeUnit::Years => now - Duration::days(self.amount * 365),
        }
    }
}

impl fmt::Display for RelativeDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.amount, self.unit.suffix())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeUnit {
    Days,
    Weeks,
    Months,
    Years,
}

impl TimeUnit {
    pub fn suffix(&self) -> &'static str {
        match self {
            Self::Days => "d",
            Self::Weeks => "w",
            Self::Months => "m",
            Self::Years => "y",
        }
    }

    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'd' => Some(Self::Days),
            'w' => Some(Self::Weeks),
            'm' => Some(Self::Months),
            'y' => Some(Self::Years),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatePreset {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    ThisYear,
    LastYear,
    // Quarter presets
    Q1,
    Q2,
    Q3,
    Q4,
}

impl DatePreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::ThisWeek => "thisweek",
            Self::LastWeek => "lastweek",
            Self::ThisMonth => "thismonth",
            Self::LastMonth => "lastmonth",
            Self::ThisYear => "thisyear",
            Self::LastYear => "lastyear",
            Self::Q1 => "q1",
            Self::Q2 => "q2",
            Self::Q3 => "q3",
            Self::Q4 => "q4",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace(['-', '_'], "").as_str() {
            "today" => Some(Self::Today),
            "yesterday" => Some(Self::Yesterday),
            "thisweek" => Some(Self::ThisWeek),
            "lastweek" => Some(Self::LastWeek),
            "thismonth" => Some(Self::ThisMonth),
            "lastmonth" => Some(Self::LastMonth),
            "thisyear" => Some(Self::ThisYear),
            "lastyear" => Some(Self::LastYear),
            "q1" => Some(Self::Q1),
            "q2" => Some(Self::Q2),
            "q3" => Some(Self::Q3),
            "q4" => Some(Self::Q4),
            _ => None,
        }
    }

    #[allow(clippy::wrong_self_convention)] // intentional: Copy type but method is expensive
    pub fn to_datetime(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let year = now.year();

        match self {
            Self::Today => {
                let date = now.date_naive();
                date.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            Self::Yesterday => {
                let date = (now - Duration::days(1)).date_naive();
                date.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            Self::ThisWeek => {
                let days_from_monday = now.weekday().num_days_from_monday() as i64;
                let date = (now - Duration::days(days_from_monday)).date_naive();
                date.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            Self::LastWeek => {
                let days_from_monday = now.weekday().num_days_from_monday() as i64;
                let date = (now - Duration::days(days_from_monday + 7)).date_naive();
                date.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            Self::ThisMonth => {
                let month = now.month();
                NaiveDate::from_ymd_opt(year, month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
            Self::LastMonth => {
                let month = now.month();
                let (y, m) = if month == 1 {
                    (year - 1, 12)
                } else {
                    (year, month - 1)
                };
                NaiveDate::from_ymd_opt(y, m, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
            Self::ThisYear => NaiveDate::from_ymd_opt(year, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::LastYear => NaiveDate::from_ymd_opt(year - 1, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::Q1 => NaiveDate::from_ymd_opt(year, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::Q2 => NaiveDate::from_ymd_opt(year, 4, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::Q3 => NaiveDate::from_ymd_opt(year, 7, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::Q4 => NaiveDate::from_ymd_opt(year, 10, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        }
    }
}

/// Parse month name to month number (1-12)
pub fn parse_month_name(s: &str) -> Option<u32> {
    match s.to_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SizeOp {
    GreaterThan(u64),
    LessThan(u64),
    Between(u64, u64),
    Equals(u64),
}

impl SizeOp {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // Range: 10mb..100mb
        if s.contains("..") {
            let parts: Vec<&str> = s.split("..").collect();
            if parts.len() == 2 {
                let min = parse_size(parts[0])?;
                let max = parse_size(parts[1])?;
                return Some(Self::Between(min, max));
            }
        }

        if let Some(size_str) = s.strip_prefix('>') {
            parse_size(size_str.trim()).map(Self::GreaterThan)
        } else if let Some(size_str) = s.strip_prefix('<') {
            parse_size(size_str.trim()).map(Self::LessThan)
        } else if let Some(size_str) = s.strip_prefix(">=") {
            parse_size(size_str.trim()).map(Self::GreaterThan)
        } else if let Some(size_str) = s.strip_prefix("<=") {
            parse_size(size_str.trim()).map(Self::LessThan)
        } else {
            parse_size(s).map(Self::Equals)
        }
    }
}

impl fmt::Display for SizeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GreaterThan(s) => write!(f, ">{}", format_size(*s)),
            Self::LessThan(s) => write!(f, "<{}", format_size(*s)),
            Self::Between(min, max) => write!(f, "{}..{}", format_size(*min), format_size(*max)),
            Self::Equals(s) => write!(f, "{}", format_size(*s)),
        }
    }
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("tb") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("gb") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("mb") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = s.strip_suffix("kb") {
        (n, 1024u64)
    } else if let Some(n) = s.strip_suffix('b') {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };

    num_str.trim().parse::<u64>().ok().map(|n| n * multiplier)
}

fn format_size(bytes: u64) -> String {
    const TB: u64 = 1024 * 1024 * 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;

    if bytes >= TB {
        format!("{}tb", bytes / TB)
    } else if bytes >= GB {
        format!("{}gb", bytes / GB)
    } else if bytes >= MB {
        format!("{}mb", bytes / MB)
    } else if bytes >= KB {
        format!("{}kb", bytes / KB)
    } else {
        format!("{}b", bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldOp {
    Equals,
    Contains,
    StartsWith,
    EndsWith,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_parsing() {
        assert_eq!(
            SizeOp::parse(">10mb"),
            Some(SizeOp::GreaterThan(10 * 1024 * 1024))
        );
        assert_eq!(
            SizeOp::parse("<1gb"),
            Some(SizeOp::LessThan(1024 * 1024 * 1024))
        );
        assert_eq!(SizeOp::parse("100kb"), Some(SizeOp::Equals(100 * 1024)));
    }

    #[test]
    fn test_date_preset() {
        let today = DatePreset::Today.to_datetime();
        assert_eq!(today.date_naive(), Utc::now().date_naive());
    }

    #[test]
    fn test_relative_date() {
        let week_ago = RelativeDate::weeks(1).to_datetime();
        let expected = Utc::now() - Duration::weeks(1);
        assert!((week_ago - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn test_source_type_parsing() {
        assert_eq!(SourceType::parse("local"), Some(SourceType::Local));
        assert_eq!(SourceType::parse("drive"), Some(SourceType::GoogleDrive));
        assert_eq!(
            SourceType::parse("googledrive"),
            Some(SourceType::GoogleDrive)
        );
        assert_eq!(
            SourceType::parse("sharepoint"),
            Some(SourceType::SharePoint)
        );
        assert_eq!(SourceType::parse("onedrive"), Some(SourceType::OneDrive));
        assert_eq!(SourceType::parse("dropbox"), Some(SourceType::Dropbox));
        assert_eq!(SourceType::parse("invalid"), None);
    }

    #[test]
    fn test_query_display() {
        assert_eq!(format!("{}", Query::Term("test".to_string())), "test");
        assert_eq!(
            format!("{}", Query::Phrase("hello world".to_string())),
            "\"hello world\""
        );
        assert_eq!(
            format!("{}", Query::Regex(".*test".to_string())),
            "/.*test/"
        );
        assert_eq!(format!("{}", Query::Wildcard("proj*".to_string())), "proj*");
        assert_eq!(format!("{}", Query::All), "*");
    }
}
