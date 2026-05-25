//! Decoder for tmux control-mode `%output` payloads.
//!
//! tmux escapes every control byte in `%output <pane> <data>` as a
//! three-digit backslash-octal sequence: `\NNN`. Backslashes
//! themselves are emitted as `\134`. Everything else passes through
//! as the raw printable byte. The encoding has no letter escapes
//! (`\n`, `\t`, `\\`); they all use the octal form.
//!
//! This module reverses that encoding so the bytes can be fed to a
//! `vt100::Parser` (or any other consumer that wants raw terminal
//! output). It is intentionally small and panic-free; malformed
//! payloads degrade by emitting the literal bytes that didn't decode,
//! which keeps the fast path forgiving of tmux version drift.

/// Reverse tmux's `%output` octal encoding.
///
/// Returns a `Vec<u8>` because the decoded payload is raw terminal
/// output and is not guaranteed to be valid UTF-8 (ANSI escape
/// sequences begin with `\x1b`, which is fine on its own, but byte
/// streams from a misbehaving agent could include lone surrogates or
/// truncated multi-byte chars).
pub fn decode_output_payload(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `\<oct><oct><oct>`. Need three bytes after `\`.
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let d1 = bytes[i + 1];
            let d2 = bytes[i + 2];
            let d3 = bytes[i + 3];
            if is_octal_digit(d1) && is_octal_digit(d2) && is_octal_digit(d3) {
                // Max value is 0o777 = 511, but tmux only emits bytes
                // (0..=255). Mask to a byte to be safe against any
                // malformed input that overflows: the worst result is
                // truncated bytes, never a panic.
                let val =
                    ((d1 - b'0') as u32 * 64 + (d2 - b'0') as u32 * 8 + (d3 - b'0') as u32) as u8;
                out.push(val);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Split `"%<paneid> <data>"` (i.e. the suffix of a `%output ...`
/// line after the literal `%output ` prefix) into the data portion.
/// Returns `None` if the prefix doesn't look like a `%output` line
/// (no pane-id marker, no space separator).
pub fn extract_output_data(after_prefix: &str) -> Option<&str> {
    let rest = after_prefix.strip_prefix('%')?;
    let sp = rest.find(' ')?;
    Some(&rest[sp + 1..])
}

#[inline]
fn is_octal_digit(b: u8) -> bool {
    (b'0'..=b'7').contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_ascii_unchanged() {
        assert_eq!(decode_output_payload("hello"), b"hello");
    }

    #[test]
    fn decodes_octal_escape_to_byte() {
        // \033 = ESC (0x1B)
        assert_eq!(decode_output_payload("\\033[31m"), b"\x1b[31m");
    }

    #[test]
    fn decodes_backslash_escape() {
        // tmux emits backslash itself as \134.
        assert_eq!(decode_output_payload("\\134"), b"\\");
    }

    #[test]
    fn decodes_newline_escape() {
        // \012 = LF (0x0A)
        assert_eq!(decode_output_payload("a\\012b"), b"a\nb");
    }

    #[test]
    fn decodes_tab_escape() {
        // \011 = TAB (0x09)
        assert_eq!(decode_output_payload("a\\011b"), b"a\tb");
    }

    #[test]
    fn decodes_mixed_payload() {
        // Real-looking ANSI sequence with embedded control bytes.
        let encoded = "\\033[1;32mhi\\033[0m\\012";
        assert_eq!(decode_output_payload(encoded), b"\x1b[1;32mhi\x1b[0m\n");
    }

    #[test]
    fn leaves_truncated_escape_as_literal() {
        // `\03` at end of string isn't a valid 3-digit octal escape;
        // we emit the bytes verbatim rather than panic.
        assert_eq!(decode_output_payload("foo\\03"), b"foo\\03");
    }

    #[test]
    fn leaves_non_octal_digits_as_literal() {
        // `\089` has a non-octal `8` after the backslash; treat the
        // whole thing as literal so we don't silently mis-decode.
        assert_eq!(decode_output_payload("\\089"), b"\\089");
    }

    #[test]
    fn extracts_output_data_after_paneid() {
        // The string passed in is what comes AFTER the literal
        // `%output ` prefix that the caller has already stripped.
        assert_eq!(extract_output_data("%1 hello\\012"), Some("hello\\012"));
        assert_eq!(extract_output_data("%12 \\033[2J"), Some("\\033[2J"));
    }

    #[test]
    fn extract_returns_none_for_malformed_input() {
        // Missing `%paneid` marker.
        assert_eq!(extract_output_data("no-pane-id data"), None);
        // Missing space separator.
        assert_eq!(extract_output_data("%1"), None);
    }

    #[test]
    fn empty_payload_decodes_to_empty() {
        assert_eq!(decode_output_payload(""), b"");
    }
}
