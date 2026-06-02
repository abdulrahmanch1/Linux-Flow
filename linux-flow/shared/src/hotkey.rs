//! Supported push-to-talk keys — single source of truth for the picker.
//!
//! Push-to-talk holds the key down while speaking, so only keys that emit no
//! character while held are viable: modifier keys, plus the extended function
//! keys F13–F24 that macro/extra keyboards expose. Each entry pairs the evdev
//! name persisted in `config.toml` with a friendly label for the UI.

/// `(evdev name stored in config, friendly UI label)`, in display order. The
/// first entry is the default. Every name here must be capturable by the evdev
/// backend and mappable by the X11 and capture paths.
pub const SUPPORTED_HOTKEYS: &[(&str, &str)] = &[
    ("KEY_RIGHTCTRL", "Right Ctrl"),
    ("KEY_LEFTCTRL", "Left Ctrl"),
    ("KEY_RIGHTALT", "Right Alt"),
    ("KEY_LEFTALT", "Left Alt"),
    ("KEY_RIGHTMETA", "Right Super"),
    ("KEY_LEFTMETA", "Left Super"),
    ("KEY_CAPSLOCK", "Caps Lock"),
    ("KEY_RIGHTSHIFT", "Right Shift"),
    ("KEY_LEFTSHIFT", "Left Shift"),
    ("KEY_F13", "F13"),
    ("KEY_F14", "F14"),
    ("KEY_F15", "F15"),
    ("KEY_F16", "F16"),
    ("KEY_F17", "F17"),
    ("KEY_F18", "F18"),
    ("KEY_F19", "F19"),
    ("KEY_F20", "F20"),
    ("KEY_F21", "F21"),
    ("KEY_F22", "F22"),
    ("KEY_F23", "F23"),
    ("KEY_F24", "F24"),
];

/// Index of a stored evdev hotkey name in [`SUPPORTED_HOTKEYS`], or `None` if it
/// is not one of the supported keys.
pub fn index_of(evdev_name: &str) -> Option<usize> {
    SUPPORTED_HOTKEYS
        .iter()
        .position(|(name, _)| *name == evdev_name)
}
