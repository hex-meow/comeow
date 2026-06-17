//! Command grammar shared by the interactive shell and script mode.
//!
//! Mirrors the CiA 309-3 ASCII syntax the user already knows, minus the
//! leading `[seq]` token (only meaningful for the 309 socket protocol):
//!
//! ```text
//!   [<node>] r[ead]  <index> <subindex> [<datatype>]
//!   [<node>] w[rite] <index> <subindex> <datatype> <value...>
//!   set node <id> | set timeout <ms> | set retries <n>
//!   sleep <ms>
//!   help [topic] | quit | exit | q
//! ```

use std::fmt;
use std::num::ParseIntError;
use std::time::Duration;

use crate::value::{self, DataType, Radix};

const CMD_KEYWORDS: &[&str] = &["read", "write", "set", "sleep", "help", "quit", "exit"];
const VERB_KEYWORDS: &[&str] = &["read", "write"];
const SET_KEYS: &[&str] = &["node", "timeout", "retries"];

/// Why a line failed to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseStatus {
    /// The line so far is a valid prefix of a command — the user just
    /// hasn't typed enough yet (a token or trailing argument is missing).
    Incomplete,
    /// A token that is present is wrong and can't become valid.
    Invalid,
}

/// A parse failure, carrying both a human message and whether the line is
/// merely incomplete or actually wrong.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub status: ParseStatus,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

fn incomplete(msg: impl Into<String>) -> ParseError {
    ParseError {
        status: ParseStatus::Incomplete,
        msg: msg.into(),
    }
}

fn invalid(msg: impl Into<String>) -> ParseError {
    ParseError {
        status: ParseStatus::Invalid,
        msg: msg.into(),
    }
}

fn is_prefix_of(tok: &str, set: &[&str]) -> bool {
    let t = tok.to_ascii_lowercase();
    !t.is_empty() && set.iter().any(|k| k.starts_with(&t))
}

/// Overall status of a line, for live (fish-style) input colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStatus {
    /// Blank line or comment.
    Empty,
    /// A complete, executable command.
    Valid,
    /// A valid prefix; the user is still typing.
    Incomplete,
    /// Contains a token that is wrong.
    Invalid,
}

/// Classify a line for input highlighting. This is `parse_line` plus, for
/// writes, a check that the value actually encodes for its datatype.
pub fn classify(line: &str) -> LineStatus {
    match parse_line(line) {
        Ok(None) => LineStatus::Empty,
        Ok(Some(Command::Write { ty, ref value, .. })) => {
            if value::encode(ty, value).is_ok() {
                LineStatus::Valid
            } else {
                LineStatus::Invalid
            }
        }
        Ok(Some(_)) => LineStatus::Valid,
        Err(e) => match e.status {
            ParseStatus::Incomplete => LineStatus::Incomplete,
            ParseStatus::Invalid => LineStatus::Invalid,
        },
    }
}

/// A parsed command. Node is `Option` so the executor can fill in the
/// configured default when omitted.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Read {
        node: Option<u8>,
        index: u16,
        sub: u8,
        ty: Option<(DataType, Radix)>,
    },
    Write {
        node: Option<u8>,
        index: u16,
        sub: u8,
        ty: DataType,
        value: String,
    },
    Sleep(Duration),
    SetNode(u8),
    SetTimeout(Duration),
    SetRetries(u8),
    Help(Option<String>),
    Quit,
}

/// Parse one line. Returns `Ok(None)` for blank lines / comments.
pub fn parse_line(line: &str) -> Result<Option<Command>, ParseError> {
    let line = strip_comment(line).trim();
    if line.is_empty() {
        return Ok(None);
    }
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cmd = parse_tokens(&tokens, line)?;
    Ok(Some(cmd))
}

fn parse_tokens(tokens: &[&str], line: &str) -> Result<Command, ParseError> {
    let head = tokens[0].to_ascii_lowercase();
    match head.as_str() {
        "quit" | "exit" | "q" => return Ok(Command::Quit),
        "help" | "?" => return Ok(Command::Help(tokens.get(1).map(|s| s.to_string()))),
        "sleep" => return parse_sleep(tokens),
        "set" => return parse_set(tokens),
        "read" | "r" | "write" | "w" => return parse_rw(None, &head, &tokens[1..], line),
        _ => {}
    }

    // Otherwise the first token must be an explicit node id, followed by a
    // read/write verb.
    let node = match parse_int_u32(tokens[0]) {
        Ok(v) if (1..=127).contains(&v) => v as u8,
        Ok(v) => return Err(invalid(format!("node id must be 1..=127, got {v}"))),
        Err(_) => {
            // Not a number. If it's the only token and a prefix of a command
            // keyword, the user is still typing; otherwise it's wrong.
            return if tokens.len() == 1 && is_prefix_of(&head, CMD_KEYWORDS) {
                Err(incomplete(format!("`{}`… (keep typing a command)", tokens[0])))
            } else {
                Err(invalid(format!("unknown command `{}`", tokens[0])))
            };
        }
    };
    let Some(verb) = tokens.get(1) else {
        return Err(incomplete("expected `read` or `write` after node id"));
    };
    let verb = verb.to_ascii_lowercase();
    match verb.as_str() {
        "read" | "r" | "write" | "w" => parse_rw(Some(node), &verb, &tokens[2..], line),
        _ if tokens.len() == 2 && is_prefix_of(&verb, VERB_KEYWORDS) => {
            Err(incomplete("keep typing `read` or `write`"))
        }
        _ => Err(invalid(format!("expected read/write, got `{verb}`"))),
    }
}

/// Parse a read/write tail. `verb` is the (lower-cased) verb; `rest` starts
/// after it. `line` is the original line (for slicing the write value).
fn parse_rw(node: Option<u8>, verb: &str, rest: &[&str], line: &str) -> Result<Command, ParseError> {
    let is_read = matches!(verb, "read" | "r");

    let Some(idx_tok) = rest.first() else {
        return Err(incomplete("expected <index>"));
    };
    let index = parse_index(idx_tok)?;

    let Some(sub_tok) = rest.get(1) else {
        return Err(incomplete("expected <subindex>"));
    };
    let sub = parse_sub(sub_tok)?;

    if is_read {
        // The datatype is optional, so index+subindex alone is a complete read.
        let ty = match rest.get(2) {
            Some(tok) => Some(parse_type(tok)?),
            None => None,
        };
        Ok(Command::Read { node, index, sub, ty })
    } else {
        let Some(ty_tok) = rest.get(2) else {
            return Err(incomplete("write requires a <datatype>"));
        };
        let (ty, _) = parse_type(ty_tok)?;
        // Everything after the datatype token is the value (may contain
        // spaces for `vs` / `hex`). Recover it from the original line.
        let Some(value) = value_remainder(line, rest[2]) else {
            return Err(incomplete("write requires a <value>"));
        };
        Ok(Command::Write { node, index, sub, ty, value })
    }
}

/// Parse a datatype token; a token that is merely a prefix of a known one
/// (e.g. `u1` for `u16`) is treated as "still typing", not wrong.
fn parse_type(tok: &str) -> Result<(DataType, Radix), ParseError> {
    DataType::parse_token(tok).map_err(|e| {
        if is_prefix_of(tok, value::TYPE_TOKENS) {
            incomplete(format!("`{tok}`… (keep typing a datatype)"))
        } else {
            invalid(e.to_string())
        }
    })
}

/// Slice everything after the datatype token from the original line, so
/// multi-word string / hex values survive whitespace splitting.
fn value_remainder(line: &str, ty_tok: &str) -> Option<String> {
    let pos = line.find(ty_tok)?;
    let after = &line[pos + ty_tok.len()..];
    let v = after.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_sleep(tokens: &[&str]) -> Result<Command, ParseError> {
    let Some(ms) = tokens.get(1) else {
        return Err(incomplete("sleep requires <milliseconds>"));
    };
    let ms = parse_int_u64(ms).map_err(|e| invalid(format!("invalid sleep value: {e}")))?;
    Ok(Command::Sleep(Duration::from_millis(ms)))
}

fn parse_set(tokens: &[&str]) -> Result<Command, ParseError> {
    let Some(key) = tokens.get(1) else {
        return Err(incomplete("set requires a key (node|timeout|retries)"));
    };
    let key = key.to_ascii_lowercase();
    if !SET_KEYS.contains(&key.as_str()) {
        return if tokens.len() == 2 && is_prefix_of(&key, SET_KEYS) {
            Err(incomplete("keep typing `node`, `timeout` or `retries`"))
        } else {
            Err(invalid(format!("unknown set key `{key}` (node|timeout|retries)")))
        };
    }
    let Some(val) = tokens.get(2) else {
        return Err(incomplete("set requires a value"));
    };
    match key.as_str() {
        "node" => Ok(Command::SetNode(parse_node(val)?)),
        "timeout" => {
            let ms = parse_int_u64(val).map_err(|e| invalid(e.to_string()))?;
            Ok(Command::SetTimeout(Duration::from_millis(ms)))
        }
        "retries" => {
            let n = parse_int_u64(val).map_err(|e| invalid(e.to_string()))?;
            if n > u8::MAX as u64 {
                return Err(invalid("retries too large (max 255)"));
            }
            Ok(Command::SetRetries(n as u8))
        }
        _ => unreachable!("set key already validated"),
    }
}

fn parse_node(s: &str) -> Result<u8, ParseError> {
    let v = parse_int_u32(s).map_err(|e| invalid(e.to_string()))?;
    if (1..=127).contains(&v) {
        Ok(v as u8)
    } else {
        Err(invalid(format!("node id must be 1..=127, got {v}")))
    }
}

fn parse_index(s: &str) -> Result<u16, ParseError> {
    let v = parse_int_u32(s).map_err(|e| invalid(e.to_string()))?;
    u16::try_from(v).map_err(|_| invalid(format!("index 0x{v:X} out of range (max 0xFFFF)")))
}

fn parse_sub(s: &str) -> Result<u8, ParseError> {
    let v = parse_int_u32(s).map_err(|e| invalid(e.to_string()))?;
    u8::try_from(v).map_err(|_| invalid(format!("subindex {v} out of range (max 255)")))
}

/// Parse an integer in decimal or with a `0x` / `0b` / `0o` prefix.
pub fn parse_int_u64(s: &str) -> Result<u64, ParseIntError> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u64::from_str_radix(bin, 2)
    } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        u64::from_str_radix(oct, 8)
    } else {
        s.parse::<u64>()
    }
}

pub fn parse_int_u32(s: &str) -> Result<u32, ParseIntError> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u32::from_str_radix(bin, 2)
    } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        u32::from_str_radix(oct, 8)
    } else {
        s.parse::<u32>()
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_and_blank() {
        assert_eq!(parse_line("   ").unwrap(), None);
        assert_eq!(parse_line("# just a comment").unwrap(), None);
    }

    #[test]
    fn read_default_node() {
        let c = parse_line("r 0x1018 1 u32").unwrap().unwrap();
        assert_eq!(
            c,
            Command::Read {
                node: None,
                index: 0x1018,
                sub: 1,
                ty: Some((DataType::U32, Radix::Dec)),
            }
        );
    }

    #[test]
    fn read_explicit_node_no_type() {
        let c = parse_line("16 read 0x6041 0").unwrap().unwrap();
        assert_eq!(
            c,
            Command::Read {
                node: Some(16),
                index: 0x6041,
                sub: 0,
                ty: None,
            }
        );
    }

    #[test]
    fn write_string_with_spaces() {
        let c = parse_line("3 w 0x1010 1 vs hello world").unwrap().unwrap();
        assert_eq!(
            c,
            Command::Write {
                node: Some(3),
                index: 0x1010,
                sub: 1,
                ty: DataType::VisibleString,
                value: "hello world".into(),
            }
        );
    }

    #[test]
    fn write_u16() {
        let c = parse_line("w 0x1017 0 u16 1000").unwrap().unwrap();
        assert_eq!(
            c,
            Command::Write {
                node: None,
                index: 0x1017,
                sub: 0,
                ty: DataType::U16,
                value: "1000".into(),
            }
        );
    }

    #[test]
    fn set_and_sleep() {
        assert_eq!(parse_line("set node 0x10").unwrap().unwrap(), Command::SetNode(16));
        assert_eq!(
            parse_line("sleep 250").unwrap().unwrap(),
            Command::Sleep(Duration::from_millis(250))
        );
    }

    #[test]
    fn quit() {
        assert_eq!(parse_line("quit").unwrap().unwrap(), Command::Quit);
        assert_eq!(parse_line("q").unwrap().unwrap(), Command::Quit);
    }

    #[test]
    fn classify_valid() {
        assert_eq!(classify(""), LineStatus::Empty);
        assert_eq!(classify("  # comment"), LineStatus::Empty);
        assert_eq!(classify("quit"), LineStatus::Valid);
        assert_eq!(classify("1 r 0x1017 0 u16"), LineStatus::Valid);
        assert_eq!(classify("r 0x1017 0"), LineStatus::Valid); // datatype optional
        assert_eq!(classify("w 0x1017 0 u16 1000"), LineStatus::Valid);
        assert_eq!(classify("set node 5"), LineStatus::Valid);
    }

    #[test]
    fn classify_incomplete_while_typing() {
        // partial command keyword
        assert_eq!(classify("re"), LineStatus::Incomplete);
        assert_eq!(classify("se"), LineStatus::Incomplete);
        // node typed, verb not yet
        assert_eq!(classify("5"), LineStatus::Incomplete);
        assert_eq!(classify("5 wr"), LineStatus::Incomplete);
        // missing trailing arguments
        assert_eq!(classify("r 0x1017"), LineStatus::Incomplete);
        assert_eq!(classify("w 0x1017 0"), LineStatus::Incomplete);
        assert_eq!(classify("w 0x1017 0 u16"), LineStatus::Incomplete); // value missing
        // partial datatype token
        assert_eq!(classify("r 0x1017 0 u1"), LineStatus::Incomplete);
        // partial set key
        assert_eq!(classify("set ret"), LineStatus::Incomplete);
    }

    #[test]
    fn classify_invalid_when_wrong() {
        // the user's example: `o` typed where the subindex goes
        assert_eq!(classify("1 r 0x1017 o"), LineStatus::Invalid);
        // wrong datatype that is not a prefix of any
        assert_eq!(classify("r 0x1017 0 zz"), LineStatus::Invalid);
        // bad index / out of range
        assert_eq!(classify("r 0xZZZ 0"), LineStatus::Invalid);
        assert_eq!(classify("r 0x1017 999"), LineStatus::Invalid);
        // write value out of range for the datatype
        assert_eq!(classify("w 0x1017 0 u8 999"), LineStatus::Invalid);
        // wrong first token that isn't a command prefix
        assert_eq!(classify("zzz"), LineStatus::Invalid);
        // node out of range
        assert_eq!(classify("200 r 0x1017 0"), LineStatus::Invalid);
    }
}
