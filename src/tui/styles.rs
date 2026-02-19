//! TUI theme and styling

use super::themes::color::parse_hex_color;
use ratatui::style::Color;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;

/// Newtype wrapper for Color that supports hex string serialization
#[derive(Debug, Clone, Copy)]
pub struct ThemeColor(pub(crate) Color);

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_str = String::deserialize(deserializer)?;
        parse_hex_color(&hex_str)
            .map(ThemeColor)
            .map_err(serde::de::Error::custom)
    }
}

impl Deref for ThemeColor {
    type Target = Color;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Color> for ThemeColor {
    fn as_ref(&self) -> &Color {
        &self.0
    }
}

impl From<ThemeColor> for Color {
    fn from(tc: ThemeColor) -> Self {
        tc.0
    }
}

impl Serialize for ThemeColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // ThemeColor is only constructed from hex parsing, which always produces RGB
        let Color::Rgb(r, g, b) = self.0 else {
            return Err(serde::ser::Error::custom(
                "ThemeColor invariant violated: expected RGB color",
            ));
        };
        serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}", r, g, b))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    // Background and borders
    pub background: ThemeColor,
    pub border: ThemeColor,
    pub terminal_border: ThemeColor,
    pub selection: ThemeColor,
    pub session_selection: ThemeColor,

    // Text colors
    pub title: ThemeColor,
    pub text: ThemeColor,
    pub dimmed: ThemeColor,
    pub hint: ThemeColor,

    // Status colors
    pub running: ThemeColor,
    pub waiting: ThemeColor,
    pub idle: ThemeColor,
    pub error: ThemeColor,
    pub terminal_active: ThemeColor,

    // UI elements
    pub group: ThemeColor,
    pub search: ThemeColor,
    pub accent: ThemeColor,
}

impl Default for Theme {
    fn default() -> Self {
        Self::phosphor()
    }
}

impl Theme {
    pub fn phosphor() -> Self {
        Self {
            background: ThemeColor(Color::Rgb(16, 20, 18)),
            border: ThemeColor(Color::Rgb(45, 70, 55)),
            terminal_border: ThemeColor(Color::Rgb(70, 130, 180)),
            selection: ThemeColor(Color::Rgb(30, 50, 40)),
            session_selection: ThemeColor(Color::Rgb(60, 60, 60)),

            title: ThemeColor(Color::Rgb(57, 255, 20)),
            text: ThemeColor(Color::Rgb(180, 255, 180)),
            dimmed: ThemeColor(Color::Rgb(80, 120, 90)),
            hint: ThemeColor(Color::Rgb(100, 160, 120)),

            running: ThemeColor(Color::Rgb(0, 255, 180)),
            waiting: ThemeColor(Color::Rgb(255, 180, 60)),
            idle: ThemeColor(Color::Rgb(60, 100, 70)),
            error: ThemeColor(Color::Rgb(255, 100, 80)),
            terminal_active: ThemeColor(Color::Rgb(130, 170, 255)),

            group: ThemeColor(Color::Rgb(100, 220, 160)),
            search: ThemeColor(Color::Rgb(180, 255, 200)),
            accent: ThemeColor(Color::Rgb(57, 255, 20)),
        }
    }
}
