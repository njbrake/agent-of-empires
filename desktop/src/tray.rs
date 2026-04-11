//! System tray: menu bar icon, session count, remote toggle, QR popover, quit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_of_empires::server::StatusChange;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

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

    // Monochrome template icon for the menu bar. `icon_as_template(true)` tells
    // macOS to auto-invert the black pixels for dark/light menu bar backgrounds.
    // Tauri's Image::new wants raw RGBA8, so decode the PNG first.
    let tray_icon_bytes = include_bytes!("../icons/tray-icon.png");
    let tray_png = image::load_from_memory(tray_icon_bytes)?.to_rgba8();
    let (tw, th) = tray_png.dimensions();
    let tray_icon = tauri::image::Image::new_owned(tray_png.into_raw(), tw, th);

    let _tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .icon_as_template(true)
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

/// Open a popover window showing one QR code per detected network interface.
/// User can click a button to switch between LAN / Tailscale / Ethernet etc.
fn show_qr_popover<R: Runtime>(app: &AppHandle<R>, port: u16, token: &str) {
    close_qr_popover(app);

    let interfaces = crate::qr::detect_interfaces();
    if interfaces.is_empty() {
        tracing::warn!("No network interfaces detected; cannot show QR code");
        return;
    }

    // Pre-generate a QR code for each interface
    let mut entries: Vec<(String, String, String, String)> = Vec::new(); // (label, name, url, qr_data_uri)
    for iface in &interfaces {
        let url = format!("http://{}:{}/?token={}", iface.ip, port, token);
        let qr_data_uri = match crate::qr::generate_qr_data_uri(&url) {
            Ok(uri) => uri,
            Err(e) => {
                tracing::error!("QR generation failed for {}: {}", iface.ip, e);
                continue;
            }
        };
        entries.push((iface.label.clone(), iface.name.clone(), url, qr_data_uri));
    }

    if entries.is_empty() {
        tracing::error!("Failed to generate any QR codes");
        return;
    }

    // Build tab buttons and QR panes
    let mut tab_buttons = String::new();
    let mut qr_panes = String::new();
    for (i, (label, name, url, data_uri)) in entries.iter().enumerate() {
        let active = if i == 0 { " active" } else { "" };
        tab_buttons.push_str(&format!(
            r#"<button class="tab{active}" data-idx="{i}" onclick="pick({i})">{label}</button>"#,
            active = active,
            i = i,
            label = html_escape(label),
        ));
        qr_panes.push_str(&format!(
            r#"<div class="pane{active}" data-idx="{i}">
                 <img src="{qr}" alt="QR for {label}">
                 <div class="sub">{name} &middot; {ip}</div>
                 <div class="url" id="url-{i}">{url}</div>
                 <button class="copy" onclick="copyUrl({i})">Copy URL</button>
               </div>"#,
            active = active,
            i = i,
            qr = data_uri,
            label = html_escape(label),
            name = html_escape(name),
            ip = {
                // Extract host from the URL string for display
                url.split('/')
                    .nth(2)
                    .and_then(|hp| hp.split(':').next())
                    .unwrap_or("")
            },
            url = html_escape(url),
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Remote Access</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    background: #fff; color: #1a1a1a;
    padding: 16px; min-width: 320px;
  }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #1e1e1e; color: #e0e0e0; }}
  }}
  h3 {{
    font-size: 13px; font-weight: 600; text-align: center; margin-bottom: 10px;
  }}
  .tabs {{
    display: flex; gap: 4px; margin-bottom: 12px;
    overflow-x: auto; padding-bottom: 2px;
  }}
  .tab {{
    flex-shrink: 0; padding: 6px 10px; font-size: 11px;
    background: #f0f0f0; border: 1px solid #ddd; border-radius: 6px;
    color: #555; cursor: pointer; font-family: inherit; white-space: nowrap;
  }}
  .tab:hover {{ background: #e8e8e8; }}
  .tab.active {{
    background: #3b82f6; color: #fff; border-color: #3b82f6;
  }}
  @media (prefers-color-scheme: dark) {{
    .tab {{ background: #2a2a2a; border-color: #3a3a3a; color: #aaa; }}
    .tab:hover {{ background: #333; }}
    .tab.active {{ background: #3b82f6; color: #fff; }}
  }}
  .pane {{ display: none; align-items: center; flex-direction: column; }}
  .pane.active {{ display: flex; }}
  .pane img {{
    width: 220px; height: 220px; margin: 4px 0;
    background: #fff; padding: 4px; border-radius: 4px;
  }}
  .sub {{
    font-size: 10px; color: #888; margin: 4px 0;
  }}
  .url {{
    font-size: 11px; color: #666; word-break: break-all;
    text-align: center; padding: 8px; user-select: all;
    background: #f5f5f5; border-radius: 6px; margin: 8px 0 4px;
    max-width: 280px; font-family: ui-monospace, monospace;
  }}
  @media (prefers-color-scheme: dark) {{
    .url {{ background: #2a2a2a; color: #aaa; }}
  }}
  .copy {{
    padding: 6px 14px; font-size: 11px;
    background: #f0f0f0; border: 1px solid #ddd; border-radius: 6px;
    color: #555; cursor: pointer; font-family: inherit;
    margin-top: 4px;
  }}
  .copy:hover {{ background: #e8e8e8; }}
  .copy.copied {{ background: #10b981; color: #fff; border-color: #10b981; }}
  @media (prefers-color-scheme: dark) {{
    .copy {{ background: #2a2a2a; border-color: #3a3a3a; color: #aaa; }}
    .copy:hover {{ background: #333; }}
  }}
  .warning {{
    font-size: 10px; color: #e67e22; text-align: center;
    margin-top: 10px; padding: 0 8px;
  }}
</style>
</head>
<body>
  <h3>Scan to connect</h3>
  <div class="tabs">{tab_buttons}</div>
  {qr_panes}
  <div class="warning">Only use on trusted networks. Auth token travels in plaintext over HTTP.</div>
<script>
function pick(idx) {{
  document.querySelectorAll('.tab').forEach(t => t.classList.toggle('active', parseInt(t.dataset.idx) === idx));
  document.querySelectorAll('.pane').forEach(p => p.classList.toggle('active', parseInt(p.dataset.idx) === idx));
}}
function copyUrl(idx) {{
  const el = document.getElementById('url-' + idx);
  if (!el) return;
  const text = el.textContent;
  navigator.clipboard.writeText(text).then(() => {{
    const btn = document.querySelectorAll('.pane')[idx].querySelector('.copy');
    const orig = btn.textContent;
    btn.textContent = 'Copied!';
    btn.classList.add('copied');
    setTimeout(() => {{ btn.textContent = orig; btn.classList.remove('copied'); }}, 1500);
  }});
}}
</script>
</body>
</html>"#
    );

    // Write to temp file and load via file:// (WKWebView dislikes huge data: URLs)
    let tmp_path = std::env::temp_dir().join("aoe-qr-popover.html");
    if let Err(e) = std::fs::write(&tmp_path, html.as_bytes()) {
        tracing::error!("Failed to write QR popover HTML: {}", e);
        return;
    }

    let file_url = match url::Url::from_file_path(&tmp_path) {
        Ok(u) => u,
        Err(_) => {
            tracing::error!("Failed to build file:// URL for {}", tmp_path.display());
            return;
        }
    };

    match tauri::WebviewWindowBuilder::new(app, "qr-popover", tauri::WebviewUrl::External(file_url))
        .title("Remote Access")
        .inner_size(360.0, 480.0)
        .resizable(false)
        .decorations(true)
        .always_on_top(true)
        .center()
        .build()
    {
        Ok(_) => tracing::info!("QR popover opened with {} interface(s)", entries.len()),
        Err(e) => {
            tracing::error!("Failed to open QR popover: {}", e);
            eprintln!("Failed to open QR popover: {}", e);
        }
    }

    // Copy the first (default) URL to clipboard for convenience
    #[cfg(target_os = "macos")]
    if let Some((_, _, first_url, _)) = entries.first() {
        let _ = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    use std::io::Write;
                    let _ = stdin.write_all(first_url.as_bytes());
                }
                child.wait()
            });
    }
}

/// Minimal HTML escape for user-visible strings that we interpolate into the
/// popover template. Interface names and URLs should be safe but this prevents
/// any injection if the inputs ever contain special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Close the QR popover window if it exists.
fn close_qr_popover<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("qr-popover") {
        let _ = window.close();
    }
}
