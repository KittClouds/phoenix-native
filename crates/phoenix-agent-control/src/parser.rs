use thiserror::Error;
use uuid::Uuid;

const MAX_TOKENS: usize = 64;
const MAX_TEXT_BYTES: usize = 512 * 1024;
type FlagList = Vec<(String, String)>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhxCommandV1 {
    AppStatus,
    WorkspaceList,
    NoteStat {
        entry_id: Option<u64>,
    },
    NoteRead {
        entry_id: Option<u64>,
        from: usize,
        to: Option<usize>,
    },
    BlockList {
        entry_id: Option<u64>,
        from: usize,
        limit: usize,
    },
    BlockInsert {
        entry_id: Option<u64>,
        after: Option<Uuid>,
        text: String,
        expected_document_revision: u64,
        idempotency_key: String,
    },
    EventsAfter {
        sequence: u64,
    },
}

impl PhxCommandV1 {
    pub fn canonical(&self) -> String {
        match self {
            Self::AppStatus => "phx app status".into(),
            Self::WorkspaceList => "phx workspace ls".into(),
            Self::NoteStat { entry_id } => format!("phx note stat{}", note_suffix(*entry_id)),
            Self::NoteRead { entry_id, from, to } => format!(
                "phx note cat{} --from {from}{}",
                note_suffix(*entry_id),
                to.map_or_else(String::new, |value| format!(" --to {value}"))
            ),
            Self::BlockList {
                entry_id,
                from,
                limit,
            } => format!(
                "phx block ls{} --from {from} --limit {limit}",
                note_suffix(*entry_id)
            ),
            Self::BlockInsert {
                entry_id,
                after,
                text,
                expected_document_revision,
                idempotency_key,
            } => format!(
                "phx block insert{}{} --text-blake3 {} --expected-document-rev {expected_document_revision} --idempotency-key {idempotency_key}",
                note_suffix(*entry_id),
                after.map_or_else(String::new, |value| format!(" --after {value}")),
                blake3::hash(text.as_bytes()).to_hex(),
            ),
            Self::EventsAfter { sequence } => format!("phx events after {sequence}"),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ParseError {
    #[error("phx command is empty")]
    Empty,
    #[error("phx commands must start with 'phx'")]
    Prefix,
    #[error("shell operator '{0}' is forbidden")]
    ShellOperator(char),
    #[error("unterminated quoted argument")]
    UnterminatedQuote,
    #[error("too many command arguments")]
    TooManyTokens,
    #[error("unsupported phx command: {0}")]
    Unsupported(String),
    #[error("missing required argument: {0}")]
    Missing(&'static str),
    #[error("invalid argument: {0}")]
    Invalid(String),
}

pub fn parse_command(raw: &str) -> Result<PhxCommandV1, ParseError> {
    let tokens = tokenize(raw)?;
    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }
    if !tokens[0].eq_ignore_ascii_case("phx") {
        return Err(ParseError::Prefix);
    }
    let domain = tokens.get(1).map(String::as_str).unwrap_or("");
    let verb = tokens.get(2).map(String::as_str).unwrap_or("");
    let args = &tokens[3..];
    match (domain, verb) {
        ("app", "status") if args.is_empty() => Ok(PhxCommandV1::AppStatus),
        ("workspace", "ls") if args.is_empty() => Ok(PhxCommandV1::WorkspaceList),
        ("note", "stat") => Ok(PhxCommandV1::NoteStat {
            entry_id: optional_entry(args)?,
        }),
        ("note", "cat") => parse_note_read(args),
        ("block", "ls") => parse_block_list(args),
        ("block", "insert") => parse_block_insert(args),
        ("events", "after") if args.len() == 1 => Ok(PhxCommandV1::EventsAfter {
            sequence: parse_u64(&args[0], "event sequence")?,
        }),
        _ => Err(ParseError::Unsupported(
            tokens.iter().take(3).cloned().collect::<Vec<_>>().join(" "),
        )),
    }
}

fn parse_block_list(args: &[String]) -> Result<PhxCommandV1, ParseError> {
    let (entry_id, flags) = split_entry_and_flags(args)?;
    validate_flags(&flags, &["from", "limit"])?;
    let from = optional_usize_flag(&flags, "from")?.unwrap_or(0);
    let limit = optional_usize_flag(&flags, "limit")?.unwrap_or(128);
    if limit == 0 || limit > 256 {
        return Err(ParseError::Invalid(
            "--limit must be between 1 and 256".into(),
        ));
    }
    Ok(PhxCommandV1::BlockList {
        entry_id,
        from,
        limit,
    })
}

fn parse_note_read(args: &[String]) -> Result<PhxCommandV1, ParseError> {
    let (entry_id, flags) = split_entry_and_flags(args)?;
    validate_flags(&flags, &["from", "to"])?;
    let from = optional_usize_flag(&flags, "from")?.unwrap_or(0);
    let to = optional_usize_flag(&flags, "to")?;
    if to.is_some_and(|value| value < from) {
        return Err(ParseError::Invalid("--to precedes --from".into()));
    }
    Ok(PhxCommandV1::NoteRead { entry_id, from, to })
}

fn parse_block_insert(args: &[String]) -> Result<PhxCommandV1, ParseError> {
    let (entry_id, flags) = split_entry_and_flags(args)?;
    validate_flags(
        &flags,
        &["text", "expected-document-rev", "idempotency-key", "after"],
    )?;
    let text = required_flag(&flags, "text")?.to_string();
    if text.is_empty() || text.len() > MAX_TEXT_BYTES {
        return Err(ParseError::Invalid("--text is empty or oversized".into()));
    }
    let expected_document_revision = parse_u64(
        required_flag(&flags, "expected-document-rev")?,
        "expected document revision",
    )?;
    let idempotency_key = required_flag(&flags, "idempotency-key")?.to_string();
    if idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ParseError::Invalid("invalid idempotency key".into()));
    }
    let after = flags
        .iter()
        .find(|(name, _)| name == "after")
        .map(|(_, value)| {
            Uuid::parse_str(value).map_err(|_| ParseError::Invalid("invalid --after UUID".into()))
        })
        .transpose()?;
    Ok(PhxCommandV1::BlockInsert {
        entry_id,
        after,
        text,
        expected_document_revision,
        idempotency_key,
    })
}

fn validate_flags(flags: &[(String, String)], allowed: &[&str]) -> Result<(), ParseError> {
    for (index, (name, _)) in flags.iter().enumerate() {
        if !allowed.contains(&name.as_str()) {
            return Err(ParseError::Invalid(format!("unknown --{name}")));
        }
        if flags[..index].iter().any(|(prior, _)| prior == name) {
            return Err(ParseError::Invalid(format!("duplicate --{name}")));
        }
    }
    Ok(())
}

fn optional_entry(args: &[String]) -> Result<Option<u64>, ParseError> {
    if args.len() > 1 || args.first().is_some_and(|value| value.starts_with("--")) {
        return Err(ParseError::Invalid("unexpected note arguments".into()));
    }
    args.first().map(|value| parse_entry(value)).transpose()
}

fn split_entry_and_flags(args: &[String]) -> Result<(Option<u64>, FlagList), ParseError> {
    let mut index = 0;
    let entry_id = if args.first().is_some_and(|value| !value.starts_with("--")) {
        index = 1;
        Some(parse_entry(&args[0])?)
    } else {
        None
    };
    let mut flags = Vec::new();
    while index < args.len() {
        let name = args[index]
            .strip_prefix("--")
            .ok_or_else(|| ParseError::Invalid(format!("unexpected argument {}", args[index])))?;
        let value = args
            .get(index + 1)
            .ok_or(ParseError::Missing("flag value"))?;
        if value.starts_with("--") {
            return Err(ParseError::Missing("flag value"));
        }
        flags.push((name.to_string(), value.clone()));
        index += 2;
    }
    Ok((entry_id, flags))
}

fn required_flag<'a>(
    flags: &'a [(String, String)],
    name: &'static str,
) -> Result<&'a str, ParseError> {
    flags
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.as_str())
        .ok_or(ParseError::Missing(name))
}

fn optional_usize_flag(
    flags: &[(String, String)],
    name: &'static str,
) -> Result<Option<usize>, ParseError> {
    flags
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .map_err(|_| ParseError::Invalid(format!("invalid --{name}")))
        })
        .transpose()
}

fn parse_entry(value: &str) -> Result<u64, ParseError> {
    let value = value.strip_prefix("note://").unwrap_or(value);
    parse_u64(value.trim_matches('/'), "note identity")
}

fn parse_u64(value: &str, label: &str) -> Result<u64, ParseError> {
    value
        .parse::<u64>()
        .map_err(|_| ParseError::Invalid(format!("invalid {label}")))
}

fn note_suffix(entry_id: Option<u64>) -> String {
    entry_id.map_or_else(String::new, |value| format!(" note://{value}"))
}

fn tokenize(raw: &str) -> Result<Vec<String>, ParseError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in raw.trim().chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if quote.is_some() && ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else if matches!(ch, '|' | ';' | '<' | '>') {
            return Err(ParseError::ShellOperator(ch));
        } else {
            current.push(ch);
        }
        if tokens.len() > MAX_TOKENS {
            return Err(ParseError::TooManyTokens);
        }
    }
    if quote.is_some() {
        return Err(ParseError::UnterminatedQuote);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.len() > MAX_TOKENS {
        return Err(ParseError::TooManyTokens);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_rejects_shell_operators() {
        assert_eq!(
            parse_command("phx note cat note://7 | powershell"),
            Err(ParseError::ShellOperator('|'))
        );
    }

    #[test]
    fn parser_builds_revision_checked_insert() {
        let command = parse_command(
            "phx block insert note://7 --text 'hello world' --expected-document-rev 9 --idempotency-key turn-1",
        )
        .expect("parse insert");
        assert!(matches!(
            command,
            PhxCommandV1::BlockInsert {
                entry_id: Some(7),
                expected_document_revision: 9,
                ref text,
                ..
            } if text == "hello world"
        ));
    }

    #[test]
    fn parser_rejects_unknown_and_duplicate_flags() {
        assert!(matches!(
            parse_command("phx note cat --wat 1"),
            Err(ParseError::Invalid(_))
        ));
        assert!(matches!(
            parse_command("phx note cat --from 1 --from 2"),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn block_listing_is_bounded_and_pageable() {
        assert_eq!(
            parse_command("phx block ls note://7 --from 256 --limit 64"),
            Ok(PhxCommandV1::BlockList {
                entry_id: Some(7),
                from: 256,
                limit: 64,
            })
        );
        assert!(matches!(
            parse_command("phx block ls --limit 257"),
            Err(ParseError::Invalid(_))
        ));
    }
}
