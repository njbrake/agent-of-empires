//! Clipboard write for preview drag-select.
//!
//! Three paths fire on every copy, best-to-worst odds of success:
//!
//! 1. **Platform subprocess** — `pbcopy` on macOS, `wl-copy` on Wayland,
//!    `xclip`/`xsel` on X11. This is the load-bearing path: it's how
//!    every other terminal app copies, it doesn't care whether the
//!    process owns a GUI handle, and it survives the TUI dropping back
//!    into raw mode mid-write.
//! 2. **Native library** via `arboard` — fallback for platforms the
//!    subprocess branch doesn't cover (e.g. Windows) or where the
//!    expected binary isn't on `PATH`.
//! 3. **OSC 52** — `\x1b]52;c;<base64>\x07` written to stdout. This is
//!    what carries the bytes back through SSH and Mosh, and through
//!    tmux when `set-clipboard on` is configured.
//!
//! Everything is best-effort: arboard / OSC 52 give us no
//! acknowledgement, and `pbcopy` may not exist on a stripped-down
//! system. Each path's outcome is logged at `info` so a user with
//! debug logging enabled (`AGENT_OF_EMPIRES_DEBUG=1`) can see which
//! mechanism succeeded.
use std::io::Write;
use std::process::{Command, Stdio};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Maximum OSC 52 payload (raw bytes before base64). Many terminals
/// cap the sequence around 100 KiB; we pick 1 MiB so single-screen
/// selections always fit and oversized selections truncate cleanly
/// instead of corrupting the terminal stream.
const MAX_BYTES: usize = 1024 * 1024;

/// Push `text` to the user's clipboard via every mechanism this
/// platform supports. Returns the number of bytes (post-truncation)
/// emitted. Per-path outcomes are logged at `tracing::info` so a
/// future "clipboard didn't update" bug report can be diagnosed with
/// `AGENT_OF_EMPIRES_DEBUG=1`.
pub fn copy_to_clipboard(text: &str) -> usize {
    let truncated = if text.len() > MAX_BYTES {
        &text.as_bytes()[..MAX_BYTES]
    } else {
        text.as_bytes()
    };
    let truncated_str = match std::str::from_utf8(truncated) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&truncated[..e.valid_up_to()]).unwrap_or(""),
    };

    let subprocess = try_subprocess(truncated_str);
    let arboard_result = try_arboard(truncated_str);
    let osc52 = write_osc52(truncated);

    tracing::info!(
        target: "tui.clipboard",
        bytes = truncated.len(),
        subprocess = format!("{:?}", subprocess).as_str(),
        arboard = format!("{:?}", arboard_result).as_str(),
        osc52_ok = osc52.is_ok(),
        "preview drag-select copy"
    );

    truncated.len()
}

/// Run a platform-specific clipboard subprocess. `Ok(cmd_name)` on
/// success so the caller can show which one landed; `Err(reason)`
/// otherwise.
#[cfg(target_os = "macos")]
fn try_subprocess(text: &str) -> Result<&'static str, String> {
    run_subprocess("pbcopy", &[], text)
}

#[cfg(target_os = "linux")]
fn try_subprocess(text: &str) -> Result<&'static str, String> {
    let mut last_err: Option<String> = None;
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match run_subprocess("wl-copy", &[], text) {
            Ok(name) => return Ok(name),
            Err(reason) => last_err = Some(reason),
        }
    }
    match run_subprocess("xclip", &["-selection", "clipboard"], text) {
        Ok(name) => Ok(name),
        Err(xclip_err) => match run_subprocess("xsel", &["--clipboard", "--input"], text) {
            Ok(name) => Ok(name),
            Err(xsel_err) => Err(last_err
                .map(|e| format!("{e}; {xclip_err}; {xsel_err}"))
                .unwrap_or_else(|| format!("{xclip_err}; {xsel_err}"))),
        },
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn try_subprocess(_text: &str) -> Result<&'static str, String> {
    Err("no subprocess clipboard for this platform".to_string())
}

fn run_subprocess(cmd: &'static str, args: &[&str], text: &str) -> Result<&'static str, String> {
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Err(format!("{cmd} spawn: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(text.as_bytes()) {
            return Err(format!("{cmd} stdin write: {e}"));
        }
    }
    match child.wait() {
        Ok(status) if status.success() => Ok(cmd),
        Ok(status) => Err(format!("{cmd} exit {status}")),
        Err(e) => Err(format!("{cmd} wait: {e}")),
    }
}

fn try_arboard(text: &str) -> Result<(), String> {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.set_text(text).map_err(|e| format!("set_text: {e}")),
        Err(e) => Err(format!("init: {e}")),
    }
}

/// Write OSC 52 to stdout, wrapping in tmux's DCS passthrough when
/// `$TMUX` is set. Without the passthrough wrapper, tmux either
/// swallows the escape (default `set-clipboard off`) or buffers it
/// internally without forwarding to the parent terminal — neither
/// of which puts bytes on the user's local clipboard. The wrapper
/// (`\x1bPtmux;\x1b<inner>\x1b\\`) tells tmux to pass the inner
/// sequence through verbatim, provided `set -g allow-passthrough on`
/// is in the user's tmux.conf (default on tmux 3.3+).
///
/// We emit BOTH the plain and the wrapped form: the wrapped form
/// reaches the parent terminal through tmux passthrough; the plain
/// form covers the case where the user isn't in tmux despite
/// `$TMUX` being set (e.g. forwarded env var from a previous shell)
/// and any terminal that ignores tmux DCS sequences will just drop
/// the wrapped one as garbage.
fn write_osc52(bytes: &[u8]) -> std::io::Result<()> {
    let encoded = STANDARD.encode(bytes);
    let in_tmux = std::env::var_os("TMUX").is_some();
    let mut stdout = std::io::stdout().lock();
    if in_tmux {
        // Tmux DCS passthrough: \x1bPtmux;<escaped inner>\x1b\\
        // where each ESC inside the inner sequence is doubled.
        // OSC 52 contains exactly one ESC (the opening), so we
        // emit it as `\x1b\x1b]52;c;...;\x07`.
        write!(stdout, "\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded)?;
    }
    write!(stdout, "\x1b]52;c;{}\x07", encoded)?;
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_oversize_payload() {
        let huge = "a".repeat(MAX_BYTES + 100);
        let copied = copy_to_clipboard(&huge);
        assert_eq!(copied, MAX_BYTES);
    }

    #[test]
    fn copies_short_payload_in_full() {
        let copied = copy_to_clipboard("hello");
        assert_eq!(copied, 5);
    }
}
