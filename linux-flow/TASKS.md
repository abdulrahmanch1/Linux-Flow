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

- [ ] **TASK-24** — Implement main entry point: load config, check keys, show wizard or tray, start hotkey listener, run pipeline on events, handle tray actions. `--debug` flag enables journald logging.

---

## Phase 12 — Distribution

- [ ] **TASK-25** — Write `install.sh` per `DESIGN.md` Distribution section.

- [ ] **TASK-26** — Write `uninstall.sh` per `DESIGN.md` Distribution section.

---

## Phase 13 — Security & Performance Fixes

- [ ] **TASK-27** — Fix pkcheck PID reuse vulnerability in `app/src/wizard_polkit.rs`. Replace `--process <pid>` with `--process <pid>,<start-time>,<uid>` where `start-time` is read from field 22 of `/proc/<pid>/stat`. Prevents CVE-2013-4288-class attacks where a malicious process terminates and a victim process inherits the same PID before polkit responds.

- [ ] **TASK-28** — Use absolute paths for clipboard commands in `daemon/src/clipboard.rs`. Replace `Command::new("wl-copy")` with `Command::new("/usr/bin/wl-copy")` and `Command::new("xclip")` with `Command::new("/usr/bin/xclip")`. Using PATH-relative commands in any process that spawns external binaries is a documented security anti-pattern regardless of privilege level.

- [ ] **TASK-29** — Cache API keys in `Pipeline` struct in `app/src/pipeline.rs`. Load `SONIOX_API_KEY` and `OPENROUTER_API_KEY` once in `Pipeline::new()` and store them as `Zeroizing<String>` fields. Remove the per-loop `secrets::resolve_key` calls. Re-fetch only on an explicit auth error response (retry-on-401). Apply `Zeroize` on `Pipeline` drop. This eliminates a libsecret round-trip from the hot path on every hotkey press.

- [ ] **TASK-30** — Fix blocking `thread::sleep` calls inside tokio async context in `daemon/src/uinput_inner.rs`. `inject_latin_char` (line 124) and `inject_via_clipboard` (lines 134, 139) call `thread::sleep` which blocks the tokio executor thread when invoked from `handle_connection` inside `tokio::spawn`. Fix: wrap the `injector.inject_text()` call in `handle_connection` (`daemon/src/server.rs`) with `tokio::task::spawn_blocking` so the sleeps happen on a blocking thread pool thread and do not stall the async runtime.

- [ ] **TASK-31** — Reduce clipboard exposure window in `daemon/src/uinput_inner.rs::inject_via_clipboard`. The current implementation writes the injected text to the system clipboard for ~150ms before clearing it. On X11, any application monitoring the clipboard can read the text during this window. Fix: reduce the pre-paste sleep to the minimum reliable value (measure with target applications), call `clipboard_clear` immediately after the Ctrl+V key-up event (not after a fixed 100ms sleep), and document the residual X11 clipboard risk in the commit message. Do not introduce new dependencies — wl-copy/xclip clipboard path is the only approach compatible with `DESIGN.md`.
