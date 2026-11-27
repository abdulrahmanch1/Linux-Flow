use crate::{error::DaemonError, keys};
use libc::c_ulong;
use std::{fs::File, io::Write};

pub const UI_DEV_CREATE: c_ulong = 0x5501;
pub const UI_DEV_DESTROY: c_ulong = 0x5502;
pub const UI_SET_EVBIT: c_ulong = 0x40045564;
pub const UI_SET_KEYBIT: c_ulong = 0x40045565;
// _IOW('U', 3, uinput_setup): sizeof=92=0x5C → 0x40000000|(0x5C<<16)|(0x55<<8)|3
pub const UI_DEV_SETUP: c_ulong = 0x405C5503;
pub const BUS_VIRTUAL: u16 = 0x06;

pub const ALL_KEYS: &[u16] = &[
    // udev's ID_INPUT_KEYBOARD heuristic requires the complete keycode 1..=31
    // block. Without Escape, Backspace and Tab, libinput ignores this as a keyboard.
    keys::KEY_ESC,
    keys::KEY_BACKSPACE,
    keys::KEY_TAB,
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
pub struct InputId {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

// sizeof = 8(InputId) + 80(name) + 4(ff_effects_max) = 92 — matches UI_DEV_SETUP size
#[repr(C)]
pub struct UinputSetup {
    pub id: InputId,
    pub name: [u8; 80],
    pub ff_effects_max: u32,
}

pub struct Inner {
    pub fd: File,
}

impl Inner {
    pub fn write_event(&mut self, typ: u16, code: u16, value: i32) -> Result<(), DaemonError> {
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

    pub fn key_event(&mut self, code: u16, down: bool) -> Result<(), DaemonError> {
        self.write_event(keys::EV_KEY, code, if down { 1 } else { 0 })?;
        self.write_event(keys::EV_SYN, keys::SYN_REPORT, 0)
    }
}
