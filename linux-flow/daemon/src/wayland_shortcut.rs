use crate::{error::DaemonError, vk_util, wayland_clipboard::State};
use std::{fs::File, os::fd::AsFd, thread, time::Duration};
use wayland_client::{delegate_noop, protocol::wl_seat, Connection, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as manager, zwp_virtual_keyboard_v1 as keyboard,
};

const KEYMAP: &str = concat!(
    r#"xkb_keymap {
    xkb_keycodes "paste" {
        minimum = 8; maximum = 255;
        <LCTL> = 37; <LFSH> = 50; <AB04> = 55;
    };
    xkb_types "paste" { include "complete" };
    xkb_compatibility "paste" { include "complete" };
    xkb_symbols "paste" {
        key <LCTL> { [ Control_L ] };
        key <LFSH> { [ Shift_L ] };
        key <AB04> { [ v, V ] };
        modifier_map Control { <LCTL> };
        modifier_map Shift { <LFSH> };
    };
};
"#,
    "\0"
);

/// One temporary keyboard for the shortcut, never for the dictated text.
/// Its Latin V is independent of the user's active Arabic/English layout.
pub(super) struct PasteKeyboard {
    conn: Connection,
    keyboard: keyboard::ZwpVirtualKeyboardV1,
    _keymap: File,
    finished: bool,
}

impl PasteKeyboard {
    pub fn new(
        conn: &Connection,
        manager: &manager::ZwpVirtualKeyboardManagerV1,
        seat: &wl_seat::WlSeat,
        qh: &QueueHandle<State>,
    ) -> Result<Self, DaemonError> {
        let keymap = vk_util::keymap_fd(KEYMAP)?;
        let keyboard = manager.create_virtual_keyboard(seat, qh, ());
        keyboard.keymap(1, keymap.as_fd(), KEYMAP.len() as u32);
        keyboard.modifiers(0, 0, 0, 0);
        Ok(Self {
            conn: conn.clone(),
            keyboard,
            _keymap: keymap,
            finished: false,
        })
    }

    pub fn press(mut self, shift: bool) -> Result<(), DaemonError> {
        // The clipboard setup roundtrip has installed the keymap in the
        // compositor; allow the focused client's own keymap handler to settle.
        thread::sleep(Duration::from_millis(60));
        let modifiers = 4 | u32::from(shift); // XKB Control and optional Shift.
        self.keyboard.modifiers(modifiers, 0, 0, 0);
        self.keyboard.key(vk_util::now_ms(), 47, 1);
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("paste key: {e}")))?;
        thread::sleep(Duration::from_millis(20));
        self.release()
    }

    fn release(&mut self) -> Result<(), DaemonError> {
        self.keyboard.key(vk_util::now_ms(), 47, 0);
        self.keyboard.modifiers(0, 0, 0, 0);
        self.keyboard.destroy();
        self.finished = true;
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("paste key release: {e}")))
    }
}

impl Drop for PasteKeyboard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.release();
        }
    }
}

delegate_noop!(State: manager::ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: keyboard::ZwpVirtualKeyboardV1);
