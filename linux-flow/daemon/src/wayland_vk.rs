use crate::vk_util::{detect_gap, keymap_fd, now_ms};
use crate::{error::DaemonError, keymap};
use std::os::fd::AsFd;
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as vk_mgr, zwp_virtual_keyboard_v1 as vk_proto,
};

const XKB_V1: u32 = 1; // WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
const KEY_PRESSED: u32 = 1;
const KEY_RELEASED: u32 = 0;

/// Roundtrip with the compositor every this many characters. A roundtrip blocks
/// until the compositor has processed everything sent so far, so it both bounds
/// how large an event backlog can pile up and — because a busy compositor takes
/// longer to answer — paces delivery in proportion to real load, unlike a fixed
/// sleep. The only feedback `zwp_virtual_keyboard_v1` can offer is this
/// compositor-side acknowledgement; the focused client's own queue stays opaque.
const ROUNDTRIP_EVERY: usize = 16;

struct VkState {
    seat: Option<wl_seat::WlSeat>,
    mgr: Option<vk_mgr::ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for VkState {
    fn event(
        s: &mut Self,
        r: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        else {
            return;
        };
        match interface.as_str() {
            "wl_seat" => s.seat = Some(r.bind(name, version.min(7), qh, ())),
            "zwp_virtual_keyboard_manager_v1" => s.mgr = Some(r.bind(name, 1, qh, ())),
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for VkState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<vk_mgr::ZwpVirtualKeyboardManagerV1, ()> for VkState {
    fn event(
        _: &mut Self,
        _: &vk_mgr::ZwpVirtualKeyboardManagerV1,
        _: vk_mgr::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<vk_proto::ZwpVirtualKeyboardV1, ()> for VkState {
    fn event(
        _: &mut Self,
        _: &vk_proto::ZwpVirtualKeyboardV1,
        _: vk_proto::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

pub struct VirtualKeyboard {
    conn: Connection,
    // Retained to roundtrip after each keymap upload so the compositor processes
    // the new layout before the corresponding key events are flushed.
    queue: EventQueue<VkState>,
    state: VkState,
    keyboard: vk_proto::ZwpVirtualKeyboardV1,
    press_gap: u32,
    char_gap: u32,
    settle_ms: u32,
    // True while the fixed base keymap is the device's active layout. A dynamic
    // run displaces it; the next base run restores it lazily, off-cadence.
    base_active: bool,
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
        let mut state = VkState {
            seat: None,
            mgr: None,
        };
        queue.roundtrip(&mut state).ok()?;

        let seat = state.seat.take()?;
        let mgr = state.mgr.take()?;
        let keyboard = mgr.create_virtual_keyboard(&seat, &qh, ());

        let (press_gap, char_gap, settle_ms) = detect_gap();
        let mut vk = Self {
            conn,
            queue,
            state,
            keyboard,
            press_gap,
            char_gap,
            settle_ms,
            base_active: false,
        };
        // Upload the fixed ASCII base keymap once, here at daemon start — off the
        // hot path, so its settle sleep costs nothing at hotkey time. ASCII runs
        // then type against this stable layout with no per-run upload, so the
        // focused client never resolves the first key against a stale layout.
        // Best-effort: on failure, type_text falls back to the dynamic path.
        vk.base_active = vk.set_keymap(&keymap::build_base_keymap()).is_ok();
        Some(vk)
    }

    /// Uploads `km` as the active XKB keymap and blocks until the compositor has
    /// processed it, so subsequent key events resolve against the new layout.
    fn set_keymap(&mut self, km: &str) -> Result<(), DaemonError> {
        let kfd = keymap_fd(km)?;
        self.keyboard.keymap(XKB_V1, kfd.as_fd(), km.len() as u32);
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland roundtrip: {e}")))?;
        // Initialize modifier state on the freshly-keymapped device. Without this,
        // many compositors (COSMIC, Mutter) discard the very first key event of a
        // new virtual keyboard, dropping the first typed character.
        self.keyboard.modifiers(0, 0, 0, 0);
        drop(kfd); // compositor has dup'd the fd via SCM_RIGHTS
        Ok(())
    }

    /// Types `text` into the focused window, one press/release pair per char.
    /// All-ASCII runs use the pre-uploaded base keymap with no per-run upload;
    /// runs with other characters fall back to a dynamic keymap. Layout-
    /// independent and Unicode-complete (Arabic, emoji, CJK) — never touches the
    /// clipboard. Modifiers are only reset to a known-empty state, never held.
    pub fn type_text(&mut self, text: &str) -> Result<(), DaemonError> {
        let mut t = now_ms();
        for seg in keymap::segments(text) {
            match base_codes(&seg) {
                Some(codes) => self.type_base(&codes, &mut t)?,
                None => self.type_dynamic(&seg, &mut t)?,
            }
        }
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland flush: {e}")))?;
        Ok(())
    }

    /// Fast path for an all-ASCII run: types against the pre-uploaded base
    /// keymap with no per-run keymap upload. Each character is still paced on the
    /// wall clock by [`Self::emit_key`] so terminals receive it individually.
    fn type_base(&mut self, codes: &[u32], t: &mut u32) -> Result<(), DaemonError> {
        if !self.base_active {
            // A prior dynamic run displaced the base layout; restore it once.
            self.set_keymap(&keymap::build_base_keymap())?;
            self.base_active = true;
        }
        self.warmup_and_settle(t)?;
        for (i, &code) in codes.iter().enumerate() {
            self.emit_key(code, t)?;
            if (i + 1) % ROUNDTRIP_EVERY == 0 {
                self.sync()?;
            }
        }
        Ok(())
    }

    /// Fallback for runs with non-ASCII characters: builds a per-run keymap and
    /// roundtrips so the compositor processes it before the real keys arrive.
    fn type_dynamic(&mut self, seg: &[char], t: &mut u32) -> Result<(), DaemonError> {
        // First-seen order assigns each distinct char an evdev keycode `1..`.
        let mut codes: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
        let mut uniq: Vec<char> = Vec::new();
        for &c in seg {
            codes.entry(c).or_insert_with(|| {
                uniq.push(c);
                uniq.len() as u32
            });
        }
        self.set_keymap(&keymap::build_keymap(&uniq))?;
        self.base_active = false; // base layout displaced by this run
        self.warmup_and_settle(t)?;
        for (i, &c) in seg.iter().enumerate() {
            if let Some(&code) = codes.get(&c) {
                self.emit_key(code, t)?;
            }
            if (i + 1) % ROUNDTRIP_EVERY == 0 {
                self.sync()?;
            }
        }
        Ok(())
    }

    /// Emits one press/release pair, flushing after each event and pausing on the
    /// wall clock so the focused client receives the character before the next
    /// arrives. Terminals (cosmic-term, wezterm) silently drop keystrokes that
    /// land in a single batch; the real sleeps pace delivery so each keystroke is
    /// processed individually. The advancing `t` keeps the event timestamps
    /// strictly ordered, which the press/release gaps also keep distinct so the
    /// compositor never coalesces a pair.
    fn emit_key(&mut self, code: u32, t: &mut u32) -> Result<(), DaemonError> {
        self.keyboard.key(*t, code, KEY_PRESSED);
        *t = t.wrapping_add(self.press_gap);
        self.flush_and_pace(self.press_gap)?;
        self.keyboard.key(*t, code, KEY_RELEASED);
        *t = t.wrapping_add(self.char_gap);
        self.flush_and_pace(self.char_gap)?;
        Ok(())
    }

    /// Flushes pending key events to the compositor, then sleeps `gap_ms` of real
    /// wall-clock time. Unlike advancing the event timestamp alone, this actually
    /// spaces delivery — the fix that keeps terminals from dropping characters.
    fn flush_and_pace(&self, gap_ms: u32) -> Result<(), DaemonError> {
        self.conn
            .flush()
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland flush: {e}")))?;
        if gap_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(gap_ms as u64));
        }
        Ok(())
    }

    /// Blocks until the compositor has processed every request sent so far. Used
    /// periodically inside the typing loop so the event backlog stays bounded and
    /// delivery paces itself against real compositor load instead of a fixed
    /// sleep alone — the only acknowledgement this protocol exposes.
    fn sync(&mut self) -> Result<(), DaemonError> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland roundtrip: {e}")))?;
        Ok(())
    }

    /// Emits one sacrificial warm-up key (VoidSymbol), flushes it via roundtrip,
    /// then sleeps `settle_ms`. The warm-up is the first event that makes the
    /// virtual keyboard the seat's active source, prompting the compositor to
    /// broadcast our keymap to the focused client; the client then applies it
    /// *asynchronously*. The roundtrip pushes the warm-up out, and the sleep
    /// gives the client time to finish applying the keymap before the first real
    /// character arrives — without it, that character resolves against the stale
    /// layout (e.g. "sudo" → "§"). The warm-up also absorbs the compositor's
    /// habit of dropping the first key event after a focus or keymap change.
    fn warmup_and_settle(&mut self, t: &mut u32) -> Result<(), DaemonError> {
        self.keyboard.key(*t, keymap::WARMUP_EVDEV, KEY_PRESSED);
        *t = t.wrapping_add(self.press_gap);
        self.keyboard.key(*t, keymap::WARMUP_EVDEV, KEY_RELEASED);
        *t = t.wrapping_add(self.press_gap);
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| DaemonError::InjectionFailed(format!("wayland roundtrip: {e}")))?;
        if self.settle_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.settle_ms as u64));
        }
        Ok(())
    }
}

/// Each segment char's fixed base-keymap evdev code, or `None` if any char is
/// outside the base set (→ caller takes the dynamic path).
fn base_codes(seg: &[char]) -> Option<Vec<u32>> {
    seg.iter().map(|&c| keymap::base_evdev(c)).collect()
}
