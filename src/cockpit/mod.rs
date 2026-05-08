//! Cockpit: native rendering of structured agent state via ACP.
//!
//! Architecture summary (see design doc v4 for the full picture):
//!
//! - aoe is an ACP **client**.
//! - Backends are ACP **agents** spawned as subprocesses.
//! - Day-one backends: `claude-code` (Anthropic's official ACP adapter) and
//!   `aoe-agent` (our Node binary, Vercel AI SDK 6).
//! - File-system access (`fs/*`) and terminal execution (`terminal/*`) are
//!   delegated from the agent to aoe via ACP. aoe owns the disk; the agent
//!   only orchestrates the model.
//! - State lives behind a single-writer actor; all mutations flow through
//!   `state::apply_event`.

pub mod acp_client;
pub mod agent_registry;
pub mod approvals;
pub mod fs_handler;
pub mod node;
pub mod permissions;
pub mod replay_buffer;
pub mod state;
pub mod supervisor;
pub mod terminal_handler;

pub use agent_registry::{AgentRegistry, AgentSpec};
pub use approvals::{Approval, ApprovalDecision, Nonce};
pub use state::{CockpitState, Event};

/// Returns true when the operator has opted in to cockpit via
/// `AOE_EXPERIMENTAL_COCKPIT=1`. Cockpit is gated behind this flag
/// while it stabilises: when unset, the web dashboard defaults to
/// tmux, the wizard hides the substrate picker, and existing cockpit
/// sessions still load (the gate is for *new* sessions).
pub fn experimental_enabled() -> bool {
    matches!(
        std::env::var("AOE_EXPERIMENTAL_COCKPIT").as_deref(),
        Ok("1") | Ok("true")
    )
}
