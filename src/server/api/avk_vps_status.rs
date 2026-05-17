//! AVK VPS sistem durum endpoint — `GET /api/avk/vps-status`.
//!
//! Daemon'ın çalıştığı host'un kompakt sistem metriklerini döner: hostname,
//! kernel, uptime, load average (1/5/15dk), bellek (toplam/kullanılan/%),
//! root disk doluluk %, CPU çekirdek sayısı. UI Dashboard "VPS Durum"
//! widget'ı 30s aralıkla render eder.
//!
//! Linux primary path: `/proc/loadavg`, `/proc/meminfo`, `/proc/uptime`,
//! `uname -nrs`, `df -P /`. macOS fallback `uptime` + `sysctl` + `df`. Mac
//! VPS değil — fallback "yok" badge çıkarır.

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::process::Command;
use std::sync::Arc;

use super::AppState;

#[derive(Serialize)]
pub struct AvkVpsStatusResponse {
    pub hostname: String,
    pub kernel: Option<String>,
    pub os: Option<String>,
    pub uptime_sec: Option<u64>,
    pub cpu_count: Option<usize>,
    /// 1, 5, 15 dakikalık load average — `None` Linux dışı.
    pub load_avg: Option<[f32; 3]>,
    pub memory: Option<MemoryStat>,
    pub disk: Option<DiskStat>,
}

#[derive(Serialize)]
pub struct MemoryStat {
    pub total_kb: u64,
    pub used_kb: u64,
    pub used_pct: u8,
}

#[derive(Serialize)]
pub struct DiskStat {
    pub mount: String,
    pub total_kb: u64,
    pub used_kb: u64,
    pub used_pct: u8,
}

pub async fn get_avk_vps_status(State(_state): State<Arc<AppState>>) -> Response {
    let hostname = read_hostname();
    let (os, kernel) = read_os_kernel();
    let uptime_sec = read_uptime();
    let cpu_count = read_cpu_count();
    let load_avg = read_loadavg();
    let memory = read_memory();
    let disk = read_disk_root();

    Json(AvkVpsStatusResponse {
        hostname,
        kernel,
        os,
        uptime_sec,
        cpu_count,
        load_avg,
        memory,
        disk,
    })
    .into_response()
}

fn read_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "bilinmiyor".to_string())
}

fn read_os_kernel() -> (Option<String>, Option<String>) {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|raw| {
            raw.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        });
    let kernel = Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (os, kernel)
}

fn read_uptime() -> Option<u64> {
    let raw = std::fs::read_to_string("/proc/uptime").ok()?;
    let secs: f64 = raw.split_whitespace().next()?.parse().ok()?;
    Some(secs as u64)
}

fn read_cpu_count() -> Option<usize> {
    std::thread::available_parallelism().ok().map(|n| n.get())
}

fn read_loadavg() -> Option<[f32; 3]> {
    let raw = std::fs::read_to_string("/proc/loadavg").ok()?;
    let mut parts = raw.split_whitespace();
    let a: f32 = parts.next()?.parse().ok()?;
    let b: f32 = parts.next()?.parse().ok()?;
    let c: f32 = parts.next()?.parse().ok()?;
    Some([a, b, c])
}

fn read_memory() -> Option<MemoryStat> {
    let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb: Option<u64> = None;
    let mut avail_kb: Option<u64> = None;
    for line in raw.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total_kb = parse_kb(v);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail_kb = parse_kb(v);
        }
        if total_kb.is_some() && avail_kb.is_some() {
            break;
        }
    }
    let total = total_kb?;
    let avail = avail_kb?;
    let used = total.saturating_sub(avail);
    let pct = if total > 0 {
        ((used as f64 / total as f64) * 100.0).round() as u8
    } else {
        0
    };
    Some(MemoryStat {
        total_kb: total,
        used_kb: used,
        used_pct: pct,
    })
}

fn parse_kb(v: &str) -> Option<u64> {
    v.trim()
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
}

fn read_disk_root() -> Option<DiskStat> {
    let out = Command::new("df").args(["-Pk", "/"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8(out.stdout).ok()?;
    // 2nd row: Filesystem  1024-blocks  Used  Available  Capacity  Mounted-on
    let row = stdout.lines().nth(1)?;
    let cols: Vec<&str> = row.split_whitespace().collect();
    if cols.len() < 6 {
        return None;
    }
    let total_kb: u64 = cols[1].parse().ok()?;
    let used_kb: u64 = cols[2].parse().ok()?;
    let pct = if total_kb > 0 {
        ((used_kb as f64 / total_kb as f64) * 100.0).round() as u8
    } else {
        0
    };
    Some(DiskStat {
        mount: cols[5].to_string(),
        total_kb,
        used_kb,
        used_pct: pct,
    })
}
