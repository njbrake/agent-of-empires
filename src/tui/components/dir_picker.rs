//! Directory picker overlay component

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use crate::tui::styles::Theme;

pub enum DirPickerResult {
    Continue,
    Cancelled,
    Selected(String),
}

pub struct DirPicker {
    active: bool,
    filter: Input,
    selected: usize,
    cwd: PathBuf,
    dirs: Vec<String>,
    /// True when read_dir failed (e.g. permission denied)
    read_error: bool,
    show_hidden: bool,
}

impl Default for DirPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl DirPicker {
    pub fn new() -> Self {
        Self {
            active: false,
            filter: Input::default(),
            selected: 0,
            cwd: PathBuf::new(),
            dirs: Vec::new(),
            read_error: false,
            show_hidden: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self, initial_path: &str) {
        let path = if initial_path.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        } else {
            let p = PathBuf::from(initial_path);
            if p.is_dir() {
                p
            } else {
                p.parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"))
            }
        };
        self.cwd = path;
        self.filter = Input::default();
        self.selected = 0;
        self.refresh_dirs();
        self.active = true;
    }

    fn refresh_dirs(&mut self) {
        let mut dirs = Vec::new();
        match std::fs::read_dir(&self.cwd) {
            Ok(entries) => {
                self.read_error = false;
                for entry in entries.flatten() {
                    // Follow symlinks: entry.path().is_dir() resolves symlinks,
                    // unlike entry.file_type().is_dir() which does not.
                    if entry.path().is_dir() {
                        if let Some(name) = entry.file_name().to_str() {
                            if self.show_hidden || !name.starts_with('.') {
                                dirs.push(name.to_string());
                            }
                        }
                    }
                }
            }
            Err(_) => {
                self.read_error = true;
            }
        }
        dirs.sort_by_key(|a| a.to_lowercase());
        self.dirs = dirs;
    }

    fn filtered_dirs(&self) -> Vec<String> {
        let filter = self.filter.value().to_lowercase();
        let has_parent = self.cwd.parent().is_some();

        let mut result = Vec::new();
        if has_parent && (filter.is_empty() || "..".starts_with(&filter)) {
            result.push("../".to_string());
        }

        for d in &self.dirs {
            if filter.is_empty() || d.to_lowercase().contains(&filter) {
                result.push(d.clone());
            }
        }
        result
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DirPickerResult {
        if key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.show_hidden = !self.show_hidden;
            self.selected = 0;
            self.refresh_dirs();
            return DirPickerResult::Continue;
        }

        let filtered = self.filtered_dirs();
        let filtered_len = filtered.len();

        match key.code {
            KeyCode::Esc => {
                self.active = false;
                DirPickerResult::Cancelled
            }
            KeyCode::Enter => {
                if filtered_len == 0 {
                    return DirPickerResult::Continue;
                }
                if self.selected < filtered_len {
                    let selected_name = &filtered[self.selected];
                    if selected_name == "../" {
                        if let Some(parent) = self.cwd.parent() {
                            self.active = false;
                            return DirPickerResult::Selected(parent.to_string_lossy().to_string());
                        }
                    } else {
                        self.active = false;
                        let path = self.cwd.join(selected_name);
                        return DirPickerResult::Selected(path.to_string_lossy().to_string());
                    }
                }
                self.active = false;
                DirPickerResult::Selected(self.cwd.to_string_lossy().to_string())
            }
            KeyCode::Tab => {
                if filtered_len > 0 && self.selected < filtered_len {
                    let selected_name = &filtered[self.selected];
                    if selected_name == "../" {
                        if let Some(parent) = self.cwd.parent() {
                            self.cwd = parent.to_path_buf();
                            self.filter = Input::default();
                            self.selected = 0;
                            self.refresh_dirs();
                        }
                    } else {
                        self.cwd = self.cwd.join(selected_name);
                        self.filter = Input::default();
                        self.selected = 0;
                        self.refresh_dirs();
                    }
                }
                DirPickerResult::Continue
            }
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                DirPickerResult::Continue
            }
            KeyCode::Down => {
                if filtered_len > 0 && self.selected < filtered_len - 1 {
                    self.selected += 1;
                }
                DirPickerResult::Continue
            }
            KeyCode::Backspace => {
                if self.filter.value().is_empty() {
                    if let Some(parent) = self.cwd.parent() {
                        self.cwd = parent.to_path_buf();
                        self.selected = 0;
                        self.refresh_dirs();
                    }
                } else {
                    self.filter.handle_event(&crossterm::event::Event::Key(key));
                    self.selected = 0;
                }
                DirPickerResult::Continue
            }
            KeyCode::Char(_) => {
                self.filter.handle_event(&crossterm::event::Event::Key(key));
                self.selected = 0;
                DirPickerResult::Continue
            }
            _ => DirPickerResult::Continue,
        }
    }

    /// Truncate a path display string from the left to fit within max_len characters,
    /// prefixing with "..." when truncated.
    fn truncate_path(path: &str, max_len: usize) -> String {
        let char_count = path.chars().count();
        if char_count <= max_len {
            return path.to_string();
        }
        let ellipsis = "...";
        let ellipsis_len = ellipsis.len(); // 3, all ASCII
        let available = max_len.saturating_sub(ellipsis_len);
        if available == 0 {
            return ellipsis.chars().take(max_len).collect();
        }
        // Take `available` characters from the right end of the path
        let skip = char_count - available;
        let tail: String = path.chars().skip(skip).collect();
        format!("{}{}", ellipsis, tail)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let filtered = self.filtered_dirs();
        let max_visible: usize = 10;
        let list_height = filtered.len().min(max_visible) as u16;
        // filter input (1) + spacer (1) + list + hint (1) + borders (2) + margin (2)
        let dialog_height = (list_height + 7).min(area.height);
        let dialog_width: u16 = 60.min(area.width.saturating_sub(4));

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);
        frame.render_widget(Clear, dialog_area);

        // " Browse: <path> " with border chars leaves dialog_width - 2 for content,
        // and the "Browse: " prefix + spaces take 10 chars.
        let max_path_len = (dialog_width as usize).saturating_sub(12);
        let path_display = Self::truncate_path(&self.cwd.to_string_lossy(), max_path_len);
        let title = format!(" Browse: {} ", path_display);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(title)
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // filter input
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // hint
            ])
            .split(inner);

        // Filter input
        let filter_value = self.filter.value();
        let filter_line = Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.text)),
            Span::styled(filter_value, Style::default().fg(theme.accent).bold()),
            Span::styled("_", Style::default().fg(theme.accent)),
        ]);
        frame.render_widget(Paragraph::new(filter_line), chunks[0]);

        // Directory list with scrolling
        let visible_height = chunks[2].height as usize;
        let scroll_offset = if visible_height > 0 && self.selected >= visible_height {
            self.selected - visible_height + 1
        } else {
            0
        };

        let mut lines: Vec<Line> = Vec::new();
        if self.read_error {
            lines.push(Line::from(Span::styled(
                "  (permission denied)",
                Style::default().fg(theme.dimmed),
            )));
        } else if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (empty directory)",
                Style::default().fg(theme.dimmed),
            )));
        } else {
            let has_more_above = scroll_offset > 0;
            let has_more_below = filtered.len() > scroll_offset + visible_height;

            if has_more_above {
                lines.push(Line::from(Span::styled(
                    format!("  [{} more above]", scroll_offset),
                    Style::default().fg(theme.dimmed),
                )));
            }

            let list_visible = if has_more_above && has_more_below {
                visible_height.saturating_sub(2)
            } else if has_more_above || has_more_below {
                visible_height.saturating_sub(1)
            } else {
                visible_height
            };

            for (i, item) in filtered
                .iter()
                .skip(scroll_offset)
                .take(list_visible)
                .enumerate()
            {
                let abs_idx = i + scroll_offset;
                let is_selected = abs_idx == self.selected;
                let prefix = if is_selected { "> " } else { "  " };
                let style = if is_selected {
                    Style::default().fg(theme.accent).bold()
                } else {
                    Style::default().fg(theme.text)
                };
                let display = if item == "../" {
                    item.clone()
                } else {
                    format!("{}/", item)
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, display),
                    style,
                )));
            }

            if has_more_below {
                let remaining = filtered.len() - scroll_offset - list_visible;
                lines.push(Line::from(Span::styled(
                    format!("  [{} more below]", remaining),
                    Style::default().fg(theme.dimmed),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines), chunks[2]);

        // Hint line
        let hint_line = Line::from(vec![
            Span::styled("Type", Style::default().fg(theme.hint)),
            Span::raw(" filter  "),
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" cd  "),
            Span::styled("C-h", Style::default().fg(theme.hint)),
            Span::raw(if self.show_hidden {
                " hide ."
            } else {
                " show ."
            }),
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" select  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);
        frame.render_widget(Paragraph::new(hint_line), chunks[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Create a temp directory with known subdirectories for deterministic tests.
    fn setup_tempdir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        let base = tmp.path().to_path_buf();
        std::fs::create_dir(base.join("alpha")).unwrap();
        std::fs::create_dir(base.join("beta")).unwrap();
        std::fs::create_dir(base.join("gamma")).unwrap();
        // Create a regular file to verify it's excluded
        std::fs::write(base.join("file.txt"), "hello").unwrap();
        (tmp, base)
    }

    #[test]
    fn test_new_is_inactive() {
        let picker = DirPicker::new();
        assert!(!picker.is_active());
    }

    #[test]
    fn test_activate_sets_cwd_and_lists_dirs() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());
        assert!(picker.is_active());
        assert_eq!(picker.cwd, base);
        assert_eq!(picker.filter.value(), "");
        assert_eq!(picker.selected, 0);
        // Should list 3 dirs, not the file
        assert_eq!(picker.dirs.len(), 3);
        assert!(picker.dirs.contains(&"alpha".to_string()));
        assert!(!picker.dirs.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_activate_with_empty_path() {
        let mut picker = DirPicker::new();
        picker.activate("");
        assert!(picker.is_active());
        assert!(picker.cwd.is_dir());
    }

    #[test]
    fn test_dirs_sorted_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        std::fs::create_dir(base.join("Zebra")).unwrap();
        std::fs::create_dir(base.join("apple")).unwrap();
        std::fs::create_dir(base.join("Banana")).unwrap();

        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());
        assert_eq!(picker.dirs, vec!["apple", "Banana", "Zebra"]);
    }

    #[test]
    fn test_esc_cancels() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        let result = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DirPickerResult::Cancelled));
        assert!(!picker.is_active());
    }

    #[test]
    fn test_enter_selects_highlighted_item() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // selected=0 is "../", so Enter on it selects the parent
        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            DirPickerResult::Selected(path) => {
                assert_eq!(path, base.parent().unwrap().to_string_lossy());
            }
            _ => panic!("Expected Selected"),
        }
        assert!(!picker.is_active());
    }

    #[test]
    fn test_enter_selects_highlighted_subdir() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // Navigate to "alpha" (index 1)
        picker.handle_key(key(KeyCode::Down));
        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            DirPickerResult::Selected(path) => {
                assert_eq!(path, base.join("alpha").to_string_lossy());
            }
            _ => panic!("Expected Selected"),
        }
        assert!(!picker.is_active());
    }

    #[test]
    fn test_backspace_empty_filter_goes_to_parent() {
        let (_tmp, base) = setup_tempdir();
        let child = base.join("alpha");
        let mut picker = DirPicker::new();
        picker.activate(&child.to_string_lossy());

        picker.handle_key(key(KeyCode::Backspace));
        assert_eq!(picker.cwd, base);
    }

    #[test]
    fn test_backspace_with_filter_removes_char() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        picker.handle_key(key(KeyCode::Char('a')));
        assert_eq!(picker.filter.value(), "a");
        picker.handle_key(key(KeyCode::Backspace));
        assert_eq!(picker.filter.value(), "");
    }

    #[test]
    fn test_tab_navigates_into_directory() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // filtered_dirs returns: ["../", "alpha", "beta", "gamma"]
        // Navigate past "../" to "alpha"
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 1);

        picker.handle_key(key(KeyCode::Tab));
        assert_eq!(picker.cwd, base.join("alpha"));
        assert_eq!(picker.filter.value(), "");
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_tab_on_parent_goes_up() {
        let (_tmp, base) = setup_tempdir();
        let child = base.join("alpha");
        let mut picker = DirPicker::new();
        picker.activate(&child.to_string_lossy());

        // "../" is at index 0
        assert_eq!(picker.selected, 0);
        picker.handle_key(key(KeyCode::Tab));
        assert_eq!(picker.cwd, base);
    }

    #[test]
    fn test_navigation_up_down() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // Can't go above 0
        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 0);

        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 1);
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 2);
        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 1);
    }

    #[test]
    fn test_navigation_clamps_at_end() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // 4 items: "../", "alpha", "beta", "gamma"
        for _ in 0..10 {
            picker.handle_key(key(KeyCode::Down));
        }
        assert_eq!(picker.selected, 3);
    }

    #[test]
    fn test_filter_narrows_results() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // "a" matches "alpha", "beta", "gamma" (but not "../")
        picker.handle_key(key(KeyCode::Char('a')));
        let filtered = picker.filtered_dirs();
        assert!(filtered.contains(&"alpha".to_string()));
        assert!(filtered.contains(&"beta".to_string()));
        assert!(filtered.contains(&"gamma".to_string()));

        // "al" matches only "alpha"
        picker.handle_key(key(KeyCode::Char('l')));
        let filtered = picker.filtered_dirs();
        assert_eq!(filtered, vec!["alpha"]);
    }

    #[test]
    fn test_filter_resets_selection() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 2);

        picker.handle_key(key(KeyCode::Char('a')));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_jk_are_filter_chars_not_navigation() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.filter.value(), "j");
        assert_eq!(picker.selected, 0);

        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.filter.value(), "jk");
    }

    #[test]
    fn test_symlinked_dirs_are_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let real_dir = base.join("real");
        std::fs::create_dir(&real_dir).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_dir, base.join("link")).unwrap();
            let mut picker = DirPicker::new();
            picker.activate(&base.to_string_lossy());
            assert!(picker.dirs.contains(&"real".to_string()));
            assert!(picker.dirs.contains(&"link".to_string()));
        }
    }

    #[test]
    fn test_files_excluded_from_listing() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());
        assert!(!picker.dirs.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_parent_entry_not_shown_at_root() {
        let mut picker = DirPicker::new();
        picker.activate("/");
        let filtered = picker.filtered_dirs();
        assert!(!filtered.contains(&"../".to_string()));
    }

    #[test]
    fn test_dotfiles_hidden_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        std::fs::create_dir(base.join(".hidden")).unwrap();
        std::fs::create_dir(base.join("visible")).unwrap();

        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());
        assert!(picker.dirs.contains(&"visible".to_string()));
        assert!(!picker.dirs.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_ctrl_h_toggles_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        std::fs::create_dir(base.join(".hidden")).unwrap();
        std::fs::create_dir(base.join("visible")).unwrap();

        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());
        assert!(!picker.dirs.contains(&".hidden".to_string()));

        let ctrl_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        picker.handle_key(ctrl_h);
        assert!(picker.show_hidden);
        assert!(picker.dirs.contains(&".hidden".to_string()));
        assert!(picker.dirs.contains(&"visible".to_string()));

        picker.handle_key(ctrl_h);
        assert!(!picker.show_hidden);
        assert!(!picker.dirs.contains(&".hidden".to_string()));
    }

    #[test]
    fn test_enter_on_empty_filtered_list_does_nothing() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // Type a filter that matches nothing
        picker.handle_key(key(KeyCode::Char('z')));
        picker.handle_key(key(KeyCode::Char('z')));
        picker.handle_key(key(KeyCode::Char('z')));
        let filtered = picker.filtered_dirs();
        assert!(filtered.is_empty());

        let result = picker.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DirPickerResult::Continue));
        assert!(picker.is_active());
    }

    #[test]
    fn test_unreadable_dir_shows_error() {
        let mut picker = DirPicker::new();
        // Activate on a path that doesn't exist to trigger read_dir failure
        picker.cwd = PathBuf::from("/nonexistent_path_that_should_not_exist");
        picker.refresh_dirs();
        assert!(picker.read_error);
        assert!(picker.dirs.is_empty());
    }

    #[test]
    fn test_parent_filter_only_matches_dots() {
        let (_tmp, base) = setup_tempdir();
        let mut picker = DirPicker::new();
        picker.activate(&base.to_string_lossy());

        // Typing "." should still show "../"
        picker.handle_key(key(KeyCode::Char('.')));
        let filtered = picker.filtered_dirs();
        assert!(filtered.contains(&"../".to_string()));

        // Typing "/" after "." should NOT show "../" (filter is "./" which doesn't match "..")
        picker.handle_key(key(KeyCode::Char('/')));
        let filtered = picker.filtered_dirs();
        assert!(!filtered.contains(&"../".to_string()));
    }

    #[test]
    fn test_truncate_path_short() {
        assert_eq!(DirPicker::truncate_path("/short", 20), "/short");
    }

    #[test]
    fn test_truncate_path_long() {
        let long = "/home/user/very/deeply/nested/directory/structure";
        let truncated = DirPicker::truncate_path(long, 30);
        assert!(truncated.starts_with("..."));
        assert!(truncated.chars().count() <= 30);
        assert!(truncated.ends_with("directory/structure"));
    }

    #[test]
    fn test_truncate_path_exact() {
        let path = "/exact";
        assert_eq!(DirPicker::truncate_path(path, 6), "/exact");
    }

    #[test]
    fn test_truncate_path_multibyte_utf8() {
        let path = "/home/user/projetcs/donnees/repertoire";
        let truncated = DirPicker::truncate_path(path, 20);
        assert!(truncated.starts_with("..."));
        assert!(truncated.chars().count() <= 20);

        // Ensure it doesn't panic on actual multi-byte chars
        let unicode_path = "/home/\u{00e9}\u{00e8}\u{00ea}/\u{00fc}\u{00f6}\u{00e4}/dir";
        let truncated = DirPicker::truncate_path(unicode_path, 10);
        assert!(truncated.starts_with("..."));
        assert!(truncated.chars().count() <= 10);
    }
}
