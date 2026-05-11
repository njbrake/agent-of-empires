//! `aoe logs` - view AoE log files (debug.log, serve.log) with a pretty viewer.
//!
//! Resolves the right path under the app data dir, picks the best available
//! viewer (lnav > bat > less > plain stdout), and prints a one-line tip when
//! `lnav` is missing so users know there's a better experience available.

use anyhow::{bail, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args)]
pub struct LogsArgs {
    /// View debug.log (default).
    #[arg(long, conflicts_with_all = ["serve", "all"])]
    pub debug: bool,

    /// View serve.log (daemon stdout/stderr).
    #[arg(long, conflicts_with_all = ["debug", "all"])]
    pub serve: bool,

    /// View both debug.log and serve.log, merged by timestamp.
    #[arg(long, conflicts_with_all = ["debug", "serve"])]
    pub all: bool,

    /// Live-tail the log.
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Show only the last N lines (fallback viewers; lnav handles its own).
    #[arg(short = 'n', long, value_name = "N")]
    pub lines: Option<usize>,

    /// Skip viewer detection; write plain log to stdout.
    #[arg(long)]
    pub no_pager: bool,

    /// Print the resolved log file path(s) and exit (no viewing).
    #[arg(long)]
    pub path: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Viewer {
    Lnav,
    Bat,
    Less,
    PlainStdout,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Mode {
    Debug,
    Serve,
    All,
}

fn debug_log_path() -> Result<PathBuf> {
    Ok(crate::session::get_app_dir()?.join("debug.log"))
}

#[cfg(feature = "serve")]
fn serve_log_path() -> Result<PathBuf> {
    crate::cli::serve::daemon_log_path()
}

pub fn detect_viewer(no_pager: bool) -> Viewer {
    if no_pager {
        return Viewer::PlainStdout;
    }
    if which::which("lnav").is_ok() {
        return Viewer::Lnav;
    }
    if which::which("bat").is_ok() {
        return Viewer::Bat;
    }
    if which::which("less").is_ok() {
        return Viewer::Less;
    }
    Viewer::PlainStdout
}

fn viewer_name(v: Viewer) -> &'static str {
    match v {
        Viewer::Lnav => "lnav",
        Viewer::Bat => "bat",
        Viewer::Less => "less",
        Viewer::PlainStdout => "plain stdout",
    }
}

pub async fn run(args: LogsArgs) -> Result<()> {
    let mode = if args.serve {
        Mode::Serve
    } else if args.all {
        Mode::All
    } else {
        Mode::Debug
    };

    #[cfg(not(feature = "serve"))]
    if matches!(mode, Mode::Serve | Mode::All) {
        bail!(
            "--serve and --all need a build with the `serve` feature; \
             this binary was built without it. Use `--debug` (default) for debug.log."
        );
    }

    let paths = resolve_paths(mode)?;

    if args.path {
        for p in &paths {
            println!("{}", p.display());
        }
        return Ok(());
    }

    if args.follow && mode == Mode::All {
        bail!("--follow is not supported with --all; pick --debug or --serve.");
    }

    // For --all, materialize a merged stream into a temp file and view that.
    // _guard keeps the tempfile alive for the duration of the viewer. If no
    // source file exists, print does-not-exist hints rather than opening an
    // empty viewer, matching the --debug/--serve behavior below.
    let (target_path, _guard) = match mode {
        Mode::All => {
            if paths.iter().all(|p| !p.exists()) {
                for p in &paths {
                    eprintln!("{} does not exist (yet).", p.display());
                }
                eprintln!("Tip: run with AGENT_OF_EMPIRES_DEBUG=1 to generate debug.log.");
                return Ok(());
            }
            let merged = merged_temp_file(&paths)?;
            (merged.path().to_path_buf(), Some(merged))
        }
        _ => (paths[0].clone(), None),
    };

    if !target_path.exists() {
        eprintln!("{} does not exist (yet).", target_path.display());
        if mode == Mode::Debug {
            eprintln!("Tip: run with AGENT_OF_EMPIRES_DEBUG=1 to generate debug.log.");
        }
        return Ok(());
    }

    let viewer = detect_viewer(args.no_pager);
    if !args.no_pager && viewer != Viewer::Lnav && std::env::var_os("AOE_NO_LNAV_TIP").is_none() {
        eprintln!(
            "Tip: install `lnav` for color, level filters, and search (https://lnav.org). \
             Set AOE_NO_LNAV_TIP=1 to silence. Falling back to {}.",
            viewer_name(viewer)
        );
    }

    run_viewer(viewer, &target_path, &args)
}

fn resolve_paths(mode: Mode) -> Result<Vec<PathBuf>> {
    match mode {
        Mode::Debug => Ok(vec![debug_log_path()?]),
        #[cfg(feature = "serve")]
        Mode::Serve => Ok(vec![serve_log_path()?]),
        #[cfg(feature = "serve")]
        Mode::All => Ok(vec![debug_log_path()?, serve_log_path()?]),
        #[cfg(not(feature = "serve"))]
        Mode::Serve | Mode::All => unreachable!("bailed earlier when serve feature is off"),
    }
}

#[cfg(feature = "serve")]
fn merged_temp_file(paths: &[PathBuf]) -> Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let debug_text = std::fs::read_to_string(&paths[0]).unwrap_or_default();
    let serve_text = std::fs::read_to_string(&paths[1]).unwrap_or_default();
    let merged = merge_by_timestamp(&debug_text, &serve_text);
    let mut tmp = tempfile::Builder::new()
        .prefix("aoe-logs-merged-")
        .suffix(".log")
        .tempfile()?;
    tmp.write_all(merged.as_bytes())?;
    tmp.flush()?;
    Ok(tmp)
}

#[cfg(not(feature = "serve"))]
fn merged_temp_file(_paths: &[PathBuf]) -> Result<tempfile::NamedTempFile> {
    unreachable!("--all requires the serve feature; bailed earlier")
}

fn run_viewer(viewer: Viewer, path: &Path, args: &LogsArgs) -> Result<()> {
    match viewer {
        Viewer::Lnav => {
            // lnav handles --follow natively and ignores --lines.
            Command::new("lnav").arg(path).status()?;
            Ok(())
        }
        Viewer::Bat => {
            // bat has no follow mode; downgrade to less +F or plain tail.
            if args.follow {
                let fallback = if which::which("less").is_ok() {
                    Viewer::Less
                } else {
                    Viewer::PlainStdout
                };
                return run_viewer(fallback, path, args);
            }
            let content = read_content(path, args.lines)?;
            pipe_through(
                Command::new("bat").args(["--paging=always", "-l", "log"]),
                &content,
            )
        }
        Viewer::Less => {
            if args.follow {
                // `less +F` on a file can't seek to "last N"; route through
                // tail when --lines is set so the user only sees the recent
                // window plus live appends.
                if let Some(n) = args.lines {
                    return tail_pipe_into(path, n, Command::new("less").args(["-R", "+F"]));
                }
                Command::new("less")
                    .arg("-R")
                    .arg("+F")
                    .arg(path)
                    .status()?;
                return Ok(());
            }
            let content = read_content(path, args.lines)?;
            pipe_through(Command::new("less").arg("-R"), &content)
        }
        Viewer::PlainStdout => {
            if args.follow {
                let mut cmd = Command::new("tail");
                if let Some(n) = args.lines {
                    cmd.args(["-n", &n.to_string()]);
                }
                cmd.arg("-F").arg(path).status()?;
                return Ok(());
            }
            let content = read_content(path, args.lines)?;
            print!("{}", content);
            Ok(())
        }
    }
}

fn read_content(path: &Path, lines: Option<usize>) -> Result<String> {
    let raw = std::fs::read_to_string(path)?;
    Ok(match lines {
        Some(n) => last_n_lines(&raw, n),
        None => raw,
    })
}

pub fn last_n_lines(text: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    let mut out = lines[start..].join("\n");
    if text.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

fn pipe_through(cmd: &mut Command, content: &str) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::piped()).spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(content.as_bytes());
    }
    let _ = child.wait()?;
    Ok(())
}

/// Spawn `tail -n N -F path` and feed its stdout into `viewer`'s stdin so the
/// viewer keeps following live appends while only showing the last N lines.
/// Kills tail when the viewer exits so we don't leak a background tail.
fn tail_pipe_into(path: &Path, lines: usize, viewer: &mut Command) -> Result<()> {
    use std::process::Stdio;
    let mut tail = Command::new("tail")
        .args(["-n", &lines.to_string(), "-F"])
        .arg(path)
        .stdout(Stdio::piped())
        .spawn()?;
    let tail_out = tail.stdout.take().expect("piped stdout");
    let status = viewer.stdin(Stdio::from(tail_out)).status();
    let _ = tail.kill();
    let _ = tail.wait();
    status?;
    Ok(())
}

/// Merge two tracing-formatted log streams by leading ISO-8601 timestamp.
/// Each emitted line is prefixed with `[debug]` / `[serve]` so the source
/// is unambiguous after merging. Continuation lines (no leading timestamp,
/// e.g. multi-line tracing payloads) ride along with their preceding head.
pub fn merge_by_timestamp(debug: &str, serve: &str) -> String {
    let mut entries: Vec<(Option<String>, String, &'static str)> = Vec::new();
    for (ts, body) in group_entries(debug) {
        entries.push((ts, body, "debug"));
    }
    for (ts, body) in group_entries(serve) {
        entries.push((ts, body, "serve"));
    }
    // Stable sort: timestamps strict-ordered; entries without a timestamp
    // (e.g. orphan continuation at file start) sink to the end.
    entries.sort_by(|a, b| match (&a.0, &b.0) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let mut out = String::new();
    for (_ts, body, tag) in entries {
        for line in body.lines() {
            out.push('[');
            out.push_str(tag);
            out.push_str("] ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn group_entries(text: &str) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    for line in text.lines() {
        if let Some(ts) = leading_timestamp(line) {
            out.push((Some(ts), line.to_string()));
        } else if let Some(last) = out.last_mut() {
            last.1.push('\n');
            last.1.push_str(line);
        } else {
            out.push((None, line.to_string()));
        }
    }
    out
}

/// `tracing-subscriber`'s default formatter prefixes each event with an
/// ISO-8601 timestamp like `2024-09-12T18:42:11.305812Z`. Detect that
/// prefix conservatively: if the first 19 chars match the date-time
/// skeleton, return the full token up to the first whitespace. Otherwise
/// return None (continuation line, blank line, or non-tracing format).
fn leading_timestamp(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let shape_ok = bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7] == b'-'
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
        && bytes[10] == b'T'
        && bytes[11].is_ascii_digit()
        && bytes[12].is_ascii_digit()
        && bytes[13] == b':'
        && bytes[14].is_ascii_digit()
        && bytes[15].is_ascii_digit()
        && bytes[16] == b':'
        && bytes[17].is_ascii_digit()
        && bytes[18].is_ascii_digit();
    if !shape_ok {
        return None;
    }
    let end = line[19..]
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| 19 + i)
        .unwrap_or(line.len());
    Some(line[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_viewer_no_pager_returns_plain_stdout() {
        assert_eq!(detect_viewer(true), Viewer::PlainStdout);
    }

    #[test]
    fn last_n_lines_returns_tail_and_preserves_trailing_newline() {
        let input = "a\nb\nc\nd\ne\n";
        assert_eq!(last_n_lines(input, 2), "d\ne\n");
    }

    #[test]
    fn last_n_lines_no_trailing_newline_in_input() {
        let input = "a\nb\nc";
        assert_eq!(last_n_lines(input, 2), "b\nc");
    }

    #[test]
    fn last_n_lines_zero_returns_empty() {
        assert_eq!(last_n_lines("a\nb\n", 0), "");
    }

    #[test]
    fn last_n_lines_n_larger_than_input_returns_full_input() {
        let input = "a\nb\n";
        assert_eq!(last_n_lines(input, 100), "a\nb\n");
    }

    #[test]
    fn last_n_lines_empty_input() {
        assert_eq!(last_n_lines("", 5), "");
    }

    #[test]
    fn leading_timestamp_extracts_iso8601_with_microseconds() {
        let ts = leading_timestamp("2024-09-12T18:42:11.305812Z  INFO foo: bar");
        assert_eq!(ts.as_deref(), Some("2024-09-12T18:42:11.305812Z"));
    }

    #[test]
    fn leading_timestamp_extracts_iso8601_no_fraction() {
        let ts = leading_timestamp("2024-09-12T18:42:11Z  INFO foo");
        assert_eq!(ts.as_deref(), Some("2024-09-12T18:42:11Z"));
    }

    #[test]
    fn leading_timestamp_rejects_non_timestamp_lines() {
        assert_eq!(leading_timestamp("    at src/foo.rs:42"), None);
        assert_eq!(leading_timestamp(""), None);
        assert_eq!(leading_timestamp("    panicked at..."), None);
    }

    #[test]
    fn merge_by_timestamp_interleaves_two_streams_in_order() {
        let dbg = "2024-01-01T00:00:01Z  INFO d1\n\
                   2024-01-01T00:00:03Z  INFO d2\n";
        let srv = "2024-01-01T00:00:02Z  INFO s1\n\
                   2024-01-01T00:00:04Z  INFO s2\n";
        let merged = merge_by_timestamp(dbg, srv);
        let lines: Vec<&str> = merged.lines().collect();
        assert_eq!(
            lines,
            vec![
                "[debug] 2024-01-01T00:00:01Z  INFO d1",
                "[serve] 2024-01-01T00:00:02Z  INFO s1",
                "[debug] 2024-01-01T00:00:03Z  INFO d2",
                "[serve] 2024-01-01T00:00:04Z  INFO s2",
            ]
        );
    }

    #[test]
    fn merge_by_timestamp_preserves_continuation_lines_with_their_head() {
        // Continuation lines have no leading timestamp; they should attach
        // to the previous head and travel together when the heads are
        // re-sorted. Use explicit `\n` to keep the indentation intact (a
        // `\`-continued raw string would strip the indent).
        let dbg = "2024-01-01T00:00:01Z  ERROR boom\n    at src/foo.rs:42\n    at src/bar.rs:7\n2024-01-01T00:00:03Z  INFO recover\n";
        let srv = "2024-01-01T00:00:02Z  INFO between\n";
        let merged = merge_by_timestamp(dbg, srv);
        let lines: Vec<&str> = merged.lines().collect();
        assert_eq!(
            lines,
            vec![
                "[debug] 2024-01-01T00:00:01Z  ERROR boom",
                "[debug]     at src/foo.rs:42",
                "[debug]     at src/bar.rs:7",
                "[serve] 2024-01-01T00:00:02Z  INFO between",
                "[debug] 2024-01-01T00:00:03Z  INFO recover",
            ]
        );
    }

    #[test]
    fn merge_by_timestamp_handles_empty_inputs() {
        assert_eq!(merge_by_timestamp("", ""), "");
        let only_dbg = "2024-01-01T00:00:01Z  INFO hi\n";
        assert_eq!(
            merge_by_timestamp(only_dbg, ""),
            "[debug] 2024-01-01T00:00:01Z  INFO hi\n"
        );
    }
}
