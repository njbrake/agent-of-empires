//! WCAG contrast helpers used by the TUI to decide when a status color
//! still reads against a background (e.g. session row fg vs the highlight
//! bg) and when we need to swap in `theme.text` for legibility.

use ratatui::style::Color;

/// WCAG 2.x relative luminance for sRGB. `None` when the color isn't an
/// `Rgb(..)` (palette / named / Reset); callers should treat that as
/// "can't determine" and fall back to a safe default.
fn relative_luminance(c: Color) -> Option<f32> {
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => return None,
    };
    let to_lin = |c: u8| -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * to_lin(r) + 0.7152 * to_lin(g) + 0.0722 * to_lin(b))
}

/// WCAG contrast ratio between two colors. `None` if either side isn't
/// `Color::Rgb` (downsampled palette themes, named colors, `Reset`).
pub fn contrast_ratio(a: Color, b: Color) -> Option<f32> {
    let la = relative_luminance(a)?;
    let lb = relative_luminance(b)?;
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// True when `fg` on `bg` clears the WCAG `threshold` (e.g. 3.0 for AA
/// Large / bold UI text, 4.5 for AA Normal).
///
/// Returns `false` for any non-Rgb pair so palette / named / Reset colors
/// take the conservative fallback path. Palette-mode themes downsample
/// every color to `Color::Indexed`, so this matches the pre-contrast
/// "always override" behavior for those modes — there's no clean way to
/// compute WCAG luminance on an xterm-256 index without an inverse table,
/// and selection-bg readability matters more than per-status color when
/// the user already opted into a lossy color space.
pub fn has_min_contrast(fg: Color, bg: Color, threshold: f32) -> bool {
    contrast_ratio(fg, bg).is_some_and(|r| r >= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(hex: u32) -> Color {
        Color::Rgb(
            ((hex >> 16) & 0xff) as u8,
            ((hex >> 8) & 0xff) as u8,
            (hex & 0xff) as u8,
        )
    }

    #[test]
    fn black_on_white_is_max_contrast() {
        let r = contrast_ratio(rgb(0x000000), rgb(0xffffff)).unwrap();
        assert!((r - 21.0).abs() < 0.01, "expected ~21, got {r}");
    }

    #[test]
    fn identical_colors_are_one() {
        let r = contrast_ratio(rgb(0x6272a4), rgb(0x6272a4)).unwrap();
        assert!((r - 1.0).abs() < 0.001, "identical → 1.0, got {r}");
    }

    #[test]
    fn ratio_is_symmetric() {
        let a = contrast_ratio(rgb(0x3c3c3c), rgb(0x50785a)).unwrap();
        let b = contrast_ratio(rgb(0x50785a), rgb(0x3c3c3c)).unwrap();
        assert!((a - b).abs() < 0.001);
    }

    #[test]
    fn dracula_dim_invisible_against_session_selection() {
        // dim == session_selection (#6272a4): contrast 1.0, must not pass
        // ANY positive threshold.
        assert!(!has_min_contrast(rgb(0x6272a4), rgb(0x6272a4), 3.0));
        assert!(!has_min_contrast(rgb(0x6272a4), rgb(0x6272a4), 1.5));
    }

    #[test]
    fn phosphor_running_against_session_selection_passes() {
        // running (#00ffb4) vs session_selection (#3c3c3c) — contrast ~8.4
        // by WCAG math, well above 3:1.
        assert!(has_min_contrast(rgb(0x00ffb4), rgb(0x3c3c3c), 3.0));
    }

    #[test]
    fn phosphor_dim_against_session_selection_fails() {
        // dim (#50785a) vs session_selection (#3c3c3c) — contrast ~2.19,
        // below 3:1. This is the case that motivated the original
        // override-to-theme.text fix.
        assert!(!has_min_contrast(rgb(0x50785a), rgb(0x3c3c3c), 3.0));
    }

    #[test]
    fn non_rgb_colors_return_none() {
        assert!(contrast_ratio(Color::Reset, rgb(0xffffff)).is_none());
        assert!(contrast_ratio(rgb(0xffffff), Color::Indexed(10)).is_none());
        assert!(!has_min_contrast(Color::Reset, rgb(0xffffff), 3.0));
        // Palette-mode (Indexed) falls into the conservative-override path.
        assert!(!has_min_contrast(Color::Indexed(2), rgb(0x3c3c3c), 3.0));
    }
}
