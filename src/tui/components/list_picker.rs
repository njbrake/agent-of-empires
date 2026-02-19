//! Reusable list picker overlay component

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use crate::tui::styles::Theme;

pub enum ListPickerResult {
    Continue,
    Cancelled,
    Selected(String),
}

pub struct ListPicker {
    active: bool,
    filter: Input,
    selected: usize,
    items: Vec<String>,
    title: String,
}

impl ListPicker {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            active: false,
            filter: Input::default(),
            selected: 0,
            items: Vec::new(),
            title: title.into(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self, items: Vec<String>) {
        self.active = true;
        self.items = items;
        self.filter = Input::default();
        self.selected = 0;
    }

    pub fn filtered_items(&self) -> Vec<&String> {
        let filter = self.filter.value().to_lowercase();
        if filter.is_empty() {
            self.items.iter().collect()
        } else {
            self.items
                .iter()
                .filter(|item| item.to_lowercase().contains(&filter))
                .collect()
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ListPickerResult {
        let filtered = self.filtered_items();
        let filtered_len = filtered.len();

        match key.code {
            KeyCode::Esc => {
                self.active = false;
                ListPickerResult::Cancelled
            }
            KeyCode::Enter => {
                let result = if filtered_len > 0 && self.selected < filtered_len {
                    ListPickerResult::Selected(filtered[self.selected].clone())
                } else {
                    ListPickerResult::Cancelled
                };
                self.active = false;
                result
            }
            KeyCode::Up | KeyCode::Char('k') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                ListPickerResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j')
                if !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if filtered_len > 0 && self.selected < filtered_len - 1 {
                    self.selected += 1;
                }
                ListPickerResult::Continue
            }
            KeyCode::Backspace | KeyCode::Char(_) => {
                self.filter.handle_event(&crossterm::event::Event::Key(key));
                self.selected = 0;
                ListPickerResult::Continue
            }
            _ => ListPickerResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let filtered = self.filtered_items();
        let max_visible: usize = 8;
        let list_height = filtered.len().min(max_visible) as u16;
        // filter input (1) + border gap (1) + list + hint (1) + borders (2) + margin (2)
        let dialog_height = (list_height + 7).min(area.height);
        let dialog_width: u16 = 50;

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);
        frame.render_widget(Clear, dialog_area);

        let title = format!(" {} ", self.title);
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

        // Item list with scrolling
        let visible_height = chunks[2].height as usize;
        let scroll_offset = if self.selected >= visible_height {
            self.selected - visible_height + 1
        } else {
            0
        };

        let mut lines: Vec<Line> = Vec::new();
        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no matches)",
                Style::default().fg(theme.dimmed),
            )));
        } else {
            for (i, item) in filtered
                .iter()
                .skip(scroll_offset)
                .take(visible_height)
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
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, item),
                    style,
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines), chunks[2]);

        // Hint line
        let hint_line = Line::from(vec![
            Span::styled("Type", Style::default().fg(theme.hint)),
            Span::raw(" filter  "),
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
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_items() -> Vec<String> {
        vec![
            "Alpha".to_string(),
            "Beta".to_string(),
            "Gamma".to_string(),
            "Delta".to_string(),
        ]
    }

    #[test]
    fn test_new_is_inactive() {
        let picker = ListPicker::new("Test");
        assert!(!picker.is_active());
    }

    #[test]
    fn test_activate() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());
        assert!(picker.is_active());
        assert_eq!(picker.selected, 0);
        assert_eq!(picker.filter.value(), "");
        assert_eq!(picker.items.len(), 4);
    }

    #[test]
    fn test_esc_cancels() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        let result = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, ListPickerResult::Cancelled));
        assert!(!picker.is_active());
    }

    #[test]
    fn test_enter_selects() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            ListPickerResult::Selected(value) => assert_eq!(value, "Alpha"),
            _ => panic!("Expected Selected"),
        }
        assert!(!picker.is_active());
    }

    #[test]
    fn test_navigation_down_up() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 1);

        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 2);

        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 1);
    }

    #[test]
    fn test_navigation_jk() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.selected, 1);

        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_navigation_clamps() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        // Can't go above 0
        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 0);

        // Go to last
        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 3);

        // Can't go past last
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 3);
    }

    #[test]
    fn test_filter_narrows_items() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Char('a')));
        let filtered = picker.filtered_items();
        // "Alpha", "Beta", "Gamma", "Delta" all contain 'a'
        assert_eq!(filtered.len(), 4);

        picker.handle_key(key(KeyCode::Char('l')));
        let filtered = picker.filtered_items();
        // Only "Alpha" contains "al"
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_case_insensitive() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Char('b')));
        let filtered = picker.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(*filtered[0], "Beta");
    }

    #[test]
    fn test_filter_resets_selection() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 2);

        picker.handle_key(key(KeyCode::Char('a')));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_enter_on_filtered_list() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        // Filter to "Beta"
        picker.handle_key(key(KeyCode::Char('b')));
        picker.handle_key(key(KeyCode::Char('e')));

        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            ListPickerResult::Selected(value) => assert_eq!(value, "Beta"),
            _ => panic!("Expected Selected"),
        }
    }

    #[test]
    fn test_enter_on_empty_filtered_list_cancels() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        // Filter to nothing
        picker.handle_key(key(KeyCode::Char('z')));
        picker.handle_key(key(KeyCode::Char('z')));
        picker.handle_key(key(KeyCode::Char('z')));

        let result = picker.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, ListPickerResult::Cancelled));
    }

    #[test]
    fn test_backspace_removes_filter_char() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        picker.handle_key(key(KeyCode::Char('z')));
        picker.handle_key(key(KeyCode::Char('z')));
        assert_eq!(picker.filtered_items().len(), 0);

        picker.handle_key(key(KeyCode::Backspace));
        picker.handle_key(key(KeyCode::Backspace));
        assert_eq!(picker.filtered_items().len(), 4);
    }

    #[test]
    fn test_select_then_navigate_on_filtered() {
        let mut picker = ListPicker::new("Test");
        picker.activate(sample_items());

        // Filter to items containing "a" -> Alpha, Beta, Gamma, Delta
        picker.handle_key(key(KeyCode::Char('a')));
        picker.handle_key(key(KeyCode::Down));

        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            ListPickerResult::Selected(value) => assert_eq!(value, "Beta"),
            _ => panic!("Expected Selected"),
        }
    }
}
