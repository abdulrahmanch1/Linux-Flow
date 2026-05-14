# linux-flow — Design

Wispr Flow-style dictation for Linux. Cloud ASR + LLM rewrite, push-to-talk, wayland-first.

## Stack
- Rust, GPL-3.0
- Wayland-first, X11 fallback
- Two processes: tray app + privileged helper (polkit, owns `/dev/uinput`, Unix socket IPC)

## Pipeline
1. Hotkey held (default: Right Ctrl) → XDG GlobalShortcuts portal (X11: XGrabKey)
2. Audio captured via libpipewire (PTT only, mic closed otherwise)
3. Streamed to Soniox WebSocket (auto-detect, multilingual, code-switching)
4. On release: full utterance → OpenRouter (`OPENROUTER_API_KEY`, model from `LLM_MODEL`, default `google/gemini-2.0-flash-lite`, with `provider.sort = "throughput"`)
5. LLM prompt: filler removal, false-start/self-correction handling, reflow grammar, punctuation, capitalization, honor "new line"/"new paragraph". Preserve input language.
6. Cleaned text → helper over Unix socket → uinput → focused window

## UI
- Tray: StatusNotifierItem, canned-loop wave animation (~10fps) while recording
- Wizard + settings: GTK4 + libadwaita
- First-run wizard steps: Soniox key → OpenRouter key + model → hotkey portal grant → polkit auth for helper

## Storage
- Prefs: `~/.config/linux-flow/config.toml`
- Secrets: libsecret (Secret Service)
- No transcript persistence. Audio + transcripts in RAM, dropped after injection.
- No telemetry. Logs to journald on `--debug` only.

## Distribution
- `install.sh` (curl-pipe) + `uninstall.sh`
- Lays down:
  - `/usr/local/bin/linux-flow`
  - `/usr/local/lib/linux-flow/linux-flow-helper`
  - `/etc/polkit-1/rules.d/50-linux-flow.rules`
  - `/etc/udev/rules.d/60-linux-flow-uinput.rules`
  - `~/.config/systemd/user/linux-flow.service` (template)
  - `/usr/share/applications/linux-flow.desktop`

## Out of v1
Command Mode, custom dictionary, snippets, per-app tone, active-window detection, always-listening/VAD, AUR/deb/rpm, self-updater, audio-reactive tray animation.
