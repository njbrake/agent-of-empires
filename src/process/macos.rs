//! macOS-specific process utilities

use std::process::Command;

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

/// Kill a process and all its descendants
/// Uses SIGTERM first, then SIGKILL after a short delay for stragglers
pub fn kill_process_tree(pid: u32) {
    // Collect all descendant PIDs first (children, grandchildren, etc.)
    let mut pids_to_kill = vec![pid];
    collect_descendants(pid, &mut pids_to_kill);

    // Kill in reverse order (children first, then parent) with SIGTERM
    for &p in pids_to_kill.iter().rev() {
        let _ = kill(Pid::from_raw(p as i32), Signal::SIGTERM);
    }

    // Brief pause to let processes handle SIGTERM
    std::thread::sleep(std::time::Duration::from_millis(50));

    // SIGKILL any survivors
    for &p in pids_to_kill.iter().rev() {
        if process_exists(p) {
            let _ = kill(Pid::from_raw(p as i32), Signal::SIGKILL);
        }
    }
}

/// Recursively collect all descendant PIDs of a process
fn collect_descendants(pid: u32, pids: &mut Vec<u32>) {
    // Use ps to find child processes
    // ps -o pid=,ppid= -A lists all processes with their PIDs and PPIDs
    let Ok(output) = Command::new("ps").args(["-o", "pid=,ppid=", "-A"]).output() else {
        return;
    };

    if !output.status.success() {
        return;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let (Ok(child_pid), Ok(ppid)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                if ppid == pid {
                    pids.push(child_pid);
                    // Recurse to find grandchildren
                    collect_descendants(child_pid, pids);
                }
            }
        }
    }
}

/// Check if a process still exists
fn process_exists(pid: u32) -> bool {
    // Use kill with signal 0 to check if process exists
    kill(Pid::from_raw(pid as i32), None).is_ok()
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
