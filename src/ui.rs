//! Terminal UI helpers — styled status lines, spinners, and progress bars.
//!
//! Everything writes to **stderr** so `comind serve`'s stdout stays pure JSON-RPC. Styling and
//! animation degrade automatically when stderr is not a TTY (pipes, CI), keeping logs clean.

use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Bold section header.
pub fn header(title: &str) {
    eprintln!("\n{}", style(title).bold().cyan());
}

/// Success line (`✓`).
pub fn ok(msg: &str) {
    eprintln!("{} {msg}", style("✓").green().bold());
}

/// Warning line (`!`).
pub fn warn(msg: &str) {
    eprintln!("{} {msg}", style("!").yellow().bold());
}

/// Error line (`✗`).
pub fn err(msg: &str) {
    eprintln!("{} {msg}", style("✗").red().bold());
}

/// In-progress step (`→`).
pub fn step(msg: &str) {
    eprintln!("{} {msg}", style("→").cyan().bold());
}

/// Dim, indented detail line.
pub fn note(msg: &str) {
    eprintln!("  {}", style(msg).dim());
}

/// Dim `label:` followed by a value.
pub fn field(label: &str, value: &str) {
    eprintln!("  {} {value}", style(format!("{label}:")).dim());
}

/// Indeterminate spinner on stderr with a steady tick.
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed:.dim}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

/// A preset unicode table that wraps to the terminal width. Add rows, then print it.
pub fn table(headers: &[&str]) -> comfy_table::Table {
    let mut t = comfy_table::Table::new();
    t.load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_header(headers.to_vec());
    t
}

/// Determinate `pos/len` progress bar on stderr.
pub fn progress(len: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::with_draw_target(Some(len), ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} [{elapsed_precise}] [{bar:32.cyan/blue}] {pos}/{len} {msg} {eta:.dim}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}
