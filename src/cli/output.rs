//! Shared CLI output helpers.
//!
//! Today this is just `print_json` for the handful of `--json`-style
//! commands. Centralizing it means the JSON pretty-print configuration
//! lives in one place and the call sites read as one line instead of
//! `println!("{}", serde_json::to_string_pretty(&x)?)` boilerplate.

use anyhow::Result;
use serde::Serialize;

/// Write a value as pretty-printed JSON to stdout, followed by a newline.
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
