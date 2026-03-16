use super::dsl::*;
use crate::document::SourceType;
use crate::error::ParseError;
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_until, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::{all_consuming, map, verify},
    multi::many0,
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

pub struct QueryParser;

impl QueryParser {
    pub fn parse(input: &str) -> Result<Query, ParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ParseError::EmptyQuery);
        }

        match all_consuming(preceded(multispace0, parse_query))(input) {
            Ok((_, query)) => Ok(query),
            Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
                Err(ParseError::UnexpectedToken {
                    position: input.len() - e.input.len(),
                    message: format!(
                        "Unexpected input: {:?}",
                        e.input.chars().take(20).collect::<String>()
                    ),
                })
            }
            Err(nom::Err::Incomplete(_)) => Err(ParseError::UnexpectedToken {
                position: input.len(),
                message: "Incomplete query".to_string(),
            }),
        }
    }
}

type ParseResult<'a, T> = IResult<&'a str, T>;

fn parse_query(input: &str) -> ParseResult<'_, Query> {
    parse_or_expr(input)
}

fn parse_or_expr(input: &str) -> ParseResult<'_, Query> {
    let (input, first) = parse_and_expr(input)?;
    let (input, rest) = many0(preceded(
        tuple((multispace1, tag_no_case("OR"), multispace1)),
        parse_and_expr,
    ))(input)?;

    if rest.is_empty() {
        Ok((input, first))
    } else {
        let mut all = vec![first];
        all.extend(rest);
        Ok((input, Query::or(all)))
    }
}

fn parse_and_expr(input: &str) -> ParseResult<'_, Query> {
    let (input, first) = parse_not_expr(input)?;
    let (input, rest) = many0(preceded(
        alt((
            map(
                tuple((multispace1, tag_no_case("AND"), multispace1)),
                |_| (),
            ),
            // Implicit AND: whitespace NOT followed by "OR" keyword
            map(multispace1, |_| ()),
        )),
        // Don't parse if next token is "OR" (case insensitive)
        verify(parse_not_expr, |q: &Query| {
            // Don't allow bare "or" term from implicit AND
            !matches!(q, Query::Term(t) if t.eq_ignore_ascii_case("or"))
        }),
    ))(input)?;

    if rest.is_empty() {
        Ok((input, first))
    } else {
        let mut all = vec![first];
        all.extend(rest);
        Ok((input, Query::and(all)))
    }
}

fn parse_not_expr(input: &str) -> ParseResult<'_, Query> {
    alt((
        map(
            preceded(
                pair(alt((tag_no_case("NOT"), tag("-"))), multispace0),
                parse_primary,
            ),
            Query::negate,
        ),
        parse_primary,
    ))(input)
}

fn parse_primary(input: &str) -> ParseResult<'_, Query> {
    alt((
        parse_grouped,
        parse_filter,
        parse_regex,
        parse_phrase,
        parse_wildcard,
        parse_term,
    ))(input)
}

fn parse_grouped(input: &str) -> ParseResult<'_, Query> {
    delimited(
        pair(char('('), multispace0),
        parse_query,
        pair(multispace0, char(')')),
    )(input)
}

fn parse_phrase(input: &str) -> ParseResult<'_, Query> {
    map(
        delimited(char('"'), take_until("\""), char('"')),
        |s: &str| Query::Phrase(s.to_string()),
    )(input)
}

fn parse_regex(input: &str) -> ParseResult<'_, Query> {
    map(
        delimited(char('/'), take_until("/"), char('/')),
        |s: &str| Query::Regex(s.to_string()),
    )(input)
}

fn parse_wildcard(input: &str) -> ParseResult<'_, Query> {
    let (input, word) = verify(
        take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '*' || c == '?'),
        |s: &str| s.contains('*') || s.contains('?'),
    )(input)?;
    Ok((input, Query::Wildcard(word.to_string())))
}

fn parse_term(input: &str) -> ParseResult<'_, Query> {
    let (input, word) =
        take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')(input)?;
    Ok((input, Query::Term(word.to_string())))
}

fn parse_filter(input: &str) -> ParseResult<'_, Query> {
    let (input, (key, _, value)) = tuple((
        take_while1(|c: char| c.is_alphanumeric() || c == '_'),
        char(':'),
        parse_filter_value,
    ))(input)?;

    let filter = match key.to_lowercase().as_str() {
        "from" | "source" | "in" => {
            let source = SourceType::parse(&value).ok_or_else(|| {
                nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
            })?;
            Filter::Source(source)
        }
        "type" | "kind" | "filetype" => Filter::FileType(value),
        "ext" | "extension" => Filter::Extension(value),
        "author" | "by" | "owner" => Filter::Author(value),
        "path" | "folder" | "dir" => Filter::Path(value),
        "tag" | "label" => Filter::Tag(value),
        "mime" | "mimetype" => Filter::MimeType(value),
        "has" => Filter::HasField(value),
        "size" => {
            let size_op = SizeOp::parse(&value).ok_or_else(|| {
                nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
            })?;
            Filter::Size(size_op)
        }
        "date" | "modified" | "created" | "indexed" => {
            let field = DateField::from_str(key).unwrap_or(DateField::Any);
            let date_op = parse_date_op_value(&value).ok_or_else(|| {
                nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
            })?;
            Filter::Date(field, date_op)
        }
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )))
        }
    };

    Ok((input, Query::Filter(filter)))
}

fn parse_filter_value(input: &str) -> ParseResult<'_, String> {
    alt((
        // Quoted value
        map(
            delimited(char('"'), take_until("\""), char('"')),
            |s: &str| s.to_string(),
        ),
        // Unquoted value (until whitespace or special chars)
        map(
            take_while1(|c: char| !c.is_whitespace() && c != ')' && c != '(' && c != '"'),
            |s: &str| s.to_string(),
        ),
    ))(input)
}

fn parse_date_op_value(value: &str) -> Option<DateOp> {
    let value = value.trim();

    // Range: 2024-01..2024-06
    if value.contains("..") {
        let parts: Vec<&str> = value.split("..").collect();
        if parts.len() == 2 {
            let start = parse_date_value(parts[0])?;
            let end = parse_date_value(parts[1])?;
            return Some(DateOp::Between(start, end));
        }
    }

    // Comparison operators
    if let Some(v) = value.strip_prefix(">=") {
        return parse_date_value(v.trim()).map(DateOp::After);
    }
    if let Some(v) = value.strip_prefix("<=") {
        return parse_date_value(v.trim()).map(DateOp::Before);
    }
    if let Some(v) = value.strip_prefix('>') {
        return parse_date_value(v.trim()).map(DateOp::After);
    }
    if let Some(v) = value.strip_prefix('<') {
        return parse_date_value(v.trim()).map(DateOp::Before);
    }

    // Equals
    parse_date_value(value).map(DateOp::Equals)
}

fn parse_date_value(value: &str) -> Option<DateValue> {
    let value = value.trim();

    // Preset: today, yesterday, thisweek, etc.
    if let Some(preset) = DatePreset::from_str(value) {
        return Some(DateValue::Preset(preset));
    }

    // Relative: 7d, 2w, 3m, 1y (with optional -ago suffix)
    let value_without_ago = value.strip_suffix("-ago").unwrap_or(value);
    if let Some((num, unit)) = parse_relative_date(value_without_ago) {
        return Some(DateValue::Relative(RelativeDate::new(num, unit)));
    }

    // Absolute date: 2024-01-15 or 2024-01 or 2024
    if let Some(dt) = parse_absolute_date(value) {
        return Some(DateValue::Absolute(dt));
    }

    None
}

fn parse_relative_date(value: &str) -> Option<(i64, TimeUnit)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let last_char = value.chars().last()?;
    let unit = TimeUnit::from_char(last_char)?;
    let num_str = &value[..value.len() - 1];
    let num: i64 = num_str.parse().ok()?;

    Some((num, unit))
}

fn parse_absolute_date(value: &str) -> Option<chrono::DateTime<Utc>> {
    // Try YYYY-MM-DD
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
    }

    // Try YYYY-MM
    if value.len() == 7 && value.chars().nth(4) == Some('-') {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() == 2 {
            let year: i32 = parts[0].parse().ok()?;
            let month: u32 = parts[1].parse().ok()?;
            let date = NaiveDate::from_ymd_opt(year, month, 1)?;
            return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
        }
    }

    // Try YYYY
    if value.len() == 4 && value.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = value.parse().ok()?;
        let date = NaiveDate::from_ymd_opt(year, 1, 1)?;
        return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
    }

    // Try month name: "march" or "march2024" or "march-2024"
    let value_normalized = value.replace(['-', '_'], "");
    if let Some(dt) = parse_month_with_year(&value_normalized) {
        return Some(dt);
    }

    // Try quarter+year: "q12024" or "q1-2024"
    if let Some(dt) = parse_quarter_with_year(&value_normalized) {
        return Some(dt);
    }

    None
}

/// Parse month name with optional year: "march", "march2024"
fn parse_month_with_year(value: &str) -> Option<chrono::DateTime<Utc>> {
    use super::dsl::parse_month_name;

    let value = value.to_lowercase();

    // Try to find where the month name ends and the year begins
    for i in 3..=9 {
        if i > value.len() {
            break;
        }
        let month_part = &value[..i];
        if let Some(month) = parse_month_name(month_part) {
            let year_part = &value[i..];
            let year = if year_part.is_empty() {
                Utc::now().year()
            } else {
                year_part.parse::<i32>().ok()?
            };
            let date = NaiveDate::from_ymd_opt(year, month, 1)?;
            return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
        }
    }

    None
}

/// Parse quarter with year: "q12024", "q22024"
fn parse_quarter_with_year(value: &str) -> Option<chrono::DateTime<Utc>> {
    let value = value.to_lowercase();

    // Must start with 'q' followed by 1-4
    if !value.starts_with('q') || value.len() < 2 {
        return None;
    }

    let quarter_char = value.chars().nth(1)?;
    let quarter: u32 = quarter_char.to_digit(10)?;
    if !(1..=4).contains(&quarter) {
        return None;
    }

    let year_part = &value[2..];
    let year = if year_part.is_empty() {
        Utc::now().year()
    } else {
        year_part.parse::<i32>().ok()?
    };

    // Q1=Jan, Q2=Apr, Q3=Jul, Q4=Oct
    let month = (quarter - 1) * 3 + 1;
    let date = NaiveDate::from_ymd_opt(year, month, 1)?;
    Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_term() {
        let query = QueryParser::parse("rust").unwrap();
        assert_eq!(query, Query::Term("rust".to_string()));
    }

    #[test]
    fn test_phrase() {
        let query = QueryParser::parse("\"hello world\"").unwrap();
        assert_eq!(query, Query::Phrase("hello world".to_string()));
    }

    #[test]
    fn test_regex() {
        let query = QueryParser::parse("/error.*code/").unwrap();
        assert_eq!(query, Query::Regex("error.*code".to_string()));
    }

    #[test]
    fn test_wildcard() {
        let query = QueryParser::parse("proj*").unwrap();
        assert_eq!(query, Query::Wildcard("proj*".to_string()));
    }

    #[test]
    fn test_source_filter() {
        let query = QueryParser::parse("from:drive").unwrap();
        assert_eq!(
            query,
            Query::Filter(Filter::Source(SourceType::GoogleDrive))
        );
    }

    #[test]
    fn test_date_filter_relative() {
        let query = QueryParser::parse("date:>7d").unwrap();
        if let Query::Filter(Filter::Date(field, DateOp::After(DateValue::Relative(rel)))) = query {
            assert_eq!(field, DateField::Any);
            assert_eq!(rel.amount, 7);
            assert_eq!(rel.unit, TimeUnit::Days);
        } else {
            panic!("Expected date filter with relative value");
        }
    }

    #[test]
    fn test_date_filter_preset() {
        let query = QueryParser::parse("modified:thisweek").unwrap();
        if let Query::Filter(Filter::Date(field, DateOp::Equals(DateValue::Preset(preset)))) = query
        {
            assert_eq!(field, DateField::Modified);
            assert_eq!(preset, DatePreset::ThisWeek);
        } else {
            panic!("Expected date filter with preset");
        }
    }

    #[test]
    fn test_size_filter() {
        let query = QueryParser::parse("size:>10mb").unwrap();
        if let Query::Filter(Filter::Size(SizeOp::GreaterThan(size))) = query {
            assert_eq!(size, 10 * 1024 * 1024);
        } else {
            panic!("Expected size filter");
        }
    }

    #[test]
    fn test_and_query() {
        let query = QueryParser::parse("rust async").unwrap();
        if let Query::And(parts) = query {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], Query::Term("rust".to_string()));
            assert_eq!(parts[1], Query::Term("async".to_string()));
        } else {
            panic!("Expected AND query");
        }
    }

    #[test]
    fn test_explicit_and() {
        let query = QueryParser::parse("rust AND async").unwrap();
        if let Query::And(parts) = query {
            assert_eq!(parts.len(), 2);
        } else {
            panic!("Expected AND query");
        }
    }

    #[test]
    fn test_or_query() {
        let query = QueryParser::parse("rust OR python").unwrap();
        if let Query::Or(parts) = query {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], Query::Term("rust".to_string()));
            assert_eq!(parts[1], Query::Term("python".to_string()));
        } else {
            panic!("Expected OR query");
        }
    }

    #[test]
    fn test_not_query() {
        let query = QueryParser::parse("NOT deprecated").unwrap();
        assert_eq!(
            query,
            Query::Not(Box::new(Query::Term("deprecated".to_string())))
        );
    }

    #[test]
    fn test_minus_not() {
        let query = QueryParser::parse("-draft").unwrap();
        assert_eq!(
            query,
            Query::Not(Box::new(Query::Term("draft".to_string())))
        );
    }

    #[test]
    fn test_grouped() {
        let query = QueryParser::parse("(rust OR python) async").unwrap();
        if let Query::And(parts) = query {
            assert_eq!(parts.len(), 2);
            if let Query::Or(inner) = &parts[0] {
                assert_eq!(inner.len(), 2);
            } else {
                panic!("Expected OR in group");
            }
        } else {
            panic!("Expected AND query");
        }
    }

    #[test]
    fn test_complex_query() {
        let query =
            QueryParser::parse("from:drive type:pdf date:>7d \"quarterly report\"").unwrap();
        if let Query::And(parts) = query {
            assert_eq!(parts.len(), 4);
        } else {
            panic!("Expected AND query with 4 parts");
        }
    }

    #[test]
    fn test_empty_query() {
        let result = QueryParser::parse("");
        assert!(matches!(result, Err(ParseError::EmptyQuery)));
    }

    #[test]
    fn test_whitespace_only() {
        let result = QueryParser::parse("   ");
        assert!(matches!(result, Err(ParseError::EmptyQuery)));
    }
}
