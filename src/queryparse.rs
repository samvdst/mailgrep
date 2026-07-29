//! Query grammar: free text + operators in one string -> AST. Pure, no I/O.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum FilterField {
    From,
    To,
    Cc,
    Folder,
    Org,
    Contact,
    Thread,
    Before,
    After,
    Date,
    Sent,
    Stored,
    Dateskew,
    Has,
    Ext,
    Attachment,
    Filename,
}

impl FilterField {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "from" => Self::From,
            "to" => Self::To,
            "cc" => Self::Cc,
            "folder" => Self::Folder,
            "org" => Self::Org,
            "contact" => Self::Contact,
            "thread" => Self::Thread,
            "before" => Self::Before,
            "after" => Self::After,
            "date" => Self::Date,
            "sent" => Self::Sent,
            "stored" => Self::Stored,
            "dateskew" => Self::Dateskew,
            "has" => Self::Has,
            "ext" => Self::Ext,
            "attachment" => Self::Attachment,
            "filename" => Self::Filename,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Clause {
    /// Unquoted free-text term (fuzzable).
    Term { text: String, negated: bool },
    /// Double-quoted exact phrase, never fuzzed.
    Phrase { text: String, negated: bool },
    /// Operator clause; comma-separated values are a disjunction.
    Filter {
        field: FilterField,
        values: Vec<String>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Query {
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, thiserror::Error)]
#[ts(export)]
#[error("bad query at `{token}`: {message}")]
pub struct QueryError {
    pub token: String,
    pub message: String,
}

fn err(token: &str, message: &str) -> QueryError {
    QueryError {
        token: token.to_string(),
        message: message.to_string(),
    }
}

/// Split into whitespace-separated tokens, keeping double-quoted runs together
/// (both `"a b"` and `filename:"a b"`).
fn tokenize(input: &str) -> Result<Vec<String>, QueryError> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(c);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if in_quotes {
        return Err(err(&cur, "unclosed quote"));
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    Ok(tokens)
}

fn valid_date(v: &str) -> bool {
    let parts: Vec<&str> = v.split('-').collect();
    match parts.as_slice() {
        [y] => y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()),
        [y, m] => {
            y.len() == 4
                && y.chars().all(|c| c.is_ascii_digit())
                && m.parse::<u32>().map(|m| (1..=12).contains(&m)).unwrap_or(false)
        }
        [y, m, d] => {
            y.len() == 4
                && y.chars().all(|c| c.is_ascii_digit())
                && m.parse::<u32>().map(|m| (1..=12).contains(&m)).unwrap_or(false)
                && d.parse::<u32>().map(|d| (1..=31).contains(&d)).unwrap_or(false)
        }
        _ => false,
    }
}

pub fn parse(input: &str) -> Result<Query, QueryError> {
    let mut clauses = Vec::new();
    for raw_token in tokenize(input)? {
        let (negated, token) = match raw_token.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest.to_string()),
            _ => (false, raw_token.clone()),
        };

        // Quoted phrase (possibly after the minus)
        if token.starts_with('"') {
            let text = token.trim_matches('"').to_string();
            if text.is_empty() {
                return Err(err(&raw_token, "empty phrase"));
            }
            clauses.push(Clause::Phrase { text, negated });
            continue;
        }

        // Operator? Only if the prefix before ':' is a known field; otherwise
        // the token is free text (URLs, times like 12:30 stay searchable).
        if let Some((prefix, rest)) = token.split_once(':') {
            if let Some(field) = FilterField::parse(&prefix.to_ascii_lowercase()) {
                if rest.is_empty() && field != FilterField::Dateskew {
                    return Err(err(&raw_token, "operator needs a value"));
                }
                let quoted = rest.starts_with('"');
                let cleaned = rest.trim_matches('"');
                let values: Vec<String> = if quoted {
                    vec![cleaned.to_string()]
                } else {
                    cleaned
                        .split(',')
                        .filter(|v| !v.is_empty())
                        .map(|v| v.to_string())
                        .collect()
                };
                match field {
                    FilterField::Before | FilterField::After | FilterField::Date
                    | FilterField::Sent | FilterField::Stored => {
                        for v in &values {
                            // allow ranges like 2021..2022 for date:/sent:/stored:
                            let ok = if let Some((a, b)) = v.split_once("..") {
                                valid_date(a) && valid_date(b)
                            } else {
                                valid_date(v)
                            };
                            if !ok {
                                return Err(err(
                                    &raw_token,
                                    "expected YYYY, YYYY-MM or YYYY-MM-DD",
                                ));
                            }
                        }
                    }
                    FilterField::Has => {
                        for v in &values {
                            if v != "attachment" {
                                return Err(err(&raw_token, "only has:attachment is supported"));
                            }
                        }
                    }
                    _ => {}
                }
                let values = if field == FilterField::Dateskew && values.is_empty() {
                    vec!["true".into()]
                } else {
                    values
                };
                if values.is_empty() {
                    return Err(err(&raw_token, "operator needs a value"));
                }
                clauses.push(Clause::Filter {
                    field,
                    values,
                    negated,
                });
                continue;
            }
        }

        clauses.push(Clause::Term {
            text: token,
            negated,
        });
    }
    Ok(Query { clauses })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(t: &str) -> Clause {
        Clause::Term { text: t.into(), negated: false }
    }

    #[test]
    fn free_text_and_operators() {
        let q = parse("boiler landlord from:@immo.ch after:2021-01 has:attachment").unwrap();
        assert_eq!(q.clauses[0], term("boiler"));
        assert_eq!(q.clauses[1], term("landlord"));
        assert_eq!(
            q.clauses[2],
            Clause::Filter { field: FilterField::From, values: vec!["@immo.ch".into()], negated: false }
        );
        assert_eq!(
            q.clauses[3],
            Clause::Filter { field: FilterField::After, values: vec!["2021-01".into()], negated: false }
        );
        assert_eq!(
            q.clauses[4],
            Clause::Filter { field: FilterField::Has, values: vec!["attachment".into()], negated: false }
        );
    }

    #[test]
    fn phrases_and_negation() {
        let q = parse(r#""exact phrase" -noise -folder:Junk"#).unwrap();
        assert_eq!(q.clauses[0], Clause::Phrase { text: "exact phrase".into(), negated: false });
        assert_eq!(q.clauses[1], Clause::Term { text: "noise".into(), negated: true });
        assert_eq!(
            q.clauses[2],
            Clause::Filter { field: FilterField::Folder, values: vec!["Junk".into()], negated: true }
        );
    }

    #[test]
    fn comma_disjunction() {
        let q = parse("ext:pdf,jpg").unwrap();
        assert_eq!(
            q.clauses[0],
            Clause::Filter { field: FilterField::Ext, values: vec!["pdf".into(), "jpg".into()], negated: false }
        );
    }

    #[test]
    fn quoted_operator_value() {
        let q = parse(r#"filename:"my contract.pdf""#).unwrap();
        assert_eq!(
            q.clauses[0],
            Clause::Filter { field: FilterField::Filename, values: vec!["my contract.pdf".into()], negated: false }
        );
    }

    #[test]
    fn unknown_prefix_is_free_text() {
        let q = parse("meeting 12:30 http://x.y/z").unwrap();
        assert_eq!(q.clauses.len(), 3);
        assert!(matches!(&q.clauses[1], Clause::Term { text, .. } if text == "12:30"));
    }

    #[test]
    fn errors_name_the_token() {
        let e = parse("after:sometime").unwrap_err();
        assert_eq!(e.token, "after:sometime");
        let e = parse(r#""unclosed"#).unwrap_err();
        assert!(e.message.contains("unclosed"));
        let e = parse("has:pony").unwrap_err();
        assert!(e.message.contains("has:attachment"));
        let e = parse("from:").unwrap_err();
        assert!(e.message.contains("value"));
    }

    #[test]
    fn date_ranges_and_dateskew() {
        assert!(parse("date:2021..2022").is_ok());
        assert!(parse("date:2021-05").is_ok());
        assert!(parse("date:21-05").is_err());
        let q = parse("dateskew:true").unwrap();
        assert!(matches!(&q.clauses[0], Clause::Filter { field: FilterField::Dateskew, .. }));
    }
}
