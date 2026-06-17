//! A line sink that works both for the interactive shell (where output
//! must not corrupt the prompt) and for script mode (plain stdout).

use rustyline::ExternalPrinter;

/// Something that can print a standalone line without disrupting the REPL.
pub trait LinePrinter: Send {
    fn print_line(&mut self, line: String);
}

/// Wraps a rustyline [`ExternalPrinter`] so background output (EMCY) is
/// coordinated with the active prompt.
pub struct ReplPrinter<P: ExternalPrinter + Send>(pub P);

impl<P: ExternalPrinter + Send> LinePrinter for ReplPrinter<P> {
    fn print_line(&mut self, line: String) {
        let _ = self.0.print(line);
    }
}

/// Plain stdout printer for script / non-interactive mode.
pub struct StdoutPrinter;

impl LinePrinter for StdoutPrinter {
    fn print_line(&mut self, line: String) {
        println!("{line}");
    }
}
