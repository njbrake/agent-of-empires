//! TUI dialog components

mod changelog;
mod confirm;
mod delete_options;
mod group_delete_options;
mod hook_trust;
mod info;
mod new_session;
mod rename;
mod welcome;

pub use changelog::ChangelogDialog;
pub use confirm::ConfirmDialog;
pub use delete_options::{DeleteDialogConfig, DeleteOptions, UnifiedDeleteDialog};
pub use group_delete_options::{GroupDeleteOptions, GroupDeleteOptionsDialog};
pub use hook_trust::{HookTrustAction, HookTrustDialog};
pub use info::InfoDialog;
pub use new_session::{NewSessionData, NewSessionDialog};
pub use rename::{RenameData, RenameDialog};
pub use welcome::WelcomeDialog;

pub enum DialogResult<T> {
    Continue,
    Cancel,
    Submit(T),
}

/// Center a dialog of given size within an area, clamping to fit.
pub fn centered_rect(
    area: ratatui::layout::Rect,
    width: u16,
    height: u16,
) -> ratatui::layout::Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    ratatui::layout::Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
