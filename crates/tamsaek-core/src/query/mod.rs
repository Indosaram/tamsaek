//! Query DSL module for parsing and representing structured search queries.
//!
//! This module provides a powerful query language for searching documents.
//!
//! # Query Syntax
//!
//! ## Basic Queries
//! - `term` - Simple word search
//! - `"exact phrase"` - Phrase matching
//! - `/regex/` - Regular expression
//! - `proj*` - Wildcard patterns
//!
//! ## Boolean Operators
//! - `a b` - Implicit AND
//! - `a AND b` - Explicit AND
//! - `a OR b` - OR
//! - `NOT a` or `-a` - NOT
//! - `(a OR b) AND c` - Grouping
//!
//! ## Filters
//! - `from:drive` - Source filter
//! - `ext:rs` - Extension filter
//! - `type:pdf` - File type filter
//! - `date:>7d` - Date filter (relative)
//! - `date:thisweek` - Date filter (preset)
//! - `modified:2024-01-01` - Date filter (absolute)
//! - `size:>10mb` - Size filter
//! - `author:john` - Author filter
//! - `path:projects/` - Path filter
//! - `tag:important` - Tag filter
//!
//! # Example
//!
//! ```
//! use tamsaek_core::query::{Query, QueryParser};
//!
//! let query = QueryParser::parse("from:drive ext:pdf date:>7d").unwrap();
//! let filters = query.extract_filters();
//! println!("Found {} filters", filters.len());
//! ```

mod dsl;
mod parser;

pub use dsl::{
    DateField, DateOp, DatePreset, DateValue, FieldOp, Filter, Query, RelativeDate, SizeOp,
    SourceType, TimeUnit,
};
pub use parser::QueryParser;
