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

    // Multiline session indicators (collapsed mode dots)
    pub worktree_indicator: Color,
    pub sandbox_indicator: Color,
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

            worktree_indicator: Color::Rgb(255, 200, 60),
            sandbox_indicator: Color::Magenta,
        }
    }
}

/// Mix a status color into a base background at 1/10th intensity.
/// Produces the "vertical tab glow" for selected session items.
pub fn tint_background(status_color: Color, base_bg: Color) -> Color {
    if let (Color::Rgb(sr, sg, sb), Color::Rgb(br, bg, bb)) = (status_color, base_bg) {
        Color::Rgb(
            br.saturating_add(sr / 10),
            bg.saturating_add(sg / 10),
            bb.saturating_add(sb / 10),
        )
    } else {
        base_bg
    }
}
