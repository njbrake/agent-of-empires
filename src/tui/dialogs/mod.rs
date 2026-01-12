//! TUI dialog components

mod confirm;
mod new_session;
mod rename;

pub use confirm::ConfirmDialog;
pub use new_session::{NewSessionData, NewSessionDialog};
pub use rename::RenameDialog;

pub enum DialogResult<T> {
    Continue,
    Cancel,
    Submit(T),
}
