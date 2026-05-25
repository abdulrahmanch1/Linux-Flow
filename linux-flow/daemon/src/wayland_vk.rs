use std::{io::Write, os::fd::AsFd, time::{SystemTime, UNIX_EPOCH}};
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as vk_mgr,
    zwp_virtual_keyboard_v1 as vk_proto,
};
use crate::error::DaemonError;

const EVDEV_V: u32 = 47;   // Linux KEY_V
const MOD_CTRL: u32 = 4;   // XKB Control modifier bitmask (index 2)
const GAP_MS: u32 = 12;    // timestamp delta between key-down and key-up
const XKB_V1: u32 = 1;     // WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
const KEY_PRESSED: u32 = 1;
const KEY_RELEASED: u32 = 0;

// Minimal two-key XKB keymap. X11 keycode = evdev + 8: LCTL=37, V(AB04)=55.
// The compositor applies this keymap only to events from this virtual device;
// the user's active layout is unaffected. Null-terminated as required by
// XKB_KEYMAP_FORMAT_TEXT_V1.
const KEYMAP: &str = concat!(
    "xkb_keymap {\n",
    "  xkb_keycodes { minimum=8; maximum=255;\n",
    "    <LCTL>=37; <AB04>=55; };\n",
    "  xkb_types   { include \"complete\" };\n",
    "  xkb_compat  { include \"complete\" };\n",
    "  xkb_symbols { key <LCTL>{[Control_L]}; key <AB04>{[v,V]}; };\n",
    "  xkb_geometry { };\n",
    "};\n\0",
);

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
    // Kept alive to maintain Wayland proxy lifetimes; not read after construction.
    _queue: EventQueue<VkState>,
    _state: VkState,
    keyboard: vk_proto::ZwpVirtualKeyboardV1,
}

fn keymap_fd() -> Result<std::fs::File, DaemonError> {
    use std::os::unix::io::FromRawFd;
    let name = c"vk-keymap";
    // SAFETY: name is a valid C string literal; MFD_CLOEXEC is a documented flag.
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return Err(DaemonError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: raw is a valid fd returned by memfd_create.
    let mut f = unsafe { std::fs::File::from_raw_fd(raw) };
    f.write_all(KEYMAP.as_bytes()).map_err(DaemonError::Io)?;
    Ok(f)
}

fn now_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
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
        let kfd = keymap_fd().ok()?;

        let keyboard = mgr.create_virtual_keyboard(&seat, &qh, ());
        keyboard.keymap(XKB_V1, kfd.as_fd(), KEYMAP.len() as u32);

        // Roundtrip ensures compositor processes keymap before first paste.
        queue.roundtrip(&mut state).ok()?;

        drop(kfd); // compositor has dup'd the fd via SCM_RIGHTS
        Some(Self { conn, _queue: queue, _state: state, keyboard })
    }

    /// Injects a layout-independent Ctrl+V with explicit wall-clock timestamps.
    /// The compositor uses our own XKB keymap so the injected keysym is always
    /// `v` (0x76), regardless of the user's active keyboard layout.
    pub fn paste(&mut self) -> Result<(), DaemonError> {
        let t = now_ms();
        self.keyboard.modifiers(MOD_CTRL, 0, 0, 0);
        self.keyboard.key(t, EVDEV_V, KEY_PRESSED);
        self.keyboard.key(t + GAP_MS, EVDEV_V, KEY_RELEASED);
        self.keyboard.modifiers(0, 0, 0, 0);
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland flush: {e}")))?;
        Ok(())
    }
}
