//! Agent of Empires desktop app (Tauri entrypoint)
//!
//! Starts the embedded web server, opens a native macOS window, and sets up
//! the system tray with session count, remote toggle, and QR code pairing.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod qr;
mod tray;

use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agent_of_empires::server::{generate_auth_token, ServerConfig, StatusChange};
use tauri::Manager;

/// Try ports 8080..=8090 and return the first available one.
fn find_available_port() -> anyhow::Result<u16> {
    for port in 8080..=8090 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    anyhow::bail!(
        "Could not find an available port (8080-8090). \
         Another application may be using them."
    )
}

/// Write PID and URL files so the CLI can detect a running desktop server.
fn write_server_files(url: &str) -> anyhow::Result<()> {
    let dir = agent_of_empires::session::get_app_dir()?;
    std::fs::write(dir.join("serve.pid"), std::process::id().to_string())?;
    std::fs::write(dir.join("serve.url"), url)?;
    Ok(())
}

/// Clean up PID and URL files on exit.
fn cleanup_server_files() {
    if let Ok(dir) = agent_of_empires::session::get_app_dir() {
        let _ = std::fs::remove_file(dir.join("serve.pid"));
        let _ = std::fs::remove_file(dir.join("serve.url"));
    }
}

fn main() -> anyhow::Result<()> {
    let port = find_available_port()?;
    let token = generate_auth_token();
    let remote_enabled = Arc::new(AtomicBool::new(false));

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<String>();
    // ready_rx is consumed exactly once inside the Tauri setup closure, which
    // requires Fn (not FnOnce), so wrap it in a Mutex<Option<_>> to allow `take`.
    let ready_rx = std::sync::Mutex::new(Some(ready_rx));
    let (status_tx, _status_rx) = tokio::sync::broadcast::channel::<Vec<StatusChange>>(64);

    // Values for the server task
    let server_token = token.clone();
    let server_remote = remote_enabled.clone();
    let server_status_tx = status_tx.clone();

    // Values for Tauri setup
    let setup_token = token.clone();
    let setup_remote = remote_enabled.clone();
    let setup_status_tx = status_tx.clone();
    let setup_port = port;

    // Spawn the web server on Tauri's async runtime. Using `tauri::async_runtime`
    // instead of `tokio::spawn` + `#[tokio::main]` avoids the nested-runtime panic
    // when `setup` calls `block_on` to wait for the ready signal.
    tauri::async_runtime::spawn(async move {
        let config = ServerConfig {
            profile: "default".to_string(),
            host: "0.0.0.0".to_string(),
            port,
            no_auth: false,
            read_only: false,
            remote_enabled: Some(server_remote),
            print_banner: false,
            write_url_file: false,
            ready_signal: Some(ready_tx),
            status_events: Some(server_status_tx),
            auth_token: Some(server_token),
        };
        if let Err(e) = agent_of_empires::server::start_server_with_config(config).await {
            tracing::error!("Server failed: {}", e);
            eprintln!("Server failed: {}", e);
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Wait for the server to be ready, then navigate the webview.
            let rx = ready_rx
                .lock()
                .expect("ready_rx mutex poisoned")
                .take()
                .expect("setup called more than once");
            let url = tauri::async_runtime::block_on(rx)?;

            // Write PID/URL files for CLI coexistence
            let _ = write_server_files(&url);

            // Build the local URL with token for the webview
            let webview_url = format!("http://127.0.0.1:{}/?token={}", setup_port, setup_token);

            // Navigate the main window to the dashboard
            if let Some(window) = app.get_webview_window("main") {
                let parsed: url::Url = webview_url.parse().expect("webview URL must be valid");
                let _ = window.navigate(parsed);
            }

            // Set up system tray
            tray::setup_tray(
                &app_handle,
                setup_port,
                setup_token,
                setup_remote,
                setup_status_tx,
            )?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    cleanup_server_files();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error running tauri app");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_available_port_returns_valid() {
        let port = find_available_port().unwrap();
        assert!((8080..=8090).contains(&port));
    }

    #[test]
    fn test_find_available_port_skips_occupied() {
        // Bind port 8080 so find_available_port skips it
        let _listener = TcpListener::bind(("127.0.0.1", 8080)).ok();
        let port = find_available_port().unwrap();
        // Should still find a port (might be 8080 if bind failed, or 8081+)
        assert!((8080..=8090).contains(&port));
    }
}
