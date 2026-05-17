//! AVK ofis başlat endpoint — `POST /api/avk/ofis-baslat`.
//!
//! Sabit, parametresiz subprocess çağırır:
//!   `/root/ajan-sistemi/apps/vps/scripts/avk-ofis-baslat`
//!
//! Script idempotent — mevcut session varsa atlar. Furkan tarayıcıdan
//! buton ile "tmux ölmüş, yeniden kur" akışını tetikler. UI 60s'lik
//! "kuruluyor" göstergesi sürer, endpoint tipik 30-60s sonra döner
//! (CLI boot bekleme + launcher inject + wait_for_cli_ready).
//!
//! Güvenlik: parametre yok, sabit path, shell yok (Command::new + args).

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::AppState;

const SCRIPT_PATH: &str = "/root/ajan-sistemi/apps/vps/scripts/avk-ofis-baslat";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Serialize)]
pub struct OfisBaslatResponse {
    pub ok: bool,
    pub script_path: String,
    pub elapsed_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub error: Option<String>,
}

pub async fn post_avk_ofis_baslat(State(_state): State<Arc<AppState>>) -> Response {
    let start = Instant::now();
    let result = tokio::task::spawn_blocking(|| run_script_with_timeout()).await;

    let elapsed_ms = start.elapsed().as_millis();

    match result {
        Ok(Ok((ok, stdout, stderr))) => Json(OfisBaslatResponse {
            ok,
            script_path: SCRIPT_PATH.to_string(),
            elapsed_ms,
            stdout_tail: tail_lines(&stdout, 20),
            stderr_tail: tail_lines(&stderr, 10),
            error: None,
        })
        .into_response(),
        Ok(Err(err)) => Json(OfisBaslatResponse {
            ok: false,
            script_path: SCRIPT_PATH.to_string(),
            elapsed_ms,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(err),
        })
        .into_response(),
        Err(err) => Json(OfisBaslatResponse {
            ok: false,
            script_path: SCRIPT_PATH.to_string(),
            elapsed_ms,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(format!("join: {err}")),
        })
        .into_response(),
    }
}

fn run_script_with_timeout() -> Result<(bool, String, String), String> {
    let mut child = Command::new(SCRIPT_PATH)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;

    let pid = child.id();
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| format!("try_wait: {e}"))? {
            Some(status) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stderr);
                }
                return Ok((status.success(), stdout, stderr));
            }
            None => {
                if start.elapsed() > SCRIPT_TIMEOUT {
                    let _ = child.kill();
                    return Err(format!(
                        "timeout {}s — pid {} kill",
                        SCRIPT_TIMEOUT.as_secs(),
                        pid
                    ));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}
