use crate::{
    clipboard::{self, DisplayServer},
    error::DaemonError,
    keymap, keys,
    server::KeyInjector,
};
use libc::{c_int, c_ulong};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::io::AsRawFd,
    sync::Mutex,
    thread,
    time::Duration,
};

// ioctl request codes from linux/uinput.h
// _IO('U', 1) and _IO('U', 2)
const UI_DEV_CREATE: c_ulong = 0x5501;
const UI_DEV_DESTROY: c_ulong = 0x5502;
// _IOW('U', 100, int=4): 0x40000000|(4<<16)|(0x55<<8)|100
const UI_SET_EVBIT: c_ulong = 0x40045564;
// _IOW('U', 101, int=4)
const UI_SET_KEYBIT: c_ulong = 0x40045565;
// _IOW('U', 3, uinput_setup): sizeof=92=0x5C → 0x40000000|(0x5C<<16)|(0x55<<8)|3
const UI_DEV_SETUP: c_ulong = 0x405C5503;
const BUS_VIRTUAL: u16 = 0x06;

const ALL_KEYS: &[u16] = &[
    keys::KEY_A,
    keys::KEY_B,
    keys::KEY_C,
    keys::KEY_D,
    keys::KEY_E,
    keys::KEY_F,
    keys::KEY_G,
    keys::KEY_H,
    keys::KEY_I,
    keys::KEY_J,
    keys::KEY_K,
    keys::KEY_L,
    keys::KEY_M,
    keys::KEY_N,
    keys::KEY_O,
    keys::KEY_P,
    keys::KEY_Q,
    keys::KEY_R,
    keys::KEY_S,
    keys::KEY_T,
    keys::KEY_U,
    keys::KEY_V,
    keys::KEY_W,
    keys::KEY_X,
    keys::KEY_Y,
    keys::KEY_Z,
    keys::KEY_0,
    keys::KEY_1,
    keys::KEY_2,
    keys::KEY_3,
    keys::KEY_4,
    keys::KEY_5,
    keys::KEY_6,
    keys::KEY_7,
    keys::KEY_8,
    keys::KEY_9,
    keys::KEY_LEFTSHIFT,
    keys::KEY_LEFTCTRL,
    keys::KEY_SPACE,
    keys::KEY_ENTER,
    keys::KEY_MINUS,
    keys::KEY_EQUAL,
    keys::KEY_LEFTBRACE,
    keys::KEY_RIGHTBRACE,
    keys::KEY_SEMICOLON,
    keys::KEY_APOSTROPHE,
    keys::KEY_COMMA,
    keys::KEY_DOT,
    keys::KEY_SLASH,
    keys::KEY_BACKSLASH,
    keys::KEY_GRAVE,
];

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}
// sizeof = 8(InputId) + 80(name) + 4(ff_effects_max) = 92 — matches UI_DEV_SETUP size
#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

struct Inner {
    fd: File,
}
pub struct UinputInjector {
    inner: Mutex<Inner>,
    display: DisplayServer,
}

impl UinputInjector {
    pub fn new() -> Result<Self, DaemonError> {
        let fd = OpenOptions::new().write(true).open("/dev/uinput")?;
        let raw = fd.as_raw_fd();

        macro_rules! ioctl1 {
            ($req:expr, $arg:expr) => {{
                // SAFETY: fd is open and valid; argument type matches the ioctl spec.
                let r = unsafe { libc::ioctl(raw, $req, $arg as c_int) };
                if r < 0 {
                    return Err(DaemonError::Io(std::io::Error::last_os_error()));
                }
            }};
        }

        ioctl1!(UI_SET_EVBIT, keys::EV_KEY);
        ioctl1!(UI_SET_EVBIT, keys::EV_SYN);
        for &code in ALL_KEYS {
            ioctl1!(UI_SET_KEYBIT, code);
        }

        let mut setup = UinputSetup {
            id: InputId {
                bustype: BUS_VIRTUAL,
                vendor: 0,
                product: 0,
                version: 0,
            },
            name: [0u8; 80],
            ff_effects_max: 0,
        };
        let label = b"linux-flow-helper";
        setup.name[..label.len()].copy_from_slice(label);

        // SAFETY: fd valid; UinputSetup is repr(C) with correct size for this ioctl.
        let r = unsafe { libc::ioctl(raw, UI_DEV_SETUP, &setup as *const UinputSetup) };
        if r < 0 {
            return Err(DaemonError::Io(std::io::Error::last_os_error()));
        }

        // SAFETY: fd valid; UI_DEV_CREATE takes no extra argument.
        let r = unsafe { libc::ioctl(raw, UI_DEV_CREATE) };
        if r < 0 {
            return Err(DaemonError::Io(std::io::Error::last_os_error()));
        }

        thread::sleep(Duration::from_millis(100));
        Ok(Self {
            inner: Mutex::new(Inner { fd }),
            display: clipboard::detect_display(),
        })
    }
}

impl Inner {
    fn write_event(&mut self, typ: u16, code: u16, value: i32) -> Result<(), DaemonError> {
        let ev = libc::input_event {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: typ,
            code,
            value,
        };
        // SAFETY: input_event is repr(C); reinterpreting as a byte slice is valid.
        let bytes = unsafe {
            std::slice::from_raw_parts(&ev as *const _ as *const u8, std::mem::size_of_val(&ev))
        };
        self.fd.write_all(bytes)?;
        Ok(())
    }

    fn key_event(&mut self, code: u16, down: bool) -> Result<(), DaemonError> {
        self.write_event(keys::EV_KEY, code, if down { 1 } else { 0 })?;
        self.write_event(keys::EV_SYN, keys::SYN_REPORT, 0)
    }

    fn inject_latin_char(&mut self, code: u16, shift: bool) -> Result<(), DaemonError> {
        if shift {
            self.key_event(keys::KEY_LEFTSHIFT, true)?;
        }
        self.key_event(code, true)?;
        self.key_event(code, false)?;
        if shift {
            self.key_event(keys::KEY_LEFTSHIFT, false)?;
        }
        thread::sleep(Duration::from_millis(1));
        Ok(())
    }

    fn inject_via_clipboard(
        &mut self,
        text: &str,
        display: DisplayServer,
    ) -> Result<(), DaemonError> {
        clipboard::clipboard_write(display, text)?;
        thread::sleep(Duration::from_millis(50));
        self.key_event(keys::KEY_LEFTCTRL, true)?;
        self.key_event(keys::KEY_V, true)?;
        self.key_event(keys::KEY_V, false)?;
        self.key_event(keys::KEY_LEFTCTRL, false)?;
        thread::sleep(Duration::from_millis(100));
        clipboard::clipboard_clear(display);
        Ok(())
    }
}

impl KeyInjector for UinputInjector {
    fn inject_text(&self, text: &str) -> Result<(), DaemonError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| DaemonError::InjectionFailed("mutex poisoned".into()))?;
        let latin_only = text.is_ascii() && text.chars().all(|c| keymap::char_to_key(c).is_some());
        if latin_only {
            for c in text.chars() {
                if let Some((code, shift)) = keymap::char_to_key(c) {
                    inner.inject_latin_char(code, shift)?;
                }
            }
        } else {
            inner.inject_via_clipboard(text, self.display)?;
        }
        Ok(())
    }
}

impl Drop for UinputInjector {
    fn drop(&mut self) {
        // get_mut() avoids locking since &mut self guarantees exclusive access.
        if let Ok(inner) = self.inner.get_mut() {
            // SAFETY: fd is valid; UI_DEV_DESTROY removes the virtual input device.
            unsafe {
                libc::ioctl(inner.fd.as_raw_fd(), UI_DEV_DESTROY);
            }
        }
    }
}
