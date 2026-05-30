//! Error display utilities for the rebake CLI.

use colored::Colorize;

/// Print a user-friendly error message to stderr.
///
/// Format:
/// ```text
/// Error: <main message>
///   Caused by: <source error 1>
///   Caused by: <source error 2>
/// ```
pub fn print_error(error: &anyhow::Error) {
    eprintln!("{}: {}", "Error".red().bold(), error);

    for cause in error.chain().skip(1) {
        eprintln!("  {}: {}", "Caused by".yellow(), cause);
    }
}

/// Print an indented error message for batch processing.
pub fn print_error_indented(error: &anyhow::Error) {
    eprintln!("  -> {}: {}", "Error".red().bold(), error);

    for cause in error.chain().skip(1) {
        eprintln!("     {}: {}", "Caused by".yellow(), cause);
    }
}
