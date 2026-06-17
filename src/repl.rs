//! Interactive shell: rustyline runs on a blocking thread and bridges to
//! the async executor over channels, so the tokio reactor (and the EMCY
//! listener) keeps running while the user sits at the prompt.

use std::borrow::Cow;
use std::path::PathBuf;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::FileHistory;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};
use tokio::sync::{mpsc, oneshot};

use crate::command::{classify, parse_line, Command, LineStatus};
use crate::executor::Job;
use crate::value::TYPE_TOKENS;

const COMMANDS: &[&str] = &["read", "write", "set", "sleep", "help", "quit", "exit"];
const VERBS: &[&str] = &["read", "write"];
const SET_KEYS: &[&str] = &["node", "timeout", "retries"];

/// A handful of well-known object-dictionary entries, offered (with their
/// names) when completing an `<index>` slot.
const COMMON_OBJECTS: &[(&str, &str)] = &[
    ("0x1000", "device type"),
    ("0x1001", "error register"),
    ("0x1008", "device name"),
    ("0x1017", "heartbeat producer time"),
    ("0x1018", "identity"),
    ("0x6040", "controlword"),
    ("0x6041", "statusword"),
    ("0x6060", "modes of operation"),
    ("0x6061", "modes of operation display"),
    ("0x6064", "position actual value"),
    ("0x607A", "target position"),
];

/// A completion candidate: the text to insert, plus an optional label
/// shown in the candidate list (e.g. the object name).
struct Cand {
    replacement: &'static str,
    label: Option<&'static str>,
}

impl Cand {
    fn plain(s: &'static str) -> Self {
        Cand {
            replacement: s,
            label: None,
        }
    }
}

pub struct CmdHelper {
    highlight: bool,
}

impl Hinter for CmdHelper {
    type Hint = String;
}
impl Validator for CmdHelper {}
impl Helper for CmdHelper {}

/// Live validation (fish-style), updated on every keystroke:
///   • green  — a complete, valid command
///   • red    — a token is wrong (e.g. `r 0x1017 o`, bad value)
///   • normal — blank, or a valid prefix still being typed
///
/// Disabled by `--no-highlight`, which also restores rustyline's optimised
/// single-keystroke insert path (a possible fix for wide-char glitches).
impl Highlighter for CmdHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.highlight {
            return Cow::Borrowed(line);
        }
        match classify(line) {
            LineStatus::Valid => Cow::Owned(format!("\x1b[32m{line}\x1b[0m")),
            LineStatus::Invalid => Cow::Owned(format!("\x1b[31m{line}\x1b[0m")),
            LineStatus::Empty | LineStatus::Incomplete => Cow::Borrowed(line),
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        self.highlight // re-evaluate the whole line on every keystroke
    }
}

impl Completer for CmdHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let before = &line[..pos];
        // Start of the word currently under the cursor (empty if we're at
        // the beginning of a fresh token after whitespace).
        let start = before
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = before[start..].to_ascii_lowercase();
        // Fully-typed tokens before the one we're completing.
        let prior: Vec<&str> = before[..start].split_whitespace().collect();

        let candidates = slot_candidates(&prior);
        let pairs = candidates
            .into_iter()
            .filter(|c| c.replacement.to_ascii_lowercase().starts_with(&prefix))
            .map(|c| Pair {
                display: match c.label {
                    Some(name) => format!("{}  ({name})", c.replacement),
                    None => c.replacement.to_string(),
                },
                replacement: c.replacement.to_string(),
            })
            .collect();
        Ok((start, pairs))
    }
}

/// Decide which candidates fit the slot at `prior.len()` given the tokens
/// already typed before the cursor.
fn slot_candidates(prior: &[&str]) -> Vec<Cand> {
    let idx = prior.len();
    if idx == 0 {
        return COMMANDS.iter().map(|s| Cand::plain(s)).collect();
    }

    match prior[0].to_ascii_lowercase().as_str() {
        "set" => {
            return if idx == 1 {
                SET_KEYS.iter().map(|s| Cand::plain(s)).collect()
            } else {
                vec![]
            };
        }
        "help" | "?" => {
            return if idx == 1 {
                vec![Cand::plain("datatype")]
            } else {
                vec![]
            };
        }
        "sleep" | "quit" | "exit" | "q" => return vec![],
        _ => {}
    }

    // read/write, with the verb either first or right after an explicit node.
    let verb_idx = if is_verb(prior[0]) {
        Some(0)
    } else if prior.len() > 1 && is_verb(prior[1]) {
        Some(1)
    } else {
        None
    };

    match verb_idx {
        Some(v) => match idx - v - 1 {
            0 => COMMON_OBJECTS
                .iter()
                .map(|(idx, name)| Cand {
                    replacement: idx,
                    label: Some(name),
                })
                .collect(),
            1 => vec![Cand::plain("0")], // subindex; 0 is the common case
            2 => TYPE_TOKENS.iter().map(|s| Cand::plain(s)).collect(),
            _ => vec![], // write value: nothing to suggest
        },
        // First token is a (numeric) node id; complete the verb next.
        None if idx == 1 && is_number(prior[0]) => {
            VERBS.iter().map(|s| Cand::plain(s)).collect()
        }
        None => vec![],
    }
}

fn is_verb(tok: &str) -> bool {
    matches!(tok.to_ascii_lowercase().as_str(), "read" | "r" | "write" | "w")
}

fn is_number(tok: &str) -> bool {
    crate::command::parse_int_u32(tok).is_ok()
}

pub type CmdEditor = Editor<CmdHelper, FileHistory>;

pub fn build_editor(highlight: bool) -> anyhow::Result<CmdEditor> {
    let config = Config::builder().auto_add_history(true).build();
    let mut editor: CmdEditor = Editor::with_config(config)?;
    editor.set_helper(Some(CmdHelper { highlight }));
    Ok(editor)
}

pub fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".comeow_history"))
}

/// Run the read-eval loop. Blocks until the user quits (Ctrl-D / `quit`).
pub fn run_loop(
    mut editor: CmdEditor,
    cmd_tx: mpsc::Sender<Job>,
    history: Option<PathBuf>,
) -> anyhow::Result<()> {
    if let Some(path) = &history {
        let _ = editor.load_history(path);
    }

    loop {
        match editor.readline("canopen> ") {
            Ok(line) => match parse_line(&line) {
                Ok(None) => continue,
                Ok(Some(Command::Quit)) => break,
                Ok(Some(cmd)) => {
                    if !dispatch(&cmd_tx, cmd) {
                        break; // executor gone
                    }
                }
                Err(e) => println!("error: {e}"),
            },
            Err(ReadlineError::Interrupted) => continue, // Ctrl-C clears the line
            Err(ReadlineError::Eof) => break,            // Ctrl-D quits
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        }
    }

    if let Some(path) = &history {
        let _ = editor.save_history(path);
    }
    Ok(())
}

/// Send a command to the executor and block for its result. Returns false
/// if the executor channel is dead (caller should exit).
fn dispatch(cmd_tx: &mpsc::Sender<Job>, cmd: Command) -> bool {
    let (resp_tx, resp_rx) = oneshot::channel();
    if cmd_tx.blocking_send((cmd, resp_tx)).is_err() {
        eprintln!("executor unavailable");
        return false;
    }
    match resp_rx.blocking_recv() {
        Ok(result) => {
            println!("{result}");
            true
        }
        Err(_) => {
            eprintln!("executor dropped the request");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repls(prior: &[&str]) -> Vec<&'static str> {
        slot_candidates(prior).into_iter().map(|c| c.replacement).collect()
    }

    #[test]
    fn first_token_is_commands() {
        assert_eq!(repls(&[]), COMMANDS);
    }

    #[test]
    fn after_set_offers_keys() {
        assert_eq!(repls(&["set"]), SET_KEYS);
        assert!(repls(&["set", "node"]).is_empty());
    }

    #[test]
    fn read_slots() {
        // verb typed -> index slot offers objects
        assert!(repls(&["r"]).contains(&"0x1018"));
        // index typed -> subindex slot
        assert_eq!(repls(&["r", "0x1018"]), vec!["0"]);
        // sub typed -> datatype slot
        assert_eq!(repls(&["r", "0x1018", "0"]), TYPE_TOKENS);
    }

    #[test]
    fn explicit_node_then_verb_then_slots() {
        // node typed -> verb slot
        assert_eq!(repls(&["5"]), VERBS);
        // node + verb -> index slot
        assert!(repls(&["5", "w"]).contains(&"0x6040"));
        // node + verb + index + sub -> datatype slot
        assert_eq!(repls(&["5", "w", "0x6040", "0"]), TYPE_TOKENS);
        // write value slot -> nothing
        assert!(repls(&["5", "w", "0x6040", "0", "u16"]).is_empty());
    }

    #[test]
    fn sleep_and_quit_have_no_completions() {
        assert!(repls(&["sleep"]).is_empty());
        assert!(repls(&["quit"]).is_empty());
    }

}
