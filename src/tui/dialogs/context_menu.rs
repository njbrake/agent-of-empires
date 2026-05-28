//! Small popup menu anchored at a screen position, used for right-click
//! context actions on the sidebar list (Rename / Delete).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    Rename,
    Delete,
}

pub struct ContextMenuDialog {
    items: Vec<(ContextMenuAction, &'static str)>,
    selected: usize,
    /// Anchor where the popup's top-left corner wants to sit. The renderer
    /// clamps this into the visible area so a click near the bottom-right
    /// edge of the screen doesn't push the menu off-frame.
    anchor: (u16, u16),
    /// Last rendered rect, captured so click-outside detection can run
    /// without re-deriving the layout.
    last_area: Rect,
}

/// Resolve a `(col, row)` mouse position to the menu item index it
/// would hit, given the last rendered `area` and the number of items.
/// `None` for clicks on the border, inside the menu but past the last
/// item, or anywhere outside the menu area.
fn row_to_item_idx(area: Rect, items_len: usize, col: u16, row: u16) -> Option<usize> {
    if !area.contains(Position::from((col, row))) {
        return None;
    }
    let inner_y = area.y.saturating_add(1);
    let last_item_y = inner_y.saturating_add(items_len as u16);
    if row < inner_y || row >= last_item_y {
        return None;
    }
    Some((row - inner_y) as usize)
}

impl ContextMenuDialog {
    pub fn for_session(anchor: (u16, u16)) -> Self {
        Self::new(
            anchor,
            vec![
                (ContextMenuAction::Rename, "Rename"),
                (ContextMenuAction::Delete, "Delete"),
            ],
        )
    }

    pub fn for_group(anchor: (u16, u16)) -> Self {
        Self::new(
            anchor,
            vec![
                (ContextMenuAction::Rename, "Rename Group"),
                (ContextMenuAction::Delete, "Delete Group"),
            ],
        )
    }

    fn new(anchor: (u16, u16), items: Vec<(ContextMenuAction, &'static str)>) -> Self {
        Self {
            items,
            selected: 0,
            anchor,
            last_area: Rect::default(),
        }
    }

    pub fn selected_action(&self) -> ContextMenuAction {
        self.items[self.selected].0
    }

    /// Returns true when `(col, row)` falls outside the last rendered area.
    /// Lets the mouse router close the menu on any click that isn't on it,
    /// matching the sidebar's web behavior in `WorkspaceSidebar.tsx`.
    pub fn click_is_outside(&self, col: u16, row: u16) -> bool {
        !self.last_area.contains(Position::from((col, row)))
    }

    /// Route a left-click at `(col, row)` to the menu. Returns:
    ///   - `Some(Submit(action))` when the click lands on an item row,
    ///   - `Some(Continue)` when the click lands on the menu but not on
    ///     an item (e.g. the rounded border), so the menu stays open,
    ///   - `None` when the click is outside the menu area, so the caller
    ///     can close it or fall through to underlying handlers.
    ///
    /// Hover-style selection moves with the click first so a near-miss
    /// still tracks where the user pointed.
    pub fn handle_click(&mut self, col: u16, row: u16) -> Option<DialogResult<ContextMenuAction>> {
        if !self.last_area.contains(Position::from((col, row))) {
            return None;
        }
        match row_to_item_idx(self.last_area, self.items.len(), col, row) {
            None => {
                // Click on top/bottom border or anywhere inside the menu
                // that isn't an item row. Keep the menu open so the user
                // can try again without re-opening it.
                Some(DialogResult::Continue)
            }
            Some(idx) => {
                self.selected = idx;
                Some(DialogResult::Submit(self.items[idx].0))
            }
        }
    }

    /// Move the selection (and thus the highlighted row) to whichever
    /// item the mouse is hovering, so the visual cue tracks the cursor
    /// the same way a desktop menu does. Returns true when the
    /// highlight actually changed, so the caller can skip a redraw on
    /// every pixel-level mouse twitch.
    pub fn handle_hover(&mut self, col: u16, row: u16) -> bool {
        let Some(idx) = row_to_item_idx(self.last_area, self.items.len(), col, row) else {
            return false;
        };
        if self.selected == idx {
            return false;
        }
        self.selected = idx;
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<ContextMenuAction> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Enter => DialogResult::Submit(self.items[self.selected].0),
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected == 0 {
                    self.selected = self.items.len() - 1;
                } else {
                    self.selected -= 1;
                }
                DialogResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1) % self.items.len();
                DialogResult::Continue
            }
            // Quick-pick hotkeys mirror the underlying actions' home-view
            // bindings, so a power user never has to arrow + Enter.
            KeyCode::Char('r') | KeyCode::Char('R') => {
                DialogResult::Submit(ContextMenuAction::Rename)
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                DialogResult::Submit(ContextMenuAction::Delete)
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let label_width = self
            .items
            .iter()
            .map(|(_, label)| label.chars().count() as u16)
            .max()
            .unwrap_or(0);
        // Two columns of inner padding, plus borders.
        let width = (label_width + 4).max(14);
        let height = self.items.len() as u16 + 2;

        let mut x = self.anchor.0;
        let mut y = self.anchor.1;
        if x + width > area.right() {
            x = area.right().saturating_sub(width);
        }
        if y + height > area.bottom() {
            y = area.bottom().saturating_sub(height);
        }
        x = x.max(area.x);
        y = y.max(area.y);
        let dialog_area = Rect {
            x,
            y,
            width: width.min(area.width),
            height: height.min(area.height),
        };
        self.last_area = dialog_area;

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let rows: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, (_, label))| {
                let style = if idx == self.selected {
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.accent)
                        .bold()
                } else {
                    Style::default().fg(theme.text)
                };
                Line::from(format!(" {label} ")).style(style)
            })
            .collect();

        frame.render_widget(Paragraph::new(rows), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn session_menu_starts_on_rename() {
        let menu = ContextMenuDialog::for_session((0, 0));
        assert_eq!(menu.selected_action(), ContextMenuAction::Rename);
    }

    #[test]
    fn down_then_enter_selects_delete() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        assert!(matches!(
            menu.handle_key(key(KeyCode::Down)),
            DialogResult::Continue
        ));
        let result = menu.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Delete)
        ));
    }

    #[test]
    fn enter_on_default_submits_rename() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        let result = menu.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Rename)
        ));
    }

    #[test]
    fn esc_cancels() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        let result = menu.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn up_wraps_from_first_to_last() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        menu.handle_key(key(KeyCode::Up));
        let result = menu.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Delete)
        ));
    }

    #[test]
    fn down_wraps_from_last_to_first() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        menu.handle_key(key(KeyCode::Down));
        menu.handle_key(key(KeyCode::Down));
        let result = menu.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Rename)
        ));
    }

    #[test]
    fn r_hotkey_submits_rename() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        // Pre-select Delete to prove the hotkey wins over the cursor.
        menu.handle_key(key(KeyCode::Down));
        let result = menu.handle_key(key(KeyCode::Char('r')));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Rename)
        ));
    }

    #[test]
    fn d_hotkey_submits_delete() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        let result = menu.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(
            result,
            DialogResult::Submit(ContextMenuAction::Delete)
        ));
    }

    #[test]
    fn unknown_key_is_continue() {
        let mut menu = ContextMenuDialog::for_session((0, 0));
        let result = menu.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(result, DialogResult::Continue));
    }

    #[test]
    fn click_is_outside_before_render_is_true() {
        let menu = ContextMenuDialog::for_session((10, 10));
        // Before a render captures `last_area`, every point should count
        // as "outside" so a stray click can't be mis-classified as "inside
        // the menu" and accidentally kept open.
        assert!(menu.click_is_outside(10, 10));
    }

    /// Stub last_area as if render had run, so click routing can be
    /// tested without spinning up a full Frame.
    fn stub_render(menu: &mut ContextMenuDialog, x: u16, y: u16, w: u16, h: u16) {
        menu.last_area = Rect::new(x, y, w, h);
    }

    #[test]
    fn click_on_first_row_submits_rename() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        // Item rows live inside the bordered block, so row y+1 is the
        // first item and y+2 is the second.
        let result = menu.handle_click(12, 11);
        assert!(matches!(
            result,
            Some(DialogResult::Submit(ContextMenuAction::Rename))
        ));
    }

    #[test]
    fn click_on_second_row_submits_delete() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        let result = menu.handle_click(12, 12);
        assert!(matches!(
            result,
            Some(DialogResult::Submit(ContextMenuAction::Delete))
        ));
    }

    #[test]
    fn click_on_border_keeps_menu_open() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        // Top border row is y itself.
        let result = menu.handle_click(12, 10);
        assert!(matches!(result, Some(DialogResult::Continue)));
    }

    #[test]
    fn click_outside_returns_none() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        let result = menu.handle_click(40, 40);
        assert!(result.is_none());
    }

    #[test]
    fn hover_moves_highlight() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        assert_eq!(menu.selected_action(), ContextMenuAction::Rename);
        let changed = menu.handle_hover(12, 12);
        assert!(changed, "hover onto second row should change highlight");
        assert_eq!(menu.selected_action(), ContextMenuAction::Delete);
    }

    #[test]
    fn hover_on_same_row_returns_false() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        // First hover lands on row 1 (Rename, already selected).
        assert!(!menu.handle_hover(12, 11));
        // Same row again -> still no change.
        assert!(!menu.handle_hover(12, 11));
    }

    #[test]
    fn hover_off_menu_leaves_selection_alone() {
        let mut menu = ContextMenuDialog::for_session((10, 10));
        stub_render(&mut menu, 10, 10, 14, 4);
        menu.handle_hover(12, 12); // Delete
        assert_eq!(menu.selected_action(), ContextMenuAction::Delete);
        assert!(!menu.handle_hover(40, 40));
        assert_eq!(
            menu.selected_action(),
            ContextMenuAction::Delete,
            "hover outside menu must not snap the highlight back"
        );
    }
}
