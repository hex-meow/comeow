//! A human-friendly CANopen SDO client for debugging.
//!
//! Pure SDO client (no full CANopen node, no own node-id) with an
//! interactive shell, a script mode, and a live EMCY listener.

mod cli;
mod command;
mod emcy;
mod executor;
mod output;
mod printer;
mod repl;
mod script;
mod transport;
mod value;

use std::time::Duration;

use clap::Parser;
use tokio::sync::mpsc;

use crate::cli::Cli;
use crate::executor::{ExecState, Job};
use crate::printer::{ReplPrinter, StdoutPrinter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let cli = Cli::parse();
    let bus = transport::open_bus(&cli).await?;
    let caps = bus.capabilities();

    let state = ExecState {
        default_node: cli.node,
        timeout: Duration::from_millis(cli.timeout),
        retries: cli.retries,
    };

    // ----- script (batch) mode -----
    if let Some(file) = cli.file.clone() {
        if !cli.no_emcy {
            emcy::spawn(bus.clone(), Box::new(StdoutPrinter));
        }
        let had_error = script::run_script(bus.clone(), &file, state, cli.stop_on_error).await?;
        if had_error {
            std::process::exit(1);
        }
        return Ok(());
    }

    // ----- interactive mode -----
    let mut editor = repl::build_editor(!cli.no_highlight)?;
    if !cli.no_emcy {
        let printer = editor.create_external_printer()?;
        emcy::spawn(bus.clone(), Box::new(ReplPrinter(printer)));
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<Job>(32);
    tokio::spawn(executor::run_executor(bus.clone(), cmd_rx, state));

    println!(
        "comeow — CANopen SDO client (fd={}, max {} bytes/frame). Type `help`, `quit` to exit.",
        caps.fd, caps.max_dlen
    );
    if cli.no_emcy {
        println!("(EMCY listener disabled)");
    }

    let history = repl::history_path();
    let handle = tokio::task::spawn_blocking(move || repl::run_loop(editor, cmd_tx, history));
    handle.await??;
    Ok(())
}
