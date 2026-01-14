//! Background Docker image pulling for TUI
//!
//! This module provides non-blocking Docker image pulls by running
//! `docker pull` in a background thread.

use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
pub struct PullRequest {
    pub session_id: String,
    pub image: String,
}

#[derive(Debug)]
pub struct PullResult {
    pub session_id: String,
    #[allow(dead_code)]
    pub image: String,
    pub success: bool,
    pub error: Option<String>,
}

pub struct ImagePuller {
    request_tx: mpsc::Sender<PullRequest>,
    result_rx: mpsc::Receiver<PullResult>,
    _handle: thread::JoinHandle<()>,
}

impl ImagePuller {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<PullRequest>();
        let (result_tx, result_rx) = mpsc::channel::<PullResult>();

        let handle = thread::spawn(move || {
            Self::pull_loop(request_rx, result_tx);
        });

        Self {
            request_tx,
            result_rx,
            _handle: handle,
        }
    }

    fn pull_loop(request_rx: mpsc::Receiver<PullRequest>, result_tx: mpsc::Sender<PullResult>) {
        while let Ok(request) = request_rx.recv() {
            let result = Self::do_pull(&request);

            if result_tx.send(result).is_err() {
                break;
            }
        }
    }

    fn do_pull(request: &PullRequest) -> PullResult {
        use std::process::Command;

        let output = Command::new("docker")
            .args(["pull", &request.image])
            .output();

        match output {
            Ok(out) if out.status.success() => PullResult {
                session_id: request.session_id.clone(),
                image: request.image.clone(),
                success: true,
                error: None,
            },
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                PullResult {
                    session_id: request.session_id.clone(),
                    image: request.image.clone(),
                    success: false,
                    error: Some(format!("Failed to pull image: {}", stderr.trim())),
                }
            }
            Err(e) => PullResult {
                session_id: request.session_id.clone(),
                image: request.image.clone(),
                success: false,
                error: Some(format!("Failed to run docker pull: {}", e)),
            },
        }
    }

    pub fn request_pull(&self, session_id: String, image: String) {
        let _ = self.request_tx.send(PullRequest { session_id, image });
    }

    pub fn try_recv_result(&self) -> Option<PullResult> {
        self.result_rx.try_recv().ok()
    }
}

impl Default for ImagePuller {
    fn default() -> Self {
        Self::new()
    }
}
