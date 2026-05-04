//! Built-in themes and the `Theme` palette struct.

use std::time::Duration;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use super::palette::color_to_palette;

/// Convert the user-configured decay duration (minutes) into a `Duration`.
/// `0` returns `Duration::ZERO`, which the freshness logic treats as
/// "fully decayed immediately" — a documented opt-out: every Idle row
/// renders with the static idle look the moment its Stop hook fires.
pub fn idle_decay_window(minutes: u64) -> Duration {
    Duration::from_secs(minutes.saturating_mul(60))
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
    /// Color for a session within the idle decay window (just transitioned
    /// to Idle, hasn't aged out yet). Held constant for the full window so
    /// the breathe rattle's pulse stays visually consistent, then snaps to
    /// `idle` once the window expires. Should sit between `waiting`
    /// (brightest, "needs you NOW") and `idle` (dimmest, "no rush") on the
    /// theme's perceived-attention scale.
    #[serde(with = "hex_color")]
    pub fresh_idle: Color,
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
    /// Color for an Idle session, given the elapsed time since it
    /// transitioned to Idle and the user-configured decay window.
    ///
    /// Two-state binary: `fresh_idle` while age is inside the window,
    /// `idle` once past it (or when age/window aren't usable: `None` age,
    /// zero window). The pulse phase deliberately holds a constant color
    /// — a continuous lerp under the breathe rattle reads as noisy. If we
    /// ever want a gradient back, add an interpolator and call it here.
    pub fn idle_color_at_age(&self, age: Option<Duration>, window: Duration) -> Color {
        let Some(age) = age else {
            return self.idle;
        };
        if window.is_zero() || age >= window {
            return self.idle;
        }
        self.fresh_idle
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
        self.fresh_idle = color_to_palette(self.fresh_idle);
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
            // Waiting sits at the brightest point in the brand ramp
            // (amber-400) so it wins on luminance against the fresh-idle
            // color. Saturation alone wasn't enough on dark backgrounds —
            // the deeper copper read as dimmer, not more urgent.
            // Fresh-idle drops one rung to amber-500 so the hierarchy is
            // bright-amber > copper-amber > slate.
            waiting: Color::Rgb(251, 191, 36),
            fresh_idle: Color::Rgb(245, 158, 11),
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
            fresh_idle: Color::Rgb(255, 123, 28),
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
            fresh_idle: Color::Rgb(255, 158, 100),
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
            // Light theme: darker = more attention. Waiting takes the
            // deepest peach, fresh_idle the mid amber, idle the lightest
            // slate so the row visibly fades into the page over time.
            waiting: Color::Rgb(254, 100, 11),
            fresh_idle: Color::Rgb(223, 142, 29),
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
            fresh_idle: Color::Rgb(255, 121, 80),
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

/// Serde helper for Color as hex string (#rrggbb)
pub(super) mod hex_color {
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

#[cfg(test)]
mod tests {
    use super::*;

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
            theme.fresh_idle,
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
    fn idle_color_at_age_boundaries() {
        let theme = Theme::empire();
        let window = idle_decay_window(20);
        // No timestamp = decayed.
        assert_eq!(theme.idle_color_at_age(None, window), theme.idle);
        // Zero age = fresh.
        assert_eq!(
            theme.idle_color_at_age(Some(Duration::ZERO), window),
            theme.fresh_idle
        );
        // Inside the window = fresh.
        assert_eq!(
            theme.idle_color_at_age(Some(window / 2), window),
            theme.fresh_idle
        );
        // At the boundary clamps to decayed (age >= window).
        assert_eq!(theme.idle_color_at_age(Some(window), window), theme.idle);
        // Past the window = decayed.
        assert_eq!(
            theme.idle_color_at_age(Some(window + Duration::from_secs(60)), window),
            theme.idle
        );
    }

    #[test]
    fn idle_color_at_age_zero_window_disables_freshness() {
        // window = 0 is the documented opt-out: every Idle row renders
        // as fully decayed regardless of age. No pulse, no fresh tint.
        let theme = Theme::empire();
        assert_eq!(
            theme.idle_color_at_age(Some(Duration::from_secs(1)), Duration::ZERO),
            theme.idle
        );
        assert_eq!(
            theme.idle_color_at_age(Some(Duration::from_secs(1_000_000)), Duration::ZERO),
            theme.idle
        );
    }

    #[test]
    fn theme_attention_hierarchy_holds() {
        // Visual hierarchy: Waiting is the most attention-grabbing state;
        // fresh-idle sits one rung dimmer; decayed idle blends in. On dark
        // backgrounds "more attention" means HIGHER perceived luminance;
        // on light backgrounds it means LOWER (the warm hues read against
        // the bright surface). The check picks the comparison direction
        // off the theme's own background. Rec. 601 is good enough for a
        // pairwise sanity check, not a formal contrast metric.
        //
        // Heuristic limit: a custom user theme with a mid-tone background
        // (luminance near the 128 cutoff) could fall on the wrong side of
        // the dark/light split and fail this assertion in surprising
        // ways. That's intentional — the test guards the 5 built-ins, not
        // arbitrary user themes loaded from `~/.config/agent-of-empires/
        // themes/*.toml`. If a custom-theme contributor needs to bypass
        // this, they should pick `fresh_idle` themselves rather than rely
        // on the test to validate it.
        fn luminance(c: Color) -> f32 {
            match c {
                Color::Rgb(r, g, b) => 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32,
                _ => 0.0,
            }
        }
        for (name, theme) in [
            ("empire", Theme::empire()),
            ("phosphor", Theme::phosphor()),
            ("tokyo_night_storm", Theme::tokyo_night_storm()),
            ("catppuccin_latte", Theme::catppuccin_latte()),
            ("dracula", Theme::dracula()),
        ] {
            let bg = luminance(theme.background);
            let dark_bg = bg < 128.0;
            let cmp = |label_a, a, label_b, b| {
                if dark_bg {
                    assert!(
                        a > b,
                        "{name} (dark bg): {label_a} luminance {a:.1} should exceed {label_b} {b:.1}"
                    );
                } else {
                    assert!(
                        a < b,
                        "{name} (light bg): {label_a} luminance {a:.1} should be below {label_b} {b:.1}"
                    );
                }
            };
            let w = luminance(theme.waiting);
            let f = luminance(theme.fresh_idle);
            let i = luminance(theme.idle);
            // Waiting beats fresh-idle.
            cmp("waiting", w, "fresh_idle", f);
            // Fresh-idle beats fully-decayed idle.
            cmp("fresh_idle", f, "idle", i);
        }
    }
}
