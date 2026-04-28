//! Suspend ratatui's alternate screen / raw mode while a child process
//! runs in the bare terminal, then restore on return. Lets `sudo`'s
//! password prompt and similar interactive shell-outs work normally.

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::stdout;

/// Suspend the TUI, run `f`, then restore. Restoration runs on the way
/// out whether `f` returns Ok, Err, or panics - the `SuspendGuard`
/// drop is unconditional.
pub fn suspend_tui_for<F, R>(f: F) -> Result<R>
where
    F: FnOnce() -> R,
{
    let _guard = SuspendGuard::new()?;
    Ok(f())
}

struct SuspendGuard;

impl SuspendGuard {
    fn new() -> Result<Self> {
        disable_raw_mode().context("disabling raw mode")?;
        execute!(stdout(), LeaveAlternateScreen).context("leaving alternate screen")?;
        Ok(SuspendGuard)
    }
}

impl Drop for SuspendGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), EnterAlternateScreen);
        let _ = enable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the function returns the inner value. We can't
    /// meaningfully test terminal-mode toggles in a unit test (the
    /// test runner doesn't own a TTY), but we can confirm the wiring
    /// compiles and propagates the return value.
    #[test]
    #[ignore = "requires a TTY; run manually with `cargo test -- --ignored`"]
    fn returns_inner_value() {
        let result: Result<i32> = suspend_tui_for(|| 42);
        assert_eq!(result.unwrap(), 42);
    }
}
