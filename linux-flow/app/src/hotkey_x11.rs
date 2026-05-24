use std::os::raw::c_int;
use std::ptr;

use tokio::sync::mpsc;
use x11::xlib::{
    AnyModifier, GrabModeAsync, KeyPress, KeyPressMask, KeyRelease, KeyReleaseMask, XCloseDisplay,
    XDefaultRootWindow, XEvent, XGrabKey, XNextEvent, XOpenDisplay, XSelectInput, XUngrabKey,
};

use crate::hotkey::{HotkeyError, HotkeyEvent};

fn evdev_name_to_x11_keycode(name: &str) -> Option<u8> {
    // X11 keycode = Linux evdev keycode + 8 (stable kernel ABI, works even
    // when the compositor omits a keysym entry for the key, e.g. Control_R
    // on COSMIC's XWayland).
    let evdev: u8 = match name {
        "KEY_RIGHTCTRL" => 97,
        "KEY_LEFTCTRL" => 29,
        "KEY_RIGHTALT" => 108,
        "KEY_LEFTALT" => 56,
        "KEY_RIGHTMETA" => 126,
        "KEY_LEFTMETA" => 125,
        "KEY_CAPSLOCK" => 58,
        "KEY_RIGHTSHIFT" => 54,
        "KEY_LEFTSHIFT" => 42,
        _ => return None,
    };
    Some(evdev + 8)
}

pub struct X11Backend;

impl X11Backend {
    /// Blocks the calling thread. Must be spawned via `std::thread::spawn`.
    pub fn run(tx: mpsc::Sender<HotkeyEvent>, hotkey: &str) -> Result<(), HotkeyError> {
        // SAFETY: All X11 calls follow documented usage patterns;
        // return values are checked before dereferencing.
        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Err(HotkeyError::X11Connect);
        }

        let root = unsafe { XDefaultRootWindow(display) };
        let keycode = evdev_name_to_x11_keycode(hotkey).ok_or(HotkeyError::X11Keycode)?;

        unsafe {
            XSelectInput(display, root, KeyPressMask | KeyReleaseMask);
            XGrabKey(
                display,
                keycode as c_int,
                AnyModifier,
                root,
                0, // owner_events = False: deliver events to grab window
                GrabModeAsync,
                GrabModeAsync,
            );
        }

        let result = Self::event_loop(display, keycode, &tx);

        unsafe {
            XUngrabKey(display, keycode as c_int, AnyModifier, root);
            XCloseDisplay(display);
        }

        result
    }

    fn event_loop(
        display: *mut x11::xlib::Display,
        keycode: u8,
        tx: &mpsc::Sender<HotkeyEvent>,
    ) -> Result<(), HotkeyError> {
        let mut event: XEvent = unsafe { std::mem::zeroed() };

        loop {
            let ret = unsafe { XNextEvent(display, &mut event) };
            if ret < 0 {
                return Err(HotkeyError::X11Connect);
            }

            let (ty, code) = unsafe { (event.type_, event.key.keycode) };

            if code != keycode as u32 {
                continue;
            }

            let hk = if ty == KeyPress {
                HotkeyEvent::Pressed
            } else if ty == KeyRelease {
                HotkeyEvent::Released
            } else {
                continue;
            };

            if tx.blocking_send(hk).is_err() {
                return Err(HotkeyError::StreamClosed);
            }
        }
    }
}
