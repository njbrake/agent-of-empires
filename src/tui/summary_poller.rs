//! Background AI summary polling for terminal output
//!
//! Calls `claude -p` in a background thread to generate concise summaries
//! of what's happening in a terminal session.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

pub struct SummaryRequest {
    pub session_id: String,
    pub terminal_output: String,
}

pub struct SummaryResult {
    pub session_id: String,
    pub summary: String,
    pub is_error: bool,
}

pub struct SummaryCache {
    pub session_id: Option<String>,
    pub summary: String,
    pub is_loading: bool,
    pub is_error: bool,
    pub last_request: Instant,
}

impl Default for SummaryCache {
    fn default() -> Self {
        Self {
            session_id: None,
            summary: String::new(),
            is_loading: false,
            is_error: false,
            last_request: Instant::now(),
        }
    }
}

pub struct SummaryPoller {
    request_tx: mpsc::Sender<SummaryRequest>,
    result_rx: mpsc::Receiver<SummaryResult>,
    _handle: thread::JoinHandle<()>,
    in_flight: bool,
}

impl SummaryPoller {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<SummaryRequest>();
        let (result_tx, result_rx) = mpsc::channel::<SummaryResult>();

        let handle = thread::spawn(move || {
            Self::polling_loop(request_rx, result_tx);
        });

        Self {
            request_tx,
            result_rx,
            _handle: handle,
            in_flight: false,
        }
    }

    fn polling_loop(
        request_rx: mpsc::Receiver<SummaryRequest>,
        result_tx: mpsc::Sender<SummaryResult>,
    ) {
        const SYSTEM_PROMPT: &str = "Summarize what is happening in this terminal session in 2-3 concise sentences. Focus on the current activity, any errors, and progress. Be terse.";

        while let Ok(request) = request_rx.recv() {
            // Truncate to last 150 lines
            let lines: Vec<&str> = request.terminal_output.lines().collect();
            let truncated = if lines.len() > 150 {
                lines[lines.len() - 150..].join("\n")
            } else {
                request.terminal_output.clone()
            };

            let result = Command::new("claude")
                .args([
                    "-p",
                    "--model",
                    "claude-haiku-4-5-20251001",
                    "--system-prompt",
                    SYSTEM_PROMPT,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(truncated.as_bytes());
                    }
                    child.wait_with_output()
                });

            let summary_result = match result {
                Ok(output) if output.status.success() => {
                    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    SummaryResult {
                        session_id: request.session_id,
                        summary,
                        is_error: false,
                    }
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    SummaryResult {
                        session_id: request.session_id,
                        summary: format!("Summary failed: {}", stderr),
                        is_error: true,
                    }
                }
                Err(e) => SummaryResult {
                    session_id: request.session_id,
                    summary: format!("Could not run claude: {}", e),
                    is_error: true,
                },
            };

            if result_tx.send(summary_result).is_err() {
                break;
            }
        }
    }

    /// Request a summary (non-blocking). Skips if a request is already in-flight.
    pub fn request_summary(&mut self, request: SummaryRequest) {
        if self.in_flight {
            return;
        }
        if self.request_tx.send(request).is_ok() {
            self.in_flight = true;
        }
    }

    /// Try to receive a result without blocking. Returns None if no result yet.
    pub fn try_recv_result(&mut self) -> Option<SummaryResult> {
        match self.result_rx.try_recv().ok() {
            Some(result) => {
                self.in_flight = false;
                Some(result)
            }
            None => None,
        }
    }
}

impl Default for SummaryPoller {
    fn default() -> Self {
        Self::new()
    }
}
