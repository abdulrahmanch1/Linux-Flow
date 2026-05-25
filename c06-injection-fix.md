# C-06 Fix Design — Layout-Independent Paste Trigger

> This document is the authoritative implementation plan for C-06.
> Every decision here is cross-checked against `DESIGN.md` and the existing source.
> Items marked **⚠ DECISION** require explicit owner sign-off before coding begins.

---

## 1. Problem Statement

`inject_via_clipboard` in `daemon/src/uinput_inner.rs:131–137` triggers paste by sending
`KEY_LEFTCTRL + KEY_V` as raw evdev scancodes through `/dev/uinput`. With a non-Latin
keyboard layout active (Arabic, Persian, Hindi, Kurdish), the compositor applies the XKB
layer on top: physical `KEY_V` (scancode 47) maps to `'ر'` — not `'v'`. Applications bind
paste to `<Primary>v` (keysym `0x76`), so the injected chord is `Ctrl+ر` — unrecognised →
no paste. The daemon returns `Ok(())`, the icon returns to Idle, and the user's text is lost
with no feedback.

**Impact:** 100% failure rate for any user with a non-Latin layout active at the moment of
injection. This includes Arabic, Persian, Kurdish, Hindi — the primary multilingual users of
a dictation tool.

---

## 2. Why KEY_PASTE (0x161) Is Not Enough

`KEY_PASTE` is already defined in `daemon/src/keys.rs:66` and registered in `ALL_KEYS`.
It was implemented and then reverted in commit `11bff59` because it suffers from the same
**Issue B** that affects `Ctrl+V`:

**Issue B — COSMIC epoll batching:**
`key_event()` writes `EV_KEY + EV_SYN` with no real wall-clock timestamp. All events land in
`/dev/uinput` within one OS scheduler quantum. The compositor's libinput handler wakes once
and reads all frames as a batch. Under load COSMIC fails to recognise the event → silent
no-op. The 12 ms `thread::sleep` between events (current workaround) reduces but cannot
eliminate this race because `thread::sleep` provides no guarantee of actual elapsed wall
time from the kernel's perspective; the events still carry `timeval {0, 0}`.

**Root cause of both issues:** uinput events carry no meaningful timestamp. The compositor
has no way to distinguish a burst from a held press. The only fix is a protocol that allows
the daemon to supply explicit, accurate wall-clock timestamps.

---

## 3. Scope Constraints (from DESIGN.md — non-negotiable)

| Constraint | Source |
|-----------|--------|
| Helper process owns all injection | DESIGN.md §Stack |
| Wayland-first; X11 fallback required, must not be removed | DESIGN.md §Stack |
| Two processes only: tray app + helper | DESIGN.md §Stack |
| No disk writes, no `/tmp` | DESIGN.md §Storage / Coding Rule #6 |
| `thiserror` for error types | Coding Rule #5 |
| No `.unwrap()` / `.expect()` | Coding Rule #5 |

---

## 4. Solution Architecture

### Core Principle

Replace only the paste-trigger mechanism. The clipboard write path (`wl-copy` / `xclip`) is
correct and unchanged. Only the four lines that send `KEY_LEFTCTRL + KEY_V` via uinput are
replaced.

### Injection Flow After Fix

```
text arrives at helper via Unix socket
        │
        ▼
clipboard::clipboard_write()        ← UNCHANGED (wl-copy / xclip)
        │
        ▼
paste_trigger::fire(display)        ← NEW: replaces uinput Ctrl+V block
        ├── DisplayServer::Wayland ──► VirtualKeyboard::paste()
        │       ├── try zwp_virtual_keyboard_v1    (GNOME, KDE, wlroots)
        │       └── try zwlr_virtual_keyboard_v1   (COSMIC, Sway, Hyprland)
        ├── DisplayServer::X11 ──────► xdotool key --clearmodifiers ctrl+v
        └── DisplayServer::Unknown ──► uinput KEY_PASTE (best-effort fallback)
```

---

## 5. Wayland Path — zwp / zwlr Virtual Keyboard

### Why This Protocol Solves Both Issues

`zwp_virtual_keyboard_v1` and `zwlr_virtual_keyboard_v1` both require the client to:
1. Supply an XKB keymap — the daemon defines its own (US English, two keys only).
2. Timestamp every event with `u32` milliseconds since an arbitrary epoch.

Because the daemon owns the keymap, the keysym for the V key is always `0x76` ('v'),
completely independent of the user's active layout. **Issue A resolved.**

Because events carry explicit wall-clock timestamps, the compositor has real temporal
separation between key-down and key-up frames even if the write calls arrive in the same
epoll batch. **Issue B resolved.**

### Protocol Selection

⚠ **DECISION — which protocols to support:**

| Option | Pros | Cons |
|--------|------|------|
| **A** `zwlr_virtual_keyboard_v1` only | Simpler code; matches fix-backlog spec | GNOME/Mutter does not support it → Wayland path silently falls to X11 fallback on GNOME |
| **B** Try `zwp_virtual_keyboard_v1` first, then `zwlr` | Covers GNOME, KDE, wlroots, COSMIC | ~30 extra lines; two protocol bindings in Cargo.toml |

DESIGN.md does not specify which protocol. Recommend **Option B** — GNOME is the dominant
desktop and failing silently to a suboptimal X11 path is a regression for GNOME users.
**Owner must confirm before implementation.**

### Minimal XKB Keymap (hardcoded string)

```xkb
xkb_keymap {
    xkb_keycodes { minimum = 8; maximum = 255;
        <LCTL> = 37; <AC03> = 54; };
    xkb_types   { include "complete" };
    xkb_compat  { include "complete" };
    xkb_symbols { key <LCTL> { [ Control_L ] };
                  key <AC03> { [ v, V ] }; };
    xkb_geometry { };
};
```

Physical scancode 37 = Left Ctrl. Physical scancode 54 = V key. This is the smallest valid
XKB string that maps exactly two keys to their US English keysyms. The compositor applies
this keymap only to events originating from the virtual device — the user's active layout
is untouched.

### Event Sequence

```
t = SystemTime::now() as millis (u32)

modifier_event(MOD_CTRL, pressed,  t)
key_event     (KEY_V,    pressed,  t)
key_event     (KEY_V,    released, t + 12)
modifier_event(MOD_CTRL, released, t + 12)
wl_display::flush()
```

The 12 ms gap is preserved as a timestamp delta — it does not require a `thread::sleep`.
The write calls complete immediately; the compositor sees real temporal separation from
the timestamps alone.

### Lifecycle

- `VirtualKeyboard` is created **once** when the helper starts, stored in the injection
  state struct, and reused for every injection call. No per-request connect/disconnect.
- If `$WAYLAND_DISPLAY` is absent at startup, construction returns `None`; the paste
  trigger falls through to the X11 or uinput path.

---

## 6. X11 Fallback Path

On `DisplayServer::X11`, spawn:

```bash
xdotool key --clearmodifiers ctrl+v
```

`--clearmodifiers` releases any held modifier keys (e.g. active Arabic layout modifier)
before sending the event, then restores them after. `xdotool` uses `XSendEvent` internally
with keysyms, not scancodes — layout-independent.

**Why subprocess instead of `x11rb` / XTest FFI?**
- `xdotool` is already a system dependency used by the broader injection ecosystem.
- Adding `x11rb` + XTest bindings to the daemon adds ~200 KB of compiled code for a
  fallback path used only on X11 sessions.
- `xdotool` is available on every tested distro (Ubuntu, Fedora, Arch, openSUSE).

⚠ **DECISION — xdotool runtime dependency:**
`install.sh` must add `xdotool` to the list of required packages for X11 sessions.
Currently `install.sh` does not install `xdotool`. Confirm before implementation.

---

## 7. Fallback — uinput KEY_PASTE

When both `$WAYLAND_DISPLAY` and `$DISPLAY` are absent (headless environments, CI, pure
evdev sessions), fall back to:

```rust
self.key_event(keys::KEY_PASTE, true)?;
self.key_event(keys::KEY_PASTE, false)?;
```

`KEY_PASTE` (`0x161`) is already defined and registered in `ALL_KEYS`. This is best-effort:
it fails on COSMIC for the same epoll reason as Ctrl+V, but an environment with neither
Wayland nor X11 is outside the supported deployment matrix.

---

## 8. Affected Files

| File | Nature of Change |
|------|-----------------|
| `daemon/src/uinput_inner.rs` | Replace lines 131–137 (`KEY_LEFTCTRL + KEY_V` block) with `paste_trigger::fire(display)` call |
| `daemon/src/paste_trigger.rs` | **New file** — `PasteTrigger` enum + `fire()` selecting Wayland / X11 / uinput |
| `daemon/src/wayland_vk.rs` | **New file** — `VirtualKeyboard` struct wrapping `zwp`/`zwlr` protocol |
| `daemon/src/uinput.rs` | Initialize `PasteTrigger` on startup; pass into `UinputInjector` |
| `daemon/Cargo.toml` | Add `wayland-client`, `wayland-protocols`, `wayland-protocols-wlr` |
| `linux-flow/install.sh` | Add `xdotool` to required packages list (X11 sessions only) |

**Files that do NOT change:**
`daemon/src/clipboard.rs`, `daemon/src/keys.rs`, `shared/`, `app/`, all IPC definitions.

---

## 9. New Dependencies

| Crate | Version | License | Purpose |
|-------|---------|---------|---------|
| `wayland-client` | `0.31` | MIT | Wayland display connection |
| `wayland-protocols` | `0.31` | MIT | `zwp_virtual_keyboard_v1` bindings |
| `wayland-protocols-wlr` | `0.2` | MIT | `zwlr_virtual_keyboard_v1` bindings |

All three are MIT-licensed and compatible with the project's GPL-3.0. They are already
transitively present in the `app/` crate (GTK4 pulls in `wayland-client`); adding them
to the daemon Cargo.toml makes them explicit and version-pinned.

No runtime binaries are added to the Wayland path. `xdotool` is the only new runtime
dependency (X11 sessions only).

---

## 10. What Does NOT Change

- `clipboard::clipboard_write()` — `wl-copy` / `xclip` subprocess, unchanged.
- IPC protocol between app and helper — `InjectText` message, unchanged.
- uinput device registration and `ALL_KEYS` capabilities — unchanged.
- Polkit rules — unchanged.
- All app-side code (`app/`) — unchanged.
- `KEY_PASTE` constant — kept; used as the uinput fallback.

---

## 11. Decisions — Confirmed

| # | Question | Decision | DESIGN.md Compatibility |
|---|----------|----------|------------------------|
| D-1 | Support `zwp_virtual_keyboard_v1` (GNOME) in addition to `zwlr`? | **✅ Yes** | Compatible — DESIGN.md mandates "Wayland-first"; zwp is the official wayland-protocols binding and covers GNOME, KDE, and wlroots compositors |
| D-2 | Add `xdotool` as a required package in `install.sh` for X11? | **✅ Yes** | Compatible — DESIGN.md explicitly requires "X11 fallback"; xdotool is the runtime tool that enables it; install.sh is the documented distribution mechanism |

No item in DESIGN.md is violated by either decision. Both are additive changes that serve
existing DESIGN.md requirements ("Wayland-first, X11 fallback").

## 12. Implementation Scope

> This file is a design document only — no code is written here.
>
> Implementation will be carried out in a dedicated session and confined to the source
> files listed in §8. All new logic goes into two new files:
> - `daemon/src/wayland_vk.rs` — Wayland virtual keyboard abstraction
> - `daemon/src/paste_trigger.rs` — unified paste-trigger dispatcher
>
> Existing files receive only targeted edits (see §8). No other files are touched.

---

## 12. Testing Matrix

| Environment | Expected path | Pass condition |
|-------------|--------------|----------------|
| COSMIC (Wayland) + Arabic layout | zwlr | Text pasted correctly |
| GNOME (Wayland) + Arabic layout | zwp | Text pasted correctly |
| KDE Plasma (Wayland) + Arabic layout | zwp or zwlr | Text pasted correctly |
| Sway (Wayland) + Arabic layout | zwlr or zwp | Text pasted correctly |
| XFCE (X11) + Arabic layout | xdotool | Text pasted correctly |
| i3 (X11) + Arabic layout | xdotool | Text pasted correctly |
| English layout (any) | same paths | No regression |
| Headless / CI | uinput KEY_PASTE | No crash; log warning |
| 200-char Arabic text | any | No character loss |
| p99 end-to-end latency | any | ≤ 700 ms (Wispr Flow benchmark) |
