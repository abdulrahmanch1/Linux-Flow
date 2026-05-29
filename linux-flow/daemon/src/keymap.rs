//! Dynamic XKB keymap generation for layout-independent Unicode typing.
//!
//! Each unique character in a text run is assigned an evdev keycode. The
//! compositor maps that keycode to the character's keysym at modifier level 0,
//! so Shift is never needed and any Unicode codepoint — Arabic, emoji, CJK —
//! types correctly regardless of the user's active keyboard layout.

use std::collections::HashSet;
use std::fmt::Write;

/// Max distinct characters per keymap. evdev codes `1..=246` map to XKB
/// keycodes `9..=254`; XKB keycode 255 (evdev [`WARMUP_EVDEV`]) is reserved
/// for the warm-up key. All stay inside the `minimum=8; maximum=255` range.
pub const MAX_KEYS: usize = 246;

/// evdev keycode of the sacrificial warm-up key (XKB keycode 255). Mapped to
/// `VoidSymbol` so it never produces output. Compositors frequently drop the
/// first key event after a keymap change; firing this first absorbs that drop.
pub const WARMUP_EVDEV: u32 = 247;

/// XKB keysym token for a character. Control characters that must trigger real
/// key behaviour use named keysyms; everything else uses the Unicode form
/// (`U` + hex codepoint), which xkbcommon resolves to the matching keysym.
fn keysym_token(c: char) -> String {
    match c {
        '\n' => "Return".to_string(),
        '\t' => "Tab".to_string(),
        _ => format!("U{:04X}", c as u32),
    }
}

/// Builds a complete, null-terminated XKB keymap mapping each char in `chars`
/// to evdev keycode `index + 1` (XKB keycode `index + 9`). `chars` must hold
/// distinct characters and be no longer than [`MAX_KEYS`].
pub fn build_keymap(chars: &[char]) -> String {
    let mut codes = String::new();
    let mut symbols = String::new();
    for (i, &c) in chars.iter().enumerate() {
        // evdev = i + 1, XKB keycode = evdev + 8 = i + 9.
        let _ = writeln!(codes, "    <K{i}>={};", i + 9);
        let _ = writeln!(symbols, "    key <K{i}>{{[{}]}};", keysym_token(c));
    }
    // Warm-up key at XKB keycode 255 (evdev WARMUP_EVDEV), produces no output.
    codes.push_str("    <KW>=255;\n");
    symbols.push_str("    key <KW>{[VoidSymbol]};\n");
    let mut km = String::from("xkb_keymap {\n");
    km.push_str("  xkb_keycodes { minimum=8; maximum=255;\n");
    km.push_str(&codes);
    km.push_str("  };\n");
    km.push_str("  xkb_types   { include \"complete\" };\n");
    km.push_str("  xkb_compat  { include \"complete\" };\n");
    km.push_str("  xkb_symbols {\n");
    km.push_str(&symbols);
    km.push_str("  };\n");
    km.push_str("  xkb_geometry { };\n");
    km.push_str("};\n\0");
    km
}

/// Splits `text` into consecutive segments, each containing at most
/// [`MAX_KEYS`] distinct characters, preserving order. `\r` is dropped. A
/// single dynamic keymap covers each returned segment.
pub fn segments(text: &str) -> Vec<Vec<char>> {
    let mut out = Vec::new();
    let mut cur: Vec<char> = Vec::new();
    let mut seen: HashSet<char> = HashSet::new();
    for c in text.chars() {
        if c == '\r' {
            continue;
        }
        if !seen.contains(&c) && seen.len() == MAX_KEYS {
            out.push(std::mem::take(&mut cur));
            seen.clear();
        }
        seen.insert(c);
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
