//! Input handling for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::Position;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::{live_send, DragKind, HomeView, PreviewSelection, TerminalMode, ViewMode};
use crate::session::config::{load_config, save_config, GroupByMode, SortOrder};
use crate::session::{list_profiles, repo_config, resolve_config_or_warn, Item, Status};
use crate::tui::app::Action;
#[cfg(feature = "serve")]
use crate::tui::dialogs::ServeAction;
use crate::tui::dialogs::{
    builtin_commands, CommandPaletteDialog, ConfirmDialog, DeleteDialogConfig, DialogResult,
    GroupDeleteOptionsDialog, HookTrustAction, HooksInstallDialog, InfoDialog, NewSessionData,
    NewSessionDialog, NoAgentsAction, PaletteAction, PaletteCommand, PaletteGroup,
    ProfilePickerAction, ProjectsDialog, RenameDialog, RenameMode, RestartDialog,
    SendMessageDialog, UnifiedDeleteDialog,
};
use crate::tui::diff::{DiffAction, DiffView};
use crate::tui::responsive;
use crate::tui::settings::{SettingsAction, SettingsView};

/// Maximum gap between two left-clicks on the same row that still
/// counts as a double-click. 400ms matches the default on most desktop
/// environments. Worth tuning if real-world feedback says it's too
/// fast for trackpads or too slow on remote sessions.
const DOUBLE_CLICK_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(400);

/// xterm bracketed-paste start sequence: `ESC [ 2 0 0 ~`. An agent that
/// has enabled bracketed paste mode (`\e[?2004h`) treats everything
/// between this marker and the matching end marker as one paste rather
/// than as keystrokes, so interior newlines accumulate in the input
/// buffer instead of firing `submit` per line.
const BRACKETED_PASTE_START: &[u8] = &[0x1b, b'[', b'2', b'0', b'0', b'~'];

/// xterm bracketed-paste end sequence: `ESC [ 2 0 1 ~`. Pairs with
/// [`BRACKETED_PASTE_START`].
const BRACKETED_PASTE_END: &[u8] = &[0x1b, b'[', b'2', b'0', b'1', b'~'];

/// Decompose pasted text into a series of `TmuxKey`s safe for the
/// live-send worker to dispatch.
///
/// Single-line pastes (no `\n` / `\r`) skip the bracketed-paste
/// wrapping and travel as a single `Literal`: a bare shell or any
/// agent that hasn't enabled `\e[?2004h` keeps working unchanged,
/// because we never emit the escape markers it would render as
/// literal text. Tabs in single-line pastes still go through as
/// `Named("Tab")` to mirror the historical path.
///
/// Multi-line pastes get wrapped in xterm bracketed-paste markers
/// (`\e[200~` / `\e[201~`) so the receiving agent sees the entire
/// payload as one paste rather than as N independent Enter keypresses.
/// Without the wrapping, agents that submit on Enter (Claude Code,
/// Codex, OpenCode, ...) post one user message per pasted line, which
/// is the bug behind #1546. The whole payload (markers, printable
/// runs, interior CRs, and tabs) goes through as a single `HexBytes`,
/// so the worker fires one `tmux send-keys -H` subprocess per paste
/// instead of one per chunk. `\r\n` pairs coalesce to a single CR so
/// Windows-line-ending pastes don't double up; other control bytes
/// (BEL, ESC, ...) are dropped rather than risk that an embedded
/// escape closes the bracketed-paste sequence on the agent's side.
pub(super) fn split_paste_for_live_send(text: &str) -> Vec<live_send::TmuxKey> {
    let has_newline = text.contains('\n') || text.contains('\r');
    if !has_newline {
        return split_inline_paste(text);
    }
    split_bracketed_paste(text)
}

fn split_inline_paste(text: &str) -> Vec<live_send::TmuxKey> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        let is_control = (ch as u32) < 0x20 || ch == '\x7f';
        if !is_control {
            buf.push(ch);
            continue;
        }
        if !buf.is_empty() {
            out.push(live_send::TmuxKey::Literal(std::mem::take(&mut buf)));
        }
        if ch == '\t' {
            out.push(live_send::TmuxKey::Named("Tab".to_string()));
        }
        // BEL, ESC, etc.: dropping is friendlier than mapping
        // to a named key that could cancel the agent's input.
    }
    if !buf.is_empty() {
        out.push(live_send::TmuxKey::Literal(buf));
    }
    out
}

fn split_bracketed_paste(text: &str) -> Vec<live_send::TmuxKey> {
    // Build one contiguous byte payload: start marker, then the paste
    // content with printables as their UTF-8 bytes / interior newlines
    // as CR (0x0d) / tabs as 0x09, then the end marker. Sending it as
    // one `HexBytes` means the worker fires exactly one `tmux send-keys
    // -H` subprocess per paste rather than one per chunk.
    let mut bytes = Vec::with_capacity(text.len() + BRACKETED_PASTE_START.len() * 2);
    bytes.extend_from_slice(BRACKETED_PASTE_START);

    let mut chars = text.chars().peekable();
    let mut utf8_buf = [0u8; 4];
    while let Some(ch) = chars.next() {
        match ch {
            '\n' => bytes.push(0x0d),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                bytes.push(0x0d);
            }
            '\t' => bytes.push(0x09),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                // Embedded ESC / BEL / etc. has no safe encoding
                // inside the paste payload; drop rather than risk a
                // bogus terminal escape closing the paste early.
            }
            c => {
                let s = c.encode_utf8(&mut utf8_buf);
                bytes.extend_from_slice(s.as_bytes());
            }
        }
    }

    bytes.extend_from_slice(BRACKETED_PASTE_END);
    vec![live_send::TmuxKey::HexBytes(bytes)]
}

fn resolve_hook_install_agent(
    tool_name: &str,
    session_config: &crate::session::config::SessionConfig,
) -> Option<&'static crate::agents::AgentDef> {
    crate::agents::get_agent(tool_name)
        .or_else(|| {
            session_config
                .agent_detect_as
                .get(tool_name)
                .and_then(|detect_as| crate::agents::get_agent(detect_as))
        })
        .filter(|agent| agent.hook_config.is_some())
}

pub(super) fn parse_hotkey(s: &str) -> Option<(KeyCode, KeyModifiers)> {
    let (modifier, key) = s.split_once('+')?;
    if !modifier.eq_ignore_ascii_case("alt") {
        return None;
    }
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some((KeyCode::Char(ch.to_ascii_lowercase()), KeyModifiers::ALT))
}

/// Validate tool hotkey strings, returning a list of human-readable warning
/// lines for any that fail to parse. Tool hotkeys must match the
/// `Alt+<single-char>` shape; everything else is rejected so the user gets
/// a clear error rather than a silently dead binding.
pub(super) fn validate_tool_hotkeys(
    tools: &std::collections::HashMap<String, crate::session::config::ToolSessionConfig>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for (name, config) in tools {
        if let Some(ref hotkey) = config.hotkey {
            if parse_hotkey(hotkey).is_none() {
                let msg = format!(
                    "Tool '{}': invalid hotkey '{}' (expected format: Alt+<letter>)",
                    name, hotkey
                );
                tracing::warn!("{}", msg);
                warnings.push(msg);
            }
        }
    }
    warnings
}

/// Build a sorted lookup list of `(tool_name, KeyCode, KeyModifiers)` for
/// every tool whose `hotkey` parses. Sorted by tool name so the
/// alphabetically-first tool wins on duplicate hotkeys. Built once at
/// startup and on settings reload, then iterated on every keystroke;
/// keep it cheap.
pub(super) fn build_tool_hotkey_cache(
    tools: &std::collections::HashMap<String, crate::session::config::ToolSessionConfig>,
) -> Vec<(String, KeyCode, KeyModifiers)> {
    let mut sorted: Vec<_> = tools.iter().collect();
    sorted.sort_by_key(|(name, _)| name.to_owned());
    sorted
        .into_iter()
        .filter_map(|(name, config)| {
            let hotkey_str = config.hotkey.as_deref()?;
            let (code, modifiers) = parse_hotkey(hotkey_str)?;
            Some((name.clone(), code, modifiers))
        })
        .collect()
}

impl HomeView {
    pub fn is_diff_open(&self) -> bool {
        self.diff_view.is_some()
    }

    pub fn hit_preview(&self, col: u16, row: u16) -> bool {
        self.preview_area.contains(Position::from((col, row)))
    }

    pub fn hit_diff(&self, col: u16, row: u16) -> bool {
        self.diff_area.contains(Position::from((col, row)))
    }

    fn open_tool_picker(&mut self) {
        self.tool_picker_dialog = Some(crate::tui::dialogs::ToolPickerDialog::new(
            &self.tool_configs,
        ));
    }

    /// Check if the key event matches any configured tool session hotkey.
    /// On duplicate hotkeys, the alphabetically-first tool name wins
    /// (the cache is built sorted by tool name).
    fn match_tool_hotkey(&self, key: &KeyEvent) -> Option<String> {
        for (name, code, modifiers) in &self.tool_hotkey_cache {
            if key.code == *code && key.modifiers == *modifiers {
                return Some(name.clone());
            }
        }
        None
    }

    pub fn hit_list(&self, col: u16, row: u16) -> bool {
        self.list_area.contains(Position::from((col, row)))
    }

    /// True when `(col, row)` lands on the side-by-side list/preview
    /// divider. The divider is the preview's left border column (one
    /// past `list_area.right()` is exclusive, so this *is* a valid hit
    /// target that hit_list / hit_preview both miss by design). Returns
    /// `false` in stacked mode, in the diff/settings/serve takeover
    /// views (which clear `divider_col`), and while any modal dialog is
    /// open, so a dialog over the divider swallows stray clicks rather
    /// than starting a hidden drag.
    pub fn hit_divider(&self, col: u16, row: u16) -> bool {
        if self.has_dialog() {
            return false;
        }
        let Some(div_col) = self.divider_col else {
            return false;
        };
        if col != div_col {
            return false;
        }
        let list_y = self.list_area.y;
        let list_bottom = self.list_area.bottom();
        row >= list_y && row < list_bottom
    }

    /// Begin a drag if `(col, row)` is on the divider, or inside the
    /// preview pane (whenever the pane is on screen, in or out of live
    /// mode). Returns true when a drag actually started, so the caller
    /// can mark the event handled and skip the row-click path.
    ///
    /// Divider drags resize the list/preview split (the only kind we
    /// had before live-send shipped). Preview-pane drags start an
    /// in-app text selection: terminal-native drag-select can't reach
    /// the preview because we capture mouse events to support wheel
    /// scroll, and we want one mechanism that also works on Mosh and
    /// mobile clients where Shift-bypass does nothing.
    pub fn handle_drag_start(&mut self, col: u16, row: u16) -> bool {
        if self.hit_divider(col, row) {
            self.drag_state = Some(DragKind::ListDivider {
                start_col: col,
                start_width: self.list_width,
            });
            return true;
        }
        // Modals that aren't live-send sit over the preview, so a
        // click inside `preview_area` while one is open is meant for
        // the modal underneath and should not seed a hidden selection
        // behind it. `handle_drag_move`'s cancel branch covers a modal
        // that opens mid-drag; this guards the start.
        if self.has_non_live_send_overlay() {
            return false;
        }
        if self.hit_preview(col, row) {
            self.preview_selection = Some(PreviewSelection {
                anchor: (col, row),
                extent: (col, row),
                finalized: false,
            });
            self.drag_state = Some(DragKind::PreviewSelect);
            return true;
        }
        false
    }

    /// Apply a drag-in-progress event. For the list divider, recompute
    /// the requested width from `(start_width + delta)` and clamp to
    /// `[10, main_area_width - PREVIEW_MIN_WIDTH]` so the preview keeps
    /// its usability floor and the value never wraps `u16`. For a
    /// preview-pane text selection, clamp the extent to the preview
    /// area and stash it on `preview_selection`; the renderer reads it
    /// each frame to paint the highlight.
    ///
    /// Returns true when state actually changed (so the caller
    /// redraws). The drag does NOT persist on every tick; the divider
    /// path saves on release, the preview-select path emits OSC 52 on
    /// release.
    pub fn handle_drag_move(&mut self, col: u16, row: u16) -> bool {
        // A dialog opened mid-drag (e.g. a hotkey pressed while the
        // mouse button is still held) shouldn't keep updating the
        // sidebar invisibly under the modal. End the drag here so the
        // next `Up(Left)` is a no-op, persisting whatever width the
        // user dragged to before the dialog covered it (handle_drag_end
        // is the normal save site, so we mirror its behavior).
        //
        // `has_dialog()` returns true while live-send is active, which
        // is exactly when preview drag-select is meant to work. Live
        // mode is therefore exempt — but a real modal (info / confirm
        // / palette / picker) opening mid-select must also kill the
        // drag and drop the selection so it can't keep mutating under
        // the modal or finalize on mouse-up behind the overlay.
        let drag_is_preview = matches!(self.drag_state, Some(DragKind::PreviewSelect));
        let cancel_drag = if drag_is_preview {
            self.has_non_live_send_overlay()
        } else {
            self.has_dialog()
        };
        if cancel_drag && self.drag_state.is_some() {
            self.drag_state = None;
            if drag_is_preview {
                self.preview_selection = None;
                self.preview_copy_pending = false;
                self.preview_copy_text = None;
            } else {
                self.save_list_width();
            }
            return false;
        }
        match self.drag_state {
            Some(DragKind::ListDivider {
                start_col,
                start_width,
            }) => {
                // i32 arithmetic so a leftward drag past the start column doesn't
                // underflow u16 before the clamp.
                let delta = col as i32 - start_col as i32;
                let proposed = start_width as i32 + delta;

                // Clamp ceiling tracks the live viewport width; if the user
                // resized the terminal mid-drag, the new width is honored. The
                // floor of 10 matches the keyboard `<` shrink limit.
                let ceiling = self
                    .main_area_width
                    .saturating_sub(responsive::PREVIEW_MIN_WIDTH);
                let max_width = ceiling.max(10);
                let clamped = proposed.clamp(10, max_width as i32) as u16;

                if clamped == self.list_width {
                    return false;
                }
                self.list_width = clamped;
                true
            }
            Some(DragKind::PreviewSelect) => {
                let Some(sel) = self.preview_selection.as_mut() else {
                    return false;
                };
                // Clamp the drag extent to the preview pane so a
                // mouse-out below the pane (very common: users drag down
                // through the last visible row, expecting the selection
                // to stop at the bottom) doesn't try to highlight
                // chrome rows on neighbouring widgets.
                let area = self.preview_area;
                if area.width == 0 || area.height == 0 {
                    return false;
                }
                let max_x = area.right().saturating_sub(1);
                let max_y = area.bottom().saturating_sub(1);
                let clamped_x = col.clamp(area.x, max_x);
                let clamped_y = row.clamp(area.y, max_y);
                let new_extent = (clamped_x, clamped_y);
                if sel.extent == new_extent {
                    return false;
                }
                sel.extent = new_extent;
                true
            }
            None => false,
        }
    }

    /// End any active drag. For the list divider, persist the final
    /// `list_width` to config so the new layout survives a restart.
    /// For a preview-pane selection, mark it `finalized` so the
    /// renderer keeps the highlight visible until the user dismisses
    /// it. The actual clipboard copy is the caller's job (see
    /// `app.rs`) so we can keep this method side-effect-free besides
    /// state, which is what the existing divider path does too.
    ///
    /// Returns true when a drag was actually in progress, so the
    /// caller can avoid a spurious redraw on every `Up(Left)` that
    /// wasn't part of a drag.
    pub fn handle_drag_end(&mut self) -> bool {
        let Some(state) = self.drag_state.take() else {
            return false;
        };
        match state {
            DragKind::ListDivider { .. } => {
                self.save_list_width();
            }
            DragKind::PreviewSelect => {
                // A bare click (no movement between Down and Up) collapses
                // anchor == extent. Treat that as "no selection" so a stray
                // click doesn't paint a 1x1 highlight or copy a single
                // character to the clipboard. Genuine multi-cell drags
                // get finalized so the renderer keeps the highlight visible
                // until dismissed; the next render also captures the
                // selected cells so the app loop can write them to the
                // user's clipboard once the buffer is drawn.
                if let Some(sel) = self.preview_selection {
                    if sel.anchor == sel.extent {
                        self.preview_selection = None;
                    } else if let Some(s) = self.preview_selection.as_mut() {
                        s.finalized = true;
                        self.preview_copy_pending = true;
                    }
                }
            }
        }
        true
    }

    /// Whether `drag_state` is currently a PreviewSelect (vs. a divider
    /// drag or nothing). Used by the Down(Left) handler in `app.rs` to
    /// tell whether `handle_drag_start` just installed a fresh selection
    /// or a divider drag.
    pub fn is_preview_select_dragging(&self) -> bool {
        matches!(self.drag_state, Some(DragKind::PreviewSelect))
    }

    /// Read the characters underneath the current preview selection
    /// from the rendered frame buffer, joined into a tmux-style flow
    /// string. Called from `paint_preview_selection` on the render
    /// that follows `handle_drag_end`, when `preview_copy_pending`
    /// is set and the buffer still holds the cells the user dragged
    /// over.
    ///
    /// The frame buffer is the authoritative source: it carries exactly
    /// what the user sees, with ansi-to-tui decoding and scroll already
    /// applied. Reading the parsed `Text` upstream of the renderer would
    /// duplicate the wrap math and skew when the preview is mid-scroll.
    pub(super) fn extract_preview_selection_text(
        &self,
        buffer: &ratatui::buffer::Buffer,
    ) -> Option<String> {
        let sel = self.preview_selection?;
        let preview = self.preview_area;
        let buf_area = buffer.area;
        let preview = preview.intersection(buf_area);
        if preview.width == 0 || preview.height == 0 {
            return None;
        }
        let ((start_col, start_row), (end_col, end_row)) = sel.ordered();
        if start_row == end_row && start_col == end_col {
            return None;
        }
        let preview_right_excl = preview.right();
        let preview_left = preview.x;
        let mut out = String::new();
        for row in start_row..=end_row {
            if row < preview.y || row >= preview.bottom() {
                if row < end_row {
                    out.push('\n');
                }
                continue;
            }
            let row_start_col = if row == start_row {
                start_col.max(preview_left)
            } else {
                preview_left
            };
            let row_end_excl = if row == end_row {
                end_col.saturating_add(1).min(preview_right_excl)
            } else {
                preview_right_excl
            };
            if row_end_excl <= row_start_col {
                if row < end_row {
                    out.push('\n');
                }
                continue;
            }
            let mut line = String::new();
            for col in row_start_col..row_end_excl {
                line.push_str(buffer[(col, row)].symbol());
            }
            // Trim only trailing whitespace per row, not leading: a
            // selection over indented code keeps the indentation,
            // while padding at the right edge of the preview (the
            // common case for unfilled rows) doesn't bloat the paste.
            out.push_str(line.trim_end());
            if row < end_row {
                out.push('\n');
            }
        }
        if out.chars().all(char::is_whitespace) {
            return None;
        }
        Some(out)
    }

    /// Drain the text captured on the last render that painted a
    /// finalized preview selection. Returns `Some` exactly once per
    /// finalized drag — `App` calls this immediately after the draw
    /// to write the bytes to the clipboard.
    pub fn take_preview_copy_text(&mut self) -> Option<String> {
        self.preview_copy_text.take()
    }

    /// Discard any in-flight preview selection. Called from key/click
    /// paths so the user dismisses the highlight by interacting with
    /// the TUI again. Returns true when state actually changed (so the
    /// caller can redraw).
    pub fn clear_preview_selection(&mut self) -> bool {
        if self.preview_selection.take().is_some() {
            // Cancel any in-progress drag too so the next Up(Left)
            // doesn't re-finalize a stale selection.
            if matches!(self.drag_state, Some(DragKind::PreviewSelect)) {
                self.drag_state = None;
            }
            // A pending capture from a previous finalized drag is
            // moot once the selection is gone; drop it so the next
            // selection starts clean.
            self.preview_copy_pending = false;
            self.preview_copy_text = None;
            true
        } else {
            false
        }
    }

    /// Route a left-click into whichever modal dialog supports mouse
    /// input. Returns true when the click was consumed, in which case
    /// the caller must NOT fall through to the divider / preview / list
    /// handlers. Two cases both count as consumed: the dialog acted on
    /// the click (Yes/No), and the click landed elsewhere on screen
    /// while a modal is open (the modal absorbs it).
    ///
    /// Only the destructive delete dialog wires Yes/No clicks today;
    /// the existing modals continue to swallow mouse events implicitly
    /// via `has_dialog()` gates in the other handlers.
    pub fn handle_dialog_click(&mut self, col: u16, row: u16) -> bool {
        if let Some(dialog) = &mut self.unified_delete_dialog {
            if let Some(result) = dialog.handle_click(col, row) {
                match result {
                    DialogResult::Continue => {}
                    DialogResult::Cancel => {
                        self.unified_delete_dialog = None;
                    }
                    DialogResult::Submit(options) => {
                        self.unified_delete_dialog = None;
                        if let Err(e) = self.delete_selected(&options) {
                            tracing::error!(target: "tui.input", "Failed to delete session: {}", e);
                        }
                    }
                }
                return true;
            }
            // Click landed inside the dialog area but missed both
            // buttons (e.g. on a checkbox or the title): swallow it so
            // the underlying list doesn't shift selection out from
            // under the modal.
            return true;
        }
        // Other dialogs also need to swallow clicks while open. The
        // existing `has_dialog()` gates inside list / preview / divider
        // handlers already do this, so no extra work here.
        false
    }

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
        // Any keystroke drops a finalized preview-pane selection. The
        // highlight pins to cell coords, so as soon as the user starts
        // doing anything else (navigating the list, opening a dialog,
        // typing through live-send, etc.) the cells underneath can
        // change and the highlight would point at unrelated content.
        // Doing the clear here covers both the live-send branch below
        // and the regular home-view path.
        self.clear_preview_selection();

        // Live-send capture wins over every other key handler. While
        // `live_send` is `Some` the home view is acting as a thin relay
        // to the target pane; dialog hotkeys, search, and list navigation
        // all suspend until the user exits with Ctrl+q.
        if self.live_send.is_some() {
            self.handle_live_send_key(key);
            return None;
        }

        // Handle unsaved changes confirmation for settings (shown over settings view)
        if self.settings_close_confirm {
            if let Some(dialog) = &mut self.confirm_dialog {
                match dialog.handle_key(key) {
                    DialogResult::Continue => return None,
                    DialogResult::Cancel => {
                        // User chose not to discard, go back to settings
                        self.confirm_dialog = None;
                        self.settings_close_confirm = false;
                        return None;
                    }
                    DialogResult::Submit(_) => {
                        // User chose to discard changes
                        if let Some(ref mut settings) = self.settings_view {
                            settings.force_close();
                        }
                        self.settings_view = None;
                        self.confirm_dialog = None;
                        self.settings_close_confirm = false;
                        let config = resolve_config_or_warn(&self.config_profile());
                        let theme_name = if config.theme.name.is_empty() {
                            "default".to_string()
                        } else {
                            config.theme.name
                        };
                        return Some(Action::SetTheme(theme_name));
                    }
                }
            }
        }

        // Handle settings view (full-screen takeover)
        if let Some(ref mut settings) = self.settings_view {
            match settings.handle_key(key) {
                SettingsAction::Continue => {
                    return None;
                }
                SettingsAction::Close => {
                    self.settings_view = None;
                    // Refresh config-dependent state in case settings changed
                    self.refresh_from_config();
                    // Reload theme from saved config
                    let config = resolve_config_or_warn(&self.config_profile());
                    let theme_name = if config.theme.name.is_empty() {
                        "default".to_string()
                    } else {
                        config.theme.name
                    };
                    return Some(Action::SetTheme(theme_name));
                }
                SettingsAction::UnsavedChangesWarning => {
                    // Show confirmation dialog
                    self.confirm_dialog = Some(ConfirmDialog::new(
                        "Unsaved Changes",
                        "You have unsaved changes. Discard them?",
                        "discard_settings",
                    ));
                    self.settings_close_confirm = true;
                    return None;
                }
                SettingsAction::PreviewTheme(name) => {
                    return Some(Action::SetTheme(name));
                }
            }
        }

        // Handle diff view (full-screen takeover)
        if let Some(ref mut diff_view) = self.diff_view {
            let action = diff_view.handle_key(key);
            if let Some((session_id, new_override)) = diff_view.take_pending_override() {
                if let Err(e) = self.apply_user_action(&session_id, |inst| {
                    inst.base_branch_override = new_override.clone();
                }) {
                    tracing::warn!(
                        target: "tui.home",
                        "Failed to persist base_branch_override: {}",
                        e
                    );
                }
            }
            match action {
                DiffAction::Continue => return None,
                DiffAction::Close => {
                    self.diff_view = None;
                    return None;
                }
                DiffAction::EditFile(path) => {
                    return Some(Action::EditFile(path));
                }
            }
        }

        // Handle serve view (full-screen takeover)
        #[cfg(feature = "serve")]
        if let Some(ref mut serve) = self.serve_view {
            match serve.handle_key(key) {
                ServeAction::Continue => return None,
                ServeAction::Close => {
                    self.serve_view = None;
                    return None;
                }
            }
        }

        // Handle no-agents dialog (highest priority, blocks all interaction)
        if let Some(dialog) = &mut self.no_agents_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(NoAgentsAction::Quit) => {
                    return Some(Action::Quit);
                }
                DialogResult::Submit(NoAgentsAction::Recheck) => {
                    let tools = crate::tmux::AvailableTools::detect();
                    if tools.any_available() {
                        self.set_available_tools(tools);
                        self.no_agents_dialog = None;
                    }
                    // If still no agents, keep dialog open (user can try again)
                }
            }
            return None;
        }

        // Handle welcome/changelog dialogs first (highest priority)
        if let Some(dialog) = &mut self.welcome_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.welcome_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.changelog_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.changelog_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.info_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.info_dialog = None;
                    if let Some(session_id) = self.pending_attach_after_warning.take() {
                        return Some(Action::AttachSession(session_id));
                    }
                }
            }
            return None;
        }

        // Command palette captures input ahead of the help overlay so its own
        // Esc/Enter/text keys reach it without going through the action match.
        if let Some(palette) = &mut self.command_palette {
            match palette.handle_key(key) {
                DialogResult::Continue => return None,
                DialogResult::Cancel => {
                    self.command_palette = None;
                    return None;
                }
                DialogResult::Submit(action) => {
                    self.command_palette = None;
                    return self.dispatch_palette_action(action, update_info);
                }
            }
        }

        // Handle tool picker dialog
        if let Some(picker) = &mut self.tool_picker_dialog {
            match picker.handle_key(key) {
                DialogResult::Continue => return None,
                DialogResult::Cancel => {
                    self.tool_picker_dialog = None;
                    return None;
                }
                DialogResult::Submit(tool_name) => {
                    self.tool_picker_dialog = None;
                    self.view_mode = ViewMode::Tool(tool_name);
                    self.preview_scroll_offset = 0;
                    self.tool_preview_cache = super::PreviewCache::default();
                    return None;
                }
            }
        }

        if let Some(dialog) = &mut self.snooze_duration_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.snooze_duration_dialog = None;
                    self.pending_snooze_session = None;
                }
                DialogResult::Submit(minutes) => {
                    self.snooze_duration_dialog = None;
                    let sid = self.pending_snooze_session.take();
                    if let Some(id) = sid {
                        if let Err(e) = self.snooze_session_for(&id, minutes) {
                            tracing::error!("snooze_session_for failed: {}", e);
                        }
                    }
                }
            }
            return None;
        }

        // Handle other dialog input
        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.show_help = false;
                    self.help_scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    self.help_scroll = self.help_scroll.saturating_add(10);
                }
                KeyCode::PageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(10);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    self.help_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    // u16::MAX overshoots intentionally; HelpOverlay::render
                    // clamps to the actual max scroll for the current layout.
                    self.help_scroll = u16::MAX;
                }
                _ => {}
            }
            return None;
        }

        if let Some(dialog) = &mut self.hooks_install_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.hooks_install_dialog = None;
                    self.pending_hooks_install_data = None;
                }
                DialogResult::Submit(_) => {
                    self.hooks_install_dialog = None;
                    // Persist the acknowledgment
                    if let Ok(mut config) =
                        crate::session::config::load_config().map(|c| c.unwrap_or_default())
                    {
                        config.app_state.has_acknowledged_agent_hooks = true;
                        if let Err(e) = crate::session::config::save_config(&config) {
                            tracing::warn!(target: "tui.input", "Failed to save config: {e}");
                        }
                    }
                    // Resume session creation
                    if let Some(data) = self.pending_hooks_install_data.take() {
                        return self.continue_session_creation(data);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.hook_trust_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.hook_trust_dialog = None;
                    self.pending_hook_trust_data = None;
                }
                DialogResult::Submit(action) => {
                    self.hook_trust_dialog = None;
                    if let Some(data) = self.pending_hook_trust_data.take() {
                        match action {
                            HookTrustAction::Trust {
                                hooks,
                                hooks_hash,
                                project_path,
                            } => {
                                if let Err(e) = repo_config::trust_repo(
                                    std::path::Path::new(&project_path),
                                    &hooks_hash,
                                ) {
                                    tracing::error!(target: "tui.input", "Failed to trust repo: {}", e);
                                }
                                let merged =
                                    repo_config::merge_hooks_with_config(&data.profile, hooks);
                                return self.create_session_with_hooks(data, merged);
                            }
                            HookTrustAction::Skip => {
                                let fallback =
                                    repo_config::resolve_global_profile_hooks(&data.profile);
                                return self.create_session_with_hooks(data, fallback);
                            }
                        }
                    }
                }
            }
            return None;
        }

        let dialog_result = self
            .new_dialog
            .as_mut()
            .map(|dialog| dialog.handle_key(key));

        if let Some(result) = dialog_result {
            match result {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    // If creation is pending, mark it as cancelled
                    if self.is_creation_pending() {
                        self.cancel_creation();
                    } else {
                        self.new_dialog = None;
                    }
                }
                DialogResult::Submit(data) => {
                    // Check if the tool uses hooks and user hasn't acknowledged yet
                    let tool_name = if data.tool.is_empty() {
                        "claude".to_string()
                    } else {
                        data.tool.clone()
                    };

                    let resolved_config = resolve_config_or_warn(&data.profile);
                    if let Some(hook_agent) =
                        resolve_hook_install_agent(&tool_name, &resolved_config.session)
                    {
                        let config = crate::session::config::load_config().ok().flatten();
                        let hooks_enabled = resolved_config.session.agent_status_hooks;
                        let acknowledged = config
                            .as_ref()
                            .map(|c| c.app_state.has_acknowledged_agent_hooks)
                            .unwrap_or(false);

                        if hooks_enabled && !acknowledged {
                            self.hooks_install_dialog = Some(HooksInstallDialog::new_for_profile(
                                hook_agent.name,
                                Some(&data.profile),
                            ));
                            self.pending_hooks_install_data = Some(data);
                            return None;
                        }
                    }

                    return self.continue_session_creation(data);
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.confirm_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.confirm_dialog = None;
                    self.pending_stop_session = None;
                    self.pending_force_remove_session = None;
                }
                DialogResult::Submit(_) => {
                    let action = dialog.action().to_string();
                    self.confirm_dialog = None;
                    if action == "delete_group" {
                        if let Err(e) = self.delete_selected_group() {
                            tracing::error!(target: "tui.input", "Failed to delete group: {}", e);
                        }
                    } else if action == "stop_session" {
                        if let Some(session_id) = self.pending_stop_session.take() {
                            return Some(Action::StopSession(session_id));
                        }
                    } else if action == "force_remove_session" {
                        if let Some(session_id) = self.pending_force_remove_session.take() {
                            if let Err(e) = self.force_remove_session(&session_id) {
                                tracing::error!(target: "tui.input", "Failed to force remove session: {}", e);
                            }
                        }
                    } else if action == "quit_during_creation" {
                        return Some(Action::Quit);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.unified_delete_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.unified_delete_dialog = None;
                }
                DialogResult::Submit(options) => {
                    self.unified_delete_dialog = None;
                    if let Err(e) = self.delete_selected(&options) {
                        tracing::error!(target: "tui.input", "Failed to delete session: {}", e);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.group_delete_options_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.group_delete_options_dialog = None;
                }
                DialogResult::Submit(options) => {
                    self.group_delete_options_dialog = None;
                    if options.delete_sessions {
                        if let Err(e) = self.delete_group_with_sessions(&options) {
                            tracing::error!(target: "tui.input", "Failed to delete group with sessions: {}", e);
                        }
                    } else if let Err(e) = self.delete_selected_group() {
                        tracing::error!(target: "tui.input", "Failed to delete group: {}", e);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.rename_dialog {
            let mode = dialog.mode();
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.rename_dialog = None;
                    self.group_rename_context = None;
                }
                DialogResult::Submit(data) => {
                    self.rename_dialog = None;
                    match mode {
                        RenameMode::Session => {
                            if let Err(e) = self.rename_selected(
                                &data.title,
                                data.group.as_deref(),
                                data.profile.as_deref(),
                            ) {
                                tracing::error!(target: "tui.input", "Failed to rename session: {}", e);
                            }
                        }
                        RenameMode::Group => {
                            if let Err(e) = self.rename_selected_group(
                                data.group.as_deref(),
                                data.profile.as_deref(),
                            ) {
                                tracing::error!(target: "tui.input", "Failed to rename group: {}", e);
                            }
                        }
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.restart_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.restart_dialog = None;
                }
                DialogResult::Submit(data) => {
                    self.restart_dialog = None;
                    let profile = data.profile.as_deref();
                    let tool = data.tool.as_deref();
                    if let Err(e) = self.restart_selected_session(profile, tool) {
                        // Surface the restart error to the user via the
                        // InfoDialog rather than only the debug log; the
                        // user explicitly initiated this action and needs
                        // to know it failed.
                        tracing::warn!("restart_selected_session failed: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Restart Failed",
                            &format!("Could not restart session: {e}"),
                        ));
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.projects_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(()) => {
                    self.projects_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.group_picker_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.group_picker_dialog = None;
                }
                DialogResult::Submit(mode) => {
                    self.group_picker_dialog = None;
                    if mode != self.group_by {
                        self.apply_group_by(mode);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.sort_picker_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.sort_picker_dialog = None;
                }
                DialogResult::Submit(order) => {
                    self.sort_picker_dialog = None;
                    if order != self.sort_order {
                        self.apply_sort_order(order);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.profile_picker_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.profile_picker_dialog = None;
                }
                DialogResult::Submit(action) => match action {
                    ProfilePickerAction::Switch(name) => {
                        self.profile_picker_dialog = None;
                        // The synthetic "all" entry (only present in filtered mode)
                        // switches back to all-profiles mode
                        let profile = if self.active_profile.is_some() && name == "all" {
                            None
                        } else {
                            Some(name)
                        };
                        if let Err(e) = self.switch_profile(profile) {
                            tracing::error!(target: "tui.input", "Failed to switch profile: {}", e);
                        }
                    }
                    ProfilePickerAction::Created(name) => {
                        self.profile_picker_dialog = None;
                        match crate::session::create_profile(&name) {
                            Ok(()) => {
                                if let Err(e) = self.switch_profile(Some(name)) {
                                    tracing::error!(target: "tui.input", "Failed to switch to new profile: {}", e);
                                }
                            }
                            Err(e) => {
                                self.info_dialog = Some(InfoDialog::new(
                                    "Error",
                                    &format!("Failed to create profile: {}", e),
                                ));
                            }
                        }
                    }
                    ProfilePickerAction::Deleted(name) => {
                        match crate::session::delete_profile(&name) {
                            Ok(()) => {
                                self.show_profile_picker();
                            }
                            Err(e) => {
                                self.profile_picker_dialog = None;
                                self.info_dialog = Some(InfoDialog::new(
                                    "Error",
                                    &format!("Failed to delete profile: {}", e),
                                ));
                            }
                        }
                    }
                },
            }
            return None;
        }

        // Send message dialog
        if let Some(dialog) = &mut self.send_message_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.send_message_dialog = None;
                    self.pending_send_session = None;
                }
                DialogResult::Submit(message) => {
                    self.send_message_dialog = None;
                    if let Some(session_id) = self.pending_send_session.take() {
                        // Defer the actual work to execute_action so the app
                        // loop can render a status indicator first. The send
                        // path may need to start a Docker container or wait
                        // for an agent splash to settle (up to several seconds
                        // total); doing it inline here would freeze the TUI
                        // with no feedback.
                        return Some(Action::SendMessage(session_id, message));
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.update_confirm_dialog {
            use crate::tui::dialogs::DialogResult;
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.update_confirm_dialog = None;
                }
                DialogResult::Submit(()) => {
                    let method = dialog.method.clone();
                    let version = dialog.latest_version.clone();
                    self.update_confirm_dialog = None;
                    return Some(Action::SpawnUpdate(method, version));
                }
            }
            return None;
        }

        // Search mode. Intentionally takes priority over the Ctrl+K palette
        // binding below: while the search input is focused, every key (including
        // Ctrl+K) feeds the search box. Users can press Esc to exit search and
        // then open the palette. Don't move this block past the Ctrl+K check
        // unless you want palette activation to clobber search input.
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                    self.search_query = Input::default();
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                KeyCode::Enter => {
                    self.search_active = false;
                    self.search_query = Input::default();
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                _ => {
                    self.search_query
                        .handle_event(&crossterm::event::Event::Key(key));
                    self.update_search();
                }
            }
            return None;
        }

        // Ctrl+K opens the command palette regardless of strict-hotkey mode.
        // Activated here (before strict normalization) so the binding stays
        // discoverable on every keymap.
        if matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.open_command_palette();
            return None;
        }

        // In strict_hotkeys mode, normalize shifted/ctrl keys to their standard
        // equivalents so the match block below doesn't need duplication.
        //
        // Mapping (strict mode only):
        //   Shift+letter actions -> pass through unchanged: each has its own
        //     `Char('UPPER') if self.strict_hotkeys` arm in the main match.
        //   Ctrl+letter relocated bindings -> uppercase: Ctrl+T->T, Ctrl+D->D, Ctrl+R->R, Ctrl+P->P, Ctrl+N->N
        //   Ctrl+G, Ctrl+O -> pass through with CTRL intact (the dispatch
        //     table matches them with their modifier; stripping CTRL would
        //     collide with the bare-lowercase typing-guard).
        //   Bare lowercase action letters -> blocked (return None)
        let key = if self.strict_hotkeys {
            self.normalize_strict_key(key)
        } else {
            Some(key)
        };
        let key = key?;

        self.dispatch_action_key(key, update_info)
    }

    /// Run the main action dispatch (the giant match block) on a key.
    /// Extracted from `handle_key` so the command palette can synthesize
    /// keys and run them through the same code path without re-entering
    /// dialog routing or strict-mode normalization.
    fn dispatch_action_key(
        &mut self,
        key: KeyEvent,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
        // Dynamic tool session hotkeys (checked before static match block)
        if let Some(tool_name) = self.match_tool_hotkey(&key) {
            if matches!(&self.view_mode, ViewMode::Tool(current) if current == &tool_name) {
                self.view_mode = ViewMode::Agent;
            } else {
                self.view_mode = ViewMode::Tool(tool_name);
                self.preview_scroll_offset = 0;
                self.tool_preview_cache = super::PreviewCache::default();
            }
            return None;
        }

        // Normal mode keybindings
        match key.code {
            KeyCode::Esc if !self.search_matches.is_empty() => {
                self.search_matches.clear();
                self.search_match_index = 0;
                self.search_query = Input::default();
            }
            KeyCode::Esc if matches!(self.view_mode, ViewMode::Tool(_)) => {
                self.view_mode = ViewMode::Agent;
            }
            KeyCode::Char('q') => return Some(Action::Quit),
            // `w` / `W` (snooze), `h` / `H` (snooze alias), and `f` / `F`
            // (favorite) are gated to Attention sort. Snooze and favorite
            // are triage primitives; they only have a visible effect
            // (and a sort impact) in Attention mode. Outside Attention,
            // mutating these flags would silently change persisted state
            // with no on-screen feedback, so we ignore the press
            // entirely. Other sort modes fall through to the existing
            // fallback bindings (`h` collapses; `w` is jump-to-next-
            // waiting in non-strict mode).
            KeyCode::Char('w')
                if !self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('W')
                if self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('h')
                if !self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('H')
                if self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('f')
                if !self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_favorite_at_cursor() {
                    tracing::error!("toggle_favorite_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('F')
                if self.strict_hotkeys && self.sort_order == SortOrder::Attention =>
            {
                if let Err(e) = self.toggle_favorite_at_cursor() {
                    tracing::error!("toggle_favorite_at_cursor failed: {}", e);
                }
            }
            // `z` / `Z`: toggle archive on the cursor's session. Archive is
            // the "park this, I'm done with it" sink. The row drops to tier
            // 99 in the Attention sort, the spinner stops, and the agent
            // pane is killed so a stale process can't keep claiming attention.
            // Pressing it again on an archived row unarchives (no kill, the
            // pane stays gone). Mnemonic: Zzz / archive box. Distinct from
            // `h`/`H` snooze (temporary, auto wakes) and separate from `d`/`D`
            // (destructive delete, unchanged).
            KeyCode::Char('z') if !self.strict_hotkeys => {
                if let Err(e) = self.toggle_archive_at_cursor() {
                    tracing::error!("toggle_archive_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('Z') if self.strict_hotkeys => {
                if let Err(e) = self.toggle_archive_at_cursor() {
                    tracing::error!("toggle_archive_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('e') if !self.strict_hotkeys => {
                self.open_restart_dialog();
            }
            KeyCode::Char('E') if self.strict_hotkeys => {
                self.open_restart_dialog();
            }
            KeyCode::F(5) => {
                self.open_restart_dialog();
            }
            KeyCode::Char('P') => {
                self.show_profile_picker();
            }
            KeyCode::Char('p') if !self.strict_hotkeys => {
                let profile = self.config_profile();
                self.projects_dialog = Some(ProjectsDialog::new(&profile));
            }
            KeyCode::Char('p')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.show_profile_picker();
            }
            #[cfg(feature = "serve")]
            KeyCode::Char('R') if !self.strict_hotkeys => {
                self.serve_view = Some(crate::tui::dialogs::ServeView::new());
            }
            #[cfg(feature = "serve")]
            KeyCode::Char('r')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.serve_view = Some(crate::tui::dialogs::ServeView::new());
            }
            #[cfg(not(feature = "serve"))]
            KeyCode::Char('R') if !self.strict_hotkeys => {
                self.info_dialog = Some(InfoDialog::new(
                    "Serve unavailable",
                    "This `aoe` binary was built without the `serve` feature, \
                     so the web dashboard, local network serving, and \
                     Cloudflare Tunnel integration are not included.\n\n\
                     To serve to your phone (LAN / Tailscale / tunnel):\n\
                       \u{2022} Install a release build from GitHub Releases, or\n\
                       \u{2022} Build from source with:\n\
                         cargo build --release --features serve\n\n\
                     Once you have a `serve`-enabled binary, press R again to \
                     open the serve dialog.",
                ));
            }
            #[cfg(not(feature = "serve"))]
            KeyCode::Char('r')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.info_dialog = Some(InfoDialog::new(
                    "Serve unavailable",
                    "This `aoe` binary was built without the `serve` feature, \
                     so the web dashboard, local network serving, and \
                     Cloudflare Tunnel integration are not included.\n\n\
                     To serve to your phone (LAN / Tailscale / tunnel):\n\
                       \u{2022} Install a release build from GitHub Releases, or\n\
                       \u{2022} Build from source with:\n\
                         cargo build --release --features serve\n\n\
                     Once you have a `serve`-enabled binary, press R again to \
                     open the serve dialog.",
                ));
            }
            KeyCode::Char('t') if !self.strict_hotkeys => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal | ViewMode::Tool(_) => ViewMode::Agent,
                };
            }
            KeyCode::Char('T') if self.strict_hotkeys => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal | ViewMode::Tool(_) => ViewMode::Agent,
                };
            }
            KeyCode::Char('T') if !self.strict_hotkeys => {
                // Quick-attach to paired terminal from any view
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
                            return None;
                        }
                    }
                    let terminal_mode = if let Some(inst) = self.get_instance(id) {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(id)
                        } else {
                            TerminalMode::Host
                        }
                    } else {
                        TerminalMode::Host
                    };
                    return Some(Action::AttachTerminal(id.clone(), terminal_mode));
                }
            }
            KeyCode::Char('t')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Quick-attach to paired terminal from any view
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
                            return None;
                        }
                    }
                    let terminal_mode = if let Some(inst) = self.get_instance(id) {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(id)
                        } else {
                            TerminalMode::Host
                        }
                    } else {
                        TerminalMode::Host
                    };
                    return Some(Action::AttachTerminal(id.clone(), terminal_mode));
                }
            }
            KeyCode::Char('c') if !self.strict_hotkeys && self.view_mode == ViewMode::Terminal => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if inst.is_sandboxed() {
                            let id = id.clone();
                            self.toggle_terminal_mode(&id);
                        } else {
                            self.info_dialog = Some(InfoDialog::new(
                                "Not Available",
                                "Only sandboxed sessions support container terminals. This session runs directly on the host.",
                            ));
                        }
                    }
                }
            }
            KeyCode::Char('C') if self.strict_hotkeys && self.view_mode == ViewMode::Terminal => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if inst.is_sandboxed() {
                            let id = id.clone();
                            self.toggle_terminal_mode(&id);
                        } else {
                            self.info_dialog = Some(InfoDialog::new(
                                "Not Available",
                                "Only sandboxed sessions support container terminals. This session runs directly on the host.",
                            ));
                        }
                    }
                }
            }
            KeyCode::Char(';') => {
                if matches!(self.view_mode, ViewMode::Tool(_)) {
                    self.view_mode = ViewMode::Agent;
                } else if !self.tool_configs.is_empty() {
                    self.open_tool_picker();
                }
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query = Input::default();
            }
            KeyCode::Char('n') if !self.search_matches.is_empty() => {
                self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
                self.cursor = self.search_matches[self.search_match_index];
                self.update_selected();
            }
            KeyCode::Char('n') if !self.strict_hotkeys => {
                if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else if !self.available_tools.any_available() {
                    self.show_no_agents();
                } else {
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    let current_profile = self.config_profile();
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    self.new_dialog = Some(NewSessionDialog::new(
                        self.available_tools.clone(),
                        existing_groups,
                        &current_profile,
                        profiles,
                    ));
                }
            }
            KeyCode::Char('N') if self.strict_hotkeys && self.search_matches.is_empty() => {
                if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else {
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    let current_profile = self.config_profile();
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    self.new_dialog = Some(NewSessionDialog::new(
                        self.available_tools.clone(),
                        existing_groups,
                        &current_profile,
                        profiles,
                    ));
                }
            }
            KeyCode::Char('N') if !self.search_matches.is_empty() => {
                self.search_match_index = if self.search_match_index == 0 {
                    self.search_matches.len() - 1
                } else {
                    self.search_match_index - 1
                };
                self.cursor = self.search_matches[self.search_match_index];
                self.update_selected();
            }
            KeyCode::Char('N') if !self.strict_hotkeys => {
                if !self.search_matches.is_empty() {
                    self.search_match_index = if self.search_match_index == 0 {
                        self.search_matches.len() - 1
                    } else {
                        self.search_match_index - 1
                    };
                    self.cursor = self.search_matches[self.search_match_index];
                    self.update_selected();
                } else if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else {
                    // Pre-filled new session from selection
                    let prefill_path = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .map(|inst| {
                            inst.worktree_info
                                .as_ref()
                                .map(|wt| wt.main_repo_path.clone())
                                .unwrap_or_else(|| inst.project_path.clone())
                        });
                    let prefill_group = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .and_then(|inst| {
                            if inst.group_path.is_empty() {
                                None
                            } else {
                                Some(inst.group_path.clone())
                            }
                        })
                        .or_else(|| self.selected_group.clone());

                    if prefill_path.is_some() || prefill_group.is_some() {
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        let current_profile = self
                            .profile_for_cursor(self.cursor)
                            .unwrap_or_else(|| self.config_profile());
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let mut dialog = NewSessionDialog::new(
                            self.available_tools.clone(),
                            existing_groups,
                            &current_profile,
                            profiles,
                        );
                        if let Some(path) = prefill_path {
                            dialog.set_path(path);
                        }
                        if let Some(group) = prefill_group {
                            dialog.set_group(group);
                        }
                        self.new_dialog = Some(dialog);
                    }
                }
            }
            KeyCode::Char('n')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Strict mode: Ctrl+N = prefill-new (legacy Shift+N relocation)
                if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else {
                    let prefill_path = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .map(|inst| {
                            inst.worktree_info
                                .as_ref()
                                .map(|wt| wt.main_repo_path.clone())
                                .unwrap_or_else(|| inst.project_path.clone())
                        });
                    let prefill_group = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .and_then(|inst| {
                            if inst.group_path.is_empty() {
                                None
                            } else {
                                Some(inst.group_path.clone())
                            }
                        })
                        .or_else(|| self.selected_group.clone());

                    if prefill_path.is_some() || prefill_group.is_some() {
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        let current_profile = self
                            .profile_for_cursor(self.cursor)
                            .unwrap_or_else(|| self.config_profile());
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let mut dialog = NewSessionDialog::new(
                            self.available_tools.clone(),
                            existing_groups,
                            &current_profile,
                            profiles,
                        );
                        if let Some(path) = prefill_path {
                            dialog.set_path(path);
                        }
                        if let Some(group) = prefill_group {
                            dialog.set_group(group);
                        }
                        self.new_dialog = Some(dialog);
                    }
                }
            }
            KeyCode::Char('s') if !self.strict_hotkeys => {
                let project_path = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .map(|inst| inst.project_path.clone());
                match SettingsView::new(&self.config_profile(), project_path) {
                    Ok(view) => self.settings_view = Some(view),
                    Err(e) => {
                        tracing::error!("Failed to open settings: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open settings: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('S') if self.strict_hotkeys => {
                let project_path = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .map(|inst| inst.project_path.clone());
                match SettingsView::new(&self.config_profile(), project_path) {
                    Ok(view) => self.settings_view = Some(view),
                    Err(e) => {
                        tracing::error!(target: "tui.input", "Failed to open settings: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open settings: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('u') => {
                if let Some(info) = update_info {
                    if info.available && self.update_confirm_dialog.is_none() {
                        let method = match crate::update::install::detect_install_method() {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!(target: "tui.input", "update detection failed: {e}");
                                return None;
                            }
                        };
                        use crate::update::install::InstallMethod;
                        if !matches!(
                            &method,
                            InstallMethod::Homebrew | InstallMethod::Tarball { .. }
                        ) {
                            let msg = match &method {
                                InstallMethod::Nix => {
                                    "Nix install: run `nix run github:agent-of-empires/agent-of-empires` to update".to_string()
                                }
                                InstallMethod::Cargo => {
                                    "Cargo install: run `cargo install --git https://github.com/agent-of-empires/agent-of-empires aoe`".to_string()
                                }
                                InstallMethod::Unknown { .. } => {
                                    "Unknown install method: run `aoe update` in a terminal for instructions".to_string()
                                }
                                _ => unreachable!(),
                            };
                            return Some(Action::SetTransientStatus(msg));
                        }
                        let needs_sudo = matches!(
                            &method,
                            InstallMethod::Tarball { binary_path }
                                if !crate::update::install::parent_is_writable(binary_path)
                        );
                        self.update_confirm_dialog =
                            Some(crate::tui::dialogs::UpdateConfirmDialog::new(
                                info.current_version.clone(),
                                info.latest_version.clone(),
                                method,
                                needs_sudo,
                            ));
                    }
                }
            }
            KeyCode::Char('D') if !self.strict_hotkeys => {
                // Open diff view - requires a selected session
                let Some(session_id) = &self.selected_session else {
                    self.info_dialog = Some(InfoDialog::new(
                        "No Session Selected",
                        "Select a session to view its diff.",
                    ));
                    return None;
                };

                let Some(inst) = self.get_instance(session_id) else {
                    self.info_dialog =
                        Some(InfoDialog::new("Error", "Could not find session data."));
                    return None;
                };

                let repo_path = std::path::PathBuf::from(&inst.project_path);
                let session_id_owned = inst.id.clone();
                let profile = inst.source_profile.clone();
                let base_override = inst.base_branch_override.clone();
                match DiffView::new_for_session(
                    repo_path,
                    Some(session_id_owned),
                    profile,
                    base_override,
                ) {
                    Ok(view) => self.diff_view = Some(view),
                    Err(e) => {
                        tracing::error!(target: "tui.input", "Failed to open diff view: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open diff view: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('d')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Strict mode: Ctrl+D = diff (legacy Shift+D relocation)
                let Some(session_id) = &self.selected_session else {
                    self.info_dialog = Some(InfoDialog::new(
                        "No Session Selected",
                        "Select a session to view its diff.",
                    ));
                    return None;
                };

                let Some(inst) = self.get_instance(session_id) else {
                    self.info_dialog =
                        Some(InfoDialog::new("Error", "Could not find session data."));
                    return None;
                };

                let repo_path = std::path::PathBuf::from(&inst.project_path);
                match DiffView::new(repo_path) {
                    Ok(view) => self.diff_view = Some(view),
                    Err(e) => {
                        tracing::error!("Failed to open diff view: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open diff view: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('x') if !self.strict_hotkeys => {
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(session_id) {
                        if matches!(
                            inst.status,
                            Status::Stopped | Status::Deleting | Status::Creating
                        ) {
                            return None;
                        }
                        let message = format!("Are you sure you want to stop '{}'?", inst.title);
                        self.pending_stop_session = Some(session_id.clone());
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Stop Session", &message, "stop_session"));
                    }
                }
            }
            KeyCode::Char('X') if self.strict_hotkeys => {
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(session_id) {
                        if matches!(
                            inst.status,
                            Status::Stopped | Status::Deleting | Status::Creating
                        ) {
                            return None;
                        }
                        let message = format!("Are you sure you want to stop '{}'?", inst.title);
                        self.pending_stop_session = Some(session_id.clone());
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Stop Session", &message, "stop_session"));
                    }
                }
            }
            KeyCode::Char('d') if !self.strict_hotkeys => {
                // Deletion only allowed in Agent View
                if self.view_mode == ViewMode::Terminal {
                    self.info_dialog = Some(InfoDialog::new(
                        "Cannot Delete Terminal",
                        "Terminals cannot be deleted directly. Switch to Agent View (press 't') and delete the agent session instead.",
                    ));
                    return None;
                }
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(session_id) {
                        if inst.status == Status::Creating {
                            return None;
                        }
                        if inst.status == Status::Deleting {
                            let message = format!(
                                "'{}' is stuck deleting. Force remove it from the session list? \
                                 (worktrees, branches, and containers will not be cleaned up)",
                                inst.title
                            );
                            self.pending_force_remove_session = Some(session_id.clone());
                            self.confirm_dialog = Some(ConfirmDialog::new(
                                "Force Remove",
                                &message,
                                "force_remove_session",
                            ));
                            return None;
                        }

                        let config = DeleteDialogConfig {
                            worktree_branch: inst
                                .worktree_info
                                .as_ref()
                                .filter(|wt| wt.managed_by_aoe)
                                .map(|wt| wt.branch.clone())
                                .or_else(|| inst.workspace_info.as_ref().map(|w| w.branch.clone())),
                            has_sandbox: inst.sandbox_info.as_ref().is_some_and(|s| s.enabled),
                            project_path: Some(inst.project_path.clone()),
                        };

                        let profile = self.config_profile();
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            inst.title.clone(),
                            config,
                            &profile,
                        ));
                    } else {
                        let profile = self.config_profile();
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            "Unknown Session".to_string(),
                            DeleteDialogConfig::default(),
                            &profile,
                        ));
                    }
                } else if let Some(group_path) = &self.selected_group {
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press 'g' and pick Manual to manage groups.",
                        ));
                        return None;
                    }
                    let prefix = format!("{}/", group_path);
                    let session_count = self
                        .instances
                        .iter()
                        .filter(|i| {
                            i.group_path == *group_path || i.group_path.starts_with(&prefix)
                        })
                        .count();

                    if session_count > 0 {
                        let has_managed_worktrees =
                            self.group_has_managed_worktrees(group_path, &prefix);
                        let has_containers = self.group_has_containers(group_path, &prefix);
                        self.group_delete_options_dialog = Some(GroupDeleteOptionsDialog::new(
                            group_path.clone(),
                            session_count,
                            has_managed_worktrees,
                            has_containers,
                        ));
                    } else {
                        let message =
                            format!("Are you sure you want to delete group '{}'?", group_path);
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Delete Group", &message, "delete_group"));
                    }
                }
            }
            KeyCode::Char('D') if self.strict_hotkeys => {
                // Strict mode: Shift+D = delete (was lowercase 'd' action)
                if self.view_mode == ViewMode::Terminal {
                    self.info_dialog = Some(InfoDialog::new(
                        "Cannot Delete Terminal",
                        "Terminals cannot be deleted directly. Switch to Agent View (press Shift+T) and delete the agent session instead.",
                    ));
                    return None;
                }
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(session_id) {
                        if inst.status == Status::Creating {
                            return None;
                        }
                        if inst.status == Status::Deleting {
                            let message = format!(
                                "'{}' is stuck deleting. Force remove it from the session list? \
                                 (worktrees, branches, and containers will not be cleaned up)",
                                inst.title
                            );
                            self.pending_force_remove_session = Some(session_id.clone());
                            self.confirm_dialog = Some(ConfirmDialog::new(
                                "Force Remove",
                                &message,
                                "force_remove_session",
                            ));
                            return None;
                        }

                        let config = DeleteDialogConfig {
                            worktree_branch: inst
                                .worktree_info
                                .as_ref()
                                .filter(|wt| wt.managed_by_aoe)
                                .map(|wt| wt.branch.clone())
                                .or_else(|| inst.workspace_info.as_ref().map(|w| w.branch.clone())),
                            has_sandbox: inst.sandbox_info.as_ref().is_some_and(|s| s.enabled),
                            project_path: Some(inst.project_path.clone()),
                        };

                        let profile = self.config_profile();
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            inst.title.clone(),
                            config,
                            &profile,
                        ));
                    } else {
                        let profile = self.config_profile();
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            "Unknown Session".to_string(),
                            DeleteDialogConfig::default(),
                            &profile,
                        ));
                    }
                } else if let Some(group_path) = &self.selected_group {
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press Ctrl+G and pick Manual to manage groups.",
                        ));
                        return None;
                    }
                    let prefix = format!("{}/", group_path);
                    let session_count = self
                        .instances
                        .iter()
                        .filter(|i| {
                            i.group_path == *group_path || i.group_path.starts_with(&prefix)
                        })
                        .count();

                    if session_count > 0 {
                        let has_managed_worktrees =
                            self.group_has_managed_worktrees(group_path, &prefix);
                        let has_containers = self.group_has_containers(group_path, &prefix);
                        self.group_delete_options_dialog = Some(GroupDeleteOptionsDialog::new(
                            group_path.clone(),
                            session_count,
                            has_managed_worktrees,
                            has_containers,
                        ));
                    } else {
                        let message =
                            format!("Are you sure you want to delete group '{}'?", group_path);
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Delete Group", &message, "delete_group"));
                    }
                }
            }
            KeyCode::Char('r') if !self.strict_hotkeys => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
                            return None;
                        }
                        // Rename is anchored to the selected session, so the dialog
                        // must open against that session's profile, not the
                        // view-level active/config profile (which can differ in
                        // all-profiles mode).
                        let current_profile = inst.source_profile.clone();
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        self.rename_dialog = Some(RenameDialog::new(
                            &inst.title,
                            &inst.group_path,
                            &current_profile,
                            profiles,
                            existing_groups,
                        ));
                    }
                } else if let Some(group_path) = &self.selected_group {
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press 'g' and pick Manual to manage groups.",
                        ));
                        return None;
                    }
                    let group_path = group_path.clone();
                    let current_profile = self
                        .selected_group_profile
                        .clone()
                        .unwrap_or_else(|| self.config_profile());
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    self.group_rename_context = Some(super::GroupRenameContext {
                        old_path: group_path.clone(),
                        old_profile: current_profile.clone(),
                    });
                    self.rename_dialog = Some(RenameDialog::new_for_group(
                        &group_path,
                        &current_profile,
                        profiles,
                        existing_groups,
                    ));
                }
            }
            KeyCode::Char('R') if self.strict_hotkeys => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
                            return None;
                        }
                        // See the corresponding `r` handler above: rename targets
                        // the selected session, so anchor on its source_profile.
                        let current_profile = inst.source_profile.clone();
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        self.rename_dialog = Some(RenameDialog::new(
                            &inst.title,
                            &inst.group_path,
                            &current_profile,
                            profiles,
                            existing_groups,
                        ));
                    }
                } else if let Some(group_path) = &self.selected_group {
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press Ctrl+G and pick Manual to manage groups.",
                        ));
                        return None;
                    }
                    let group_path = group_path.clone();
                    let current_profile = self
                        .selected_group_profile
                        .clone()
                        .unwrap_or_else(|| self.config_profile());
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    self.group_rename_context = Some(super::GroupRenameContext {
                        old_path: group_path.clone(),
                        old_profile: current_profile.clone(),
                    });
                    self.rename_dialog = Some(RenameDialog::new_for_group(
                        &group_path,
                        &current_profile,
                        profiles,
                        existing_groups,
                    ));
                }
            }
            KeyCode::Char('m') if !self.strict_hotkeys => {
                self.open_send_message_dialog();
            }
            KeyCode::Char('M') if self.strict_hotkeys => {
                self.open_send_message_dialog();
            }
            // Tab enters live-send mode on the selected running session.
            // Free at the home-view top level (settings/cockpit/dialogs
            // own their own Tab handlers), and the entry-vs-send
            // distinction is unambiguous: this branch only fires when
            // `live_send` is None, because the live-send capture at the
            // top of `handle_key` short-circuits otherwise. While in
            // live mode Tab is sent verbatim to the agent.
            KeyCode::Tab => {
                if let Some(action) = self.start_live_send() {
                    return Some(action);
                }
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.show_sort_picker();
            }
            // Plain lowercase 'o' opens the sort picker only OUTSIDE strict mode.
            // In strict mode, bare 'o' falls through to the typing-guard catch-all
            // (compose dialog), per the no-destructive-lowercase contract.
            KeyCode::Char('o') if !self.strict_hotkeys => {
                self.show_sort_picker();
            }
            // Shift+O in strict mode arrives here as Char('O') (normalize_strict_key
            // no longer lowercases 'O') so it's the one bare-letter sort hotkey in
            // strict mode. Also matches Shift+O in non-strict mode.
            KeyCode::Char('O') => {
                self.show_sort_picker();
            }
            // ±10 navigation: Shift+Up/Down, PageUp/PageDown, OR { / }.
            // iPad-friendly ±10 aliases for PageUp/PageDown. iPads have no
            // PageUp/PageDown keys, and Cmd combos are typically stripped by
            // SSH/Mosh before reaching the TTY. Shift+Up/Down arrives intact
            // on every terminal we test, and `{` / `}` (Shift+`[` / Shift+`]`)
            // pass through as plain chars so Cmd+Shift+`[` / `]` works whether
            // or not the terminal forwards Cmd. Both bind to the same step
            // size as PageUp/PageDown to keep the mental model simple.
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_cursor(-10);
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_cursor(10);
            }
            KeyCode::Char('{') => {
                self.move_cursor(-10);
            }
            KeyCode::Char('}') => {
                self.move_cursor(10);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_cursor(-1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_cursor(1);
            }
            KeyCode::PageUp => {
                self.move_cursor(-10);
            }
            KeyCode::PageDown => {
                self.move_cursor(10);
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.mouse_pos = None;
                self.update_selected();
            }
            KeyCode::Char('g')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.show_group_picker();
            }
            KeyCode::Char('g') if !self.strict_hotkeys => {
                self.show_group_picker();
            }
            KeyCode::End | KeyCode::Char('G') if !self.flat_items.is_empty() => {
                self.cursor = self.flat_items.len() - 1;
                self.mouse_pos = None;
                self.update_selected();
            }
            KeyCode::Enter => {
                if self.selected_session.is_some() {
                    return self.activate_selected_session();
                } else if let Some(Item::Group { path, .. }) = self.flat_items.get(self.cursor) {
                    let path = path.clone();
                    self.toggle_group_collapsed(&path);
                }
            }
            // `<` shrinks the list pane width; `>` grows it. Capital
            // H/L used to be aliases here but H is now the advertised
            // snooze key (mnemonic: Hide), so width controls live on
            // the angle-bracket characters only.
            KeyCode::Char('<') => {
                self.shrink_list();
            }
            KeyCode::Char('>') => {
                self.grow_list();
            }
            // `i`/`I`: toggle the preview info header (profile/tool/path/
            // status/sandbox/worktree). Persisted across runs. The hint
            // rendered on the outer Preview block title advertises this.
            KeyCode::Char('i') if !self.strict_hotkeys => {
                self.toggle_preview_info();
            }
            KeyCode::Char('I') if self.strict_hotkeys => {
                self.toggle_preview_info();
            }
            // Bare `h` collapses only in strict mode; in non-strict the
            // earlier `Char('h') if !self.strict_hotkeys` arm catches it
            // for Snooze, so we'd never reach here. Pairing it with Left
            // keeps the help overlay's "h/←" claim honest and mirrors the
            // unconditional `l`/Right binding below.
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if !collapsed {
                        let path = path.clone();
                        self.toggle_group_collapsed(&path);
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if *collapsed {
                        let path = path.clone();
                        self.toggle_group_collapsed(&path);
                    }
                }
            }
            // Upstream PR #796 added `w` for jump-to-next-waiting after the
            // snooze feature (a19337b) had already taken `w`/`W`. In non-strict
            // mode the snooze arm at line 707 catches first, so this jump arm
            // was always dead. In strict mode it leaked through and preempted
            // the typing-guard below; bare `w` jumped the cursor instead of
            // opening compose like every other lowercase letter. Gate it.
            KeyCode::Char('w') if !self.strict_hotkeys => {
                self.jump_to_next_waiting();
            }
            // Strict-mode typing guard: any bare lowercase letter that isn't a
            // navigation key (j/k/h/l) is treated as inadvertent typing; open
            // the compose dialog pre-filled with that character instead of
            // firing an action or swallowing the keypress.
            KeyCode::Char(c)
                if self.strict_hotkeys
                    && key.modifiers == KeyModifiers::NONE
                    && c.is_ascii_lowercase() =>
            {
                self.capture_letter_to_compose(c);
            }
            _ => {}
        }

        None
    }

    /// Build and show the command palette. Combines the static `builtin_commands`
    /// with dynamic jump-to-session and jump-to-group entries built from the
    /// current `flat_items`.
    fn open_command_palette(&mut self) {
        let serve_enabled = cfg!(feature = "serve");
        let mut entries: Vec<PaletteCommand> = builtin_commands(serve_enabled, self.strict_hotkeys);

        // Quit command (separate so the lifetime mapping is clear and we
        // can keep it out of `builtin_commands` to avoid pulling KeyCode
        // imports into the palette module).
        let quit_hotkey = if self.strict_hotkeys { "Q" } else { "q" };
        entries.push(PaletteCommand {
            id: "quit",
            title: "Quit Agent of Empires".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["exit", "close"],
            hotkey: quit_hotkey,
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        });

        // Dynamic session/group entries: one per flat_items row, so the user
        // can fuzzy-search and jump straight to it. We tag in-flight sessions
        // (Creating / Deleting) in the title so the user knows that picking
        // Stop/Delete from the palette will be a no-op for those rows.
        for (idx, item) in self.flat_items.iter().enumerate() {
            match item {
                Item::Session { id, .. } => {
                    let Some(inst) = self.get_instance(id) else {
                        continue;
                    };
                    let status_tag = match inst.status {
                        Status::Creating => " [creating]",
                        Status::Deleting => " [deleting]",
                        Status::Stopped => " [stopped]",
                        _ => "",
                    };
                    let title = if inst.group_path.is_empty() {
                        format!("Jump to session: {}{}", inst.title, status_tag)
                    } else {
                        format!(
                            "Jump to session: {} ({}){}",
                            inst.title, inst.group_path, status_tag
                        )
                    };
                    entries.push(PaletteCommand {
                        id: "jump-session",
                        title,
                        group: PaletteGroup::Sessions,
                        keywords: vec!["session", "jump", "select"],
                        hotkey: "",
                        payload: PaletteAction::JumpToCursor(idx),
                    });
                }
                Item::Group { name, path, .. } => {
                    // The synthetic Archived section header is not a real
                    // group; skip it so the palette doesn't surface the
                    // sentinel path or invite Jump-to-group navigation
                    // that the rest of the codebase intentionally
                    // disarms.
                    if crate::session::is_archived_section_path(path) {
                        continue;
                    }
                    let label = if name == path {
                        format!("Jump to group: {}", name)
                    } else {
                        format!("Jump to group: {} ({})", name, path)
                    };
                    entries.push(PaletteCommand {
                        id: "jump-group",
                        title: label,
                        group: PaletteGroup::Groups,
                        keywords: vec!["group", "jump"],
                        hotkey: "",
                        payload: PaletteAction::JumpToCursor(idx),
                    });
                }
            }
        }

        // Tool session entries, sorted by name for stable palette ordering
        // (matches the tool picker dialog's alphabetical order).
        let mut tools_sorted: Vec<_> = self.tool_configs.iter().collect();
        tools_sorted.sort_by_key(|(name, _)| name.to_owned());
        for (name, config) in tools_sorted {
            let hotkey_label = config
                .hotkey
                .as_deref()
                .map(|h| format!(" [{}]", h))
                .unwrap_or_default();
            entries.push(PaletteCommand {
                id: "tool-session",
                title: format!("Open tool: {}{}", name, hotkey_label),
                group: PaletteGroup::Actions,
                keywords: vec!["tool", "session"],
                hotkey: "",
                payload: PaletteAction::ToolSession(name.clone()),
            });
        }

        self.command_palette = Some(CommandPaletteDialog::new(entries));
    }

    /// Apply a palette pick. `Key` re-enters the action dispatch with the
    /// synthesized event (bypassing strict normalization, which the palette
    /// already accounts for); `JumpToCursor` moves the selection.
    fn dispatch_palette_action(
        &mut self,
        action: PaletteAction,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
        match action {
            PaletteAction::Key(synth) => {
                // Clear leftover search-cycle state before dispatching. Some
                // action keys (`n`, `N`) are dual-purpose: they cycle search
                // matches when matches are active, otherwise open new-session
                // dialogs. The palette's mental model is "run the named
                // action," so we drop search state here to make sure a pick
                // of "New session" never silently turns into a search-cycle.
                if !self.search_matches.is_empty() {
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                self.dispatch_action_key(synth, update_info)
            }
            PaletteAction::JumpToCursor(idx) => {
                if !self.flat_items.is_empty() {
                    self.cursor = idx.min(self.flat_items.len() - 1);
                    self.update_selected();
                }
                None
            }
            PaletteAction::ToolSession(tool_name) => {
                self.view_mode = ViewMode::Tool(tool_name);
                self.preview_scroll_offset = 0;
                self.tool_preview_cache = super::PreviewCache::default();
                None
            }
            PaletteAction::EnterLiveSend => self.start_live_send(),
            PaletteAction::OpenSortPicker => {
                self.show_sort_picker();
                None
            }
            PaletteAction::OpenGroupPicker => {
                self.show_group_picker();
                None
            }
        }
    }

    fn jump_to_next_waiting(&mut self) {
        let len = self.flat_items.len();
        if len == 0 {
            return;
        }

        // Pass 1: forward-walk from cursor+1, wrapping, for the next Waiting
        // session OR a freshly-stopped Idle session (within
        // `idle_decay_window`). Both states are "needs your attention" and
        // cycle together so repeated `w` taps move through the actionable
        // backlog regardless of which hook fired.
        let window = self.idle_decay_window;
        let start = (self.cursor + 1) % len;
        for i in 0..len - 1 {
            let idx = (start + i) % len;
            let id = match self.flat_items.get(idx) {
                Some(Item::Session { id, .. }) => id.clone(),
                _ => continue,
            };
            if let Some(inst) = self.get_instance(&id) {
                let is_actionable = inst.status == Status::Waiting
                    || matches!(inst.idle_age(), Some(age) if age < window);
                if is_actionable {
                    self.cursor = idx;
                    self.update_selected();
                    return;
                }
            }
        }

        // Pass 2: fall back to the most-recently-accessed Idle session, skipping
        // the cursor. Sessions never attached (last_accessed_at == None) rank
        // last but remain eligible.
        let mut best: Option<(usize, Option<chrono::DateTime<chrono::Utc>>)> = None;
        for idx in 0..len {
            if idx == self.cursor {
                continue;
            }
            let id = match self.flat_items.get(idx) {
                Some(Item::Session { id, .. }) => id.clone(),
                _ => continue,
            };
            let Some(inst) = self.get_instance(&id) else {
                continue;
            };
            if inst.status != Status::Idle {
                continue;
            }
            let ts = inst.last_accessed_at;
            let beats = match best {
                None => true,
                Some((_, b)) => match (ts, b) {
                    (Some(a), Some(b)) => a > b,
                    (Some(_), None) => true,
                    (None, _) => false,
                },
            };
            if beats {
                best = Some((idx, ts));
            }
        }

        if let Some((idx, _)) = best {
            self.cursor = idx;
            self.update_selected();
            return;
        }

        self.info_dialog = Some(InfoDialog::new(
            "No Available Sessions",
            "No sessions are currently waiting or idle.",
        ));
    }

    pub(super) fn move_cursor(&mut self, delta: i32) {
        if self.flat_items.is_empty() {
            return;
        }

        let new_cursor = if delta < 0 {
            self.cursor.saturating_sub((-delta) as usize)
        } else {
            (self.cursor + delta as usize).min(self.flat_items.len() - 1)
        };

        self.cursor = new_cursor;
        // Keyboard nav overrides any prior hover. Without this, when mosh
        // (or any prediction layer) eats the `Moved` event that fires as
        // the cursor leaves the list, the hover background stays painted
        // on the row the mouse was last on while the keyboard-selected
        // row also paints — two highlighted rows at once. handle_hover
        // only clears `mouse_pos` when it RECEIVES an off-list Moved, so
        // any keyboard transition has to clear it directly.
        self.mouse_pos = None;
        self.update_selected();
    }

    /// Resolve the action that "activating" the currently-selected session
    /// should produce (cockpit open, attach to tmux session, attach to a
    /// tool session, etc.). Returns `None` for in-flight sessions
    /// (`Creating`/`Deleting`) and when no session is selected. Shared
    /// between the `Enter` keybind and double-click activation so the two
    /// paths can't drift.
    pub(super) fn activate_selected_session(&mut self) -> Option<Action> {
        let id = self.selected_session.clone()?;
        if let Some(inst) = self.get_instance(&id) {
            if matches!(inst.status, Status::Deleting | Status::Creating) {
                return None;
            }
            if inst.is_cockpit_mode() {
                #[cfg(feature = "serve")]
                {
                    return Some(Action::OpenCockpit(id));
                }
                #[cfg(not(feature = "serve"))]
                {
                    return Some(Action::SetTransientStatus(
                        "Cockpit session: rebuild with --features serve to attach".to_string(),
                    ));
                }
            }
        }
        match self.view_mode {
            ViewMode::Agent => {
                // `default_attach_mode = LiveSend` swaps the historical
                // tmux attach for live-send mode on Enter / double-click.
                // Cockpit was already handled above (the resolver also
                // returns None for cockpit, so the match is double-safe);
                // Terminal/Tool views keep their existing paths regardless.
                //
                // Route through `start_live_send` so the same-target
                // guard (already-live on this session) is honored: a
                // double-click on the live row would otherwise re-run
                // ensure_pane_ready and respawn the worker for no
                // reason. `start_live_send` returns `None` for that
                // and for cockpit/creating rows; in either of those
                // cases we leave activation alone (cockpit was already
                // dispatched to OpenCockpit above; same-target re-click
                // is intentionally a no-op).
                if matches!(
                    self.default_attach_mode(&id),
                    Some(crate::session::NewSessionAttachMode::LiveSend)
                ) {
                    self.start_live_send()
                } else {
                    Some(Action::AttachSession(id))
                }
            }
            ViewMode::Terminal => {
                let terminal_mode = if let Some(inst) = self.get_instance(&id) {
                    if inst.is_sandboxed() {
                        self.get_terminal_mode(&id)
                    } else {
                        TerminalMode::Host
                    }
                } else {
                    TerminalMode::Host
                };
                Some(Action::AttachTerminal(id, terminal_mode))
            }
            ViewMode::Tool(ref tool_name) => Some(Action::AttachToolSession(id, tool_name.clone())),
        }
    }

    pub(super) fn update_selected(&mut self) {
        if let Some(item) = self.flat_items.get(self.cursor) {
            let prev_session = self.selected_session.clone();
            match item {
                Item::Session { id, .. } => {
                    self.selected_session = Some(id.clone());
                    self.selected_group = None;
                    self.selected_group_profile = None;
                }
                Item::Group { path, .. } => {
                    self.selected_session = None;
                    if crate::session::is_archived_section_path(path) {
                        // The synthetic Archived section is not a real
                        // group: it can't be renamed, deleted, archived,
                        // or moved. Leaving `selected_group` unset
                        // disarms every keybind that branches on
                        // `selected_group.is_some()` (rename, delete,
                        // archive group, etc.) without each one having
                        // to special-case the sentinel.
                        self.selected_group = None;
                        self.selected_group_profile = None;
                    } else {
                        self.selected_group = Some(path.clone());
                        self.selected_group_profile = self.profile_for_cursor(self.cursor);
                    }
                }
            }
            if self.selected_session != prev_session {
                self.preview_scroll_offset = 0;
            }
        }
    }

    /// Put the cursor back on `selected_session` after a `flat_items` rebuild
    /// (sort toggle, group_by toggle). Mode flips reshape the list, especially
    /// when Attention sort is involved, so index-based clamping lands the
    /// cursor on whatever happened to slide into the old slot. Seeking by
    /// session id keeps focus on the row the user was actually looking at.
    /// Falls back to the legacy clamp when there was no prior selection or
    /// the session is no longer in the flat list (e.g., collapsed under a
    /// group header).
    pub(super) fn reseat_cursor_after_rebuild(&mut self) {
        if let Some(sid) = self.selected_session.clone() {
            for (idx, item) in self.flat_items.iter().enumerate() {
                if let Item::Session { id, .. } = item {
                    if *id == sid {
                        self.cursor = idx;
                        self.update_selected();
                        return;
                    }
                }
            }
        }
        self.cursor = self.cursor.min(self.flat_items.len().saturating_sub(1));
        self.update_selected();
    }

    fn apply_sort_order(&mut self, new_order: SortOrder) {
        self.sort_order = new_order;
        self.flat_items = self.build_flat_items();
        if self.search_active && !self.search_query.value().is_empty() {
            self.update_search();
        } else {
            self.reseat_cursor_after_rebuild();
        }
        if let Ok(mut config) = load_config().map(|c| c.unwrap_or_default()) {
            config.app_state.sort_order = Some(self.sort_order);
            if let Err(e) = save_config(&config) {
                tracing::warn!(target: "tui.input", "Failed to save sort order: {}", e);
            }
        }
    }

    fn apply_group_by(&mut self, new_mode: GroupByMode) {
        self.group_by = new_mode;
        self.flat_items = self.build_flat_items();
        self.reseat_cursor_after_rebuild();
        match load_config().map(|c| c.unwrap_or_default()) {
            Ok(mut config) => {
                config.app_state.group_by = Some(self.group_by);
                if let Err(e) = save_config(&config) {
                    tracing::warn!(target: "tui.input", "Failed to save group_by mode: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!(target: "tui.input", "Failed to load config for group_by save: {}", e);
            }
        }
    }

    fn toggle_group_collapsed(&mut self, path: &str) {
        // The synthetic Archived section is not a member of any
        // GroupTree; its collapsed state lives on HomeView and persists
        // separately. Route here before either branch tries to mutate a
        // nonexistent group.
        if crate::session::is_archived_section_path(path) {
            self.toggle_archived_section();
            return;
        }
        if self.group_by == GroupByMode::Project {
            let collapsed = self
                .project_group_collapsed
                .get(path)
                .copied()
                .unwrap_or(false);
            self.project_group_collapsed
                .insert(path.to_string(), !collapsed);
            self.flat_items = self.build_flat_items();
            return;
        }
        // Route to the correct profile's GroupTree
        let profile = self.profile_for_cursor(self.cursor);
        if let Some(profile) = profile {
            if let Some(tree) = self.group_trees.get_mut(&profile) {
                tree.toggle_collapsed(path);
            }
        }
        self.flat_items = self.build_flat_items();
        if let Err(e) = self.save() {
            tracing::error!(target: "tui.input", "Failed to save group state: {}", e);
        }
    }

    /// Route a mouse-wheel-up at (col, row) to the pane under the cursor:
    /// diff view (if open) → diff scroll; list pane → list cursor up;
    /// preview pane → preview scroll. Returns `true` if the UI should
    /// redraw. Scrolls do not cross pane boundaries: a wheel over the
    /// preview never moves the list cursor, even when the preview is at
    /// its scroll boundary or has no session selected.
    pub fn handle_scroll_up(&mut self, col: u16, row: u16) -> bool {
        const STEP: u16 = 3;
        // Any scroll repositions the preview content under the
        // selection rect, so a leftover highlight from a previous drag
        // would point at unrelated text. Drop it before changing
        // offsets so the highlight disappears alongside the scroll.
        self.clear_preview_selection();
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_up(STEP);
            return true;
        }
        // Live-send mode lets the user scroll the preview to read
        // agent history without exiting, but list scroll is suppressed
        // (changing the selection mid-live-send would silently aim the
        // next keystroke at a different pane than the one the user is
        // looking at). All other modals swallow scroll entirely.
        if self.live_send.is_some() {
            if !self.hit_preview(col, row) {
                return false;
            }
        } else {
            if self.has_dialog() {
                return false;
            }
            if self.hit_list(col, row) {
                self.move_cursor(-1);
                return true;
            }
            if !self.hit_preview(col, row) {
                return false;
            }
        }
        if self.selected_session.is_none() {
            return false;
        }

        let active_cache = match self.view_mode {
            ViewMode::Agent => &self.preview_cache,
            ViewMode::Terminal => {
                let terminal_mode = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .map(|inst| {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(&inst.id)
                        } else {
                            TerminalMode::Host
                        }
                    })
                    .unwrap_or(TerminalMode::Host);
                match terminal_mode {
                    TerminalMode::Container => &self.container_terminal_preview_cache,
                    TerminalMode::Host => &self.terminal_preview_cache,
                }
            }
            ViewMode::Tool(_) => &self.tool_preview_cache,
        };

        let visible_height = active_cache.dimensions.1.saturating_sub(1) as usize;
        let real_max = active_cache.captured_lines.saturating_sub(visible_height) as u16;

        let new_offset = self.preview_scroll_offset.saturating_add(STEP);
        let clamped = new_offset.min(real_max);
        if clamped == self.preview_scroll_offset {
            return false;
        }
        self.preview_scroll_offset = clamped;
        true
    }

    /// Map a (col, row) inside the list's inner content rect to a
    /// `flat_items` index, or `None` for rows that don't resolve to a real
    /// item (search bar, `[N more above/below]` indicator rows, empty list,
    /// outside the inner rect, dialog open, diff view active). Shared by
    /// `handle_click` and `hovered_index` so selection and hover use the
    /// exact same math.
    ///
    /// Live-send is intentionally NOT treated as a blocking dialog here.
    /// `has_dialog()` returns true while live mode is active so other
    /// surfaces (key shortcuts, preview-click, scroll wheel) stay
    /// frozen, but clicks on list rows are how the user switches the
    /// live target session: blocking them would make the feature
    /// unreachable via mouse.
    pub(super) fn resolve_row_to_index(&self, col: u16, row: u16) -> Option<usize> {
        if self.diff_view.is_some() || self.has_non_live_send_overlay() {
            return None;
        }
        let inner = self.list_inner_area;
        if !inner.contains(Position::from((col, row))) {
            return None;
        }
        if self.flat_items.is_empty() {
            return None;
        }
        let visible_height = if self.search_active {
            (inner.height as usize).saturating_sub(1)
        } else {
            inner.height as usize
        };
        if visible_height == 0 {
            return None;
        }
        let row_in_inner = row.saturating_sub(inner.y) as usize;
        if self.search_active && row_in_inner + 1 == inner.height as usize {
            return None;
        }

        let scroll = crate::tui::components::scroll::calculate_scroll(
            self.flat_items.len(),
            self.cursor,
            visible_height,
        );
        let row_offset = if scroll.has_more_above { 1 } else { 0 };
        if row_in_inner < row_offset {
            return None;
        }
        let item_row = row_in_inner - row_offset;
        if item_row >= scroll.list_visible {
            return None;
        }
        let abs_idx = scroll.scroll_offset + item_row;
        if abs_idx >= self.flat_items.len() {
            return None;
        }
        Some(abs_idx)
    }

    /// Currently hovered `flat_items` index, derived from the last mouse
    /// position. `None` when the mouse is off the list or over a row that
    /// doesn't resolve to a real item. Recomputed on every call so wheel
    /// scrolls implicitly move the hover with the items under the cursor.
    pub(super) fn hovered_index(&self) -> Option<usize> {
        self.mouse_pos
            .and_then(|(c, r)| self.resolve_row_to_index(c, r))
    }

    /// Route a left-click at (col, row) inside the session list. A
    /// single click on a session row selects it AND requests live-send
    /// mode for that row (same `Action::EnterLiveSend` that Tab would
    /// emit); a single click on a group row toggles its collapsed
    /// state; a second click on the same session row within
    /// `DOUBLE_CLICK_THRESHOLD` activates the session (the same Action
    /// the `Enter` keybind would have produced) so users can still
    /// drop into a full tmux attach without going through live mode.
    /// Returns the action for the caller to dispatch, or `None` for
    /// no-op clicks (group toggle, cockpit/creating rows, same-session
    /// re-clicks while already live). The caller redraws unconditionally
    /// so the moved cursor / toggled group always paints before the
    /// action executes. Gated by `has_dialog()` (via
    /// `resolve_row_to_index`) so clicks don't shift selection out
    /// from under an open modal.
    pub fn handle_click(&mut self, col: u16, row: u16) -> Option<Action> {
        self.handle_click_at(std::time::Instant::now(), col, row)
    }

    /// Same as `handle_click`, but the caller supplies `now`. Used by
    /// unit tests to drive double-click detection deterministically
    /// without relying on `thread::sleep`.
    pub(super) fn handle_click_at(
        &mut self,
        now: std::time::Instant,
        col: u16,
        row: u16,
    ) -> Option<Action> {
        let abs_idx = self.resolve_row_to_index(col, row)?;

        let is_double_click = matches!(
            self.last_click,
            Some((prev_time, _, prev_row))
                if prev_row == row
                    && now.duration_since(prev_time) <= DOUBLE_CLICK_THRESHOLD
        );
        self.last_click = Some((now, col, row));

        let item = self.flat_items[abs_idx].clone();
        if is_double_click {
            // First click already selected the row (and toggled a group);
            // the second click only activates a session. Re-toggling a
            // group on the second click would undo the first toggle and
            // flicker, so groups intentionally swallow the second click.
            //
            // We re-sync `cursor` to `abs_idx` before activating because
            // anything between the two clicks (an arrow keypress, a
            // status-poll-driven re-sort) can move the cursor away from
            // the row the user is actually double-clicking. Without this,
            // `activate_selected_session()` reads `selected_session` —
            // which tracks `cursor`, not the click target — and we'd open
            // the wrong session.
            return match item {
                Item::Session { .. } => {
                    if self.cursor != abs_idx {
                        self.cursor = abs_idx;
                        self.update_selected();
                    }
                    self.activate_selected_session()
                }
                Item::Group { .. } => None,
            };
        }

        match item {
            Item::Group { path, .. } => {
                self.toggle_group_collapsed(&path);
                None
            }
            Item::Session { id, .. } => {
                if self.cursor != abs_idx {
                    self.cursor = abs_idx;
                    self.update_selected();
                }
                // Single-click behavior is user-configurable via
                // `SessionConfig::click_action`. `LiveSend` (default,
                // historical behavior) enters live-send for the clicked
                // row, or switches the live target when already in live
                // mode. `SelectOnly` stops at the cursor update above so
                // the user can browse preview content without ever
                // entering live-send; double-click still activates via
                // `default_attach_mode`. `click_action` returns `None`
                // for cockpit-mode sessions, where `start_live_send`
                // already short-circuits, so the historical fall-through
                // is fine.
                if matches!(
                    self.click_action(&id),
                    Some(crate::session::ClickAction::SelectOnly)
                ) {
                    None
                } else {
                    self.start_live_send()
                }
            }
        }
    }

    /// Record the mouse position from a `MouseEventKind::Moved` event so
    /// the list can render a hover highlight on the row under the cursor.
    /// `mouse_pos` is cleared when the cursor leaves `list_inner_area`.
    /// Returns `true` only when the resolved hovered item changes, so the
    /// caller can skip a redraw on every pixel-level mouse twitch.
    pub fn handle_hover(&mut self, col: u16, row: u16) -> bool {
        let new_pos = if self.list_inner_area.contains(Position::from((col, row))) {
            Some((col, row))
        } else {
            None
        };
        let prev_idx = self.hovered_index();
        self.mouse_pos = new_pos;
        let new_idx = self.hovered_index();
        prev_idx != new_idx
    }

    /// Route a mouse-wheel-down at (col, row); see handle_scroll_up.
    pub fn handle_scroll_down(&mut self, col: u16, row: u16) -> bool {
        const STEP: u16 = 3;
        // Mirror handle_scroll_up: a stale highlight pinned to cells
        // whose content just moved would mislead, so drop it first.
        self.clear_preview_selection();
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_down(STEP);
            return true;
        }
        // See handle_scroll_up for the live-send / has_dialog reasoning.
        if self.live_send.is_some() {
            if !self.hit_preview(col, row) {
                return false;
            }
        } else {
            if self.has_dialog() {
                return false;
            }
            if self.hit_list(col, row) {
                self.move_cursor(1);
                return true;
            }
            if !self.hit_preview(col, row) {
                return false;
            }
        }
        if self.selected_session.is_none() {
            return false;
        }
        if self.preview_scroll_offset == 0 {
            return false;
        }
        self.preview_scroll_offset = self.preview_scroll_offset.saturating_sub(STEP);
        true
    }

    /// Route a bracketed paste event to the active text input dialog.
    ///
    /// Live-send mode wins above every dialog: a paste while the user is
    /// "attached" should stream straight to the agent's pane, not buffer
    /// in a dialog the user isn't even looking at. Text-input dialogs
    /// (rename / send_message / new) come next so multi-line dictation
    /// lands in whichever dialog the user is actively typing into. The
    /// settings view is checked last; its paste handler strips newlines,
    /// which would destroy multi-line dictation if we checked it first.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(state) = self.live_send.clone() {
            // Mirror the live-send key path: any interaction dismisses
            // the finalized highlight so it doesn't follow agent output
            // through subsequent renders.
            self.clear_preview_selection();
            if let Some(worker) = &self.live_send_worker {
                for key in split_paste_for_live_send(text) {
                    worker.send(key);
                }
            }
            self.stamp_last_accessed(&state.session_id);
            return;
        }
        if let Some(ref mut dialog) = self.rename_dialog {
            dialog.handle_paste(text);
            return;
        }
        if let Some(ref mut dialog) = self.send_message_dialog {
            dialog.handle_paste(text);
            return;
        }
        if let Some(ref mut dialog) = self.new_dialog {
            dialog.handle_paste(text);
            return;
        }
        if let Some(ref mut settings) = self.settings_view {
            settings.handle_paste(text);
            return;
        }

        // No dialog open: route the paste into a new compose dialog if the
        // selected session is runnable. If not, stash in pending_paste so the
        // next dialog open (typically the next `m` press) drains it. Never
        // throw voice text on the floor; losing dictation is worse than
        // silently catching it.
        if let Some((id, title)) = self.resolve_paste_target() {
            self.pending_send_session = Some(id);
            let mut dialog = SendMessageDialog::new(&title);
            dialog.handle_paste(text);
            self.send_message_dialog = Some(dialog);
            return;
        }

        // No running sessions at all (or all Creating). Stash for later;
        // the user will see the text on next 'm' / dialog open.
        match self.pending_paste.as_mut() {
            Some(buf) => buf.push_str(text),
            None => self.pending_paste = Some(text.to_string()),
        }
    }

    /// Open the restart dialog for the currently-selected session. The dialog
    /// pre-fills profile + AI engine from the instance's current values, and on
    /// submit restarts the session, optionally migrating to the picked profile
    /// and/or swapping the AI engine. No-op if no session is selected or the
    /// selected session is mid-transition.
    fn open_restart_dialog(&mut self) {
        // Match the new-session paths: bail with the no-agents modal if no
        // tool is installed, instead of opening a picker with an empty
        // tool list the user would have to submit blank.
        if !self.available_tools.any_available() {
            self.show_no_agents();
            return;
        }
        let Some(id) = self.selected_session.clone() else {
            return;
        };
        let Some(inst) = self.get_instance(&id) else {
            return;
        };
        if matches!(inst.status, Status::Deleting | Status::Creating) {
            return;
        }
        let current_title = inst.title.clone();
        let current_profile = if inst.source_profile.is_empty() {
            self.active_profile
                .clone()
                .unwrap_or_else(|| "default".to_string())
        } else {
            inst.source_profile.clone()
        };
        let current_tool = inst.tool.clone();
        let profiles = list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
        let tools: Vec<String> = self.available_tools.available_list().to_vec();
        self.restart_dialog = Some(RestartDialog::new(
            &current_title,
            &current_profile,
            &current_tool,
            profiles,
            tools,
        ));
    }

    /// Attempt to enter live-send mode against the currently-selected
    /// session. Unlike `resolve_paste_target`, this does NOT require
    /// the tmux pane to already exist: `prepare_live_send` calls
    /// `ensure_pane_ready` which revives stopped sessions (Docker
    /// start, splash wait, resume cascade). Without this relaxation
    /// Tab would silently no-op on dead-but-recoverable rows and the
    /// "Reviving..." toast plumbing would never fire.
    ///
    /// Still no-ops on group headers, empty lists, and Creating rows
    /// (no instance yet, nothing to revive). Also no-ops when the
    /// selected session is already the live-send target so click-to-
    /// enter doesn't re-run ensure_pane_ready / drop the live worker
    /// when the user clicks the same row twice.
    pub(super) fn start_live_send(&mut self) -> Option<Action> {
        let id = self.selected_session.clone()?;
        if self.live_send.as_ref().is_some_and(|s| s.session_id == id) {
            return None;
        }
        let inst = self.get_instance(&id)?;
        if matches!(inst.status, Status::Creating | Status::Deleting) {
            return None;
        }
        // Cockpit-mode sessions are not tmux-backed (HomeView's attach
        // path special-cases them away from tmux). Live-send has no
        // target in that mode, so silently no-op rather than enqueue
        // an Action::EnterLiveSend that would fail downstream.
        if inst.is_cockpit_mode() {
            return None;
        }
        Some(Action::EnterLiveSend(id))
    }

    /// Translate one key event in live-send mode and hand the result to
    /// the background worker. The worker owns the tmux Session and runs
    /// `send-keys` off the UI thread so a slow fork+exec never blocks
    /// the redraw loop; literal-key runs coalesce into a single tmux
    /// call so fast typing isn't N forks. Ctrl+q clears `live_send`
    /// and drops the worker (which closes its channel, exiting the
    /// thread cleanly on the next iteration).
    ///
    /// Before dispatching we re-verify that the target session still
    /// exists at the same tmux name as it had at entry time. If a peer
    /// process deleted the session or a rename diverged the name from
    /// what the worker is targeting, the user would otherwise type
    /// into the void with only a `tracing::warn!` for company. Auto-
    /// exit + info dialog instead.
    fn handle_live_send_key(&mut self, key: KeyEvent) {
        let Some(state) = self.live_send.clone() else {
            return;
        };

        // `handle_key` already cleared any finalized preview
        // selection at the top, so the highlight doesn't linger
        // across the keystroke that switched the user out of
        // copy-and-look mode. The PageUp/PageDown scroll keys below
        // would otherwise need their own dismissal; the shared
        // top-of-handle_key clear covers them too.

        // Shift+PageUp / Shift+PageDown scroll the preview pane
        // without forwarding to the agent. Matches the terminal-
        // emulator convention (xterm, gnome-terminal, iTerm, etc.)
        // where shift+page operates on the outer scrollback, not the
        // inner program. Bare PageUp/PageDown still goes to the agent
        // so agents that page their own UI keep working.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if shift && !ctrl && !alt {
            const PAGE_STEP: u16 = 10;
            match key.code {
                KeyCode::PageUp => {
                    self.preview_scroll_offset =
                        self.preview_scroll_offset.saturating_add(PAGE_STEP);
                    return;
                }
                KeyCode::PageDown => {
                    self.preview_scroll_offset =
                        self.preview_scroll_offset.saturating_sub(PAGE_STEP);
                    return;
                }
                _ => {}
            }
        }

        // Exit-chord check runs before the drift check: exiting is
        // always safe, and the user pressing the chord to escape a
        // drifted/stuck live mode shouldn't hit a "session ended"
        // dialog on the way out.
        if live_send::chord_list_matches(&state.exit_chords, key) {
            self.exit_live_send_and_restore_sizing(&state);
            return;
        }
        if let Some(reason) = self.live_send_drift_reason(&state) {
            self.exit_live_send_and_restore_sizing(&state);
            self.info_dialog = Some(InfoDialog::new("Live send ended", reason));
            return;
        }
        match live_send::translate(key) {
            live_send::LiveDispatch::Ignore => {}
            live_send::LiveDispatch::Send(tmux_key) => {
                if let Some(worker) = &self.live_send_worker {
                    worker.send(tmux_key);
                }
                self.stamp_last_accessed(&state.session_id);
            }
        }
    }

    /// Tear down live-send state and restore the tmux window's
    /// automatic sizing policy. live-send's per-keystroke resize loop
    /// forces tmux into manual sizing; if we leave it that way, the
    /// next tmux attach from a full-size terminal stays cramped at
    /// the preview-pane dimensions live-send left behind. Re-setting
    /// `window-size latest` is best-effort: failures are swallowed so
    /// a stuck pane never blocks the user's exit.
    fn exit_live_send_and_restore_sizing(&mut self, state: &live_send::LiveSendState) {
        let session = crate::tmux::Session::from_name(&state.tmux_name);
        session.reset_size_to_latest_client();
        self.live_send = None;
        self.live_send_worker = None;
        self.live_send_last_resize = None;
        // Preview selections also work outside live mode now, but a
        // live-mode highlight pins to the live-resized pane coords,
        // and exiting reflows the preview back to its normal size.
        // Drop the selection so the highlight can't survive into a
        // pane it no longer points at.
        self.clear_preview_selection();
    }

    /// Returns `Some(reason)` if the live-send target has drifted out
    /// from under us between entry and now. Three drift modes:
    /// - Instance row deleted (peer / web cockpit / another aoe killed
    ///   it).
    /// - Title renamed (which regenerates the tmux session name; the
    ///   worker is now targeting a stale name).
    /// - tmux session itself is gone (`tmux kill-session`, server
    ///   restart) even though our instance row says otherwise. We use
    ///   the existing `session_exists_from_cache` lookup so this costs
    ///   a hashmap probe per keystroke (the status poller refreshes
    ///   the cache every 500ms anyway). If the cache has no entry
    ///   (`None`, e.g. before first refresh) we don't claim drift; the
    ///   instance + name checks above are still the load-bearing
    ///   safety net.
    ///
    /// The caller uses the message verbatim in the info dialog, so
    /// phrase it as a user-facing sentence.
    fn live_send_drift_reason(&self, state: &live_send::LiveSendState) -> Option<&'static str> {
        let Some(inst) = self.get_instance(&state.session_id) else {
            return Some("Session was deleted while live mode was active.");
        };
        let current_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
        if current_name != state.tmux_name {
            return Some("Session was renamed while live mode was active.");
        }
        if crate::tmux::session_exists_from_cache(&state.tmux_name) == Some(false) {
            return Some("tmux pane went away while live mode was active.");
        }
        None
    }

    /// Open the send-message dialog for the currently-selected running session.
    /// If pending_paste has accumulated text from earlier untargeted pastes,
    /// drain it into the dialog so voice/dictation captured before a session
    /// was picked still gets used. No-op if no running session is targetable.
    fn open_send_message_dialog(&mut self) {
        let Some((id, title)) = self.resolve_paste_target() else {
            return;
        };
        self.pending_send_session = Some(id);
        let mut dialog = SendMessageDialog::new(&title);
        if let Some(buf) = self.pending_paste.take() {
            if !buf.is_empty() {
                dialog.handle_paste(&buf);
            }
        }
        self.send_message_dialog = Some(dialog);
    }

    /// Resolve a target session id + title for an untargeted paste/type-burst.
    /// Only returns Some when an explicit, runnable session is selected.
    ///
    /// Cases that return None (caller stashes to `pending_paste`):
    /// - Cursor on a group header (`selected_session` is None).
    /// - No selection at all (empty list, no sessions).
    /// - Selected session is non-running (Stopped, Error, Creating, or tmux
    ///   pane gone).
    ///
    /// Why no first-running fallback: silently dispatching paste/dictation
    /// to "whichever session sorts first" misroutes voice messages across
    /// groups. A user with cursor on the "backend" group expanding it to
    /// browse, dictating, and having the paste land in a "frontend" session
    /// is exactly the misrouting the archived-selection fix is preventing.
    /// Stashing to `pending_paste` is strictly better: the status-bar
    /// indicator surfaces the captured count, and the next `m` against a
    /// runnable selection drains it into the compose dialog.
    ///
    /// Defensive fall-through: when `selected_session` references an id
    /// that no longer maps to an instance (deleted underneath us between
    /// select and paste, shouldn't happen in steady state), we also stash
    /// rather than reroute.
    fn resolve_paste_target(&self) -> Option<(String, String)> {
        let pick = |inst: &crate::session::Instance| -> Option<(String, String)> {
            if inst.status == Status::Creating {
                return None;
            }
            let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title).ok();
            if tmux_session.as_ref().is_some_and(|s| s.exists()) {
                Some((inst.id.clone(), inst.title.clone()))
            } else {
                None
            }
        };

        let id = self.selected_session.as_ref()?;
        let inst = self.get_instance(id)?;
        pick(inst)
    }

    /// Strict-mode typing guard: a bare lowercase letter was pressed outside
    /// navigation (j/k/h/l). Treat it as inadvertent typing; open the compose
    /// dialog for the selected session pre-filled with that character. Mirrors
    /// handle_paste's dialog-delegation + fallback logic.
    fn capture_letter_to_compose(&mut self, c: char) {
        let s = c.to_string();
        if let Some(ref mut dialog) = self.send_message_dialog {
            dialog.handle_paste(&s);
            return;
        }
        if let Some(ref mut dialog) = self.new_dialog {
            dialog.handle_paste(&s);
            return;
        }
        if let Some(ref mut dialog) = self.rename_dialog {
            dialog.handle_paste(&s);
            return;
        }

        if let Some((id, title)) = self.resolve_paste_target() {
            self.pending_send_session = Some(id);
            let mut dialog = SendMessageDialog::new(&title);
            dialog.handle_paste(&s);
            self.send_message_dialog = Some(dialog);
            return;
        }

        match self.pending_paste.as_mut() {
            Some(buf) => buf.push_str(&s),
            None => self.pending_paste = Some(s),
        }
    }

    /// Re-score matches after a reload without moving the cursor.
    pub(super) fn refresh_search_matches(&mut self) {
        let query = self.search_query.value();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_match_index = 0;
            return;
        }

        use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<(usize, u16)> = Vec::new();
        let mut buf = Vec::new();

        for (idx, item) in self.flat_items.iter().enumerate() {
            let haystack = match item {
                Item::Session { id, .. } => {
                    if let Some(inst) = self.get_instance(id) {
                        format!("{} {}", inst.title, inst.project_path)
                    } else {
                        continue;
                    }
                }
                Item::Group { name, path, .. } => {
                    format!("{} {}", name, path)
                }
            };

            let haystack_utf32 = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = atom.score(haystack_utf32, &mut matcher) {
                scored.push((idx, score));
            }
        }

        scored.sort_by_key(|a| std::cmp::Reverse(a.1));
        self.search_matches = scored.into_iter().map(|(idx, _)| idx).collect();
        // Clamp match_index in case matches shrank
        if self.search_matches.is_empty() {
            self.search_match_index = 0;
        } else if self.search_match_index >= self.search_matches.len() {
            self.search_match_index = self.search_matches.len() - 1;
        }
    }

    pub(super) fn update_search(&mut self) {
        self.search_matches.clear();
        self.search_match_index = 0;

        let query = self.search_query.value();
        if query.is_empty() {
            return;
        }

        use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<(usize, u16)> = Vec::new();
        let mut buf = Vec::new();

        for (idx, item) in self.flat_items.iter().enumerate() {
            let haystack = match item {
                Item::Session { id, .. } => {
                    if let Some(inst) = self.get_instance(id) {
                        format!("{} {}", inst.title, inst.project_path)
                    } else {
                        continue;
                    }
                }
                Item::Group { name, path, .. } => {
                    format!("{} {}", name, path)
                }
            };

            let haystack_utf32 = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = atom.score(haystack_utf32, &mut matcher) {
                scored.push((idx, score));
            }
        }

        scored.sort_by_key(|a| std::cmp::Reverse(a.1));
        self.search_matches = scored.into_iter().map(|(idx, _)| idx).collect();

        if let Some(&best) = self.search_matches.first() {
            self.cursor = best;
            self.update_selected();
        }
    }

    /// Continue session creation after agent hooks acknowledgment.
    /// Runs the repo hook trust check and then creates the session.
    fn continue_session_creation(&mut self, data: NewSessionData) -> Option<Action> {
        match repo_config::check_hook_trust(std::path::Path::new(&data.path)) {
            Ok(repo_config::HookTrustStatus::NeedsTrust { hooks, hooks_hash }) => {
                use crate::tui::dialogs::HookTrustDialog;
                self.hook_trust_dialog =
                    Some(HookTrustDialog::new(hooks, hooks_hash, data.path.clone()));
                self.pending_hook_trust_data = Some(data);
                None
            }
            Ok(repo_config::HookTrustStatus::Trusted(repo_hooks)) => {
                let merged = repo_config::merge_hooks_with_config(&data.profile, repo_hooks);
                self.create_session_with_hooks(data, merged)
            }
            Ok(repo_config::HookTrustStatus::NoHooks) => {
                let fallback = repo_config::resolve_global_profile_hooks(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
            Err(e) => {
                tracing::warn!(target: "tui.input", "Failed to check repo hooks: {}", e);
                let fallback = repo_config::resolve_global_profile_hooks(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
        }
    }

    /// Create a session with optional hooks. Delegates to the background
    /// `CreationPoller` when hooks are present, when the session is sandboxed,
    /// or when a worktree branch is requested (to avoid freezing the TUI on
    /// slow git hooks like `post-checkout`).
    pub(super) fn create_session_with_hooks(
        &mut self,
        data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) -> Option<Action> {
        let has_hooks = hooks
            .as_ref()
            .is_some_and(|h| !h.on_create.is_empty() || !h.on_launch.is_empty());
        let has_worktree = data.worktree_enabled;

        if data.sandbox || has_hooks || has_worktree {
            self.request_creation(data, hooks);
            return None;
        }

        match self.create_session(data) {
            Ok(session_id) => {
                self.new_dialog = None;
                Some(Action::AttachAfterCreate(session_id))
            }
            Err(e) => {
                tracing::error!(target: "tui.input", "Failed to create session: {}", e);
                if let Some(dialog) = &mut self.new_dialog {
                    dialog.set_error(e.to_string());
                }
                None
            }
        }
    }

    /// In strict_hotkeys mode, normalize key events so the main match block
    /// doesn't need per-key duplication. Returns `None` to swallow bare
    /// lowercase action letters that would otherwise fire destructive actions.
    fn normalize_strict_key(&self, key: KeyEvent) -> Option<KeyEvent> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let bare = key.modifiers == KeyModifiers::NONE;
        let shift_only = key.modifiers == KeyModifiers::SHIFT;
        let has_search = !self.search_matches.is_empty();

        // n/N are dual-purpose: search next/prev AND new session/new-from-selection.
        // When search matches exist, let them through unchanged for vi-style navigation.
        if has_search {
            match key.code {
                KeyCode::Char('n') if bare => return Some(key),
                KeyCode::Char('N') if bare || shift_only => return Some(key),
                _ => {}
            }
        }

        match key.code {
            // Ctrl+letter relocations: map to the uppercase letter they replace
            // Ctrl+T -> T (attach terminal), Ctrl+D -> D (diff view),
            // Ctrl+R -> R (serve), Ctrl+P -> P (profiles), Ctrl+N -> N (new from selection)
            KeyCode::Char(c @ ('t' | 'd' | 'r' | 'p' | 'n')) if ctrl => Some(KeyEvent::new(
                KeyCode::Char(c.to_ascii_uppercase()),
                KeyModifiers::NONE,
            )),
            // Ctrl+G and Ctrl+O stay as-is. The dispatch table already has
            // strict-mode arms that match `Char('g')`/`Char('o')` *with*
            // the CTRL modifier; stripping CTRL here would make the
            // post-normalize key indistinguishable from bare lowercase
            // input and route Ctrl+G into the typing-guard catch-all.
            KeyCode::Char('g') if ctrl => Some(key),
            KeyCode::Char('o') if ctrl => Some(key),
            // Shifted action letters pass through unchanged. Each letter has its
            // own `Char('UPPER') if self.strict_hotkeys` arm in the main match.
            // Lowercasing here would route the chord into a dead arm guarded
            // `if !self.strict_hotkeys`, so the action would silently no-op.
            // Affects D (delete), R (rename), N, X, S, M, T, C, Q, O.
            //
            // Side benefit: passing through unchanged also makes the chords work
            // on iOS Mosh, where Shift+letter is delivered as the bare uppercase
            // keycode without a Shift modifier.
            // Bare lowercase letters pass through; the main match falls through
            // to a catch-all that opens the compose dialog pre-filled with the
            // letter (strict-mode typing-guard). Navigation keys j/k/h/l are
            // handled by their own arms before the catch-all fires.
            _ => Some(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{SessionConfig, ToolSessionConfig};

    #[test]
    fn hook_install_agent_uses_detect_as_for_custom_codex_wrapper() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("wrapped-codex".to_string(), "codex".to_string());

        let agent = resolve_hook_install_agent("wrapped-codex", &config).unwrap();

        assert_eq!(agent.name, "codex");
    }

    #[test]
    fn hook_install_agent_keeps_builtin_agent_resolution_first() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("opencode".to_string(), "codex".to_string());

        assert!(resolve_hook_install_agent("opencode", &config).is_none());
    }

    #[test]
    fn hook_install_agent_ignores_unknown_detect_as_target() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("wrapped-agent".to_string(), "missing-agent".to_string());

        assert!(resolve_hook_install_agent("wrapped-agent", &config).is_none());
    }

    #[test]
    fn parse_hotkey_accepts_alt_letter() {
        let (code, mods) = parse_hotkey("Alt+g").expect("valid");
        assert_eq!(code, KeyCode::Char('g'));
        assert_eq!(mods, KeyModifiers::ALT);
    }

    #[test]
    fn parse_hotkey_is_case_insensitive_on_modifier() {
        assert!(parse_hotkey("alt+g").is_some());
        assert!(parse_hotkey("ALT+g").is_some());
        assert!(parse_hotkey("aLt+g").is_some());
    }

    #[test]
    fn parse_hotkey_normalizes_letter_to_lowercase() {
        let (code, _) = parse_hotkey("Alt+G").expect("valid");
        assert_eq!(code, KeyCode::Char('g'));
    }

    #[test]
    fn parse_hotkey_accepts_digit() {
        let (code, mods) = parse_hotkey("Alt+1").expect("valid");
        assert_eq!(code, KeyCode::Char('1'));
        assert_eq!(mods, KeyModifiers::ALT);
    }

    #[test]
    fn parse_hotkey_rejects_non_alt_modifier() {
        assert!(parse_hotkey("Ctrl+g").is_none());
        assert!(parse_hotkey("Shift+g").is_none());
        assert!(parse_hotkey("Cmd+g").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_multi_char_key() {
        assert!(parse_hotkey("Alt+gg").is_none());
        assert!(parse_hotkey("Alt+F1").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_missing_modifier() {
        assert!(parse_hotkey("g").is_none());
        assert!(parse_hotkey("Alt").is_none());
        assert!(parse_hotkey("").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_wrong_separator() {
        assert!(parse_hotkey("Alt-g").is_none());
        assert!(parse_hotkey("Alt g").is_none());
    }

    #[test]
    fn validate_tool_hotkeys_reports_each_invalid_entry() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "yazi".to_string(),
            ToolSessionConfig {
                command: "yazi".into(),
                hotkey: Some("Ctrl+f".into()),
            },
        );
        tools.insert(
            "tig".to_string(),
            ToolSessionConfig {
                command: "tig".into(),
                hotkey: Some("Alt+too-long".into()),
            },
        );
        let warnings = validate_tool_hotkeys(&tools);
        assert_eq!(warnings.len(), 2);
        let joined = warnings.join("|");
        assert!(joined.contains("yazi"));
        assert!(joined.contains("tig"));
        assert!(!joined.contains("lazygit"));
    }

    #[test]
    fn validate_tool_hotkeys_empty_when_all_valid_or_unset() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "rg".to_string(),
            ToolSessionConfig {
                command: "rg --files".into(),
                hotkey: None,
            },
        );
        assert!(validate_tool_hotkeys(&tools).is_empty());
    }

    #[test]
    fn build_tool_hotkey_cache_sorts_by_name_and_skips_invalid() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "zoxide".to_string(),
            ToolSessionConfig {
                command: "z".into(),
                hotkey: Some("Alt+z".into()),
            },
        );
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "broken".to_string(),
            ToolSessionConfig {
                command: "x".into(),
                hotkey: Some("Ctrl+x".into()),
            },
        );
        tools.insert(
            "no-hotkey".to_string(),
            ToolSessionConfig {
                command: "y".into(),
                hotkey: None,
            },
        );

        let cache = build_tool_hotkey_cache(&tools);
        // Two valid entries, sorted by name.
        assert_eq!(cache.len(), 2);
        assert_eq!(cache[0].0, "lazygit");
        assert_eq!(cache[0].1, KeyCode::Char('g'));
        assert_eq!(cache[0].2, KeyModifiers::ALT);
        assert_eq!(cache[1].0, "zoxide");
        assert_eq!(cache[1].1, KeyCode::Char('z'));
    }

    #[test]
    fn build_tool_hotkey_cache_tie_break_favors_alphabetically_first_name() {
        let mut tools = std::collections::HashMap::new();
        // Both bind Alt+g; alphabetical winner is "alpha".
        tools.insert(
            "beta".to_string(),
            ToolSessionConfig {
                command: "b".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "alpha".to_string(),
            ToolSessionConfig {
                command: "a".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        let cache = build_tool_hotkey_cache(&tools);
        assert_eq!(cache[0].0, "alpha");
        assert_eq!(cache[1].0, "beta");
    }
}
