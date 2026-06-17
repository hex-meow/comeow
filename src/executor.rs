//! The async side: drives the SDO crate and owns mutable session state.

use std::sync::Arc;
use std::time::Duration;

use can_transport::CanBus;
use canopen_sdo::asynch::{download_bytes_retry, upload_bytes_retry};
use tokio::sync::{mpsc, oneshot};

use crate::command::Command;
use crate::output::CmdResult;
use crate::value::{self, DataType, Radix};

/// Mutable session state, owned solely by the executor task (no locks).
pub struct ExecState {
    pub default_node: Option<u8>,
    pub timeout: Duration,
    pub retries: u8,
}

impl ExecState {
    pub fn timeout_opt(&self) -> Option<Duration> {
        Some(self.timeout)
    }
}

/// A unit of work sent from the REPL/script to the executor.
pub type Job = (Command, oneshot::Sender<CmdResult>);

/// Executor task: serialize commands against the bus, reply on the oneshot.
pub async fn run_executor(bus: Arc<dyn CanBus>, mut rx: mpsc::Receiver<Job>, mut state: ExecState) {
    while let Some((cmd, resp)) = rx.recv().await {
        let result = execute(&*bus, cmd, &mut state).await;
        // Receiver may be gone if the caller timed out / quit; ignore.
        let _ = resp.send(result);
    }
}

/// Execute one command against the bus.
pub async fn execute(bus: &dyn CanBus, cmd: Command, state: &mut ExecState) -> CmdResult {
    match cmd {
        Command::Read { node, index, sub, ty } => {
            let node = match resolve_node(node, state) {
                Ok(n) => n,
                Err(e) => return CmdResult::Error(e),
            };
            match upload_bytes_retry(bus, node, index, sub, state.timeout_opt(), state.retries).await
            {
                Ok(bytes) => {
                    let rendered = match ty {
                        Some((dt, radix)) => format_typed(dt, radix, &bytes),
                        None => value::format_raw(&bytes),
                    };
                    CmdResult::Value(format!("0x{index:04X}:{sub:02X} = {rendered}"))
                }
                Err(e) => CmdResult::from_async_err(e),
            }
        }
        Command::Write { node, index, sub, ty, value } => {
            let node = match resolve_node(node, state) {
                Ok(n) => n,
                Err(e) => return CmdResult::Error(e),
            };
            let data = match value::encode(ty, &value) {
                Ok(d) => d,
                Err(e) => return CmdResult::Error(e.to_string()),
            };
            match download_bytes_retry(bus, node, index, sub, &data, state.timeout_opt(), state.retries)
                .await
            {
                Ok(()) => CmdResult::Ok,
                Err(e) => CmdResult::from_async_err(e),
            }
        }
        Command::Sleep(d) => {
            tokio::time::sleep(d).await;
            CmdResult::Ok
        }
        Command::SetNode(n) => {
            state.default_node = Some(n);
            CmdResult::Info(format!("default node = {n} (0x{n:02X})"))
        }
        Command::SetTimeout(d) => {
            state.timeout = d;
            CmdResult::Info(format!("timeout = {} ms", d.as_millis()))
        }
        Command::SetRetries(n) => {
            state.retries = n;
            CmdResult::Info(format!("retries = {n}"))
        }
        Command::Help(topic) => CmdResult::Info(help_text(topic.as_deref())),
        Command::Quit => CmdResult::Ok, // handled by the REPL loop, not here
    }
}

fn resolve_node(node: Option<u8>, state: &ExecState) -> Result<u8, String> {
    node.or(state.default_node)
        .ok_or_else(|| "no node given and no default set; use `set node <id>`".to_string())
}

fn format_typed(dt: DataType, radix: Radix, bytes: &[u8]) -> String {
    value::format(dt, radix, bytes)
}

pub fn help_text(topic: Option<&str>) -> String {
    match topic {
        Some("datatype") | Some("datatypes") => "\
datatypes: b  u8 u16 u32 u64  i8 i16 i32 i64  x8 x16 x32 x64 (hex display)
           r32 r64 (float)  vs (visible string)  hex (raw bytes)"
            .into(),
        _ => "\
commands:
  [<node>] r[ead]  <index> <sub> [<datatype>]   read an object
  [<node>] w[rite] <index> <sub> <datatype> <v> write an object
  set node <id>            set the default node (omit <node> afterwards)
  set timeout <ms>         set the per-op SDO timeout
  set retries <n>          set the retry count
  sleep <ms>               pause (handy in scripts)
  help [datatype]          show this help
  quit | exit | q          leave
indexes/subindexes accept 0x.. hex or decimal. EMCY frames print live.
  example:  set node 0x10  then  r 0x1018 1 u32  or  w 0x6040 0 u16 0x000F"
            .into(),
    }
}
