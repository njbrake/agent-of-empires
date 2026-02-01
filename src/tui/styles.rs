//! TUI theme and styling

use ratatui::style::Color;

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
        }
    }
}
