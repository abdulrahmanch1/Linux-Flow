use std::os::raw::{c_int, c_ulong};
use std::ptr;

use tokio::sync::mpsc;
use x11::keysym::XK_Control_R;
use x11::xlib::{
    AnyModifier, GrabModeAsync, KeyPress, KeyPressMask, KeyRelease, KeyReleaseMask,
    XCloseDisplay, XDefaultRootWindow, XEvent, XGrabKey, XKeysymToKeycode, XNextEvent,
    XOpenDisplay, XSelectInput, XUngrabKey,
};

use crate::hotkey::{HotkeyError, HotkeyEvent};

pub struct X11Backend;

impl X11Backend {
    /// Blocks the calling thread. Must be spawned via `std::thread::spawn`.
    pub fn run(tx: mpsc::Sender<HotkeyEvent>) -> Result<(), HotkeyError> {
        // SAFETY: All X11 calls follow documented usage patterns;
        // return values are checked before dereferencing.
        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Err(HotkeyError::X11Connect);
        }

        let root = unsafe { XDefaultRootWindow(display) };
        let keycode = unsafe { XKeysymToKeycode(display, XK_Control_R as c_ulong) };

        if keycode == 0 {
            unsafe { XCloseDisplay(display) };
            return Err(HotkeyError::X11Keycode);
        }

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
            unsafe { XNextEvent(display, &mut event) };

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
