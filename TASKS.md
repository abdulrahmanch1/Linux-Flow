# linux-flow — Tasks

> Read `DESIGN.md` before starting any task.

---

## Phase 1 — Scaffold

- [x] **TASK-01** — Create Cargo workspace with two binary crates: the main user-space process and the privileged helper.

- [x] **TASK-02** — Add all required dependencies to each crate based on the tech stack in `DESIGN.md`.

---

## Phase 2 — Privileged Helper

- [x] **TASK-03** — Implement the Unix Domain Socket server in the helper. Framed protocol: 4-byte big-endian length prefix + UTF-8 payload. Respond `0x00` on success, `0x01` on failure. Delete stale socket on startup. Remove socket and exit cleanly on SIGTERM.

- [x] **TASK-04** — Implement keystroke injection via `/dev/uinput`. Inject any received UTF-8 string as keyboard events into the focused window.

- [x] **TASK-05** — Create the four static system files: polkit rule, udev rule, systemd user unit, XDG desktop entry. Paths per `DESIGN.md` Distribution section.

---

## Phase 3 — Config & Secrets

- [x] **TASK-06** — Implement config load/save for `~/.config/linux-flow/config.toml`. Schema and defaults per `DESIGN.md` Storage section.

- [x] **TASK-07** — Implement API key resolution: env var first, then libsecret. Write always goes to libsecret. Per `DESIGN.md` Storage section.

---

## Phase 4 — Audio

- [x] **TASK-08** — Implement PTT audio capture via PipeWire. Mic opens on start, closes fully on stop. Returns raw PCM bytes. Format: PCM S16LE, 16 kHz, mono. Nothing written to disk.

---

## Phase 5 — Hotkey

- [x] **TASK-09** — Detect display server at runtime (Wayland vs X11).

- [x] **TASK-10** — Implement Wayland hotkey registration via XDG GlobalShortcuts portal.

- [x] **TASK-11** — Implement X11 hotkey via XGrabKey in a dedicated thread.

- [x] **TASK-12** — Implement hotkey manager that dispatches to the correct backend and emits press/release events.

---

## Phase 6 — Cloud Pipeline

- [x] **TASK-13** — Implement Soniox WebSocket ASR client. Stream PCM audio, return final transcript. Connection timeout: 3s. Stream timeout: 8s after EOS.

- [x] **TASK-14** — Implement OpenRouter LLM client. Send raw transcript with the system prompt from `DESIGN.md` Pipeline section. Return cleaned text. Timeout 5s.

- [x] **TASK-15** — Implement pipeline orchestrator. On hotkey press: start audio. On release: stop audio → Soniox → OpenRouter (fall back to raw on LLM error) → send to helper via socket. Emit `PipelineEvent` variants for tray and notification wiring.

---

## Phase 7 — Tray

- [x] **TASK-16** — Implement tray icon via StatusNotifierItem. Four states: Idle, Recording, Processing, Error. Canned-loop wave animation at ~10fps for Recording state. Right-click menu: "linux-flow" label (non-clickable), separator, Settings…, About, separator, Quit.

---

## Phase 8 — Wizard

- [x] **TASK-17** — Implement wizard page 1: Soniox API key entry. Validate non-empty before proceeding.

- [x] **TASK-18** — Implement wizard page 2: OpenRouter API key and model entry. Pre-fill model with default from `DESIGN.md`. Validate both non-empty.

- [x] **TASK-19** — Implement wizard page 3: hotkey setup. On Wayland: XDG GlobalShortcuts portal. On X11: key-capture widget. Default Right Ctrl. Show success/failure before proceeding.

- [x] **TASK-20** — Implement wizard page 4: polkit authorization. Explain why polkit is needed, "Authorize" button triggers polkit dialog.

- [x] **TASK-21** — Implement wizard completion: write keys to libsecret, save `config.toml`, show tray. Auto-show wizard on startup if config or keys are missing.

---

## Phase 9 — Settings

- [x] **TASK-22** — Implement settings window (`AdwPreferencesWindow`, GTK4 + libadwaita). Rows: hotkey, OpenRouter model, Soniox language, Soniox key (change), OpenRouter key (change). Changes apply immediately without restart.

---

## Phase 10 — Notifications

- [x] **TASK-23** — Wire all `PipelineEvent` variants to tray state changes and desktop notifications (libnotify when no window is open, `AdwToast` when a window is open).

---

## Phase 11 — Entry Point

- [x] **TASK-24** — Implement main entry point: load config, check keys, show wizard or tray, start hotkey listener, run pipeline on events, handle tray actions. `--debug` flag enables journald logging.

---

## Phase 12 — Distribution

- [x] **TASK-25** — Write `install.sh` per `DESIGN.md` Distribution section.

- [x] **TASK-26** — Write `uninstall.sh` per `DESIGN.md` Distribution section.

---

## Phase 13 — Security & Performance Fixes

- [x] **TASK-27** — Fix pkcheck PID reuse vulnerability in `app/src/wizard_polkit.rs`. Replace `--process <pid>` with `--process <pid>,<start-time>,<uid>` where `start-time` is read from field 22 of `/proc/<pid>/stat`. Prevents CVE-2013-4288-class attacks where a malicious process terminates and a victim process inherits the same PID before polkit responds.

- [x] **TASK-28** — Use absolute paths for clipboard commands in `daemon/src/clipboard.rs`. Replace `Command::new("wl-copy")` with `Command::new("/usr/bin/wl-copy")` and `Command::new("xclip")` with `Command::new("/usr/bin/xclip")`. Using PATH-relative commands in any process that spawns external binaries is a documented security anti-pattern.

- [x] **TASK-29** — Cache API keys in `Pipeline` struct in `app/src/pipeline.rs`. Load `SONIOX_API_KEY` and `OPENROUTER_API_KEY` once in `Pipeline::new()` and store as `Zeroizing<String>` fields. Re-fetch only on an explicit auth error response (retry-on-401). Apply `Zeroize` on `Pipeline` drop. Eliminates a libsecret round-trip from the hot path on every hotkey press.

- [x] **TASK-30** — Fix blocking `thread::sleep` calls inside tokio async context in `daemon/src/uinput_inner.rs`. Wrap the `injector.inject_text()` call in `handle_connection` (`daemon/src/server.rs`) with `tokio::task::spawn_blocking` so the sleeps happen on a blocking thread pool thread and do not stall the async runtime.

- [x] **TASK-31** — Reduce clipboard exposure window in `daemon/src/uinput_inner.rs::inject_via_clipboard`. Reduce the pre-paste sleep to the minimum reliable value, call `clipboard_clear` immediately after the Ctrl+V key-up event rather than after a fixed 100ms sleep, and document the residual X11 clipboard risk in the commit message.

---

## Phase 14 — Bug Fixes (Audit Round 1)

> Bugs discovered during a full static audit after Phase 13 was complete.
> Each task documents the root cause, the exact fix, and a pre-analysis confirming the fix resolves the problem.
> Implement in order: TASK-32 → TASK-36.

- [x] **TASK-32** — Fix Wayland hotkey ignoring user configuration.
  - **Root cause:** `hotkey.rs:45` calls `WaylandBackend::run(tx.clone())` but never passes the `hotkey` local variable (defined on line 40). `hotkey_wayland.rs:28` hardcodes `preferred_trigger("Control_R")` regardless of what the user configured. The X11 path on line 49 already passes `hotkey` correctly; only Wayland is broken.
  - **Fix:** Add a `hotkey: String` parameter to `WaylandBackend::run()`. In `hotkey.rs:45`, pass `hotkey.clone()` as the second argument. In `hotkey_wayland.rs:28`, replace the `"Control_R"` literal with the received parameter.
  - **Files:** `app/src/hotkey_wayland.rs`, `app/src/hotkey.rs`
  - **Verified:** `preferred_trigger` is advisory — the compositor uses it as the default binding suggestion. Passing the user's configured key ensures the portal dialog opens with the correct binding. ✓

- [x] **TASK-33** — Fix first-run wizard not re-appearing when API keys are deleted from the keyring.
  - **Root cause:** `wizard.rs:27-31` — `needs_setup()` only checks whether the config file exists on disk. It does not probe libsecret. If a user deletes keys from the keyring while the config file remains, `needs_setup()` returns `false`, the wizard is skipped, and `Pipeline::new()` fails with `PipelineError::KeyLoad` causing a silent exit.
  - **Fix:** Make `needs_setup()` `async`. After the config-file check, call `secrets::resolve_key()` for both `"soniox"` and `"openrouter"`; return `true` if either fails. In `main.rs:48`, change the call to `rt.block_on(wizard::needs_setup())`.
  - **Files:** `app/src/wizard.rs`, `app/src/main.rs`
  - **Verified:** `SecretsError::NotFound` is already returned when a key is absent. `resolve_key()` checks env vars first, so users with `SONIOX_API_KEY` / `OPENROUTER_API_KEY` set are unaffected. ✓

- [x] **TASK-34** — Fix language and model settings not applying until app restart.
  - **Root cause:** `pipeline.rs:72` — `self.config` is loaded once at startup (`main.rs:83`) and never refreshed. Language or model changes saved in Settings are written to disk but never re-read by the running pipeline, violating DESIGN.md "Changes apply immediately without restart".
  - **Fix:** At the start of each recording session, right after the hotkey-press is received and before line 72, add: `if let Ok(fresh) = Config::load() { self.config = fresh; }`. This covers both `soniox.language` (line 72) and `llm.model` (line 104).
  - **Files:** `app/src/pipeline.rs`
  - **Verified:** `Config::load()` is a small synchronous TOML parse that runs before audio capture starts — outside the latency-sensitive path (hotkey release → inject). The `if let Ok` guard silently keeps the old config on any transient read failure, which is the correct fallback. ✓

- [x] **TASK-35** — Fix silent audio capture failures not surfacing to the user.
  - **Root cause:** `audio.rs:39-43` — when `capture_loop()` fails, the error goes only to `eprintln!`. The pipeline has no way to detect the failure. The dropped `pcm_tx` causes `pcm_rx` to close immediately; ASR then processes zero audio and returns an empty result; the user sees nothing injected and receives no error notification.
  - **Fix:** Add a `std::sync::mpsc::channel::<AudioError>()` inside `start()`. Return the receiver alongside `pcm_rx`: change the return type to `Result<(mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<AudioError>), AudioError>`. The thread sends on the error channel before exiting on failure. In `pipeline.rs`, after `audio.stop()` (which joins the thread), call `audio_err_rx.try_recv()`; if an error is present, send `PipelineEvent::Error` and `continue` the loop before processing ASR output.
  - **Files:** `app/src/audio.rs`, `app/src/pipeline.rs`
  - **Verified:** `std::sync::mpsc` is used (not tokio) because the capture thread is non-async. `try_recv()` after `stop()` is safe — the thread has joined and the sender has either sent or been dropped. Checking before ASR output prevents a confusing duplicate error if ASR also fails on the closed channel. ✓

- [x] **TASK-36** — Fix X11 backend silently falling back to Right Ctrl for unrecognized hotkey names.
  - **Root cause:** `hotkey_x11.rs:25` — the wildcard arm of `evdev_to_keysym()` returns `XK_Control_R` for any key name not in the match table, masking configuration errors with no log or error. The `HotkeyError::X11Keycode` error message also says "Right Control key not found in X11 keymap" which is misleading.
  - **Fix:** Change `evdev_to_keysym()` to return `Option<c_ulong>`. The wildcard arm returns `None`. In `X11Backend::run()`, handle `None` with `evdev_to_keysym(hotkey).ok_or(HotkeyError::X11Keycode)?`. Update the `X11Keycode` error message to "configured hotkey is not supported on X11".
  - **Files:** `app/src/hotkey_x11.rs`, `app/src/hotkey.rs`
  - **Verified:** `XKeysymToKeycode` already catches keysym→keycode failures (returns 0, caught at line 45). This fix adds an earlier guard at the name-lookup stage so unknown keys are rejected before they silently become the wrong key. ✓

---

## Phase 15 — Bug Fixes (Audit Round 2)

> Bugs discovered during a second full static audit after Phase 14 was complete.
> Implement in order: TASK-37 → TASK-42.

- [x] **TASK-37** — Fix silent settings save failures not surfacing to the user.
  - **Root cause:** `settings.rs:84` — `patch_config()` wraps `cfg.save()` in `let _ = ...`, silently discarding any I/O or serialization error. The confirm button then calls `win.close()` unconditionally, so the user sees the window disappear and assumes settings were saved. `wizard_complete.rs:39` uses `Config::load().unwrap_or_default()`, silently replacing a corrupted or unreadable config file with defaults, discarding the user's existing configuration.
  - **Fix:** (a) Change `patch_config()` to return `Result<(), String>` by removing `let _ =` and using `.map_err(|e| e.to_string())`. In `confirm_btn.connect_clicked`, wrap the call in `if let Err(e) = patch_config(...)` — on error, call `win.add_toast(adw::Toast::new(&format!("Failed to save settings: {e}")))` and `return` without closing the window. (b) In `wizard_complete.rs:39`, change `Config::load().unwrap_or_default()` to `Config::load().map_err(|e| e.to_string())?` so a genuine config load failure surfaces to the status label via the existing error channel.
  - **Files:** `app/src/settings.rs`, `app/src/wizard_complete.rs`
  - **Verified:** `Config::load()` already returns `Ok(Self::default())` for a missing file (line 64–66 of `shared/src/config.rs`), so the `?` in wizard_complete only fires for real errors (permissions, corrupt TOML, no HOME). The `adw::Toast` path in settings is reachable via disk-full or permission-denied scenarios. ✓

- [x] **TASK-38** — Surface injection failures from daemon back to the user.
  - **Root cause:** `daemon/src/server.rs:129-131` — when `inject_text()` returns an error, it is logged to stderr only. The daemon writes `0x01` on failure (per protocol), but `app/src/inject.rs` does not read the response byte and treats every sent frame as success.
  - **Fix:** In `inject.rs`, after writing the framed payload, read 1 byte response: `0x00` → `Ok(())`, `0x01` or any read error → `Err(...)`. In `pipeline.rs`, the existing `inject_text` error path already sends `PipelineEvent::Error`, so no change needed there.
  - **Files:** `app/src/inject.rs`, `daemon/src/server.rs`
  - **Verified:** The daemon protocol already sends `0x00`/`0x01` per TASK-03. Only the app-side reader is missing. ✓

- [x] **TASK-39** — Zeroize Bearer Token string in LLM client.
  - **Root cause:** `llm.rs:114-120` — `format!("Bearer {}", *api_key)` allocates a plain `String` that is never zeroized; it lives in the reqwest header map until the response arrives. `pipeline.rs:67` clones `self.soniox_key` into the spawned task, creating a second live copy of the secret.
  - **Fix:** Wrap the `auth` string in `Zeroizing<String>`, pass it to reqwest as `reqwest::header::HeaderValue::from_str(&*auth)?`, then explicitly `auth.zeroize()` before awaiting the response. For `soniox_key`, share via `Arc<Zeroizing<String>>` instead of `clone()` so only one allocation exists.
  - **Files:** `app/src/llm.rs`, `app/src/pipeline.rs`

- [x] **TASK-40** — Validate `/proc/<pid>/stat` fields before passing to pkcheck.
  - **Root cause:** `wizard_polkit.rs:85,93` — `read_proc_start_time()` splits the stat line on whitespace and indexes field 22 without checking the field count. A truncated or adversarially crafted `/proc` entry silently returns an empty `start_time`, which is then passed to `pkcheck` without digit-only validation.
  - **Fix:** After splitting, assert `fields.len() >= 22` and return `Err` otherwise. After extracting `start_time`, reject with `Err` if it contains any non-digit character before constructing the `pkcheck` argument.
  - **Files:** `app/src/wizard_polkit.rs`

- [x] **TASK-41** — Fix race condition in hotkey deduplicator and non-atomic tray frame counter.
  - **Root cause:** (a) `hotkey.rs:50-63` — three backend threads (`WaylandBackend`, `X11Backend`, `EvdevBackend`) all send into the same `raw_tx` `mpsc::Sender` concurrently. `mpsc::Sender` is safe to clone and share, but events from concurrent backends can interleave, causing a Pressed from one backend to pair with a Released from another. (b) `tray.rs:67-68` — `self.frame` is a plain `usize` mutated from the animation timer callback and read from the paint callback without synchronization.
  - **Fix:** (a) Add a deduplication lock: track the last-seen event per backend with an `Arc<Mutex<Option<HotkeyEvent>>>` so only the backend that sent Press can send the matching Release. (b) Change `self.frame` to `Arc<AtomicUsize>` and use `fetch_add` / `load(Ordering::Relaxed)`.
  - **Files:** `app/src/hotkey.rs`, `app/src/tray.rs`

- [x] **TASK-42** — Reduce fixed sleeps on the injection hot path.
  - **Root cause:** (a) `daemon/uinput_inner.rs:124` — a hard-coded `sleep(1ms)` between every character adds 1ms × N characters of latency to the end-to-end pipeline, violating the <200ms injection budget for typical dictation lengths. (b) `daemon/uinput_inner.rs:142` — `clipboard_clear()` is called synchronously after the Ctrl+V key-up event, blocking the daemon response for the full clear duration. (c) `daemon/uinput.rs:70` — a hard-coded `sleep(100ms)` in device setup runs on every startup.
  - **Fix:** (a) Reduce inter-character sleep to the minimum required by the kernel uinput flush cycle (empirically 500µs; measure with `evtest` if needed). (b) Spawn `clipboard_clear()` in a detached `tokio::task::spawn_blocking` call so it runs after the response is sent. (c) Lower the device-ready sleep to 20ms and remove it entirely if the device opens cleanly without it.
  - **Files:** `daemon/src/uinput_inner.rs`, `daemon/src/uinput.rs`
