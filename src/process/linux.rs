//! Linux-specific process utilities

use std::fs;
use std::path::Path;

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tracing::debug;

/// Kill a process and all its descendants
/// Uses SIGTERM first, then SIGKILL after a short delay for stragglers
pub fn kill_process_tree(pid: u32) {
    // Collect all descendant PIDs first (children, grandchildren, etc.)
    let mut pids_to_kill = vec![pid];
    collect_descendants(pid, &mut pids_to_kill);

    debug!(
        pid,
        descendants = ?pids_to_kill,
        "Killing process tree"
    );

    // Kill in reverse order (children first, then parent) with SIGTERM
    for &p in pids_to_kill.iter().rev() {
        let _ = kill(Pid::from_raw(p as i32), Signal::SIGTERM);
    }

    // Brief pause to let processes handle SIGTERM gracefully
    std::thread::sleep(std::time::Duration::from_millis(100));

    // SIGKILL any survivors
    for &p in pids_to_kill.iter().rev() {
        if process_exists(p) {
            debug!(pid = p, "Process survived SIGTERM, sending SIGKILL");
            let _ = kill(Pid::from_raw(p as i32), Signal::SIGKILL);
        }
    }
}

/// Recursively collect all descendant PIDs of a process
fn collect_descendants(pid: u32, pids: &mut Vec<u32>) {
    let proc_dir = Path::new("/proc");
    if !proc_dir.exists() {
        return;
    }

    let Ok(entries) = fs::read_dir(proc_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip non-numeric entries
        let Ok(child_pid) = name_str.parse::<u32>() else {
            continue;
        };

        // Read the process's parent PID
        let stat_path = entry.path().join("stat");
        let Ok(content) = fs::read_to_string(&stat_path) else {
            continue;
        };

        if let Some(ppid) = parse_stat_field(&content, 3) {
            if ppid as u32 == pid {
                pids.push(child_pid);
                // Recurse to find grandchildren
                collect_descendants(child_pid, pids);
            }
        }
    }
}

/// Check if a process still exists
fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

/// Get the foreground process group leader for a shell PID
/// Walks the process tree to find the actual foreground process
pub fn get_foreground_pid(shell_pid: u32) -> Option<u32> {
    // Read the shell's stat to get its controlling terminal
    let stat_path = format!("/proc/{}/stat", shell_pid);
    let stat_content = fs::read_to_string(&stat_path).ok()?;

    // Parse stat: pid (comm) state ppid pgrp session tty_nr tpgid ...
    // tpgid (field 8, 0-indexed 7) is the foreground process group ID
    let tpgid = parse_stat_field(&stat_content, 7)?;

    if tpgid <= 0 {
        return Some(shell_pid);
    }

    // Find a process in the foreground process group
    // The tpgid is a process group ID, we need to find a process in that group
    find_process_in_group(tpgid as u32).or(Some(shell_pid))
}

/// Find a process that belongs to the given process group
fn find_process_in_group(pgrp: u32) -> Option<u32> {
    let proc_dir = Path::new("/proc");
    if !proc_dir.exists() {
        return None;
    }

    for entry in fs::read_dir(proc_dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip non-numeric entries
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let pid: u32 = name_str.parse().ok()?;
        let stat_path = entry.path().join("stat");

        if let Ok(content) = fs::read_to_string(&stat_path) {
            // Field 5 (0-indexed 4) is the process group ID
            if let Some(proc_pgrp) = parse_stat_field(&content, 4) {
                if proc_pgrp as u32 == pgrp {
                    return Some(pid);
                }
            }
        }
    }

    None
}

/// Parse a specific field from /proc/[pid]/stat
/// Fields are space-separated but comm (field 2) can contain spaces and is in parens
fn parse_stat_field(content: &str, field_idx: usize) -> Option<i64> {
    // Find the closing paren of comm field, then parse from there
    let close_paren = content.rfind(')')?;
    let after_comm = &content[close_paren + 2..]; // Skip ") "

    // Fields after comm start at index 2 (state is index 2)
    // So field_idx 4 means we want the 3rd field after comm (index 2 in after_comm split)
    let adjusted_idx = field_idx.checked_sub(2)?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields.get(adjusted_idx)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stat_field() {
        // Example stat line (simplified)
        let stat = "1234 (bash) S 1233 1234 1234 34816 1234 4194304 1234 0 0 0";
        // Fields: pid(0) comm(1) state(2) ppid(3) pgrp(4) session(5) tty(6) tpgid(7) ...

        assert_eq!(parse_stat_field(stat, 3), Some(1233)); // ppid
        assert_eq!(parse_stat_field(stat, 4), Some(1234)); // pgrp
        assert_eq!(parse_stat_field(stat, 7), Some(1234)); // tpgid
    }

    #[test]
    fn test_parse_stat_field_with_spaces_in_comm() {
        // Process name can contain spaces
        let stat = "1234 (my process) S 1233 1234 1234 34816 5678 4194304 1234 0 0 0";
        assert_eq!(parse_stat_field(stat, 3), Some(1233)); // ppid
        assert_eq!(parse_stat_field(stat, 7), Some(5678)); // tpgid
    }

    #[test]
    fn test_parse_stat_field_with_parens_in_comm() {
        // Process name can contain parentheses
        let stat = "1234 (test (proc)) S 1233 1234 1234 34816 5678 4194304 1234 0 0 0";
        assert_eq!(parse_stat_field(stat, 3), Some(1233)); // ppid
        assert_eq!(parse_stat_field(stat, 7), Some(5678)); // tpgid
    }

    #[test]
    fn test_parse_stat_field_negative_values() {
        let stat = "1234 (bash) S -1 1234 1234 34816 -1 4194304 1234 0 0 0";
        assert_eq!(parse_stat_field(stat, 3), Some(-1)); // ppid can be -1 for init
    }

    #[test]
    fn test_parse_stat_field_invalid_index() {
        let stat = "1234 (bash) S 1233 1234";
        // Index 7 doesn't exist
        assert_eq!(parse_stat_field(stat, 7), None);
    }

    #[test]
    fn test_parse_stat_field_index_too_low() {
        let stat = "1234 (bash) S 1233 1234 1234 34816 1234";
        // Index 0 and 1 are pid and comm which are handled differently
        assert_eq!(parse_stat_field(stat, 0), None);
        assert_eq!(parse_stat_field(stat, 1), None);
    }

    #[test]
    fn test_parse_stat_field_state_field() {
        let stat = "1234 (bash) S 1233 1234 1234 34816 1234";
        // Index 2 is state, but it's a char not a number
        assert_eq!(parse_stat_field(stat, 2), None);
    }

    #[test]
    fn test_parse_stat_field_empty_string() {
        assert_eq!(parse_stat_field("", 3), None);
    }

    #[test]
    fn test_parse_stat_field_no_closing_paren() {
        let stat = "1234 (bash S 1233 1234";
        assert_eq!(parse_stat_field(stat, 3), None);
    }

    #[test]
    fn test_parse_stat_field_large_values() {
        // 64-bit values are common for some fields
        // Field 9 (index 9) should have the large value
        let stat = "1234 (bash) S 1233 1234 1234 34816 1234 4194304 9223372036854775807 0 0 0";
        assert_eq!(parse_stat_field(stat, 9), Some(9223372036854775807_i64));
    }

    #[test]
    fn test_parse_stat_field_zero() {
        let stat = "1234 (bash) S 0 1234 1234 0 0 4194304 1234 0 0 0";
        assert_eq!(parse_stat_field(stat, 3), Some(0)); // ppid of 0
        assert_eq!(parse_stat_field(stat, 6), Some(0)); // tty of 0
    }
}
