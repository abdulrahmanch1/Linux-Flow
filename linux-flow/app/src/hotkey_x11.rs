use std::os::raw::c_int;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::mpsc;
use x11::xlib::{
    AnyModifier, GrabModeAsync, KeyPress, KeyPressMask, KeyRelease, KeyReleaseMask, XCloseDisplay,
    XDefaultRootWindow, XErrorEvent, XEvent, XGrabKey, XNextEvent, XOpenDisplay, XPending,
    XSelectInput, XSetErrorHandler, XSync, XUngrabKey,
};

static XGRAB_FAILED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn xlib_error_handler(
    _display: *mut x11::xlib::Display,
    _event: *mut XErrorEvent,
) -> c_int {
    XGRAB_FAILED.store(true, Ordering::SeqCst);
    0
}

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
    pub fn run(
        tx: mpsc::Sender<HotkeyEvent>,
        hotkey: &str,
        stop: Arc<AtomicBool>,
    ) -> Result<(), HotkeyError> {
        // SAFETY: All X11 calls follow documented usage patterns;
        // return values are checked before dereferencing.
        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Err(HotkeyError::X11Connect);
        }

        let root = unsafe { XDefaultRootWindow(display) };
        let keycode = evdev_name_to_x11_keycode(hotkey).ok_or(HotkeyError::X11Keycode)?;

        // Install a non-fatal error handler so BadAccess from XGrabKey does not
        // call exit() via Xlib's default handler and kill the whole process.
        XGRAB_FAILED.store(false, Ordering::SeqCst);
        unsafe { XSetErrorHandler(Some(xlib_error_handler)) };

        unsafe {
            XSelectInput(display, root, KeyPressMask | KeyReleaseMask);
            XGrabKey(
                display,
                keycode as c_int,
                AnyModifier,
                root,
                0, // owner_events = False
                GrabModeAsync,
                GrabModeAsync,
            );
            XSync(display, 0); // flush so any BadAccess error is delivered now
        }

        if XGRAB_FAILED.load(Ordering::SeqCst) {
            unsafe { XCloseDisplay(display) };
            return Err(crate::hotkey::HotkeyError::X11GrabFailed);
        }

        let result = Self::event_loop(display, keycode, &tx, &stop);

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
        stop: &AtomicBool,
    ) -> Result<(), HotkeyError> {
        let mut event: XEvent = unsafe { std::mem::zeroed() };

        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(HotkeyError::StreamClosed);
            }

            // Poll so the stop flag is checked every ~50 ms instead of
            // blocking indefinitely inside XNextEvent.
            if unsafe { XPending(display) } == 0 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }

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
