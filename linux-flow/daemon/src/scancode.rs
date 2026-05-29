//! ASCII-to-evdev-scancode lookup for the last-resort uinput typing path.
//!
//! Raw `/dev/uinput` cannot carry a custom keymap, so it can only reproduce
//! characters that exist on a US layout. This path is reached only when neither
//! the Wayland virtual keyboard nor XWayland is available; non-ASCII characters
//! are skipped by the caller. Returns `(keycode, needs_shift)`.

use crate::keys;

pub fn ascii_scancode(c: char) -> Option<(u16, bool)> {
    let lower = |k: u16| Some((k, false));
    let upper = |k: u16| Some((k, true));
    match c {
        'a' => lower(keys::KEY_A), 'A' => upper(keys::KEY_A),
        'b' => lower(keys::KEY_B), 'B' => upper(keys::KEY_B),
        'c' => lower(keys::KEY_C), 'C' => upper(keys::KEY_C),
        'd' => lower(keys::KEY_D), 'D' => upper(keys::KEY_D),
        'e' => lower(keys::KEY_E), 'E' => upper(keys::KEY_E),
        'f' => lower(keys::KEY_F), 'F' => upper(keys::KEY_F),
        'g' => lower(keys::KEY_G), 'G' => upper(keys::KEY_G),
        'h' => lower(keys::KEY_H), 'H' => upper(keys::KEY_H),
        'i' => lower(keys::KEY_I), 'I' => upper(keys::KEY_I),
        'j' => lower(keys::KEY_J), 'J' => upper(keys::KEY_J),
        'k' => lower(keys::KEY_K), 'K' => upper(keys::KEY_K),
        'l' => lower(keys::KEY_L), 'L' => upper(keys::KEY_L),
        'm' => lower(keys::KEY_M), 'M' => upper(keys::KEY_M),
        'n' => lower(keys::KEY_N), 'N' => upper(keys::KEY_N),
        'o' => lower(keys::KEY_O), 'O' => upper(keys::KEY_O),
        'p' => lower(keys::KEY_P), 'P' => upper(keys::KEY_P),
        'q' => lower(keys::KEY_Q), 'Q' => upper(keys::KEY_Q),
        'r' => lower(keys::KEY_R), 'R' => upper(keys::KEY_R),
        's' => lower(keys::KEY_S), 'S' => upper(keys::KEY_S),
        't' => lower(keys::KEY_T), 'T' => upper(keys::KEY_T),
        'u' => lower(keys::KEY_U), 'U' => upper(keys::KEY_U),
        'v' => lower(keys::KEY_V), 'V' => upper(keys::KEY_V),
        'w' => lower(keys::KEY_W), 'W' => upper(keys::KEY_W),
        'x' => lower(keys::KEY_X), 'X' => upper(keys::KEY_X),
        'y' => lower(keys::KEY_Y), 'Y' => upper(keys::KEY_Y),
        'z' => lower(keys::KEY_Z), 'Z' => upper(keys::KEY_Z),
        '1' => lower(keys::KEY_1), '!' => upper(keys::KEY_1),
        '2' => lower(keys::KEY_2), '@' => upper(keys::KEY_2),
        '3' => lower(keys::KEY_3), '#' => upper(keys::KEY_3),
        '4' => lower(keys::KEY_4), '$' => upper(keys::KEY_4),
        '5' => lower(keys::KEY_5), '%' => upper(keys::KEY_5),
        '6' => lower(keys::KEY_6), '^' => upper(keys::KEY_6),
        '7' => lower(keys::KEY_7), '&' => upper(keys::KEY_7),
        '8' => lower(keys::KEY_8), '*' => upper(keys::KEY_8),
        '9' => lower(keys::KEY_9), '(' => upper(keys::KEY_9),
        '0' => lower(keys::KEY_0), ')' => upper(keys::KEY_0),
        '-' => lower(keys::KEY_MINUS), '_' => upper(keys::KEY_MINUS),
        '=' => lower(keys::KEY_EQUAL), '+' => upper(keys::KEY_EQUAL),
        '[' => lower(keys::KEY_LEFTBRACE), '{' => upper(keys::KEY_LEFTBRACE),
        ']' => lower(keys::KEY_RIGHTBRACE), '}' => upper(keys::KEY_RIGHTBRACE),
        ';' => lower(keys::KEY_SEMICOLON), ':' => upper(keys::KEY_SEMICOLON),
        '\'' => lower(keys::KEY_APOSTROPHE), '"' => upper(keys::KEY_APOSTROPHE),
        ',' => lower(keys::KEY_COMMA), '<' => upper(keys::KEY_COMMA),
        '.' => lower(keys::KEY_DOT), '>' => upper(keys::KEY_DOT),
        '/' => lower(keys::KEY_SLASH), '?' => upper(keys::KEY_SLASH),
        '\\' => lower(keys::KEY_BACKSLASH), '|' => upper(keys::KEY_BACKSLASH),
        '`' => lower(keys::KEY_GRAVE), '~' => upper(keys::KEY_GRAVE),
        ' ' => lower(keys::KEY_SPACE),
        '\n' => lower(keys::KEY_ENTER),
        _ => None,
    }
}
