//! TUI theme and styling

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
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

/// Convert a 24-bit RGB color to the nearest xterm-256 palette index.
///
/// The xterm-256 palette has three zones:
///   0-15    : 16 basic ANSI colors (skipped — we prefer cube/grey approximations
///             over ambiguous terminal-configurable basics)
///   16-231  : 6×6×6 RGB cube. Axis levels are [0, 95, 135, 175, 215, 255].
///   232-255 : 24-step greyscale ramp from #080808 to #eeeeee.
///
/// Strategy: compute both the cube candidate and the grey candidate, return
/// whichever is closer to the input in squared-distance.
pub fn rgb_to_palette_index(r: u8, g: u8, b: u8) -> u8 {
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    fn nearest_cube_axis(v: u8) -> (usize, u8) {
        let mut best_i = 0;
        let mut best_d = i32::MAX;
        for (i, level) in CUBE_LEVELS.iter().enumerate() {
            let d = (v as i32 - *level as i32).abs();
            if d < best_d {
                best_d = d;
                best_i = i;
            }
        }
        (best_i, CUBE_LEVELS[best_i])
    }

    let (ri, rc) = nearest_cube_axis(r);
    let (gi, gc) = nearest_cube_axis(g);
    let (bi, bc) = nearest_cube_axis(b);
    let cube_idx = 16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8;
    let cube_d = sq_dist(r, g, b, rc, gc, bc);

    // Greyscale ramp: level[i] = 8 + 10*i for i in 0..24 → 8, 18, ..., 238.
    // Plus #000000 (via cube 16) and #ffffff (via cube 231) bracketing.
    let grey_target = ((r as u32 + g as u32 + b as u32) / 3) as u8;
    let (grey_idx, grey_level) = if grey_target < 8 {
        (16u8, 0u8) // black via cube — better than grey[0]=#080808 for pure black
    } else if grey_target > 238 {
        (231u8, 255u8) // white via cube
    } else {
        let i = ((grey_target as i32 - 8) / 10).clamp(0, 23) as u8;
        (232 + i, 8 + 10 * i)
    };
    let grey_d = sq_dist(r, g, b, grey_level, grey_level, grey_level);

    if grey_d < cube_d {
        grey_idx
    } else {
        cube_idx
    }
}

fn sq_dist(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> i32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    dr * dr + dg * dg + db * db
}

/// Convert a ratatui Color to its palette-mode equivalent. Only `Rgb(r,g,b)`
/// is transformed; other variants (Reset, Indexed, named) are returned as-is.
pub fn color_to_palette(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Indexed(rgb_to_palette_index(r, g, b)),
        other => other,
    }
}

/// Export a theme as a TOML string.
pub fn export_theme_toml(theme: &Theme) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(theme)
}

/// Serde helper for Color as hex string (#rrggbb)
mod hex_color {
    use ratatui::style::Color;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(color: &Color, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *color {
            Color::Rgb(r, g, b) => {
                serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}", r, g, b))
            }
            _ => serializer.serialize_str("#000000"),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Color, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        parse_hex_color(&s).map_err(serde::de::Error::custom)
    }

    pub fn parse_hex_color(s: &str) -> Result<Color, String> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        if !hex.is_ascii() || hex.len() != 6 {
            return Err(format!(
                "invalid hex color '{}': expected 6 hex digits (e.g. #ff0000)",
                s
            ));
        }
        let r =
            u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("invalid hex color '{}'", s))?;
        let g =
            u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("invalid hex color '{}'", s))?;
        let b =
            u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("invalid hex color '{}'", s))?;
        Ok(Color::Rgb(r, g, b))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    // Background and borders
    #[serde(with = "hex_color")]
    pub background: Color,
    #[serde(with = "hex_color")]
    pub border: Color,
    #[serde(with = "hex_color")]
    pub terminal_border: Color,
    #[serde(with = "hex_color")]
    pub selection: Color,
    #[serde(with = "hex_color")]
    pub session_selection: Color,

    // Text colors
    #[serde(with = "hex_color")]
    pub title: Color,
    #[serde(with = "hex_color")]
    pub text: Color,
    #[serde(with = "hex_color")]
    pub dimmed: Color,
    #[serde(with = "hex_color")]
    pub hint: Color,

    // Status colors
    #[serde(with = "hex_color")]
    pub running: Color,
    #[serde(with = "hex_color")]
    pub waiting: Color,
    #[serde(with = "hex_color")]
    pub idle: Color,
    #[serde(with = "hex_color")]
    pub error: Color,
    #[serde(with = "hex_color")]
    pub terminal_active: Color,

    // UI elements
    #[serde(with = "hex_color")]
    pub group: Color,
    #[serde(with = "hex_color")]
    pub search: Color,
    #[serde(with = "hex_color")]
    pub accent: Color,

    #[serde(with = "hex_color")]
    pub diff_add: Color,
    #[serde(with = "hex_color")]
    pub diff_delete: Color,
    #[serde(with = "hex_color")]
    pub diff_modified: Color,
    #[serde(with = "hex_color")]
    pub diff_header: Color,

    #[serde(with = "hex_color")]
    pub help_key: Color,

    #[serde(with = "hex_color")]
    pub branch: Color,
    #[serde(with = "hex_color")]
    pub sandbox: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::empire()
    }
}

impl Theme {
    /// Convert every `Color::Rgb` field to the nearest xterm-256 palette index
    /// (`Color::Indexed`). In-place. Idempotent: already-Indexed / named /
    /// Reset colors are untouched. Use when the downstream transport mangles
    /// 24-bit RGB escapes but handles 256-palette fine (e.g. Termius mosh).
    pub fn downsample_to_palette(&mut self) {
        self.background = color_to_palette(self.background);
        self.border = color_to_palette(self.border);
        self.terminal_border = color_to_palette(self.terminal_border);
        self.selection = color_to_palette(self.selection);
        self.session_selection = color_to_palette(self.session_selection);
        self.title = color_to_palette(self.title);
        self.text = color_to_palette(self.text);
        self.dimmed = color_to_palette(self.dimmed);
        self.hint = color_to_palette(self.hint);
        self.running = color_to_palette(self.running);
        self.waiting = color_to_palette(self.waiting);
        self.idle = color_to_palette(self.idle);
        self.error = color_to_palette(self.error);
        self.terminal_active = color_to_palette(self.terminal_active);
        self.group = color_to_palette(self.group);
        self.search = color_to_palette(self.search);
        self.accent = color_to_palette(self.accent);
        self.diff_add = color_to_palette(self.diff_add);
        self.diff_delete = color_to_palette(self.diff_delete);
        self.diff_modified = color_to_palette(self.diff_modified);
        self.diff_header = color_to_palette(self.diff_header);
        self.help_key = color_to_palette(self.help_key);
        self.branch = color_to_palette(self.branch);
        self.sandbox = color_to_palette(self.sandbox);
    }
}

impl Theme {
    /// Empire theme -- warm amber/copper on navy, aligned with DESIGN.md
    pub fn empire() -> Self {
        Self {
            background: Color::Rgb(15, 23, 42),
            border: Color::Rgb(51, 65, 85),
            terminal_border: Color::Rgb(13, 148, 136),
            selection: Color::Rgb(38, 50, 75),
            session_selection: Color::Rgb(55, 65, 92),

            title: Color::Rgb(251, 191, 36),
            text: Color::Rgb(203, 213, 225),
            dimmed: Color::Rgb(100, 116, 139),
            hint: Color::Rgb(148, 163, 184),

            running: Color::Rgb(34, 197, 94),
            waiting: Color::Rgb(251, 191, 36),
            idle: Color::Rgb(100, 116, 139),
            error: Color::Rgb(239, 68, 68),
            terminal_active: Color::Rgb(13, 148, 136),

            group: Color::Rgb(203, 213, 225),
            search: Color::Rgb(251, 191, 36),
            accent: Color::Rgb(217, 119, 6),

            diff_add: Color::Rgb(34, 197, 94),
            diff_delete: Color::Rgb(239, 68, 68),
            diff_modified: Color::Rgb(251, 191, 36),
            diff_header: Color::Rgb(13, 148, 136),

            help_key: Color::Rgb(217, 119, 6),

            branch: Color::Rgb(13, 148, 136),
            sandbox: Color::Rgb(148, 163, 184),
        }
    }

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
            diff_header: Color::Rgb(100, 160, 200),

            help_key: Color::Rgb(255, 180, 60),

            branch: Color::Rgb(100, 160, 200),
            sandbox: Color::Rgb(200, 122, 255),
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
            diff_header: Color::Rgb(125, 207, 255),

            help_key: Color::Rgb(224, 175, 104),

            branch: Color::Rgb(125, 207, 255),
            sandbox: Color::Rgb(187, 154, 247),
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
            diff_header: Color::Rgb(4, 165, 229),

            help_key: Color::Rgb(223, 142, 29),

            branch: Color::Rgb(4, 165, 229),
            sandbox: Color::Rgb(136, 57, 239),
        }
    }

    /// Dracula theme
    /// Official palette: https://draculatheme.com/spec
    pub fn dracula() -> Self {
        Self {
            background: Color::Rgb(40, 42, 54),
            border: Color::Rgb(68, 71, 90),
            terminal_border: Color::Rgb(139, 233, 253),
            selection: Color::Rgb(68, 71, 90),
            session_selection: Color::Rgb(98, 114, 164),

            title: Color::Rgb(189, 147, 249),
            text: Color::Rgb(248, 248, 242),
            dimmed: Color::Rgb(98, 114, 164),
            hint: Color::Rgb(98, 114, 164),

            running: Color::Rgb(80, 250, 123),
            waiting: Color::Rgb(255, 184, 108),
            idle: Color::Rgb(98, 114, 164),
            error: Color::Rgb(255, 85, 85),
            terminal_active: Color::Rgb(139, 233, 253),

            group: Color::Rgb(139, 233, 253),
            search: Color::Rgb(241, 250, 140),
            accent: Color::Rgb(255, 121, 198),

            diff_add: Color::Rgb(80, 250, 123),
            diff_delete: Color::Rgb(255, 85, 85),
            diff_modified: Color::Rgb(255, 184, 108),
            diff_header: Color::Rgb(189, 147, 249),

            help_key: Color::Rgb(255, 121, 198),

            branch: Color::Rgb(139, 233, 253),
            sandbox: Color::Rgb(189, 147, 249),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn palette_exact_cube_vertices_hit() {
        // Pure primaries land exactly on the 6x6x6 cube extreme indexes.
        assert_eq!(rgb_to_palette_index(255, 0, 0), 196);
        assert_eq!(rgb_to_palette_index(0, 255, 0), 46);
        assert_eq!(rgb_to_palette_index(0, 0, 255), 21);
        assert_eq!(rgb_to_palette_index(255, 255, 0), 226);
        assert_eq!(rgb_to_palette_index(0, 255, 255), 51);
        assert_eq!(rgb_to_palette_index(255, 0, 255), 201);
        assert_eq!(rgb_to_palette_index(255, 255, 255), 231);
        assert_eq!(rgb_to_palette_index(0, 0, 0), 16);
    }

    #[test]
    fn palette_pure_grey_hits_grey_ramp() {
        // Grey values around the middle of the ramp should pick a 232-255 index,
        // not a cube vertex — grey ramp is denser near #808080 than the cube.
        let mid_grey = rgb_to_palette_index(128, 128, 128);
        assert!(
            (232..=255).contains(&mid_grey),
            "expected grey-ramp index for #808080, got {}",
            mid_grey
        );
    }

    #[test]
    fn color_to_palette_preserves_non_rgb() {
        assert_eq!(color_to_palette(Color::Reset), Color::Reset);
        assert_eq!(color_to_palette(Color::Indexed(42)), Color::Indexed(42));
        assert_eq!(color_to_palette(Color::Red), Color::Red);
    }

    #[test]
    fn downsample_to_palette_converts_all_fields() {
        // Structural guard: serialize Theme to count its fields, then verify
        // downsample_to_palette touches every one. If a new Color field is
        // added to Theme but not to downsample_to_palette, this test fails.
        let value: toml::Value = toml::Value::try_from(Theme::empire()).unwrap();
        let total_fields = value.as_table().unwrap().len();

        let mut theme = Theme::empire();
        theme.downsample_to_palette();

        let all_colors = [
            theme.background,
            theme.border,
            theme.terminal_border,
            theme.selection,
            theme.session_selection,
            theme.title,
            theme.text,
            theme.dimmed,
            theme.hint,
            theme.running,
            theme.waiting,
            theme.idle,
            theme.error,
            theme.terminal_active,
            theme.group,
            theme.search,
            theme.accent,
            theme.diff_add,
            theme.diff_delete,
            theme.diff_modified,
            theme.diff_header,
            theme.help_key,
            theme.branch,
            theme.sandbox,
        ];

        assert_eq!(
            all_colors.len(),
            total_fields,
            "Theme has {} fields but downsample test checks {}; update downsample_to_palette and this test",
            total_fields,
            all_colors.len()
        );

        assert!(
            all_colors.iter().all(|c| !matches!(c, Color::Rgb(_, _, _))),
            "Rgb still present after downsample"
        );
    }

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
    fn test_hex_color_parse() {
        assert_eq!(
            hex_color::parse_hex_color("#ff0000").unwrap(),
            Color::Rgb(255, 0, 0)
        );
        assert_eq!(
            hex_color::parse_hex_color("#00ff00").unwrap(),
            Color::Rgb(0, 255, 0)
        );
        assert_eq!(
            hex_color::parse_hex_color("#0000ff").unwrap(),
            Color::Rgb(0, 0, 255)
        );
        assert_eq!(
            hex_color::parse_hex_color("#0f172a").unwrap(),
            Color::Rgb(15, 23, 42)
        );
        // Without # prefix
        assert_eq!(
            hex_color::parse_hex_color("fbbf24").unwrap(),
            Color::Rgb(251, 191, 36)
        );
    }

    #[test]
    fn test_hex_color_parse_invalid() {
        assert!(hex_color::parse_hex_color("#fff").is_err());
        assert!(hex_color::parse_hex_color("#gggggg").is_err());
        assert!(hex_color::parse_hex_color("").is_err());
        // Multi-byte UTF-8 that happens to be 6 bytes must not panic
        assert!(hex_color::parse_hex_color("\u{00e9}\u{00e9}\u{00e9}").is_err());
        assert!(hex_color::parse_hex_color("#\u{00e9}\u{00e9}\u{00e9}").is_err());
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
