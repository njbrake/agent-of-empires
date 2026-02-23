//! TUI theme and styling

use ratatui::style::Color;
use tracing::warn;

pub const AVAILABLE_THEMES: &[&str] = &["phosphor", "tokyo-night-storm", "catppuccin-latte"];

pub fn load_theme(name: &str) -> Theme {
    match name {
        "phosphor" => Theme::phosphor(),
        "tokyo-night-storm" => Theme::tokyo_night_storm(),
        "catppuccin-latte" => Theme::catppuccin_latte(),
        _ => {
            warn!("Unknown theme '{}', falling back to phosphor", name);
            Theme::phosphor()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    // Background and borders
    pub background: Color,
    pub border: Color,
    pub terminal_border: Color,
    pub selection: Color,
    pub session_selection: Color,

    // Text colors
    pub title: Color,
    pub text: Color,
    pub dimmed: Color,
    pub hint: Color,

    // Status colors
    pub running: Color,
    pub waiting: Color,
    pub idle: Color,
    pub error: Color,
    pub terminal_active: Color,

    // UI elements
    pub group: Color,
    pub search: Color,
    pub accent: Color,

    pub diff_add: Color,
    pub diff_delete: Color,
    pub diff_modified: Color,
    pub diff_context: Color,
    pub diff_header: Color,

    pub help_key: Color,

    pub branch: Color,
    pub sandbox: Color,
    pub worktree_managed: Color,
    pub worktree_manual: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::phosphor()
    }
}

impl Theme {
    pub fn phosphor() -> Self {
        Self {
            background: Color::Rgb(16, 20, 18),
            border: Color::Rgb(45, 70, 55),
            terminal_border: Color::Rgb(70, 130, 180),
            selection: Color::Rgb(30, 50, 40),
            session_selection: Color::Rgb(60, 60, 60),

            title: Color::Rgb(57, 255, 20),
            text: Color::Rgb(180, 255, 180),
            dimmed: Color::Rgb(80, 120, 90),
            hint: Color::Rgb(100, 160, 120),

            running: Color::Rgb(0, 255, 180),
            waiting: Color::Rgb(255, 180, 60),
            idle: Color::Rgb(60, 100, 70),
            error: Color::Rgb(255, 100, 80),
            terminal_active: Color::Rgb(130, 170, 255),

            group: Color::Rgb(100, 220, 160),
            search: Color::Rgb(180, 255, 200),
            accent: Color::Rgb(57, 255, 20),

            diff_add: Color::Rgb(0, 255, 180),
            diff_delete: Color::Rgb(255, 100, 80),
            diff_modified: Color::Rgb(255, 180, 60),
            diff_context: Color::Rgb(80, 120, 90),
            diff_header: Color::Rgb(100, 160, 200),

            help_key: Color::Rgb(255, 180, 60),

            branch: Color::Rgb(100, 160, 200),
            sandbox: Color::Rgb(200, 122, 255),
            worktree_managed: Color::Rgb(100, 220, 160),
            worktree_manual: Color::Rgb(255, 180, 60),
        }
    }

    pub fn tokyo_night_storm() -> Self {
        Self {
            background: Color::Rgb(36, 40, 59),
            border: Color::Rgb(65, 72, 104),
            terminal_border: Color::Rgb(61, 89, 161),
            selection: Color::Rgb(54, 74, 130),
            session_selection: Color::Rgb(65, 72, 104),

            title: Color::Rgb(122, 162, 247),
            text: Color::Rgb(192, 202, 245),
            dimmed: Color::Rgb(86, 95, 137),
            hint: Color::Rgb(122, 162, 247),

            running: Color::Rgb(158, 206, 106),
            waiting: Color::Rgb(224, 175, 104),
            idle: Color::Rgb(86, 95, 137),
            error: Color::Rgb(247, 118, 142),
            terminal_active: Color::Rgb(122, 162, 247),

            group: Color::Rgb(125, 207, 255),
            search: Color::Rgb(187, 154, 247),
            accent: Color::Rgb(122, 162, 247),

            diff_add: Color::Rgb(158, 206, 106),
            diff_delete: Color::Rgb(247, 118, 142),
            diff_modified: Color::Rgb(224, 175, 104),
            diff_context: Color::Rgb(86, 95, 137),
            diff_header: Color::Rgb(125, 207, 255),

            help_key: Color::Rgb(224, 175, 104),

            branch: Color::Rgb(125, 207, 255),
            sandbox: Color::Rgb(187, 154, 247),
            worktree_managed: Color::Rgb(158, 206, 106),
            worktree_manual: Color::Rgb(224, 175, 104),
        }
    }

    pub fn catppuccin_latte() -> Self {
        Self {
            background: Color::Rgb(239, 241, 245),
            border: Color::Rgb(188, 192, 204),
            terminal_border: Color::Rgb(4, 165, 229),
            selection: Color::Rgb(220, 224, 232),
            session_selection: Color::Rgb(204, 208, 218),

            title: Color::Rgb(30, 102, 245),
            text: Color::Rgb(76, 79, 105),
            dimmed: Color::Rgb(172, 176, 190),
            hint: Color::Rgb(32, 159, 181),

            running: Color::Rgb(64, 160, 43),
            waiting: Color::Rgb(223, 142, 29),
            idle: Color::Rgb(156, 160, 176),
            error: Color::Rgb(210, 15, 57),
            terminal_active: Color::Rgb(30, 102, 245),

            group: Color::Rgb(23, 146, 153),
            search: Color::Rgb(114, 135, 253),
            accent: Color::Rgb(254, 100, 11),

            diff_add: Color::Rgb(64, 160, 43),
            diff_delete: Color::Rgb(210, 15, 57),
            diff_modified: Color::Rgb(223, 142, 29),
            diff_context: Color::Rgb(156, 160, 176),
            diff_header: Color::Rgb(4, 165, 229),

            help_key: Color::Rgb(223, 142, 29),

            branch: Color::Rgb(4, 165, 229),
            sandbox: Color::Rgb(136, 57, 239),
            worktree_managed: Color::Rgb(64, 160, 43),
            worktree_manual: Color::Rgb(223, 142, 29),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_phosphor() {
        let theme = load_theme("phosphor");
        assert_eq!(theme.title, Color::Rgb(57, 255, 20));
        assert_eq!(theme.background, Color::Rgb(16, 20, 18));
    }

    #[test]
    fn test_load_catppuccin_latte() {
        let theme = load_theme("catppuccin-latte");
        assert_eq!(theme.title, Color::Rgb(30, 102, 245));
        assert_eq!(theme.background, Color::Rgb(239, 241, 245));
    }

    #[test]
    fn test_load_invalid_fallback() {
        let theme = load_theme("nonexistent-theme");
        assert_eq!(theme.title, Color::Rgb(57, 255, 20));
        assert_eq!(theme.background, Color::Rgb(16, 20, 18));
    }

    #[test]
    fn test_load_tokyo_night_storm() {
        let theme = load_theme("tokyo-night-storm");
        assert_eq!(theme.title, Color::Rgb(122, 162, 247));
        assert_eq!(theme.background, Color::Rgb(36, 40, 59));
    }

    #[test]
    fn test_available_themes_count() {
        assert_eq!(AVAILABLE_THEMES.len(), 3);
        assert!(AVAILABLE_THEMES.contains(&"phosphor"));
        assert!(AVAILABLE_THEMES.contains(&"tokyo-night-storm"));
        assert!(AVAILABLE_THEMES.contains(&"catppuccin-latte"));
    }
}
