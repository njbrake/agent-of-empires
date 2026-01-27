//! macOS-specific process utilities

use std::collections::HashMap;
use std::process::Command;

use nix::errno::Errno;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tracing::debug;

/// Kill a process and all its descendants
/// Uses SIGTERM first, then SIGKILL after a short delay for stragglers
pub fn kill_process_tree(pid: u32) {
    // Build a map of parent -> children by parsing the process list once
    let children_map = build_children_map();

    // Collect all descendant PIDs (children, grandchildren, etc.)
    let mut pids_to_kill = vec![pid];
    collect_descendants_from_map(pid, &children_map, &mut pids_to_kill);

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

/// Build a map of parent PID -> list of child PIDs by parsing `ps` output once
fn build_children_map() -> HashMap<u32, Vec<u32>> {
    let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();

    let Ok(output) = Command::new("ps").args(["-o", "pid=,ppid=", "-A"]).output() else {
        return children_map;
    };

    if !output.status.success() {
        return children_map;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let (Ok(child_pid), Ok(ppid)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                children_map.entry(ppid).or_default().push(child_pid);
            }
        }
    }

    children_map
}

/// Recursively collect all descendant PIDs using the pre-built children map
fn collect_descendants_from_map(
    pid: u32,
    children_map: &HashMap<u32, Vec<u32>>,
    pids: &mut Vec<u32>,
) {
    if let Some(children) = children_map.get(&pid) {
        for &child_pid in children {
            pids.push(child_pid);
            collect_descendants_from_map(child_pid, children_map, pids);
        }
    }
}

/// Check if a process still exists
fn process_exists(pid: u32) -> bool {
    // Use kill with signal 0 to check if process exists
    // EPERM means the process exists but we lack permission (still exists)
    // ESRCH means the process doesn't exist
    match kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

/// Get the foreground process group leader for a shell PID
pub fn get_foreground_pid(shell_pid: u32) -> Option<u32> {
    // Use ps to get the foreground process group
    // ps -o tpgid= -p <pid> gives us the terminal foreground process group ID
    let output = Command::new("ps")
        .args(["-o", "tpgid=", "-p", &shell_pid.to_string()])
        .output()
        .ok()?;

    if !output.status.success() {
        return Some(shell_pid);
    }

    let tpgid: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;

    if tpgid <= 0 {
        return Some(shell_pid);
    }

    // Find a process in the foreground group
    find_process_in_group(tpgid as u32).or(Some(shell_pid))
}

/// Find a process belonging to the given process group
fn find_process_in_group(pgrp: u32) -> Option<u32> {
    // Use ps to find processes in this group
    // ps -o pid=,pgid= -A lists all processes with their PIDs and PGIDs
    let output = Command::new("ps")
        .args(["-o", "pid=,pgid=", "-A"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let (Ok(pid), Ok(proc_pgrp)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                if proc_pgrp == pgrp {
                    return Some(pid);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_descendants_from_map_empty() {
        let children_map = HashMap::new();
        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);
        assert_eq!(pids, vec![100]);
    }

    #[test]
    fn test_collect_descendants_from_map_single_child() {
        let mut children_map = HashMap::new();
        children_map.insert(100, vec![101]);

        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);
        assert_eq!(pids, vec![100, 101]);
    }

    #[test]
    fn test_collect_descendants_from_map_multiple_children() {
        let mut children_map = HashMap::new();
        children_map.insert(100, vec![101, 102, 103]);

        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);
        assert_eq!(pids, vec![100, 101, 102, 103]);
    }

    #[test]
    fn test_collect_descendants_from_map_nested() {
        // Tree: 100 -> 101 -> 102 -> 103
        let mut children_map = HashMap::new();
        children_map.insert(100, vec![101]);
        children_map.insert(101, vec![102]);
        children_map.insert(102, vec![103]);

        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);
        assert_eq!(pids, vec![100, 101, 102, 103]);
    }

    #[test]
    fn test_collect_descendants_from_map_branching() {
        // Tree: 100 -> [101, 102], 101 -> [103, 104], 102 -> [105]
        let mut children_map = HashMap::new();
        children_map.insert(100, vec![101, 102]);
        children_map.insert(101, vec![103, 104]);
        children_map.insert(102, vec![105]);

        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);

        // Should contain all PIDs
        assert!(pids.contains(&100));
        assert!(pids.contains(&101));
        assert!(pids.contains(&102));
        assert!(pids.contains(&103));
        assert!(pids.contains(&104));
        assert!(pids.contains(&105));
        assert_eq!(pids.len(), 6);
    }

    #[test]
    fn test_collect_descendants_unrelated_processes() {
        // Map has processes, but none are descendants of 100
        let mut children_map = HashMap::new();
        children_map.insert(200, vec![201, 202]);
        children_map.insert(300, vec![301]);

        let mut pids = vec![100];
        collect_descendants_from_map(100, &children_map, &mut pids);
        assert_eq!(pids, vec![100]);
    }
}
