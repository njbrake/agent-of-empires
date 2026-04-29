//! TUI dialog components

mod changelog;
mod confirm;
mod custom_instruction;
mod delete_options;
mod group_delete_options;
mod hook_trust;
mod hooks_install;
mod info;
mod new_session;
mod no_agents;
mod profile_picker;
mod rename;
mod send_message;
#[cfg(feature = "serve")]
mod serve;
mod update_confirm;
mod welcome;

pub use changelog::ChangelogDialog;
pub use confirm::ConfirmDialog;
pub use custom_instruction::CustomInstructionDialog;
pub use delete_options::{DeleteDialogConfig, DeleteOptions, UnifiedDeleteDialog};
pub use group_delete_options::{GroupDeleteOptions, GroupDeleteOptionsDialog};
pub use hook_trust::{HookTrustAction, HookTrustDialog};
pub use hooks_install::HooksInstallDialog;
pub use info::InfoDialog;
pub use new_session::{NewSessionData, NewSessionDialog};
pub use no_agents::{NoAgentsAction, NoAgentsDialog};
pub use profile_picker::{ProfileEntry, ProfilePickerAction, ProfilePickerDialog};
pub use rename::{RenameData, RenameDialog, RenameMode};
pub use send_message::SendMessageDialog;
#[cfg(feature = "serve")]
pub use serve::{ServeAction, ServeView};
pub use update_confirm::UpdateConfirmDialog;
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
