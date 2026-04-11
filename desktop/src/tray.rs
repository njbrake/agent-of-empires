//! System tray: menu bar icon, session count, remote toggle, QR popover, quit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_of_empires::server::StatusChange;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

/// Set up the system tray icon with menu items and event handlers.
pub fn setup_tray<R: Runtime>(
    app: &AppHandle<R>,
    port: u16,
    token: String,
    remote_enabled: Arc<AtomicBool>,
    status_tx: tokio::sync::broadcast::Sender<Vec<StatusChange>>,
) -> anyhow::Result<()> {
    let sessions_item = MenuItemBuilder::new("No sessions")
        .enabled(false)
        .build(app)?;
    let remote_item = MenuItemBuilder::with_id("remote_toggle", "Remote Access: Off").build(app)?;
    let open_item = MenuItemBuilder::with_id("open_dashboard", "Open Dashboard").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&sessions_item)
        .separator()
        .item(&remote_item)
        .item(&open_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("Agent of Empires")
        .menu(&menu)
        .on_menu_event({
            let app_handle = app.clone();
            let remote_flag = remote_enabled.clone();
            let remote_menu_item = remote_item.clone();
            let tray_port = port;
            let tray_token = token.clone();
            move |_tray, event| match event.id().as_ref() {
                "remote_toggle" => {
                    let was_on = remote_flag.fetch_xor(true, Ordering::SeqCst);
                    let now_on = !was_on;

                    let label = if now_on {
                        "Remote Access: On"
                    } else {
                        "Remote Access: Off"
                    };
                    let _ = remote_menu_item.set_text(label);

                    if now_on {
                        show_qr_popover(&app_handle, tray_port, &tray_token);
                    } else {
                        close_qr_popover(&app_handle);
                    }
                }
                "open_dashboard" => {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    crate::cleanup_server_files();
                    app_handle.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    // Spawn background task: poll status changes, update tray tooltip + notifications
    let poll_handle = app.clone();
    let mut status_rx = status_tx.subscribe();
    let sessions_item_clone = sessions_item.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match status_rx.recv().await {
                Ok(changes) => {
                    update_tray_from_changes(&poll_handle, &sessions_item_clone, &changes);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    Ok(())
}

/// Update tray tooltip and fire notifications for attention-needed sessions.
fn update_tray_from_changes<R: Runtime>(
    app: &AppHandle<R>,
    _sessions_item: &tauri::menu::MenuItem<R>,
    changes: &[StatusChange],
) {
    for change in changes {
        if change.new_status.contains("Waiting") || change.new_status.contains("NeedsInput") {
            let _ = app
                .notification()
                .builder()
                .title("Agent of Empires")
                .body(format!(
                    "{} is waiting for input in {}",
                    change.title, change.project_path
                ))
                .show();
        }
    }
}

/// Open a frameless popover window showing a QR code for remote access.
fn show_qr_popover<R: Runtime>(app: &AppHandle<R>, port: u16, token: &str) {
    // Close existing popover if open
    close_qr_popover(app);

    let lan_ips = crate::qr::detect_lan_ips();
    if lan_ips.is_empty() {
        tracing::warn!("No LAN IP addresses detected; cannot show QR code");
        return;
    }

    let ip = lan_ips[0];
    let url = format!("http://{}:{}/?token={}", ip, port, token);
    let qr_data_uri = match crate::qr::generate_qr_data_uri(&url) {
        Ok(uri) => uri,
        Err(e) => {
            tracing::error!("Failed to generate QR code: {}", e);
            return;
        }
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    background: #fff; color: #1a1a1a;
    display: flex; flex-direction: column; align-items: center;
    padding: 16px; min-width: 280px;
  }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #1e1e1e; color: #e0e0e0; }}
  }}
  img {{ width: 200px; height: 200px; margin: 8px 0; }}
  .url {{
    font-size: 11px; color: #666; word-break: break-all;
    text-align: center; padding: 8px; user-select: all;
    background: #f5f5f5; border-radius: 6px; margin: 8px 0;
    max-width: 260px;
  }}
  @media (prefers-color-scheme: dark) {{
    .url {{ background: #2a2a2a; color: #aaa; }}
  }}
  .warning {{
    font-size: 10px; color: #e67e22; text-align: center;
    margin-top: 8px;
  }}
  h3 {{ font-size: 13px; font-weight: 600; }}
</style>
</head>
<body>
  <h3>Scan to connect</h3>
  <img src="{qr_data_uri}" alt="QR Code">
  <div class="url">{url}</div>
  <div class="warning">Only use on trusted networks (home, office).</div>
</body>
</html>"#
    );

    let data_url = format!(
        "data:text/html;base64,{}",
        crate::qr::base64_encode_bytes(html.as_bytes())
    );

    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "qr-popover",
        tauri::WebviewUrl::External(data_url.parse().unwrap()),
    )
    .title("Remote Access")
    .inner_size(300.0, 380.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .center()
    .build();

    // Copy URL to clipboard via the shell
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    use std::io::Write;
                    let _ = stdin.write_all(url.as_bytes());
                }
                child.wait()
            });
    }
}

/// Close the QR popover window if it exists.
fn close_qr_popover<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("qr-popover") {
        let _ = window.close();
    }
}
