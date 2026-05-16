//! Per-agent restart recovery dispatch.
//!
//! Each coding agent has its own restart quirks: where its transcript lives,
//! what "oversized" means for its parser, how to fall back when the transcript
//! is missing. Rather than baking Claude-specific assumptions into the session
//! restart path, this module exposes a [`HarnessRecovery`] trait and dispatches
//! by `tool: &str` (matching the existing per-agent contract used by
//! [`crate::tmux::status_detection`]).
//!
//! The first implementation is [`claude::ClaudeRecovery`]. Other agents (codex,
//! opencode, gemini) can ship their own implementations without touching the
//! restart codepath in `instance.rs`.

pub mod claude;

pub use claude::RecoveryOutcome;

/// Per-agent transcript/state recovery applied at restart time.
///
/// Implementations are zero-sized dispatch types; state lives in the agent's
/// on-disk artifacts (transcript files, archives) and is rediscovered each
/// call. Implementations should be conservative: prefer
/// [`RecoveryOutcome::NotApplicable`] over panicking when inputs are malformed
/// or unsupported. Filesystem errors during an active restoration are the only
/// case that warrants returning `Err`.
pub trait HarnessRecovery: Send + Sync {
    /// Attempt to recover the transcript/state for `sid` rooted at
    /// `project_path`. Returns the cascade outcome so the caller can log it
    /// and decide on follow-up behavior (e.g. fresh-launch fallback when
    /// [`RecoveryOutcome::NoArchiveFreshLaunch`]).
    fn recover(&self, sid: &str, project_path: &str) -> anyhow::Result<RecoveryOutcome>;
}

/// Look up the recovery implementation for a given agent tool string. Returns
/// `None` when the agent does not (yet) ship its own recovery; callers should
/// treat that as a no-op and let the existing restart path run unchanged.
pub fn for_tool(tool: &str) -> Option<&'static dyn HarnessRecovery> {
    match tool {
        "claude" => Some(&claude::ClaudeRecovery),
        _ => None,
    }
}
