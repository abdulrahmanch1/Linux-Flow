//! Standalone helpers for the Wayland virtual keyboard: timing detection,
//! keymap memfd creation, and a monotonic millisecond clock.

use crate::error::DaemonError;
use std::io::Write;

/// Timing parameters (ms): `(press_gap, char_gap, settle_ms)`. All three are
/// real wall-clock sleeps applied by the typing loop, not mere event-timestamp
/// deltas: each key event is flushed to the compositor and then the matching gap
/// is slept, so delivery is genuinely spaced instead of handed over in one burst.
///
/// `press_gap` separates a key's press from its own release; too small and the
/// compositor coalesces the pair and drops the character. `char_gap` separates
/// one key's release from the next key's press. Terminals (cosmic-term, wezterm)
/// silently drop keystrokes that arrive in a single batch — without a real pause
/// they never process each keystroke individually — so `char_gap` is what makes
/// terminals reliable; tune it *down* empirically per machine to reclaim typing
/// speed without losing characters.
/// `settle_ms` is a real pause after uploading a keymap, before any key is sent:
/// the focused application is a *separate* Wayland client that receives the new
/// keymap asynchronously from the compositor, and our own roundtrip cannot
/// observe when *it* has applied it. Without it some clients resolve the first
/// keys against the stale layout, turning the run's first characters into a wrong
/// glyph (e.g. "sudo" → "§").
///
/// The defaults are deliberately conservative and uniform across desktops. The
/// underlying protocol (`zwp_virtual_keyboard_v1`) is fire-and-forget: there is
/// no per-event acknowledgement, so any gap is a blind bet on how fast the
/// focused client drains its event queue. Per-desktop magic numbers only ever
/// guessed at that — they do not vary with the real bottleneck (client load), so
/// the smallest values dropped characters under load on every desktop. A single
/// safe baseline, paired with the periodic roundtrip in the typing loop (which
/// genuinely adapts to compositor load), is more robust than per-name tuning.
/// All three are overridable via `LINUX_FLOW_VK_PRESS_GAP_MS`,
/// `LINUX_FLOW_VK_CHAR_GAP_MS`, and `LINUX_FLOW_VK_SETTLE_MS` for empirical
/// tuning without a rebuild — drop `char_gap` on a fast machine to reclaim speed.
pub(crate) fn detect_gap() -> (u32, u32, u32) {
    // Conservative baseline shared by all desktops. `char_gap` is the critical
    // value — it widens the margin against the typing-rate vs. client-drain race
    // that drops characters under load.
    let (mut press_gap, mut char_gap, mut settle_ms) = (8, 12, 60);
    if let Some(v) = env_gap("LINUX_FLOW_VK_PRESS_GAP_MS") {
        press_gap = v;
    }
    if let Some(v) = env_gap("LINUX_FLOW_VK_CHAR_GAP_MS") {
        char_gap = v;
    }
    if let Some(v) = env_gap("LINUX_FLOW_VK_SETTLE_MS") {
        settle_ms = v;
    }
    (press_gap, char_gap, settle_ms)
}

fn env_gap(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// Creates an anonymous in-memory file holding `km`, for passing the keymap to
/// the compositor via `SCM_RIGHTS`.
pub(crate) fn keymap_fd(km: &str) -> Result<std::fs::File, DaemonError> {
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

/// Monotonic milliseconds since the first call, used for key event timestamps.
pub(crate) fn now_ms() -> u32 {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<u64> = OnceLock::new();
    let abs_ms = {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
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
