//! Small terminal-UI helpers: consistent spinners for long-running steps.
//!
//! The spinner ticks on its own thread, so it keeps animating even while the
//! main thread is blocked waiting on a child process (e.g. `docker compose`).
//! Coloring is TTY-aware (via `console`), so CI logs stay clean, and the final
//! status line is always printed — spinner or not.

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

const TICKS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Start an animated spinner with the given message.
pub fn spinner(message: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .expect("valid spinner template")
            .tick_strings(TICKS),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(message.into());
    pb
}

/// Clear the spinner and print a green success line.
pub fn finish_ok(pb: &ProgressBar, message: impl Into<String>) {
    pb.finish_and_clear();
    println!("  {} {}", style("✓").green().bold(), message.into());
}

/// Clear the spinner and print a red failure line.
pub fn finish_err(pb: &ProgressBar, message: impl Into<String>) {
    pb.finish_and_clear();
    println!("  {} {}", style("✗").red().bold(), message.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_then_finish_ok() {
        let pb = spinner("working…");
        finish_ok(&pb, "done");
    }

    #[test]
    fn spinner_then_finish_err() {
        let pb = spinner("working…");
        finish_err(&pb, "failed");
    }
}
