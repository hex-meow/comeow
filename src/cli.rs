//! Command-line argument parsing.

use std::path::PathBuf;

use clap::Parser;

/// Human-friendly CANopen SDO client for debugging.
///
/// Acts as a pure SDO client (no full CANopen node, no own node-id) and
/// passively prints EMCY messages from any node on the bus.
#[derive(Parser, Debug)]
#[command(name = "comeow", version, about)]
pub struct Cli {
    /// SocketCAN interface to use, e.g. `can0` (Linux, the default backend).
    /// Defaults to `can0` when omitted; ignored if `--gs-usb` is given.
    #[arg(value_name = "IFACE")]
    pub iface: Option<String>,

    /// Use a gs_usb / candleLight USB adapter (cross-platform) instead of
    /// SocketCAN. Requires the `gs_usb` feature.
    #[arg(long)]
    pub gs_usb: bool,

    /// gs_usb: enable CAN-FD (1M/5M preset) instead of classic 1M.
    #[arg(long, requires = "gs_usb")]
    pub fd: bool,

    /// gs_usb: channel index on a multi-channel adapter.
    #[arg(long, default_value_t = 0, requires = "gs_usb")]
    pub channel: u16,

    /// Default node id used when a command omits it (1..127, hex or dec).
    #[arg(long, value_parser = parse_node)]
    pub node: Option<u8>,

    /// Per-operation SDO timeout, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    pub timeout: u64,

    /// Retry count for each SDO operation (on timeout / I/O error).
    #[arg(long, default_value_t = 1)]
    pub retries: u8,

    /// Run commands from a script file instead of the interactive shell.
    #[arg(short = 'f', long, value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// Script mode: abort on the first error (default: keep going).
    #[arg(long, requires = "file")]
    pub stop_on_error: bool,

    /// Disable the background EMCY listener.
    #[arg(long)]
    pub no_emcy: bool,

    /// Disable live red/green input highlighting. Workaround for a possible
    /// wide-character (CJK) rendering glitch in some terminals.
    #[arg(long)]
    pub no_highlight: bool,
}

/// Parse a node id given as decimal or `0x..` hex, validated to 1..=127.
fn parse_node(s: &str) -> Result<u8, String> {
    let v = crate::command::parse_int_u32(s).map_err(|e| e.to_string())?;
    if (1..=127).contains(&v) {
        Ok(v as u8)
    } else {
        Err(format!("node id must be in 1..=127, got {v}"))
    }
}
