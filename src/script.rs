//! Batch mode: run read/write/sleep commands from a file, sequentially.

use std::path::Path;
use std::sync::Arc;

use can_transport::CanBus;

use crate::command::{parse_line, Command};
use crate::executor::{execute, ExecState};

/// Execute a script file. Returns `Ok(true)` if any command errored.
pub async fn run_script(
    bus: Arc<dyn CanBus>,
    path: &Path,
    mut state: ExecState,
    stop_on_error: bool,
) -> anyhow::Result<bool> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read script `{}`: {e}", path.display()))?;

    let mut had_error = false;
    for (lineno, raw) in text.lines().enumerate() {
        let cmd = match parse_line(raw) {
            Ok(None) => continue,
            Ok(Some(Command::Quit)) => break,
            Ok(Some(cmd)) => cmd,
            Err(e) => {
                eprintln!("line {}: error: {e}", lineno + 1);
                had_error = true;
                if stop_on_error {
                    anyhow::bail!("stopped at line {} (parse error)", lineno + 1);
                }
                continue;
            }
        };

        println!("> {}", raw.trim());
        let result = execute(&*bus, cmd, &mut state).await;
        println!("  {result}");
        if result.is_error() {
            had_error = true;
            if stop_on_error {
                anyhow::bail!("stopped at line {} due to error", lineno + 1);
            }
        }
    }
    Ok(had_error)
}
