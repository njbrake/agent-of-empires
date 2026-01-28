//! tmux integration module

mod session;
pub mod status_bar;
mod status_detection;
mod terminal_session;
mod utils;

pub use session::Session;
pub use status_bar::{get_session_info_for_current, get_status_for_current_session};
pub use status_detection::{
    detect_claude_status, detect_codex_status, detect_gemini_status, detect_opencode_status,
    detect_vibe_status,
};
pub use terminal_session::{ContainerTerminalSession, TerminalSession};

use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub const SESSION_PREFIX: &str = "aoe_";
pub const TERMINAL_PREFIX: &str = "aoe_term_";
pub const CONTAINER_TERMINAL_PREFIX: &str = "aoe_cterm_";

static SESSION_CACHE: RwLock<SessionCache> = RwLock::new(SessionCache {
    data: None,
    time: None,
});

struct SessionCache {
    data: Option<HashMap<String, i64>>,
    time: Option<Instant>,
}

pub fn refresh_session_cache() {
    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_activity}",
        ])
        .output();

    let new_data = match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut map = HashMap::new();
            for line in stdout.lines() {
                if let Some((name, activity)) = line.split_once('\t') {
                    let activity: i64 = activity.parse().unwrap_or(0);
                    map.insert(name.to_string(), activity);
                }
            }
            Some(map)
        }
        _ => None,
    };

    if let Ok(mut cache) = SESSION_CACHE.write() {
        cache.data = new_data;
        cache.time = Some(Instant::now());
    }
}

pub fn session_exists_from_cache(name: &str) -> Option<bool> {
    let cache = SESSION_CACHE.read().ok()?;

    // Cache valid for 2 seconds
    if cache
        .time
        .map(|t| t.elapsed() > Duration::from_secs(2))
        .unwrap_or(true)
    {
        return None;
    }

    cache.data.as_ref().map(|m| m.contains_key(name))
}

pub fn get_current_session_name() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
        .ok()?;

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

pub fn is_tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

pub fn is_claude_available() -> bool {
    Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_opencode_available() -> bool {
    Command::new("which")
        .arg("opencode")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_codex_available() -> bool {
    Command::new("which")
        .arg("codex")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_vibe_available() -> bool {
    Command::new("vibe").arg("--version").output().is_ok()
}

pub fn is_gemini_available() -> bool {
    Command::new("which")
        .arg("gemini")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct AvailableTools {
    pub claude: bool,
    pub opencode: bool,
    pub vibe: bool,
    pub codex: bool,
    pub gemini: bool,
}

impl AvailableTools {
    pub fn detect() -> Self {
        Self {
            claude: is_claude_available(),
            opencode: is_opencode_available(),
            vibe: is_vibe_available(),
            codex: is_codex_available(),
            gemini: is_gemini_available(),
        }
    }

    pub fn any_available(&self) -> bool {
        self.claude || self.opencode || self.vibe || self.codex || self.gemini
    }

    pub fn available_list(&self) -> Vec<&'static str> {
        let mut tools = Vec::new();
        if self.claude {
            tools.push("claude");
        }
        if self.opencode {
            tools.push("opencode");
        }
        if self.vibe {
            tools.push("vibe");
        }
        if self.codex {
            tools.push("codex");
        }
        if self.gemini {
            tools.push("gemini");
        }
        tools
    }
}
