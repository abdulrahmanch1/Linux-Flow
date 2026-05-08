# linux-flow — Tasks

> Read `@PRD.md` before starting.

---

## Phase 1 — Scaffold

- [x] **TASK-01** — Create Cargo workspace with two binary crates: the main user-space process and the privileged helper.

- [x] **TASK-02** — Add all required dependencies to each crate based on the tech stack in PRD Section 2.

---

## Phase 2 — Privileged Helper

- [x] **TASK-03** — Implement the Unix Domain Socket server in the helper. Framed protocol: 4-byte big-endian length prefix + UTF-8 payload. Respond `0x00` on success, `0x01` on failure. Delete stale socket on startup. Remove socket and exit cleanly on SIGTERM.

- [x] **TASK-04** — Implement keystroke injection via `/dev/uinput`. Inject any received UTF-8 string as keyboard events into the focused window.

- [x] **TASK-05** — Create the four static system files: polkit rule, udev rule, systemd user unit, XDG desktop entry. Paths and content per PRD Section 13.

---

## Phase 3 — Config & Secrets

- [x] **TASK-06** — Implement config load/save for `~/.config/linux-flow/config.toml`. Schema and defaults per PRD Section 7.

- [x] **TASK-07** — Implement API key resolution: env var first, then libsecret. Write always goes to libsecret. Per PRD Section 7.

---

## Phase 4 — Audio

- [x] **TASK-08** — Implement PTT audio capture via PipeWire. Mic opens on start, closes fully on stop. Returns raw PCM bytes. Format per PRD Section 6. Nothing written to disk.

---

## Phase 5 — Hotkey

- [x] **TASK-09** — Detect display server at runtime (Wayland vs X11).

- [x] **TASK-10** — Implement Wayland hotkey registration via XDG GlobalShortcuts portal.

- [x] **TASK-11** — Implement X11 hotkey via XGrabKey in a dedicated thread.

- [x] **TASK-12** — Implement hotkey manager that dispatches to the correct backend and emits press/release events.

---

## Phase 6 — Cloud Pipeline

- [x] **TASK-13** — Implement Soniox WebSocket ASR client. Stream PCM audio, return final transcript. Connection timeout: 3s. Stream timeout: 8s. Per PRD Sections 5 and 10.

- [ ] **TASK-14** — Implement OpenRouter LLM client. Send raw transcript with system prompt from PRD Section 8. Return cleaned text. Timeout 5s.

- [ ] **TASK-15** — Implement pipeline orchestrator. On hotkey press: start audio. On release: stop audio → Soniox → OpenRouter (fall back to raw on LLM error) → send to helper via socket. Emit pipeline events per PRD Section 10.

---

## Phase 7 — Tray

- [ ] **TASK-16** — Implement tray icon via StatusNotifierItem. Four states: Idle, Recording, Processing, Error. Wave animation at ~10fps for Recording. Right-click menu with Settings, About, Quit. Per PRD Section 9.1.

---

## Phase 8 — Wizard

- [ ] **TASK-17** — Implement wizard page 1: Soniox API key entry.

- [ ] **TASK-18** — Implement wizard page 2: OpenRouter API key and model entry.

- [ ] **TASK-19** — Implement wizard page 3: hotkey setup via platform portal or key-capture widget.

- [ ] **TASK-20** — Implement wizard page 4: polkit authorization.

- [ ] **TASK-21** — Implement wizard completion: persist keys and config, show tray. Auto-show wizard on startup if config or keys are missing. Per PRD Section 9.2.

---

## Phase 9 — Settings

- [ ] **TASK-22** — Implement settings window with all groups and rows per PRD Section 9.3. Changes apply immediately without restart.

---

## Phase 10 — Notifications

- [ ] **TASK-23** — Wire all pipeline events to tray state changes and toast notifications using exact messages from PRD Section 10.

---

## Phase 11 — Entry Point

- [ ] **TASK-24** — Implement main entry point: load config, check keys, show wizard or tray, start hotkey listener, run pipeline on events, handle tray actions. `--debug` flag enables journald logging.

---

## Phase 12 — Distribution

- [ ] **TASK-25** — Write `install.sh` per PRD Section 13.

- [ ] **TASK-26** — Write `uninstall.sh` per PRD Section 13.
