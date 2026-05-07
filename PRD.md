# Product Requirements Document: linux-flow
**Version:** 1.2  
**License:** GPL-3.0

---

## 1. Project Overview

**linux-flow** is a Wayland-first, push-to-talk dictation tool for Linux. It captures audio while a hotkey is held, streams it to a cloud ASR service (Soniox), passes the transcript through an LLM rewrite layer (OpenRouter) to produce fluent human-sounding text, and injects the result directly into the focused window.

**Target Users:** Linux power users (developers, writers, content creators) seeking fast, low-friction voice-to-text on Wayland or X11 desktops.

---

## 2. Technical Stack

| Component | Technology |
|---|---|
| Language | Rust (stable, edition 2021), GPL-3.0 |
| Display protocol | Wayland-first; X11 fallback |
| UI framework | GTK4 + libadwaita |
| Audio capture | libpipewire |
| ASR | Soniox WebSocket API |
| LLM editor | OpenRouter REST API |
| Default LLM model | `google/gemini-2.0-flash-lite` (overridable via `LLM_MODEL` env var) |
| OpenRouter routing | `provider.sort = "throughput"` |
| IPC | Unix Domain Socket |
| Privilege escalation | polkit |
| Input injection | `/dev/uinput` via Privileged Helper |
| Secret storage | libsecret (Secret Service API) |
| Configuration | TOML at `~/.config/linux-flow/config.toml` |
| Logging | journald, only when `--debug` flag is active |

---

## 3. System Requirements

**Kernel:** Linux >= 5.15

**Wayland compositors:** GNOME (Mutter >= 43), KDE Plasma (KWin >= 5.27), Sway >= 1.8

**X11:** Any compositor supporting `XGrabKey`

**Required runtime libraries:** `libpipewire-0.3`, `libgtk-4`, `libadwaita-1`, `libsecret-1`, `libdbus-1`, polkit daemon, PipeWire >= 0.3.48, systemd

**Network:** Outbound HTTPS/WSS to `soniox.com:443` and `openrouter.ai:443` only. No inbound ports.

---

## 4. Architecture

The application runs as two separate OS processes to enforce least-privilege boundaries for input injection.

```
+------------------------------------------+
|           User-Space Process             |
|  UI, audio capture, cloud pipeline,      |
|  config, secrets, state management       |
+--------------------+---------------------+
                     |
              Unix Domain Socket
         /run/user/{UID}/linux-flow-helper.sock
                     |
+--------------------v---------------------+
|          Privileged Helper               |
|  Receives text -> emits keystrokes       |
|  via /dev/uinput into focused window     |
+------------------------------------------+
```

### User-Space Process
Owns all UI, audio capture, network I/O, configuration, and secret management. Delivers cleaned text to the Helper over the Unix socket for injection.

### Privileged Helper
Performs keystroke injection only. No network access, no config access, no knowledge of API keys. Authorized once at install time via polkit. Runs as a systemd user service.

---

## 5. Execution Pipeline

1. User holds hotkey -> microphone opens via libpipewire.
2. PCM audio streams in real-time to Soniox WebSocket (multilingual, auto-detect, code-switching enabled).
3. User releases hotkey -> end-of-stream signal sent; Soniox returns the final transcript.
4. Raw transcript sent to OpenRouter for LLM editing (see Section 8).
5. Cleaned text sent over Unix socket to the Privileged Helper.
6. Helper injects text as keystrokes via `/dev/uinput` into the focused window.
7. All audio and transcript data is dropped from RAM immediately after injection.

> **Injection strategy:** The Helper uses two strategies based on character type:
> - **ASCII / Latin text:** direct `/dev/uinput` key events.
> - **Non-Latin text (Arabic, CJK, etc.):** write to clipboard via `wl-clipboard` (Wayland) or `xclip` (X11), emit `Ctrl+V`, then immediately clear the clipboard. The clipboard is used only as an internal transport mechanism and is never left populated.

---

## 6. Audio Specification

| Parameter | Value |
|---|---|
| Format | PCM signed 16-bit little-endian (S16LE) |
| Sample rate | 16,000 Hz |
| Channels | 1 (mono) |
| Buffer | RAM only - never written to disk |

The microphone is fully closed (not muted) between PTT presses. Audio is streamed to Soniox as captured and never held in a full-session buffer.

---

## 7. Configuration Schema

Location: `~/.config/linux-flow/config.toml`

```toml
[general]
hotkey = "KEY_RIGHTCTRL"   # evdev key name; default: Right Ctrl

[llm]
model = "google/gemini-2.0-flash-lite"  # overridable via LLM_MODEL env var

[soniox]
language = "auto"   # "auto" = multilingual auto-detect; or ISO code e.g. "en", "fr"

[ui]
show_tray = true
```

API keys are never stored in `config.toml`. They are read in the following priority order:

1. Environment variable (`OPENROUTER_API_KEY` / `SONIOX_API_KEY`) — takes precedence if set.
2. libsecret under schema `ai.linux-flow` — used otherwise.

If neither source provides a key, the app must refuse to start with a clear error. The first-run wizard stores keys into libsecret.

---

## 8. LLM Editing Behavior

The LLM must apply the following transformations to the raw transcript. These are product requirements.

1. Remove filler words: "um", "uh", "like", "you know", "so", "basically", "literally", "actually" when used as fillers rather than meaningful words.
2. Remove false starts and self-corrections. Example: "I went to - actually I drove to the store" -> "I drove to the store".
3. Remove word repetitions from speech disfluency. Example: "I I went to the the store" -> "I went to the store".
4. Fix grammar and sentence flow without adding new content or changing meaning.
5. Apply correct punctuation and capitalization.
6. Translate the spoken phrase "new line" into a literal newline character.
7. Translate the spoken phrase "new paragraph" into two newline characters.
8. **Preserve the original input language. Do not translate under any circumstances.**
9. **Do not add, invent, or infer content that was not spoken.**
10. Output only the cleaned text with no preamble, explanation, or surrounding quotes.

---

## 9. UI & UX

### 9.1 Tray Icon (StatusNotifierItem)

Four required states:

| State | Visual | Trigger |
|---|---|---|
| Idle | Static microphone icon | App running, not recording |
| Recording | Looping wave animation (~10fps) | Hotkey held |
| Processing | Pulsing indicator | Hotkey released, awaiting result |
| Error | Warning icon | Any pipeline failure |

The Error state persists until the next successful recording cycle.

**Tray right-click menu:**
1. "linux-flow" (non-clickable label)
2. Separator
3. Settings...
4. About
5. Separator
6. Quit

### 9.2 First-Run Wizard

Shown automatically when no valid config and secrets exist. Uses `AdwNavigationView` with four sequential pages:

**Page 1 - Soniox API Key**
Explanation of Soniox with a link to obtain a key. Password entry field. Validated as non-empty before proceeding.

**Page 2 - OpenRouter API Key & Model**
Password entry for OpenRouter key. Text entry for model ID, pre-filled with the default. Both validated as non-empty.

**Page 3 - Hotkey**
On Wayland: triggers XDG GlobalShortcuts portal. On X11: key-capture widget. Default is Right Ctrl. Shows success or failure feedback before proceeding.

**Page 4 - Privilege Grant**
Explains why polkit is needed. "Authorize" button triggers the polkit dialog. On completion, secrets are written to libsecret and `config.toml` is created.

### 9.3 Settings Window

An `AdwPreferencesWindow` accessible from the tray menu. Changes take effect immediately without restarting.

| Group | Row | Control |
|---|---|---|
| Hotkey | Push-to-talk hotkey | Current key label + "Change..." button |
| AI Model | OpenRouter model | Inline editable text entry |
| Speech Recognition | Language | Dropdown: Auto / EN / FR / ES / DE / HI / Other |
| API Keys | Soniox key | "Change..." -> password dialog |
| API Keys | OpenRouter key | "Change..." -> password dialog |

---

## 10. Error Handling

All errors surface as a tray icon state change to Error plus a notification. The app must never crash silently.

**Notification delivery:** `AdwToast` is shown if the Settings window is currently open. When no window is open, errors are delivered via `libnotify` desktop notification as fallback. Both channels are triggered simultaneously when a window is open.

| Failure | User-visible message |
|---|---|
| libsecret unavailable at startup | Fatal dialog: "Secret Service unavailable. Cannot start." App exits. |
| Missing API key | "API key not configured. Open Settings to add it." |
| Soniox connection failure (connection timeout: 3s; stream timeout: 8s) | "Could not connect to Soniox. Check your connection and API key." |
| Soniox auth error (401) | "Soniox API key is invalid. Open Settings to update it." |
| Soniox stream error mid-recording | "Speech recognition failed. Please try again." |
| OpenRouter connection failure (timeout: 5s) | "Could not reach OpenRouter. Check your connection." |
| OpenRouter auth error (401) | "OpenRouter API key is invalid. Open Settings to update it." |
| OpenRouter empty/malformed response | Inject raw Soniox transcript without LLM cleaning. Warning toast: "AI editing unavailable; injecting raw transcript." |
| Helper socket not found | "Input helper is not running. Try restarting linux-flow." |
| Helper injection failure | "Could not inject text into the focused window." |

No retries in v1. Failures are reported immediately.

---

## 11. Non-Functional Requirements

| Requirement | Target |
|---|---|
| End-to-end latency (hotkey release -> first keystroke) | < 1,600ms p90 on stable broadband |
| Memory - idle | < 80 MB RSS |
| Memory - active recording | < 150 MB RSS |
| CPU - idle | < 0.5% |
| CPU - recording | < 10% |
| Microphone open time after hotkey press | < 100ms |
| Cold start (tray visible) | < 2 seconds |

---

## 12. Security & Privacy

- **Zero telemetry.** No analytics or usage reporting. The only outbound connections are Soniox (audio) and OpenRouter (transcript).
- **Audio never written to disk.** All PCM and transcript data lives in RAM and is dropped immediately after injection.
- **API keys never in plaintext on disk.** Stored exclusively via libsecret.
- **Least privilege.** The Privileged Helper performs keystroke injection only - no network, no config, no secrets.
- **Debug logs** must never include raw audio data or API key values.

---

## 13. Distribution

Installation is handled by `install.sh` (curl-pipe, idempotent). Uninstallation by `uninstall.sh`.

**Installed paths:**

| Path | Purpose |
|---|---|
| `/usr/local/bin/linux-flow` | Main binary |
| `/usr/local/lib/linux-flow/linux-flow-helper` | Privileged Helper binary |
| `/etc/polkit-1/rules.d/50-linux-flow.rules` | polkit authorization rules |
| `/etc/udev/rules.d/60-linux-flow-uinput.rules` | udev rules for `/dev/uinput` access |
| `~/.config/systemd/user/linux-flow.service` | Systemd user service (autostart on login) |
| `/usr/share/applications/linux-flow.desktop` | XDG desktop entry |

`uninstall.sh` removes all of the above and does not touch `~/.config/linux-flow/` or libsecret entries.

---

## 14. Out of Scope (v1)

- Command Mode (LLM commands over selected text)
- Custom dictionaries or snippets
- Per-app tone / active-window context detection
- Always-listening / Voice Activity Detection (VAD)
- Native distro packages (AUR, .deb, .rpm)
- Self-updater
- Audio-reactive tray animation
- Offline / local ASR mode
- User-facing Clipboard-based injection as a primary method (internal clipboard fallback for non-Latin character injection is permitted — see Section 5)
- Multiple simultaneous hotkey bindings
