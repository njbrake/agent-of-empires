//! Transcript recovery for Claude Code sessions.
//!
//! When a session is restarted but its `~/.claude/projects/<encoded>/<sid>.jsonl`
//! transcript is missing or pathologically large, `claude --resume <sid>` either
//! fails outright or thrashes on autocompact. This module surfaces a best-effort
//! recovery cascade applied at restart time:
//!
//! 1. If the transcript exists and is within a sane size budget, do nothing.
//! 2. If it is missing, look for a thrash archive
//!    (`<proj>/archived/<sid>.jsonl.thrash-*`) and restore the most recent one,
//!    trimmed.
//! 3. If it is present but oversized, trim it in place.
//! 4. If none of the above apply (no transcript, no archive), report
//!    `NoArchiveFreshLaunch` so the caller can fall through to a fresh launch.
//!
//! The trim algorithm preserves the JSONL header (first `HEADER_LINES`), the
//! most recent `compactMetadata` event (which carries the compressed summary
//! the live conversation depends on), and the last `TAIL_LINES` lines of fresh
//! content. Everything in between is pre-summarized history and is safe to
//! drop.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Number of leading JSONL lines treated as session header (sessionId anchors,
/// boot metadata, environment).
const HEADER_LINES: usize = 5;

/// Default trailing window kept after trimming. Matches the personal-dev
/// `cx-cleave` reference implementation.
const TAIL_LINES: usize = 300;

/// Transcript files larger than this are considered "oversized" and eligible
/// for in-place trimming on restart. 50 MiB is well past the point where
/// `claude --resume` reliably thrashes on the host. Empirically chosen; tune
/// if upstream Claude Code changes its parser ceiling.
const OVERSIZE_BYTES: u64 = 50 * 1024 * 1024;

/// Outcome of a `recover_transcript_for_sid` call. Encodes which branch of the
/// cascade fired so callers can log it and choose follow-up behavior (e.g.
/// fresh-launch fallback when `NoArchiveFreshLaunch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// Transcript was readable and within size budget; nothing to do.
    TranscriptOk,
    /// Transcript was missing; restored a trimmed copy from a thrash archive.
    RestoredFromArchive { archive: PathBuf },
    /// Transcript existed but was oversized; trimmed it in place.
    TrimmedInPlace {
        original_lines: usize,
        kept_lines: usize,
    },
    /// Transcript missing and no archive available. Caller should launch
    /// fresh (without `--resume`) to keep the pane alive.
    NoArchiveFreshLaunch,
    /// Recovery does not apply to this session (e.g. non-Claude tool, no
    /// session ID, project dir missing). No action taken.
    NotApplicable,
}

/// Best-effort recovery for a Claude session's transcript.
///
/// `sid` is the Claude session UUID. `project_path` is the host workspace path
/// (Instance::project_path); it is encoded with Claude's project-dir naming
/// convention to locate `~/.claude/projects/<encoded>/<sid>.jsonl`.
///
/// Returns the outcome of the cascade. Never returns an error for the
/// "transcript already fine" case; only filesystem errors during restoration
/// or trim escape.
pub fn recover_transcript_for_sid(sid: &str, project_path: &str) -> Result<RecoveryOutcome> {
    let claude_home = match resolve_claude_home() {
        Some(p) => p,
        None => {
            tracing::debug!("recovery: cannot resolve Claude home; skipping");
            return Ok(RecoveryOutcome::NotApplicable);
        }
    };

    let encoded = encode_claude_project_path(project_path);
    let project_dir = claude_home.join("projects").join(&encoded);
    if !project_dir.is_dir() {
        tracing::debug!(
            "recovery: project dir missing for {}: {}",
            sid,
            project_dir.display()
        );
        return Ok(RecoveryOutcome::NotApplicable);
    }

    let live_path = project_dir.join(format!("{sid}.jsonl"));

    match fs::metadata(&live_path) {
        Ok(meta) => {
            if meta.len() <= OVERSIZE_BYTES {
                return Ok(RecoveryOutcome::TranscriptOk);
            }
            tracing::warn!(
                "recovery: transcript oversized ({} bytes > {}); trimming {}",
                meta.len(),
                OVERSIZE_BYTES,
                live_path.display()
            );
            let lines = read_lines(&live_path)?;
            let total = lines.len();
            let trimmed = trim_jsonl(&lines);
            write_atomic(&live_path, &trimmed)?;
            Ok(RecoveryOutcome::TrimmedInPlace {
                original_lines: total,
                kept_lines: trimmed.len(),
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                "recovery: transcript missing for sid {} at {}; scanning archive",
                sid,
                live_path.display()
            );
            let archive_dir = project_dir.join("archived");
            let newest = newest_archive_for_sid(&archive_dir, sid)?;
            match newest {
                Some(archive) => {
                    let lines = read_lines(&archive)
                        .with_context(|| format!("reading archive {}", archive.display()))?;
                    let trimmed = trim_jsonl(&lines);
                    write_atomic(&live_path, &trimmed)?;
                    tracing::info!(
                        "recovery: restored {} ({} lines) from {}",
                        live_path.display(),
                        trimmed.len(),
                        archive.display()
                    );
                    Ok(RecoveryOutcome::RestoredFromArchive { archive })
                }
                None => {
                    tracing::info!(
                        "recovery: no archive for sid {} in {}; caller should fresh-launch",
                        sid,
                        archive_dir.display()
                    );
                    Ok(RecoveryOutcome::NoArchiveFreshLaunch)
                }
            }
        }
        Err(err) => Err(err).with_context(|| format!("stat {}", live_path.display())),
    }
}

/// Pure trim algorithm. Takes JSONL lines, returns the kept subset. Exposed
/// for unit tests; callers should prefer `recover_transcript_for_sid`.
///
/// Strategy:
/// - Keep the first `HEADER_LINES` lines (session header).
/// - Find the last line containing a top-level `compactMetadata` key; if
///   present, keep that one line (it carries the summary of older history).
/// - Keep the last `TAIL_LINES` lines.
/// - Deduplicate ranges so a short input that overlaps still produces the
///   expected union.
pub fn trim_jsonl(lines: &[String]) -> Vec<String> {
    if lines.len() <= HEADER_LINES + TAIL_LINES + 1 {
        // Short enough that trimming wouldn't drop anything meaningful.
        return lines.to_vec();
    }

    let header_end = HEADER_LINES.min(lines.len());
    let last_compact_idx = find_last_compact_metadata(lines);

    let mut kept: Vec<String> = Vec::with_capacity(HEADER_LINES + TAIL_LINES + 1);
    for line in &lines[..header_end] {
        kept.push(line.clone());
    }

    let tail_floor = match last_compact_idx {
        Some(i) => {
            if i >= header_end {
                kept.push(lines[i].clone());
            }
            i + 1
        }
        None => header_end,
    };

    let tail_start = lines.len().saturating_sub(TAIL_LINES).max(tail_floor);
    for line in &lines[tail_start..] {
        kept.push(line.clone());
    }

    kept
}

fn find_last_compact_metadata(lines: &[String]) -> Option<usize> {
    let mut last: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('{') {
            continue;
        }
        let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.get("compactMetadata").is_some() {
            last = Some(i);
        }
    }
    last
}

fn newest_archive_for_sid(archive_dir: &Path, sid: &str) -> Result<Option<PathBuf>> {
    if !archive_dir.is_dir() {
        return Ok(None);
    }
    let prefix = format!("{sid}.jsonl.thrash-");
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in
        fs::read_dir(archive_dir).with_context(|| format!("read_dir {}", archive_dir.display()))?
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(_, t)| modified > *t) {
            best = Some((path, modified));
        }
    }
    Ok(best.map(|(p, _)| p))
}

fn read_lines(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(raw.lines().map(|l| format!("{l}\n")).collect())
}

fn write_atomic(target: &Path, lines: &[String]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent for {}", target.display()))?;
    fs::create_dir_all(parent).ok();
    let tmp = parent.join(format!(
        ".{}.recovery.tmp",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("xfer")
    ));
    {
        let mut f =
            fs::File::create(&tmp).with_context(|| format!("create tmp {}", tmp.display()))?;
        for line in lines {
            f.write_all(line.as_bytes())?;
        }
        f.flush()?;
    }
    fs::rename(&tmp, target)
        .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;
    Ok(())
}

/// Encode a project path into Claude Code's directory naming convention.
/// Mirrors the encoder in `session::capture`; duplicated here to keep this
/// module standalone (recovery should not pull in the broader capture
/// surface).
fn encode_claude_project_path(project_path: &str) -> String {
    project_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn resolve_claude_home() -> Option<PathBuf> {
    if let Ok(val) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(val));
    }
    dirs::home_dir().map(|h| h.join(".claude"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_lines(n: usize, marker: &str) -> Vec<String> {
        (0..n)
            .map(|i| format!("{{\"i\":{},\"m\":\"{}\"}}\n", i, marker))
            .collect()
    }

    fn compact_line() -> String {
        "{\"compactMetadata\":{\"preTokens\":160000,\"postTokens\":40000}}\n".to_string()
    }

    #[test]
    fn trim_short_input_is_passthrough() {
        let lines = make_lines(10, "x");
        let kept = trim_jsonl(&lines);
        assert_eq!(kept, lines);
    }

    #[test]
    fn trim_long_input_keeps_header_compact_and_tail() {
        let mut lines = make_lines(5, "header");
        lines.push(compact_line());
        lines.extend(make_lines(2000, "body"));
        lines.extend(make_lines(300, "tail"));

        let kept = trim_jsonl(&lines);
        assert!(kept.len() < lines.len(), "expected trim");
        // First 5 are header.
        for (i, line) in kept.iter().take(5).enumerate() {
            assert!(line.contains("\"header\""), "row {} = {}", i, line);
        }
        // Sixth row is the compactMetadata summary.
        assert!(kept[5].contains("compactMetadata"));
        // Tail is preserved; last line should be from the trailing window.
        assert!(kept.last().unwrap().contains("\"tail\""));
    }

    #[test]
    fn trim_no_compact_keeps_header_and_tail() {
        let mut lines = make_lines(5, "header");
        lines.extend(make_lines(2000, "body"));
        lines.extend(make_lines(300, "tail"));

        let kept = trim_jsonl(&lines);
        assert!(kept[0].contains("\"header\""));
        // No compactMetadata row inserted.
        assert!(!kept.iter().any(|l| l.contains("compactMetadata")));
        assert!(kept.last().unwrap().contains("\"tail\""));
    }

    /// Build an isolated $HOME/.claude tree and point CLAUDE_CONFIG_DIR at it.
    /// Returns the TempDir guard plus the encoded project dir.
    fn isolated_claude_home(project_path: &str) -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let claude_home = temp.path().join(".claude");
        let encoded: String = project_path
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let proj = claude_home.join("projects").join(&encoded);
        fs::create_dir_all(&proj).unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", &claude_home);
        (temp, proj)
    }

    #[test]
    #[serial_test::serial]
    fn recovery_transcript_ok_when_small_and_present() {
        let project = "/tmp/aoe-recovery-ok";
        let (_g, proj_dir) = isolated_claude_home(project);
        let sid = "11111111-1111-1111-1111-111111111111";
        let live = proj_dir.join(format!("{sid}.jsonl"));
        fs::write(&live, "{\"hello\":\"world\"}\n").unwrap();

        let out = recover_transcript_for_sid(sid, project).unwrap();
        assert_eq!(out, RecoveryOutcome::TranscriptOk);
    }

    #[test]
    #[serial_test::serial]
    fn recovery_missing_restores_from_archive() {
        let project = "/tmp/aoe-recovery-archive";
        let (_g, proj_dir) = isolated_claude_home(project);
        let sid = "22222222-2222-2222-2222-222222222222";
        let archive_dir = proj_dir.join("archived");
        fs::create_dir_all(&archive_dir).unwrap();

        // Synthesize an archived transcript big enough to exercise the trim path.
        let mut body = String::new();
        for i in 0..5 {
            body.push_str(&format!("{{\"hdr\":{i}}}\n"));
        }
        body.push_str("{\"compactMetadata\":{\"preTokens\":100000,\"postTokens\":20000}}\n");
        for i in 0..2000 {
            body.push_str(&format!("{{\"body\":{i}}}\n"));
        }
        for i in 0..300 {
            body.push_str(&format!("{{\"tail\":{i}}}\n"));
        }
        let archive_path = archive_dir.join(format!("{sid}.jsonl.thrash-20260512-120000"));
        fs::write(&archive_path, body).unwrap();

        let out = recover_transcript_for_sid(sid, project).unwrap();
        match out {
            RecoveryOutcome::RestoredFromArchive { archive } => {
                assert_eq!(archive, archive_path);
            }
            other => panic!("expected RestoredFromArchive, got {:?}", other),
        }

        let live = proj_dir.join(format!("{sid}.jsonl"));
        let restored = fs::read_to_string(&live).unwrap();
        assert!(restored.contains("compactMetadata"));
        assert!(restored.contains("\"tail\":299"));
        let line_count = restored.lines().count();
        assert!(line_count < 2300, "expected trim, got {} lines", line_count);
        assert!(
            line_count >= 305,
            "expected header+compact+tail, got {}",
            line_count
        );
    }

    #[test]
    #[serial_test::serial]
    fn recovery_missing_no_archive_returns_fresh_launch() {
        let project = "/tmp/aoe-recovery-fresh";
        let (_g, _proj_dir) = isolated_claude_home(project);
        let sid = "33333333-3333-3333-3333-333333333333";

        let out = recover_transcript_for_sid(sid, project).unwrap();
        assert_eq!(out, RecoveryOutcome::NoArchiveFreshLaunch);
    }

    #[test]
    #[serial_test::serial]
    fn recovery_oversized_trims_in_place() {
        let project = "/tmp/aoe-recovery-oversize";
        let (_g, proj_dir) = isolated_claude_home(project);
        let sid = "44444444-4444-4444-4444-444444444444";
        let live = proj_dir.join(format!("{sid}.jsonl"));

        // Build a file larger than OVERSIZE_BYTES with a recognizable compact
        // line we can verify post-trim.
        let mut f = fs::File::create(&live).unwrap();
        for i in 0..5 {
            f.write_all(format!("{{\"hdr\":{i}}}\n").as_bytes())
                .unwrap();
        }
        f.write_all(b"{\"compactMetadata\":{\"preTokens\":999,\"postTokens\":1}}\n")
            .unwrap();
        // Pad with junk past the oversize threshold.
        let pad = "{\"junk\":\"".to_string() + &"x".repeat(1024) + "\"}\n";
        let needed = (OVERSIZE_BYTES as usize / pad.len()) + 1;
        for _ in 0..needed {
            f.write_all(pad.as_bytes()).unwrap();
        }
        for i in 0..300 {
            f.write_all(format!("{{\"tail\":{i}}}\n").as_bytes())
                .unwrap();
        }
        drop(f);

        let original_size = fs::metadata(&live).unwrap().len();
        assert!(original_size > OVERSIZE_BYTES);

        let out = recover_transcript_for_sid(sid, project).unwrap();
        match out {
            RecoveryOutcome::TrimmedInPlace {
                original_lines,
                kept_lines,
            } => {
                assert!(kept_lines < original_lines);
                assert!(kept_lines >= 305);
            }
            other => panic!("expected TrimmedInPlace, got {:?}", other),
        }

        let after = fs::read_to_string(&live).unwrap();
        assert!(after.contains("compactMetadata"));
        assert!(after.contains("\"tail\":299"));
    }

    #[test]
    #[serial_test::serial]
    fn recovery_no_project_dir_is_not_applicable() {
        let temp = TempDir::new().unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", temp.path().join(".claude"));
        let out = recover_transcript_for_sid(
            "55555555-5555-5555-5555-555555555555",
            "/tmp/nonexistent-aoe-project",
        )
        .unwrap();
        assert_eq!(out, RecoveryOutcome::NotApplicable);
    }
}
