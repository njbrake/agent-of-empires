//! Terminal utilities and helpers

/// Returns the current terminal size as (width, height), or None if unavailable.
pub fn get_size() -> Option<(u16, u16)> {
    crossterm::terminal::size().ok()
}
