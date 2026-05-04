//! Command palette dialog: fuzzy-searchable list of named TUI actions.
//!
//! Mirrors the web UI's `CommandPalette` (web/src/components/command-palette/).
//! Activated with Ctrl+K. Each entry maps to either a synthesized `KeyEvent`
//! that the existing input dispatcher consumes (so the palette is additive,
//! not a parallel command implementation), or a "jump to cursor" payload for
//! the dynamic session/group entries.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;
use unicode_width::UnicodeWidthStr;

use super::DialogResult;
use crate::tui::styles::Theme;

/// Group buckets, rendered in this order. Mirrors `web/src/components/command-palette/groups.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteGroup {
    Actions,
    Views,
    Settings,
    Sessions,
    Groups,
}

impl PaletteGroup {
    fn label(&self) -> &'static str {
        match self {
            PaletteGroup::Actions => "Actions",
            PaletteGroup::Views => "Views",
            PaletteGroup::Settings => "Settings",
            PaletteGroup::Sessions => "Sessions",
            PaletteGroup::Groups => "Groups",
        }
    }

    fn order(&self) -> u8 {
        match self {
            PaletteGroup::Actions => 0,
            PaletteGroup::Views => 1,
            PaletteGroup::Settings => 2,
            PaletteGroup::Sessions => 3,
            PaletteGroup::Groups => 4,
        }
    }
}

/// What the dialog asks the input handler to do when the user picks an entry.
pub enum PaletteAction {
    /// Synthesize a key event and run it through the normal action dispatcher.
    /// Bypasses strict-hotkey normalization, so palette entries should encode
    /// the lowercase / non-strict form of the binding (e.g. `n` not `N`).
    Key(KeyEvent),
    /// Move the cursor to a position in `flat_items` (used for session/group jump items).
    JumpToCursor(usize),
}

/// One entry in the palette. `payload` is what gets returned when the user picks it.
pub struct PaletteCommand {
    pub id: &'static str,
    pub title: String,
    pub group: PaletteGroup,
    pub keywords: Vec<&'static str>,
    /// Human-readable hotkey shown on the right (e.g. "n", "Ctrl+D"). Empty if no binding.
    pub hotkey: &'static str,
    pub payload: PaletteAction,
}

/// Pick the right hotkey label for a binding given the strict-hotkeys setting.
/// In strict mode, plain action letters require Shift; relocated bindings use
/// Ctrl. The mapping mirrors `normalize_strict_key` in `home/input.rs` and the
/// labels in `components/help.rs`.
fn hotkey_label(non_strict: &'static str, strict: &'static str, strict_mode: bool) -> &'static str {
    if strict_mode {
        strict
    } else {
        non_strict
    }
}

/// Built-in named commands. Excludes pure-navigation keys (j/k, arrows,
/// PageUp/PageDown, h/l for collapse) since those don't belong in a palette.
/// `strict_hotkeys` controls only the displayed hotkey label; the synthesized
/// payload bypasses strict-mode normalization either way.
pub fn builtin_commands(serve_enabled: bool, strict_hotkeys: bool) -> Vec<PaletteCommand> {
    let mut cmds = vec![
        PaletteCommand {
            id: "new-session",
            title: "New session".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["create", "add", "spawn"],
            hotkey: hotkey_label("n", "N", strict_hotkeys),
            payload: PaletteAction::Key(key('n')),
        },
        PaletteCommand {
            id: "new-from-selection",
            title: "New session from selection".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["create", "duplicate", "clone"],
            hotkey: hotkey_label("N", "Ctrl+N", strict_hotkeys),
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)),
        },
        PaletteCommand {
            id: "attach",
            title: "Attach to selected session".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["open", "enter"],
            hotkey: "Enter",
            payload: PaletteAction::Key(key_code(KeyCode::Enter)),
        },
        PaletteCommand {
            id: "attach-terminal",
            title: "Attach to paired terminal".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["shell", "host"],
            hotkey: hotkey_label("T", "Ctrl+T", strict_hotkeys),
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT)),
        },
        PaletteCommand {
            id: "send-message",
            title: "Send message to agent".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["prompt", "tell", "say"],
            hotkey: hotkey_label("m", "M", strict_hotkeys),
            payload: PaletteAction::Key(key('m')),
        },
        PaletteCommand {
            id: "stop",
            title: "Stop session".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["kill", "end", "halt"],
            hotkey: hotkey_label("x", "X", strict_hotkeys),
            payload: PaletteAction::Key(key('x')),
        },
        PaletteCommand {
            id: "delete",
            title: "Delete session or group".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["remove", "trash"],
            hotkey: hotkey_label("d", "D", strict_hotkeys),
            payload: PaletteAction::Key(key('d')),
        },
        PaletteCommand {
            id: "rename",
            title: "Rename or move to group".to_string(),
            group: PaletteGroup::Actions,
            keywords: vec!["title", "label", "move", "regroup"],
            hotkey: hotkey_label("r", "R", strict_hotkeys),
            payload: PaletteAction::Key(key('r')),
        },
        PaletteCommand {
            id: "diff",
            title: "Open diff view".to_string(),
            group: PaletteGroup::Views,
            keywords: vec!["git", "changes"],
            hotkey: hotkey_label("D", "Ctrl+D", strict_hotkeys),
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT)),
        },
        PaletteCommand {
            id: "toggle-view",
            title: "Toggle Agent / Terminal view".to_string(),
            group: PaletteGroup::Views,
            keywords: vec!["switch", "shell"],
            hotkey: hotkey_label("t", "T", strict_hotkeys),
            payload: PaletteAction::Key(key('t')),
        },
        PaletteCommand {
            id: "cycle-sort",
            title: "Cycle sort order".to_string(),
            group: PaletteGroup::Views,
            keywords: vec!["order", "sort"],
            hotkey: hotkey_label("o", "O", strict_hotkeys),
            payload: PaletteAction::Key(key('o')),
        },
        PaletteCommand {
            id: "cycle-group-by",
            title: "Cycle group by".to_string(),
            group: PaletteGroup::Views,
            keywords: vec!["group", "project"],
            hotkey: hotkey_label("g", "Ctrl+G", strict_hotkeys),
            payload: PaletteAction::Key(key('g')),
        },
        PaletteCommand {
            id: "next-waiting",
            title: "Jump to next waiting / idle session".to_string(),
            group: PaletteGroup::Views,
            keywords: vec!["jump", "next", "waiting", "idle"],
            hotkey: "w",
            payload: PaletteAction::Key(key('w')),
        },
        PaletteCommand {
            id: "settings",
            title: "Open settings".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["preferences", "config"],
            hotkey: hotkey_label("s", "S", strict_hotkeys),
            payload: PaletteAction::Key(key('s')),
        },
        PaletteCommand {
            id: "profiles",
            title: "Switch profile".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["account", "switch"],
            hotkey: "P",
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)),
        },
        PaletteCommand {
            id: "help",
            title: "Show keyboard shortcuts".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["keys", "shortcuts"],
            hotkey: "?",
            payload: PaletteAction::Key(key('?')),
        },
    ];

    if serve_enabled {
        cmds.push(PaletteCommand {
            id: "serve",
            title: "Open serve (LAN / Tunnel)".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["web", "remote", "phone", "tunnel"],
            hotkey: hotkey_label("R", "Ctrl+R", strict_hotkeys),
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
        });
    }

    cmds
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub struct CommandPaletteDialog {
    input: Input,
    /// All entries (built-ins + dynamic session/group jumps), in display order.
    entries: Vec<PaletteCommand>,
    /// Indices into `entries` matching the current query, in score order.
    matches: Vec<usize>,
    /// Cursor within `matches`.
    selected: usize,
}

impl CommandPaletteDialog {
    pub fn new(entries: Vec<PaletteCommand>) -> Self {
        let mut dialog = Self {
            input: Input::default(),
            entries,
            matches: Vec::new(),
            selected: 0,
        };
        dialog.recompute_matches();
        dialog
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<PaletteAction> {
        // Ctrl+K toggles the palette: if the user re-presses the activation
        // key, close it (matches VS Code / cmdk behavior). Without this branch
        // the wildcard arm would forward Ctrl+K to tui_input, which silently
        // discards it, leaving the palette stuck open until Esc.
        if matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return DialogResult::Cancel;
        }
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                DialogResult::Continue
            }
            KeyCode::Down => {
                if !self.matches.is_empty() && self.selected + 1 < self.matches.len() {
                    self.selected += 1;
                }
                DialogResult::Continue
            }
            KeyCode::Enter => {
                let Some(&idx) = self.matches.get(self.selected) else {
                    return DialogResult::Cancel;
                };
                // Move the chosen entry out so we can return its payload by value.
                let cmd = self.entries.swap_remove(idx);
                DialogResult::Submit(cmd.payload)
            }
            _ => {
                self.input.handle_event(&crossterm::event::Event::Key(key));
                self.recompute_matches();
                DialogResult::Continue
            }
        }
    }

    fn recompute_matches(&mut self) {
        use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let query = self.input.value().trim();
        if query.is_empty() {
            // No query: show everything in the original (group, insertion) order.
            self.matches = sort_indices_by_group(&self.entries);
            self.selected = 0;
            return;
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<(usize, u16)> = Vec::new();
        let mut buf = Vec::new();
        for (idx, cmd) in self.entries.iter().enumerate() {
            let mut haystack = cmd.title.clone();
            for kw in &cmd.keywords {
                haystack.push(' ');
                haystack.push_str(kw);
            }
            let h = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = atom.score(h, &mut matcher) {
                scored.push((idx, score));
            }
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        self.matches = scored.into_iter().map(|(idx, _)| idx).collect();
        self.selected = 0;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width: u16 = area.width.saturating_sub(8).clamp(40, 70);
        let dialog_height: u16 = area.height.saturating_sub(6).clamp(10, 20);
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .style(Style::default().bg(theme.background))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent))
            .title(Line::styled(
                " Commands ",
                Style::default().fg(theme.title).bold(),
            ));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // input
                Constraint::Length(1), // separator
                Constraint::Min(1),    // list
                Constraint::Length(1), // hint
            ])
            .split(inner);

        // Input row
        let input_line = Line::from(vec![
            Span::styled("> ", Style::default().fg(theme.accent).bold()),
            Span::styled(self.input.value(), Style::default().fg(theme.text)),
            Span::styled("_", Style::default().fg(theme.accent)),
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[0]);

        // Separator
        let sep = "─".repeat(chunks[1].width as usize);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                sep,
                Style::default().fg(theme.dimmed),
            ))),
            chunks[1],
        );

        // List
        let list_area = chunks[2];
        let visible = list_area.height as usize;
        let scroll_offset = if self.selected >= visible {
            self.selected - visible + 1
        } else {
            0
        };

        let mut lines: Vec<Line> = Vec::new();
        if self.matches.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No matches",
                Style::default().fg(theme.dimmed),
            )));
        } else {
            let mut last_group: Option<PaletteGroup> = None;
            for (display_idx, &entry_idx) in self
                .matches
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible)
            {
                let cmd = &self.entries[entry_idx];

                // Show group header on transition (only when no query, since
                // fuzzy results mix groups by score and headers would be confusing).
                let show_headers = self.input.value().trim().is_empty();
                if show_headers && last_group != Some(cmd.group) {
                    lines.push(Line::from(Span::styled(
                        cmd.group.label(),
                        Style::default().fg(theme.accent).bold(),
                    )));
                    last_group = Some(cmd.group);
                }

                let is_selected = display_idx == self.selected;
                let prefix = if is_selected { "▶ " } else { "  " };
                let title_style = if is_selected {
                    Style::default().fg(theme.title).bold()
                } else {
                    Style::default().fg(theme.text)
                };
                let row_width = list_area.width as usize;
                let hotkey_width = if cmd.hotkey.is_empty() {
                    0
                } else {
                    cmd.hotkey.width() + 2
                };
                let title_max = row_width
                    .saturating_sub(prefix.width())
                    .saturating_sub(hotkey_width);
                let truncated_title = truncate_with_ellipsis(&cmd.title, title_max);
                let title_width = truncated_title.width();
                let pad_len = row_width
                    .saturating_sub(prefix.width())
                    .saturating_sub(title_width)
                    .saturating_sub(hotkey_width);
                let padding = " ".repeat(pad_len);
                let mut spans = vec![
                    Span::styled(prefix, title_style),
                    Span::styled(truncated_title, title_style),
                    Span::raw(padding),
                ];
                if !cmd.hotkey.is_empty() {
                    spans.push(Span::styled(cmd.hotkey, Style::default().fg(theme.hint)));
                }
                lines.push(Line::from(spans));
            }
        }
        frame.render_widget(Paragraph::new(lines), list_area);

        // Hint footer
        let footer_left = Line::from(vec![
            Span::styled("↑↓", Style::default().fg(theme.hint)),
            Span::raw(" navigate  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" run  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" close"),
        ]);
        frame.render_widget(Paragraph::new(footer_left), chunks[3]);
    }
}

/// Truncate a string to fit within `max_cols` terminal columns, appending "…"
/// if cut. Uses Unicode display width (so a wide char like an emoji counts as
/// 2 cells), and only ever cuts on char boundaries so this can't panic on
/// session titles with multi-byte characters.
fn truncate_with_ellipsis(s: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    if max_cols == 1 {
        // Not enough room for ellipsis + content; return original and let the
        // surrounding layout truncate at the column boundary.
        return s.to_string();
    }
    if s.width() <= max_cols {
        return s.to_string();
    }
    // Reserve 1 cell for the ellipsis, then walk char-by-char until adding
    // the next char would exceed the budget. Tracking width per char avoids
    // mid-grapheme byte-slicing.
    let budget = max_cols - 1;
    let mut used = 0usize;
    let mut cut_byte = 0usize;
    for (i, ch) in s.char_indices() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        cut_byte = i + ch.len_utf8();
    }
    format!("{}…", &s[..cut_byte])
}

/// Stable sort: primary by group order, secondary by original insertion order.
/// Used when no query is active so the palette has a predictable layout.
fn sort_indices_by_group(entries: &[PaletteCommand]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by_key(|&i| (entries[i].group.order(), i));
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn ke(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_dialog() -> CommandPaletteDialog {
        CommandPaletteDialog::new(builtin_commands(false, false))
    }

    #[test]
    fn empty_query_shows_all_entries_grouped() {
        let dialog = make_dialog();
        assert_eq!(dialog.matches.len(), dialog.entries.len());
        // First match should be in the Actions group (lowest order).
        let first = &dialog.entries[dialog.matches[0]];
        assert_eq!(first.group, PaletteGroup::Actions);
    }

    #[test]
    fn fuzzy_filters_to_matching_entries() {
        let mut dialog = make_dialog();
        dialog.handle_key(ke(KeyCode::Char('r')));
        dialog.handle_key(ke(KeyCode::Char('e')));
        dialog.handle_key(ke(KeyCode::Char('n')));
        // "ren" should match "Rename or move to group" near the top.
        let top = &dialog.entries[dialog.matches[0]];
        assert!(
            top.title.to_lowercase().contains("rename"),
            "got: {}",
            top.title
        );
    }

    #[test]
    fn keywords_match_searches() {
        // "Move session to group" complaint from issue #889: searching for
        // "move" should surface the rename entry via its keyword.
        let mut dialog = make_dialog();
        for c in "move".chars() {
            dialog.handle_key(ke(KeyCode::Char(c)));
        }
        assert!(!dialog.matches.is_empty(), "'move' should match something");
        let top = &dialog.entries[dialog.matches[0]];
        assert_eq!(top.id, "rename");
    }

    #[test]
    fn enter_submits_payload() {
        let mut dialog = make_dialog();
        // Filter to a known entry so we control which payload comes back.
        for c in "settings".chars() {
            dialog.handle_key(ke(KeyCode::Char(c)));
        }
        let result = dialog.handle_key(ke(KeyCode::Enter));
        match result {
            DialogResult::Submit(PaletteAction::Key(synth)) => {
                assert_eq!(synth.code, KeyCode::Char('s'));
            }
            _ => panic!("expected Submit(Key)"),
        }
    }

    #[test]
    fn esc_cancels() {
        let mut dialog = make_dialog();
        let result = dialog.handle_key(ke(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn ctrl_k_toggles_closed() {
        let mut dialog = make_dialog();
        let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert!(matches!(dialog.handle_key(ctrl_k), DialogResult::Cancel));
        // Same with uppercase K (some terminals send Ctrl+Shift+K as `K`).
        let ctrl_shift_k = KeyEvent::new(KeyCode::Char('K'), KeyModifiers::CONTROL);
        assert!(matches!(
            dialog.handle_key(ctrl_shift_k),
            DialogResult::Cancel
        ));
    }

    #[test]
    fn navigation_clamps() {
        let mut dialog = make_dialog();
        // Up at top stays at 0.
        dialog.handle_key(ke(KeyCode::Up));
        assert_eq!(dialog.selected, 0);

        // Walk to the bottom, then Down should clamp.
        let len = dialog.matches.len();
        for _ in 0..len + 5 {
            dialog.handle_key(ke(KeyCode::Down));
        }
        assert_eq!(dialog.selected, len - 1);
    }

    #[test]
    fn no_match_query_shows_empty() {
        let mut dialog = make_dialog();
        for c in "zzzqxqxq".chars() {
            dialog.handle_key(ke(KeyCode::Char(c)));
        }
        assert!(dialog.matches.is_empty());
        // Enter on empty result should cancel rather than panic.
        let result = dialog.handle_key(ke(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn serve_command_only_with_feature() {
        let with = builtin_commands(true, false);
        let without = builtin_commands(false, false);
        assert!(with.iter().any(|c| c.id == "serve"));
        assert!(!without.iter().any(|c| c.id == "serve"));
    }

    #[test]
    fn hotkey_labels_follow_strict_mode() {
        // Picks one entry whose label moves under strict mode and one whose
        // binding gets relocated to Ctrl. Catches regressions where strict
        // mode was forgotten when adding a new entry.
        let normal = builtin_commands(false, false);
        let strict = builtin_commands(false, true);

        let new_normal = normal.iter().find(|c| c.id == "new-session").unwrap();
        let new_strict = strict.iter().find(|c| c.id == "new-session").unwrap();
        assert_eq!(new_normal.hotkey, "n");
        assert_eq!(new_strict.hotkey, "N");

        let diff_normal = normal.iter().find(|c| c.id == "diff").unwrap();
        let diff_strict = strict.iter().find(|c| c.id == "diff").unwrap();
        assert_eq!(diff_normal.hotkey, "D");
        assert_eq!(diff_strict.hotkey, "Ctrl+D");

        // Bindings without a strict variant (Enter, w, ?, P) stay the same.
        let attach_normal = normal.iter().find(|c| c.id == "attach").unwrap();
        let attach_strict = strict.iter().find(|c| c.id == "attach").unwrap();
        assert_eq!(attach_normal.hotkey, "Enter");
        assert_eq!(attach_strict.hotkey, "Enter");
    }

    #[test]
    fn jump_to_cursor_payload_round_trips() {
        // Build a custom palette with one dynamic jump entry so we can
        // exercise the JumpToCursor path the same way real session items do.
        let entries = vec![PaletteCommand {
            id: "jump-test",
            title: "Jump to my-session".to_string(),
            group: PaletteGroup::Sessions,
            keywords: vec!["session"],
            hotkey: "",
            payload: PaletteAction::JumpToCursor(7),
        }];
        let mut dialog = CommandPaletteDialog::new(entries);
        let result = dialog.handle_key(ke(KeyCode::Enter));
        match result {
            DialogResult::Submit(PaletteAction::JumpToCursor(idx)) => assert_eq!(idx, 7),
            _ => panic!("expected JumpToCursor"),
        }
    }

    #[test]
    fn truncate_handles_multibyte_chars() {
        // Naive byte slicing would panic mid-emoji; this exercises the
        // dynamic "Jump to session: 😀 my-session" rendering path.
        let s = "😀 my-session-with-a-long-title";
        // No-op when string already fits.
        assert_eq!(truncate_with_ellipsis(s, 100), s);
        // Truncation lands on a char boundary and appends ellipsis.
        let out = truncate_with_ellipsis(s, 5);
        assert!(out.ends_with('…'), "got {out:?}");
        assert!(out.width() <= 5);
        // Tiny budget returns the original to avoid producing useless "…".
        assert_eq!(truncate_with_ellipsis(s, 1), s);
        // Pure ASCII still works.
        assert_eq!(truncate_with_ellipsis("hello world", 7), "hello …");

        // Cut budget that lands mid-emoji under any naive char/byte impl:
        // "ab😀cd" is bytes [a, b, 0xF0,0x9F,0x98,0x80, c, d] and the emoji
        // takes 2 display cols. With max_cols=3, budget=2 cells, the function
        // must keep "ab" + ellipsis (the emoji would push to 4 cells).
        let cut = truncate_with_ellipsis("ab😀cd", 3);
        assert_eq!(cut, "ab…");
        assert!(cut.width() <= 3);

        // Max_cols=4 leaves no room for the emoji either (a=1 + b=1 + 😀=2
        // = 4, but we need 1 cell reserved for ellipsis -> budget=3, and
        // a+b+😀 = 4 overflows it). Should still keep "ab".
        let cut = truncate_with_ellipsis("ab😀cd", 4);
        assert_eq!(cut, "ab…");

        // CJK width: each char is 2 cells. With max_cols=5 (budget=4) we
        // keep two CJK chars + ellipsis.
        let cut = truncate_with_ellipsis("中文测试abc", 5);
        assert_eq!(cut, "中文…");
        assert!(cut.width() <= 5);

        // Zero-budget returns empty (the surrounding layout will allocate no
        // visible cells). Previously this returned the full string and
        // overflowed the row.
        assert_eq!(truncate_with_ellipsis("anything", 0), "");
    }
}
