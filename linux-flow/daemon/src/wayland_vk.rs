use std::{io::Write, os::fd::AsFd};
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as vk_mgr,
    zwp_virtual_keyboard_v1 as vk_proto,
};
use crate::{error::DaemonError, keymap};

const GAP_MS: u32 = 4;     // timestamp delta between successive key events
const XKB_V1: u32 = 1;     // WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
const KEY_PRESSED: u32 = 1;
const KEY_RELEASED: u32 = 0;

struct VkState {
    seat: Option<wl_seat::WlSeat>,
    mgr: Option<vk_mgr::ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for VkState {
    fn event(s: &mut Self, r: &wl_registry::WlRegistry, ev: wl_registry::Event,
             _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        let wl_registry::Event::Global { name, interface, version } = ev else { return };
        match interface.as_str() {
            "wl_seat" => s.seat = Some(r.bind(name, version.min(7), qh, ())),
            "zwp_virtual_keyboard_manager_v1" => s.mgr = Some(r.bind(name, 1, qh, ())),
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for VkState {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<vk_mgr::ZwpVirtualKeyboardManagerV1, ()> for VkState {
    fn event(_: &mut Self, _: &vk_mgr::ZwpVirtualKeyboardManagerV1, _: vk_mgr::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<vk_proto::ZwpVirtualKeyboardV1, ()> for VkState {
    fn event(_: &mut Self, _: &vk_proto::ZwpVirtualKeyboardV1, _: vk_proto::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

pub struct VirtualKeyboard {
    conn: Connection,
    // Retained to roundtrip after each keymap upload so the compositor processes
    // the new layout before the corresponding key events are flushed.
    queue: EventQueue<VkState>,
    state: VkState,
    keyboard: vk_proto::ZwpVirtualKeyboardV1,
}

fn keymap_fd(km: &str) -> Result<std::fs::File, DaemonError> {
    use std::os::unix::io::FromRawFd;
    let name = c"vk-keymap";
    // SAFETY: name is a valid C string literal; MFD_CLOEXEC is a documented flag.
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return Err(DaemonError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: raw is a valid fd returned by memfd_create.
    let mut f = unsafe { std::fs::File::from_raw_fd(raw) };
    f.write_all(km.as_bytes()).map_err(DaemonError::Io)?;
    Ok(f)
}

fn now_ms() -> u32 {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<u64> = OnceLock::new();
    let abs_ms = {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: ts is stack-allocated and valid; CLOCK_MONOTONIC never fails on Linux.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        (ts.tv_sec as u64 * 1_000) + (ts.tv_nsec as u64 / 1_000_000)
    };
    // Anchor to the first call so the returned value starts near 0 regardless of
    // system uptime. Without this, the cast to u32 overflows after ~49.7 days and
    // produces timestamps compositors reject as out-of-order.
    let epoch = *EPOCH.get_or_init(|| abs_ms);
    (abs_ms.saturating_sub(epoch)) as u32
}

impl VirtualKeyboard {
    /// Connects to `$WAYLAND_DISPLAY` and creates a virtual keyboard via
    /// `zwp_virtual_keyboard_v1` (supported on GNOME 47+, KDE, wlroots, COSMIC).
    /// Returns `None` if Wayland is unavailable or the protocol is not exposed.
    pub fn new() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let mut queue: EventQueue<VkState> = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut state = VkState { seat: None, mgr: None };
        queue.roundtrip(&mut state).ok()?;

        let seat = state.seat.take()?;
        let mgr = state.mgr.take()?;
        let keyboard = mgr.create_virtual_keyboard(&seat, &qh, ());

        // The keymap is uploaded per text run (it carries that run's characters),
        // so no initial keymap is set here.
        Some(Self { conn, queue, state, keyboard })
    }

    /// Uploads `km` as the active XKB keymap and blocks until the compositor has
    /// processed it, so subsequent key events resolve against the new layout.
    fn set_keymap(&mut self, km: &str) -> Result<(), DaemonError> {
        let kfd = keymap_fd(km)?;
        self.keyboard.keymap(XKB_V1, kfd.as_fd(), km.len() as u32);
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland roundtrip: {e}")))?;
        drop(kfd); // compositor has dup'd the fd via SCM_RIGHTS
        Ok(())
    }

    /// Types `text` into the focused window by generating, per run, a dynamic
    /// XKB keymap whose keys are the run's exact characters, then emitting a
    /// press/release pair per character. Layout-independent and Unicode-complete
    /// (Arabic, emoji, CJK) — never touches the clipboard or modifiers.
    pub fn type_text(&mut self, text: &str) -> Result<(), DaemonError> {
        let mut t = now_ms();
        for seg in keymap::segments(text) {
            // First-seen order assigns each distinct char an evdev keycode `1..`.
            let mut codes: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
            let mut uniq: Vec<char> = Vec::new();
            for &c in &seg {
                codes.entry(c).or_insert_with(|| {
                    uniq.push(c);
                    uniq.len() as u32
                });
            }
            self.set_keymap(&keymap::build_keymap(&uniq))?;
            for &c in &seg {
                if let Some(&code) = codes.get(&c) {
                    self.keyboard.key(t, code, KEY_PRESSED);
                    t = t.wrapping_add(GAP_MS);
                    self.keyboard.key(t, code, KEY_RELEASED);
                    t = t.wrapping_add(GAP_MS);
                }
            }
        }
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland flush: {e}")))?;
        Ok(())
    }
}
