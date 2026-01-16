//! TUI dialog components

mod changelog;
mod confirm;
mod delete_options;
mod group_delete_options;
mod new_session;
mod rename;
mod welcome;

pub use changelog::ChangelogDialog;
pub use confirm::ConfirmDialog;
pub use delete_options::{DeleteOptions, DeleteOptionsDialog};
pub use group_delete_options::{GroupDeleteOptions, GroupDeleteOptionsDialog};
pub use new_session::{NewSessionData, NewSessionDialog};
pub use rename::RenameDialog;
pub use welcome::WelcomeDialog;

pub enum DialogResult<T> {
    Continue,
    Cancel,
    Submit(T),
}
