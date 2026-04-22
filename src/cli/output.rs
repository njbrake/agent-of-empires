//! Shared CLI output helpers.
//!
//! Today this is just `print_json` for the handful of `--json`-style
//! commands. Centralizing it means the JSON pretty-print configuration
//! lives in one place and the call sites read as one line instead of
//! `println!("{}", serde_json::to_string_pretty(&x)?)` boilerplate.

use std::io::Write;

use anyhow::Result;
use serde::Serialize;

/// Write a value as pretty-printed JSON to stdout, followed by a newline.
/// Streams directly to a locked stdout writer so we don't materialize the
/// entire JSON payload as an intermediate `String` for large outputs
/// (`aoe list --json` against a workspace with many sessions).
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}
