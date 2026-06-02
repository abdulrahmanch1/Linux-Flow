//! Supported push-to-talk keys — single source of truth for the picker.
//!
//! Push-to-talk holds the key down while speaking, so only modifier keys are
//! viable: they produce no character while held. Each entry pairs the evdev
//! name persisted in `config.toml` with a friendly label for the UI.

/// `(evdev name stored in config, friendly UI label)`, in display order. The
/// first entry is the default. Every name here must be capturable by the evdev
/// backend; a consistency test enforces that.
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
];

/// Index of a stored evdev hotkey name in [`SUPPORTED_HOTKEYS`], or `None` if it
/// is not one of the supported keys.
pub fn index_of(evdev_name: &str) -> Option<usize> {
    SUPPORTED_HOTKEYS
        .iter()
        .position(|(name, _)| *name == evdev_name)
}
