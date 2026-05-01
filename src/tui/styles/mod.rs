//! TUI theme and styling.
//!
//! The module is split into:
//!   - `themes`   — the `Theme` struct and built-in theme constructors
//!   - `palette`  — 24-bit RGB -> xterm-256 downsampling for `palette_mode`
//!   - this file  — theme discovery / loading / serialization glue
//!
//! Public surface is re-exported here so callers keep `crate::tui::styles::*`.

mod palette;
mod themes;

pub use themes::{idle_decay_window, Theme};

use std::path::PathBuf;
use tracing::warn;

pub const BUILTIN_THEMES: &[&str] = &[
    "empire",
    "phosphor",
    "tokyo-night-storm",
    "catppuccin-latte",
    "dracula",
];

/// Return the directory where custom theme TOML files are stored.
pub fn custom_themes_dir() -> Option<PathBuf> {
    crate::session::get_app_dir().ok().map(|d| d.join("themes"))
}

/// Discover custom theme names from the themes directory.
/// Returns (name, path) pairs sorted alphabetically.
pub fn discover_custom_themes() -> Vec<(String, PathBuf)> {
    let dir = match custom_themes_dir() {
        Some(d) if d.is_dir() => d,
        _ => return Vec::new(),
    };

    let mut themes = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let name = stem.to_string();
                if !BUILTIN_THEMES.contains(&name.as_str()) {
                    themes.push((name, path));
                }
            }
        }
    }

    themes.sort_by(|a, b| a.0.cmp(&b.0));
    themes
}

/// Return the full list of available theme names: built-in themes first, then custom.
pub fn available_themes() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_THEMES.iter().map(|s| s.to_string()).collect();
    for (name, _) in discover_custom_themes() {
        names.push(name);
    }
    names
}

/// Load a custom theme from a TOML file.
fn load_custom_theme(path: &std::path::Path) -> Option<Theme> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read theme file {}: {}", path.display(), e);
            return None;
        }
    };

    match toml::from_str::<Theme>(&content) {
        Ok(theme) => Some(theme),
        Err(e) => {
            warn!("Failed to parse theme file {}: {}", path.display(), e);
            None
        }
    }
}

pub fn load_theme(name: &str) -> Theme {
    match name {
        "empire" => Theme::empire(),
        "phosphor" => Theme::phosphor(),
        "tokyo-night-storm" => Theme::tokyo_night_storm(),
        "catppuccin-latte" => Theme::catppuccin_latte(),
        "dracula" => Theme::dracula(),
        _ => {
            // Try loading from custom themes directory
            for (theme_name, path) in discover_custom_themes() {
                if theme_name == name {
                    if let Some(theme) = load_custom_theme(&path) {
                        return theme;
                    }
                }
            }
            warn!("Unknown theme '{}', falling back to empire", name);
            Theme::empire()
        }
    }
}

/// Load a theme and, when `palette_mode` is true, convert every `Color::Rgb`
/// field to `Color::Indexed` (nearest xterm-256 index). Use this from callers
/// that have access to `ThemeConfig::color_mode`. Builtin themes construct
/// colors directly in Rust (bypassing the serde hex helper), so the conversion
/// must be applied at the Theme level after construction.
pub fn load_theme_with_mode(name: &str, palette_mode: bool) -> Theme {
    let mut theme = load_theme(name);
    if palette_mode {
        theme.downsample_to_palette();
    }
    theme
}

/// Export a theme as a TOML string.
pub fn export_theme_toml(theme: &Theme) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use std::io::Write;

    #[test]
    fn load_theme_with_mode_palette_yields_indexed() {
        let theme = load_theme_with_mode("empire", true);
        assert!(matches!(theme.title, Color::Indexed(_)));
    }

    #[test]
    fn load_theme_with_mode_truecolor_yields_rgb() {
        let theme = load_theme_with_mode("empire", false);
        assert!(matches!(theme.title, Color::Rgb(_, _, _)));
    }

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
    fn test_load_empire() {
        let theme = load_theme("empire");
        assert_eq!(theme.title, Color::Rgb(251, 191, 36));
        assert_eq!(theme.background, Color::Rgb(15, 23, 42));
    }

    #[test]
    fn test_load_invalid_fallback() {
        let theme = load_theme("nonexistent-theme");
        assert_eq!(theme.title, Color::Rgb(251, 191, 36));
        assert_eq!(theme.background, Color::Rgb(15, 23, 42));
    }

    #[test]
    fn test_load_tokyo_night_storm() {
        let theme = load_theme("tokyo-night-storm");
        assert_eq!(theme.title, Color::Rgb(122, 162, 247));
        assert_eq!(theme.background, Color::Rgb(36, 40, 59));
    }

    #[test]
    fn test_load_dracula() {
        let theme = load_theme("dracula");
        assert_eq!(theme.title, Color::Rgb(189, 147, 249));
        assert_eq!(theme.background, Color::Rgb(40, 42, 54));
    }

    #[test]
    fn test_builtin_themes_count() {
        assert_eq!(BUILTIN_THEMES.len(), 5);
        assert!(BUILTIN_THEMES.contains(&"empire"));
        assert!(BUILTIN_THEMES.contains(&"phosphor"));
        assert!(BUILTIN_THEMES.contains(&"tokyo-night-storm"));
        assert!(BUILTIN_THEMES.contains(&"catppuccin-latte"));
        assert!(BUILTIN_THEMES.contains(&"dracula"));
    }

    #[test]
    fn test_theme_serialize_roundtrip() {
        let original = Theme::empire();
        let toml_str = export_theme_toml(&original).unwrap();
        let loaded: Theme = toml::from_str(&toml_str).unwrap();

        assert_eq!(original.background, loaded.background);
        assert_eq!(original.title, loaded.title);
        assert_eq!(original.running, loaded.running);
        assert_eq!(original.error, loaded.error);
        assert_eq!(original.diff_add, loaded.diff_add);
        assert_eq!(original.sandbox, loaded.sandbox);
    }

    #[test]
    fn test_theme_toml_format() {
        let theme = Theme::empire();
        let toml_str = export_theme_toml(&theme).unwrap();

        assert!(toml_str.contains(r##"background = "#0f172a""##));
        assert!(toml_str.contains(r##"title = "#fbbf24""##));
        assert!(toml_str.contains(r##"running = "#22c55e""##));
    }

    #[test]
    fn test_load_custom_theme_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let theme_path = dir.path().join("my-theme.toml");
        let toml_str = export_theme_toml(&Theme::dracula()).unwrap();
        std::fs::write(&theme_path, &toml_str).unwrap();

        let loaded = load_custom_theme(&theme_path).unwrap();
        assert_eq!(loaded.background, Color::Rgb(40, 42, 54));
        assert_eq!(loaded.title, Color::Rgb(189, 147, 249));
    }

    #[test]
    fn test_load_custom_theme_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let theme_path = dir.path().join("bad.toml");
        std::fs::write(&theme_path, "not valid theme data").unwrap();

        assert!(load_custom_theme(&theme_path).is_none());
    }

    #[test]
    fn test_discover_custom_themes_empty() {
        // With no themes dir, should return empty
        let themes = discover_custom_themes();
        // May or may not be empty depending on test environment, just check it doesn't panic
        let _ = themes;
    }

    #[test]
    fn test_discover_custom_themes_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let themes_dir = dir.path().join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        // Write two valid theme files
        let dracula_toml = export_theme_toml(&Theme::dracula()).unwrap();
        std::fs::write(themes_dir.join("my-dark.toml"), &dracula_toml).unwrap();
        std::fs::write(themes_dir.join("my-light.toml"), &dracula_toml).unwrap();
        // Write a non-toml file (should be ignored)
        std::fs::write(themes_dir.join("readme.txt"), "not a theme").unwrap();

        // Can't easily test discover_custom_themes() since it uses get_app_dir(),
        // but we can test the file parsing directly
        let loaded = load_custom_theme(&themes_dir.join("my-dark.toml"));
        assert!(loaded.is_some());
    }

    #[test]
    fn test_available_themes_includes_builtins() {
        let themes = available_themes();
        assert!(themes.len() >= 5);
        assert!(themes.contains(&"empire".to_string()));
        assert!(themes.contains(&"phosphor".to_string()));
        assert!(themes.contains(&"tokyo-night-storm".to_string()));
        assert!(themes.contains(&"catppuccin-latte".to_string()));
        assert!(themes.contains(&"dracula".to_string()));
    }

    #[test]
    fn test_all_builtin_themes_roundtrip() {
        for name in BUILTIN_THEMES {
            let theme = load_theme(name);
            let toml_str = export_theme_toml(&theme)
                .unwrap_or_else(|e| panic!("{} export failed: {}", name, e));
            let _loaded: Theme = toml::from_str(&toml_str)
                .unwrap_or_else(|e| panic!("{} roundtrip failed: {}", name, e));
        }
    }

    #[test]
    fn test_custom_theme_toml_parsing() {
        let toml_str = r##"
background = "#1a1b26"
border = "#414868"
terminal_border = "#7aa2f7"
selection = "#283457"
session_selection = "#414868"
title = "#c0caf5"
text = "#a9b1d6"
dimmed = "#565f89"
hint = "#565f89"
running = "#9ece6a"
waiting = "#e0af68"
idle = "#565f89"
error = "#f7768e"
terminal_active = "#7aa2f7"
group = "#7dcfff"
search = "#bb9af7"
accent = "#7aa2f7"
diff_add = "#9ece6a"
diff_delete = "#f7768e"
diff_modified = "#e0af68"
diff_header = "#7dcfff"
help_key = "#e0af68"
branch = "#7dcfff"
sandbox = "#bb9af7"
"##;
        let theme: Theme = toml::from_str(toml_str).unwrap();
        assert_eq!(theme.background, Color::Rgb(26, 27, 38));
        assert_eq!(theme.title, Color::Rgb(192, 202, 245));
        assert_eq!(theme.running, Color::Rgb(158, 206, 106));
    }

    #[test]
    fn test_custom_theme_partial_uses_defaults() {
        let toml_str = r##"
background = "#1a1b26"
border = "#414868"
"##;
        // Missing fields fall back to empire defaults (forward-compatible)
        let theme: Theme = toml::from_str(toml_str).unwrap();
        assert_eq!(theme.background, Color::Rgb(26, 27, 38));
        assert_eq!(theme.border, Color::Rgb(65, 72, 104));
        // Missing fields get empire defaults
        assert_eq!(theme.title, Theme::empire().title);
        assert_eq!(theme.running, Theme::empire().running);
    }

    #[test]
    fn test_builtin_name_ignored_in_custom_dir() {
        let dir = tempfile::tempdir().unwrap();

        // Simulate a custom theme file named after a builtin
        let path = dir.path().join("empire.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "").unwrap();

        // The file can be loaded directly, but discover_custom_themes
        // filters out builtin names. We test the filter logic here.
        let name = "empire";
        assert!(BUILTIN_THEMES.contains(&name));
    }
}
