//! Snooze duration picker. Opens when the user presses `h`/`H`/`w`/`W`
//! on a non-snoozed session; single-key shortcuts so the choice is
//! one keystroke after the trigger.
//!
//! Mapping principle: the digit IS the duration where it can be.
//!   1..6 → that many hours
//!   8    → 24 hours (one day)
//!   0    → 1 week
//!   7,9  → unbound (no preset, fall through; reserved for future)

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

const ONE_HOUR: u32 = 60;
const TWO_HOURS: u32 = 2 * 60;
const THREE_HOURS: u32 = 3 * 60;
const FOUR_HOURS: u32 = 4 * 60;
const FIVE_HOURS: u32 = 5 * 60;
const SIX_HOURS: u32 = 6 * 60;
const ONE_DAY: u32 = 24 * 60;
const ONE_WEEK: u32 = 7 * 24 * 60;

pub struct SnoozeDurationDialog {
    title: String,
}

impl SnoozeDurationDialog {
    pub fn new(session_title: &str) -> Self {
        Self {
            title: session_title.to_string(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<u32> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => DialogResult::Cancel,
            KeyCode::Char('1') => DialogResult::Submit(ONE_HOUR),
            KeyCode::Char('2') => DialogResult::Submit(TWO_HOURS),
            KeyCode::Char('3') => DialogResult::Submit(THREE_HOURS),
            KeyCode::Char('4') => DialogResult::Submit(FOUR_HOURS),
            KeyCode::Char('5') => DialogResult::Submit(FIVE_HOURS),
            KeyCode::Char('6') => DialogResult::Submit(SIX_HOURS),
            KeyCode::Char('8') => DialogResult::Submit(ONE_DAY),
            KeyCode::Char('0') => DialogResult::Submit(ONE_WEEK),
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 52, 14);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.waiting))
            .title(" Snooze ")
            .title_style(Style::default().fg(theme.waiting).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // session title
                Constraint::Length(1), // spacer
                Constraint::Length(1), // 1
                Constraint::Length(1), // 2
                Constraint::Length(1), // 3
                Constraint::Length(1), // 4
                Constraint::Length(1), // 5
                Constraint::Length(1), // 6
                Constraint::Length(1), // 8
                Constraint::Length(1), // 0
            ])
            .split(inner);

        let subject = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{}  ", self.title),
                Style::default().fg(theme.text).bold(),
            ),
            Span::styled("how long?", Style::default().fg(theme.dimmed)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(subject, chunks[0]);

        let key_style = Style::default().fg(theme.waiting).bold();
        let text_style = Style::default().fg(theme.text);
        let row = |k: &'static str, label: &'static str| {
            Paragraph::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("[{}]", k), key_style),
                Span::raw("  "),
                Span::styled(label, text_style),
            ]))
        };
        frame.render_widget(row("1", "1 hour"), chunks[2]);
        frame.render_widget(row("2", "2 hours"), chunks[3]);
        frame.render_widget(row("3", "3 hours"), chunks[4]);
        frame.render_widget(row("4", "4 hours"), chunks[5]);
        frame.render_widget(row("5", "5 hours"), chunks[6]);
        frame.render_widget(row("6", "6 hours"), chunks[7]);
        frame.render_widget(row("8", "24 hours (1 day)"), chunks[8]);
        frame.render_widget(row("0", "1 week"), chunks[9]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn digit_presets() {
        let cases: &[(char, u32)] = &[
            ('1', 60),
            ('2', 120),
            ('3', 180),
            ('4', 240),
            ('5', 300),
            ('6', 360),
            ('8', 1440),
            ('0', 10080),
        ];
        for (digit, minutes) in cases {
            let mut d = SnoozeDurationDialog::new("sess");
            match d.handle_key(k(KeyCode::Char(*digit))) {
                DialogResult::Submit(m) => assert_eq!(m, *minutes, "digit {digit}"),
                _ => panic!("expected Submit({minutes}) for digit {digit}"),
            }
        }
    }

    #[test]
    fn esc_cancels() {
        let mut d = SnoozeDurationDialog::new("sess");
        assert!(matches!(
            d.handle_key(k(KeyCode::Esc)),
            DialogResult::Cancel
        ));
    }

    #[test]
    fn q_cancels() {
        let mut d = SnoozeDurationDialog::new("sess");
        assert!(matches!(
            d.handle_key(k(KeyCode::Char('q'))),
            DialogResult::Cancel
        ));
    }

    #[test]
    fn unknown_continues() {
        let mut d = SnoozeDurationDialog::new("sess");
        assert!(matches!(
            d.handle_key(k(KeyCode::Char('x'))),
            DialogResult::Continue
        ));
    }

    #[test]
    fn seven_and_nine_unbound() {
        let mut d = SnoozeDurationDialog::new("sess");
        assert!(matches!(
            d.handle_key(k(KeyCode::Char('7'))),
            DialogResult::Continue
        ));
        assert!(matches!(
            d.handle_key(k(KeyCode::Char('9'))),
            DialogResult::Continue
        ));
    }
}
