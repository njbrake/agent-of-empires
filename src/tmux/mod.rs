//! tmux integration module

mod session;
pub mod status_bar;
pub(crate) mod status_detection;
mod terminal_session;
pub(crate) mod utils;

pub use session::Session;
pub use status_bar::{get_session_info_for_current, get_status_for_current_session};
pub use status_detection::detect_status_from_content;
pub use terminal_session::{ContainerTerminalSession, TerminalSession};

use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub const SESSION_PREFIX: &str = "aoe_";
pub const TERMINAL_PREFIX: &str = "aoe_term_";
pub const CONTAINER_TERMINAL_PREFIX: &str = "aoe_cterm_";

/// Pre-fetched pane metadata from a single `tmux list-panes -a` call.
#[derive(Debug, Clone)]
pub struct PaneMetadata {
    pub pane_dead: bool,
    pub pane_current_command: Option<String>,
    pub pane_pid: Option<u32>,
}

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

/// Batch-fetch pane metadata for all aoe sessions in a single tmux subprocess call.
/// Returns a map from session name to metadata for the first window's first pane.
pub fn batch_pane_metadata() -> HashMap<String, PaneMetadata> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{pane_index}\t#{pane_dead}\t#{pane_current_command}\t#{pane_pid}",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            parse_pane_metadata(&stdout)
        }
        _ => HashMap::new(),
    }
}

/// Parse the output of `tmux list-panes -a` into a map of session name to pane metadata.
/// Filters to aoe sessions, pane index 0, and takes only the first window per session.
fn parse_pane_metadata(output: &str) -> HashMap<String, PaneMetadata> {
    let mut map = HashMap::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }

        let session_name = parts[0];
        if !session_name.starts_with(SESSION_PREFIX) {
            continue;
        }

        // Only take pane 0 (the agent pane). aoe pins pane-base-index to 0.
        if parts[1] != "0" {
            continue;
        }

        // First occurrence per session = first window's pane 0 (list-panes
        // returns windows in index order).
        if map.contains_key(session_name) {
            continue;
        }

        map.insert(
            session_name.to_string(),
            PaneMetadata {
                pane_dead: parts[2] == "1",
                pane_current_command: if parts[3].is_empty() {
                    None
                } else {
                    Some(parts[3].to_string())
                },
                pane_pid: parts[4].parse::<u32>().ok(),
            },
        );
    }

    map
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

fn is_agent_available(agent: &crate::agents::AgentDef) -> bool {
    use crate::agents::DetectionMethod;
    match &agent.detection {
        DetectionMethod::Which(binary) => {
            // First try direct `which` (fast path).
            let direct = Command::new("which")
                .arg(binary)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if direct {
                return true;
            }
            // Fall back to a login shell so version-manager PATHs (NVM, etc.) are loaded.
            let shell = crate::session::user_shell();
            Command::new(&shell)
                .args(["-lc", &format!("which {}", binary)])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        DetectionMethod::RunWithArg(binary, arg) => {
            if Command::new(binary)
                .arg(arg)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return true;
            }
            let shell = crate::session::user_shell();
            Command::new(&shell)
                .args(["-lc", &format!("{} {}", binary, arg)])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }
}

#[derive(Debug, Clone)]
pub struct AvailableTools {
    available: Vec<&'static str>,
}

impl AvailableTools {
    pub fn detect() -> Self {
        let available = crate::agents::AGENTS
            .iter()
            .filter(|a| is_agent_available(a))
            .map(|a| a.name)
            .collect();
        Self { available }
    }

    pub fn any_available(&self) -> bool {
        !self.available.is_empty()
    }

    pub fn available_list(&self) -> Vec<&'static str> {
        self.available.clone()
    }

    #[cfg(test)]
    pub fn with_tools(tools: &[&'static str]) -> Self {
        Self {
            available: tools.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pane_metadata_basic() {
        let output = "aoe_my_proj_abc12345\t0\t0\tclaude\t12345\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 1);
        let meta = map.get("aoe_my_proj_abc12345").unwrap();
        assert!(!meta.pane_dead);
        assert_eq!(meta.pane_current_command.as_deref(), Some("claude"));
        assert_eq!(meta.pane_pid, Some(12345));
    }

    #[test]
    fn test_parse_pane_metadata_dead_pane() {
        let output = "aoe_proj_abc12345\t0\t1\tbash\t99\n";
        let map = parse_pane_metadata(output);
        let meta = map.get("aoe_proj_abc12345").unwrap();
        assert!(meta.pane_dead);
    }

    #[test]
    fn test_parse_pane_metadata_filters_non_aoe_sessions() {
        let output = "\
user_session\t0\t0\tbash\t100\n\
aoe_proj_abc12345\t0\t0\tclaude\t200\n\
my_tmux\t0\t0\tvim\t300\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("aoe_proj_abc12345"));
    }

    #[test]
    fn test_parse_pane_metadata_filters_non_zero_panes() {
        let output = "\
aoe_proj_abc12345\t0\t0\tclaude\t100\n\
aoe_proj_abc12345\t1\t0\tbash\t101\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 1);
        let meta = map.get("aoe_proj_abc12345").unwrap();
        assert_eq!(meta.pane_current_command.as_deref(), Some("claude"));
        assert_eq!(meta.pane_pid, Some(100));
    }

    #[test]
    fn test_parse_pane_metadata_first_window_wins() {
        // Two windows both have pane 0, first window's data should be kept
        let output = "\
aoe_proj_abc12345\t0\t0\tclaude\t100\n\
aoe_proj_abc12345\t0\t1\tbash\t200\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 1);
        let meta = map.get("aoe_proj_abc12345").unwrap();
        assert!(!meta.pane_dead);
        assert_eq!(meta.pane_pid, Some(100));
    }

    #[test]
    fn test_parse_pane_metadata_empty_output() {
        assert!(parse_pane_metadata("").is_empty());
    }

    #[test]
    fn test_parse_pane_metadata_malformed_lines() {
        let output = "\
too\tfew\tfields\n\
aoe_proj_abc12345\t0\t0\tclaude\t100\n\
\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_parse_pane_metadata_empty_command() {
        let output = "aoe_proj_abc12345\t0\t0\t\t100\n";
        let map = parse_pane_metadata(output);
        let meta = map.get("aoe_proj_abc12345").unwrap();
        assert!(meta.pane_current_command.is_none());
    }

    #[test]
    fn test_parse_pane_metadata_invalid_pid() {
        let output = "aoe_proj_abc12345\t0\t0\tclaude\tnot_a_pid\n";
        let map = parse_pane_metadata(output);
        let meta = map.get("aoe_proj_abc12345").unwrap();
        assert!(meta.pane_pid.is_none());
    }

    #[test]
    fn test_parse_pane_metadata_multiple_sessions() {
        let output = "\
aoe_proj_a_abc12345\t0\t0\tclaude\t100\n\
aoe_proj_b_def67890\t0\t0\topencode\t200\n\
aoe_proj_c_ghi11111\t0\t1\tbash\t300\n";
        let map = parse_pane_metadata(output);
        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get("aoe_proj_a_abc12345")
                .unwrap()
                .pane_current_command
                .as_deref(),
            Some("claude")
        );
        assert_eq!(
            map.get("aoe_proj_b_def67890")
                .unwrap()
                .pane_current_command
                .as_deref(),
            Some("opencode")
        );
        assert!(map.get("aoe_proj_c_ghi11111").unwrap().pane_dead);
    }
}
