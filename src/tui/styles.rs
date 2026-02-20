//! TUI theme and styling

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::session::Status;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

            diff_add: Color::Rgb(0, 255, 0),
            diff_delete: Color::Rgb(255, 0, 0),
            diff_modified: Color::Rgb(255, 255, 0),
            diff_context: Color::Rgb(128, 128, 128),
            diff_header: Color::Rgb(0, 255, 255),

            help_key: Color::Rgb(255, 255, 0),

            branch: Color::Rgb(0, 255, 255),
            sandbox: Color::Rgb(255, 0, 255),
            worktree_managed: Color::Rgb(0, 255, 0),
            worktree_manual: Color::Rgb(255, 255, 0),
        }
    }

    pub fn status_color(&self, status: &Status) -> Color {
        match status {
            Status::Running => self.running,
            Status::Waiting => self.waiting,
            Status::Idle => self.idle,
            Status::Error => self.error,
            Status::Starting => self.waiting,
            Status::Deleting => self.error,
        }
    }
}
