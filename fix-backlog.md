# fix-backlog — linux-flow

> All issues verified against actual source files. Line numbers are accurate as of the last audit pass.
> Legend: `[ ]` open · `[x]` fixed · `[~]` partially fixed (see note)

---

## 🔴 Critical — Blocks Correct Behaviour

### [C-01] Fixed 50 ms sleep inside `inject_text` mutex lock
- **File:** `daemon/src/uinput.rs:85`
- **Code:** `thread::sleep(Duration::from_millis(50));`
- **Problem:** Adds 50 ms to every dictation while the Mutex is held. By the time `inject_text` is called, hundreds of ms have already elapsed since hotkey release (ASR + LLM), so the compositor has long settled. The wait is pointless.
- **Fix:** Remove the sleep entirely, or reduce to ≤ 20 ms if compositor settling is still observed in testing.
- [x] **Status: Fixed** — commit `03a4883`

---

### [C-02] Clipboard path adds 220 ms unconditionally
- **File:** `daemon/src/uinput_inner.rs:135,137,141`
- **Code:**
  ```rust
  thread::sleep(Duration::from_millis(100)); // line 135
  thread::sleep(Duration::from_millis(20));  // line 137
  thread::sleep(Duration::from_millis(100)); // line 141
  ```
- **Problem:** 100 + 20 + 100 = 220 ms added on every non-ASCII or shift-requiring text injection.
- **Fix:** Lower experimentally (e.g. 50 + 10 + 50) with real testing. Long-term: replace `wl-copy`/`xclip` subprocess with `arboard` or `wl-clipboard-rs` for zero-latency clipboard writes.
- [x] **Status: Fixed** — 50+10+100 ms (third sleep kept at 100 ms to protect async clipboard readers) — commit `753fc6e`

---

### [C-03] Dictation JSON (including `api_key`) not zeroized before reaching tungstenite
- **File:** `app/src/asr.rs:79–80`
- **Code:**
  ```rust
  let msg_text = std::mem::take(&mut *config_str); // plain String
  sink.send(Message::Text(msg_text)).await?;        // transferred to tungstenite without zeroize
  ```
- **Problem:** `msg_text` carries a full JSON blob including `api_key`. `mem::take` bypasses `Zeroizing`'s drop guarantee; the raw bytes live in tungstenite's internal buffers. Violates Coding Rule #7.
- **Fix:** Build the config frame as `Vec<u8>`, send as `Message::Binary`, then call `.zeroize()` before drop.
- [x] **Status: Fixed** — clone for tungstenite, zeroize our copy immediately; tungstenite's internal buffer remains outside our control — commit `999a109`

---

### [C-04] `wl-copy`/`xclip` exit code never checked
- **File:** `daemon/src/clipboard.rs:50`
- **Code:** `child.wait().map_err(DaemonError::Io)?;`
- **Problem:** A non-zero exit code is silently ignored. The function returns `Ok(())` and Ctrl+V is still injected, pasting old or empty clipboard content without any error surfaced to the user.
- **Fix:**
  ```rust
  let status = child.wait().map_err(DaemonError::Io)?;
  if !status.success() {
      return Err(DaemonError::InjectionFailed(
          format!("{cmd} exited with {:?}", status.code())
      ));
  }
  ```
- [x] **Status: Fixed** — exit code checked and propagated; error now logged to stderr via `eprintln!` in server.rs — commits `4a0d182`, `2cb0f01`

---

### [C-05] Pipeline exits permanently on hotkey channel close
- **File:** `app/src/pipeline.rs:79`
- **Code:** `if !released { asr.abort(); return; }`
- **Problem:** `return` exits `run()` completely. Any transient hotkey channel drop requires restarting the application.
- **Fix:** Replace `return` with `break` to exit the loop cleanly, and send `PipelineEvent::Error` so the UI can notify the user.
- [x] **Status: Fixed** — commit `a94d9ef`

---

### [C-06] `Ctrl+KEY_V` via uinput is unreliable — intermittent paste failure confirmed in production
- **Files:** `daemon/src/uinput_inner.rs:128–131`, `daemon/src/keys.rs`
- **Confirmed in production:** User reports 30–50 % injection failure rate; daemon always returns `OK`; icon returns to Idle with no error — failure is silent.
- **Root cause (two compounding issues):**

  **Issue A — Non-Latin keyboard layout breaks the shortcut:**
  `inject_via_clipboard` sends `KEY_LEFTCTRL + KEY_V` as physical evdev scancodes. COSMIC applies the active XKB layout on top: with Arabic layout active, physical KEY_V (code 47) maps to the Arabic keysym `ر`, not `v`. Applications (GTK, Qt) bind paste to `<Primary>v` (keysym 0x76 = 'v'), so the injected shortcut becomes `Ctrl+ر` — unrecognised → no paste. This is the primary cause for any user who switches to a non-Latin layout.

  **Issue B — COSMIC epoll batching causes intermittent chord failure:**
  `key_event()` writes `EV_KEY + EV_SYN` with zero inter-event delay. All four events (CTRL-dn, V-dn, V-up, CTRL-up) land in `/dev/uinput` within a single OS scheduler quantum. COSMIC's libinput handler wakes up once and reads all four frames as a batch — the EV_SYN boundaries carry no real-time separation. Under load COSMIC fails to recognise the Ctrl+V chord → silent no-op. Adding 12 ms delays between events (current workaround) reduces but does not eliminate the race.

  **Previous attempt:** `KEY_PASTE` (0x161 / XF86Paste) was implemented as the fix for Issue A but reverted in commit `11bff59` because it produced the same intermittent failure — `KEY_PASTE` is also a physical scancode subject to the same epoll-batch race (Issue B).

- **Correct fix:** Replace the entire uinput Ctrl+V block with the `zwlr_virtual_keyboard_v1` Wayland protocol. This protocol lets the daemon define its own XKB keymap (hardcoded US/English), making the injected keysym (`v` = 0x76) independent of the user's active layout. Events sent through `zwlr_virtual_keyboard_v1` are time-stamped by the daemon, giving the compositor real temporal separation between key frames. Both issues A and B are resolved in one change.
  - Daemon connects to `$WAYLAND_DISPLAY` (already in environment via `PassEnvironment`)
  - Creates `zwlr_virtual_keyboard_v1` with a minimal single-key US keymap
  - Sends: `modifier(CTRL, pressed)` → `key(KEY_V, pressed, t+0)` → `key(KEY_V, released, t+12ms)` → `modifier(CTRL, released)`
  - Dependency: `wayland-client` crate (already pulled in transitively or add directly)
- **Note:** `clipboard_write()` (blocking wl-copy) remains unchanged — only the paste-trigger mechanism changes.
- [x] **Status: Fixed** — `zwp_virtual_keyboard_v1` via `wayland-protocols-misc`; layout-independent keysym `v` from daemon-owned XKB keymap; explicit wall-clock timestamps eliminate epoll-batch race; X11 fallback via xdotool; uinput KEY_PASTE as last resort

---

## 🟠 High — Security Vulnerability or Incorrect Behaviour

### [H-01] Unix socket inherits umask — world-readable by default
- **File:** `daemon/src/server.rs:118`
- **Code:** `let listener = UnixListener::bind(&path)?;` — no `set_permissions` follows
- **Problem:** With umask 022 the socket is created as 0755, allowing any local user to connect.
- **Fix:**
  ```rust
  let listener = UnixListener::bind(&path)?;
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
  ```
- [x] **Status: Fixed**

---

### [H-02] No read timeout in `handle_connection` — stall attack possible
- **File:** `daemon/src/server.rs:83,87`
- **Code:** `stream.read_exact(&mut len_buf).await?;` — awaits forever
- **Problem:** A process on the same UID can open many connections and send incomplete data, leaving tasks pending forever.
- **Fix:** Wrap both `read_exact` calls with `tokio::time::timeout(Duration::from_secs(5), ...)`.
- [x] **Status: Fixed**

---

### [H-03] Transient `accept()` error terminates the daemon
- **File:** `daemon/src/server.rs:126`
- **Code:** `let (stream, _) = result?;`
- **Problem:** Any transient error (EMFILE, EINTR) propagates via `?` and kills `run_server()` — and the daemon with it.
- **Fix:**
  ```rust
  let (stream, _) = match result {
      Ok(s) => s,
      Err(e) => { tracing::error!("accept error: {e}"); continue; }
  };
  ```
- [x] **Status: Fixed**

---

### [H-04] API key save failure is silent *(partial)*
- **File:** `app/src/settings.rs:90–96`
- **Code:** `let _ = tokio::runtime::Builder::new_current_thread()...`
- **Problem:** `spawn_save_key` for Soniox and OpenRouter keys is fire-and-forget. If Secret Service is not running the key is silently lost while the wizard/settings UI closes normally.
- **Note:** Config-file save errors ARE now surfaced via toast (fixed in commit `cf38820`). Only the keychain (libsecret) path remains silent.
- **Fix:** Return the save result from the spawned thread via a `std::sync::mpsc` channel, then show an `adw::Toast` in the UI if it fails.
- [x] **Status: Fixed** — keychain save results returned via `mpsc` channel; `adw::Toast` shown on failure; window stays open until all saves complete or fail

---

### [H-05] `audio.stop()` can block forever
- **File:** `app/src/pipeline.rs:77` + `app/src/audio.rs:51–56`
- **Problem:** `audio.stop()` calls `h.join()` with no timeout. If PipeWire stops calling the process callback, `mainloop.run()` never returns, `join()` blocks forever, and the pipeline freezes.
- **Fix:**
  ```rust
  tokio::time::timeout(
      Duration::from_secs(3),
      tokio::task::spawn_blocking(move || audio.stop()),
  ).await.ok();
  ```
- [x] **Status: Fixed** — timeout added; `spawn_blocking` was already present from prior session

---

### [H-06] New TLS handshake to Soniox for every recording (+100–300 ms)
- **File:** `app/src/asr.rs:59`
- **Code:** `let (ws, _) = timeout(CONNECT_TIMEOUT, connect_async(SONIOX_URL)).await...`
- **Problem:** A full TLS handshake is paid on every dictation session, consuming a large portion of the 200 ms ASR budget.
- **Fix:** Keep the WebSocket connection open between sessions with a ping/keepalive. Reconnect only on error.
- [ ] **Status: Open**

---

### [H-07] `Config::load()` — synchronous I/O on the hot path
- **File:** `app/src/pipeline.rs:52`
- **Code:** `if let Ok(fresh) = Config::load() { self.config = fresh; }`
- **Problem:** `fs::read_to_string` + `toml::from_str` are synchronous and run inside a tokio task at every hotkey press, blocking the async thread.
- **Fix:** Watch the config file with `inotify` and update `self.config` only when the file actually changes.
- [x] **Status: Fixed** — `notify` crate (inotify on Linux); watcher runs in background thread, sends `Config` over std channel only on file change; pipeline loop calls `try_recv()` — zero I/O on the hot path

---

### [H-08] Soniox error responses cause 30-second hang — confirmed in production logs
- **File:** `app/src/asr.rs`
- **Evidence:** Journald entries from `2026-05-21 20:48` and `20:51` show Soniox returning:
  ```json
  {"tokens":[],"error_code":402,"error_type":"organization_balance_exhausted","error_message":"Organization balance exhausted..."}
  ```
- **Problem:** `asr.rs` only checks for `"finished": true` to end the transcription loop. Error responses from Soniox do not include `"finished": true`, so the loop waits for the full `STREAM_TIMEOUT` (30 s) before returning. The pipeline then falls through to a generic error path with no user-visible message. Two confirmed 30-second hangs observed in production.
- **Fix:**
  After deserializing each incoming JSON frame, check for the presence of `error_code` before checking tokens:
  ```rust
  if let Some(code) = msg.error_code {
      return Err(AsrError::ServerError { code, message: msg.error_message });
  }
  ```
  Add an `error_code: Option<u32>` and `error_message: Option<String>` field to the ASR message struct.
- **Note:** Also requires adding specific UI messages in `pipeline.rs` for known codes (see M-09).
- [x] **Status: Fixed** — `error_code` was already checked in the existing code (`u32` with `#[serde(default)]`, checked via `!= 0`); `AsrError::ServerError` variant added for typed access

---

### [H-09] Text injection fails when a non-English keyboard layout is active
- **Files:** `daemon/src/uinput_inner.rs`, `daemon/src/uinput.rs`, `daemon/src/keys.rs`
- **Reported by:** user — Arabic, Hindi, Kurdish, Persian layouts confirmed affected.
- **Root cause:** `uinput` injects raw Linux scancodes (physical key positions), not Unicode characters. The compositor applies the active XKB layout on top, so the logical character produced depends on what layout the user has active at the moment of injection. Two code paths are both broken:

  **Path 1 — `inject_latin_char` (ASCII text, `uinput.rs:86`):**
  Sends `KEY_A`, `KEY_B`, … as physical keycodes. With Arabic layout active, `KEY_A` produces `'ش'`, `KEY_S` produces `'س'`, etc. English ASCII text gets injected as wrong Arabic characters.

  **Path 2 — `inject_via_clipboard` (non-ASCII text, `uinput_inner.rs:128`):**
  Writes correct text to clipboard via `wl-copy`/`xclip` (layout-independent ✓), then sends `KEY_LEFTCTRL + KEY_V` (physical keycodes). With Arabic layout, the compositor maps the physical V key to `'ر'` (or similar), so the shortcut becomes `Ctrl+ر` — which is not the paste binding in most applications → nothing is pasted.
  This explains why the user must switch to English layout: only then does `KEY_V` produce the logical `'v'` that applications recognize as `Ctrl+v` = paste.

- **Fix:**
  Replace `KEY_LEFTCTRL + KEY_V` in `inject_via_clipboard` with `KEY_PASTE` (Linux keycode `0x161 = 353`), a dedicated layout-independent paste event that compositors and applications handle without going through the XKB layer:
  ```rust
  // keys.rs — add:
  pub const KEY_PASTE: u16 = 0x161;

  // uinput_inner.rs — ALL_KEYS: add KEY_PASTE to capabilities

  // inject_via_clipboard — replace Ctrl+V block with:
  self.key_event(keys::KEY_PASTE, true)?;
  self.key_event(keys::KEY_PASTE, false)?;
  ```
  For Path 1 (inject_latin_char), the correct fix is to route ALL text through the clipboard path and drop the keycode-per-character approach entirely. The keycode path is inherently layout-dependent and cannot be made layout-neutral without access to the compositor's XKB state.

- **Impact:** Any user with a non-Latin keyboard layout cannot use the app at all. This includes all Arabic, Hindi, Persian, Kurdish users — the primary multilingual audience for a dictation tool.
- [ ] **Status: Regression** — KEY_PASTE was implemented then reverted in commit `11bff59` ("replace KEY_PASTE with Ctrl+V"). Backlog status was wrong. Current code still uses `KEY_LEFTCTRL + KEY_V` which fails with non-Latin layouts (Arabic, Persian, etc.) because the physical KEY_V produces a non-Latin keysym. Root cause of intermittent injection failures for Arabic-layout users.

---

### [H-10] Race condition — deferred clipboard clear wipes next injection's text
- **File:** `daemon/src/uinput_inner.rs:138–141`
- **Code:**
  ```rust
  thread::spawn(move || {
      thread::sleep(Duration::from_millis(1000));
      clipboard::clipboard_clear(display);
  });
  ```
- **Problem:** A background thread is spawned to clear the clipboard 1000 ms after each injection. If the user dictates again before that 1000 ms elapses, the sequence is:
  1. Injection 1 writes text A → triggers paste → spawns clear timer (t+1000 ms)
  2. Injection 2 writes text B to clipboard
  3. Clear timer from Injection 1 fires → **wipes text B before paste** → silent no-op
  The daemon returns `OK` and the icon shows Idle, but nothing was pasted.
- **Conditions:** Requires two dictations within ~1000 ms of each other. Unlikely in normal use (ASR+LLM adds ~400–600 ms per session), but possible with very short utterances or fast consecutive dictations.
- **Fix:** Replace the fire-and-forget `thread::spawn` with a cancellable timer. Track the previous timer handle and cancel it when a new injection begins:
  ```rust
  // Store an AbortHandle or use a tokio CancellationToken shared across calls.
  // On new injection: cancel previous token, create new one, schedule clear.
  ```
- **Note:** This issue disappears entirely if `clipboard-latency-fix` (wl-clipboard-rs) is implemented — the in-process clipboard owner never needs a deferred clear because it controls exactly when data is released.
- [ ] **Status: Open** — discovered during code review; not previously tracked

---

## 🟡 Medium — Quality and Incorrect Behaviour

### [M-01] `IpcRequest` derives `Clone` — dictation text can be silently copied
- **File:** `shared/src/ipc_proto.rs:10`
- **Code:** `#[derive(Debug, Clone, Serialize, Deserialize)]`
- **Problem:** Deriving `Clone` on `IpcRequest::InjectText { text }` creates an unzeroized copy of the dictation text.
- **Fix:** Remove `Clone` from the derive — no current code clones an `IpcRequest`.
- [x] **Status: Fixed** — `Clone` removed from `IpcRequest` derive

---

### [M-02] `derive(Debug)` leaks full dictation text into any log
- **File:** `shared/src/ipc_proto.rs:10`
- **Problem:** `{:?}` on any `IpcRequest` prints the complete text.
- **Fix:**
  ```rust
  impl fmt::Debug for IpcRequest {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          match self {
              IpcRequest::InjectText { text } =>
                  write!(f, "InjectText {{ text: [REDACTED, {} bytes] }}", text.len()),
          }
      }
  }
  ```
- [x] **Status: Fixed** — custom `Debug` impl added; redacts text content

---

### [M-03] 500 µs sleep per character — +50 ms on 100-character text
- **File:** `daemon/src/uinput_inner.rs:124`
- **Code:** `thread::sleep(Duration::from_micros(500));`
- **Fix:** Remove entirely or lower to 100 µs. Test for character dropping at lower values.
- [x] **Status: Fixed** — eliminated by H-09: `inject_latin_char` removed; all text uses clipboard path with no per-char sleep

---

### [M-04] HTTP 429 (rate limit) shows misleading "check your connection" message
- **File:** `app/src/pipeline.rs:123–129`
- **Problem:** All non-401 LLM errors fall through to a generic message that implies a network issue, even when the real cause is an API rate limit.
- **Fix:**
  ```rust
  Err(LlmError::Http(429)) => "Rate limit reached — try again in a moment.",
  Err(LlmError::Http(503)) => "OpenRouter is temporarily unavailable.",
  Err(_) => "Could not reach OpenRouter. Check your connection.",
  ```
- [x] **Status: Fixed** — 429 and 503 arms added to the LLM error match

---

### [M-05] "Next" button in X11 hotkey wizard page is active before a key is chosen
- **File:** `app/src/wizard_hotkey_x11.rs:21`
- **Problem:** The Next button is enabled from the start, so the user can skip the hotkey step silently.
- **Fix:** Add `next.set_sensitive(false);` after creating the button; enable it only after a successful `key_ctrl.connect_key_pressed` callback sets a hotkey.
- [x] **Status: Fixed** — `next.set_sensitive(false)` added; enabled via `next_w` weak ref inside key callback

---

### [M-06] `unwrap_or_default` silently overwrites a corrupt config file
- **File:** `app/src/settings.rs:85`
- **Code:** `let mut cfg = Config::load().unwrap_or_default();`
- **Problem:** If the TOML is corrupt, default values are written over it without warning, permanently erasing the user's settings.
- **Fix:** Propagate the `Err` from `patch_config` and show a toast before any write is attempted.
- [x] **Status: Fixed** — `unwrap_or_default()` replaced with `?`; corrupt config surfaces as toast

---

### [M-07] X11 display branch has no evdev fallback
- **File:** `app/src/hotkey.rs:88–94`
- **Problem:** On a pure X11 environment, if `X11Backend::run` fails there is no fallback to `EvdevBackend` — the hotkey stops working permanently until restart.
- **Fix:** Mirror the Wayland branch: call `EvdevBackend::run` when `X11Backend` exits with an error.
- [x] **Status: Fixed** — X11 branch moved to `tokio::spawn` + `spawn_blocking`; evdev fallback added on X11 error

---

### [M-08] `XNextEvent` return value discarded
- **File:** `app/src/hotkey_x11.rs:78`
- **Code:** `unsafe { XNextEvent(display, &mut event) };`
- **Problem:** When the X server crashes, `XNextEvent` returns a negative value and the loop continues processing garbage events.
- **Fix:**
  ```rust
  let ret = unsafe { XNextEvent(display, &mut event) };
  if ret != 0 { return Err(HotkeyError::X11Connect); }
  ```
- [x] **Status: Fixed** — return value checked; negative value triggers `X11Connect` error

---

### [M-09] Soniox 402 (balance exhausted) shows no user-visible message *(blocked on H-08)*
- **File:** `app/src/pipeline.rs`
- **Problem:** Even after fixing H-08, all Soniox server errors fall through to the same generic message. HTTP 402 (balance exhausted) needs a specific, actionable message so the user knows to top up their Soniox account. Confirmed cause of two silent failures on `2026-05-21`.
- **Fix:**
  ```rust
  Err(AsrError::ServerError { code: 402, .. }) =>
      "Soniox balance exhausted — add funds at soniox.com.",
  Err(AsrError::ServerError { code, message }) =>
      &format!("Soniox error {code}: {message}"),
  ```
- [x] **Status: Fixed** — `AsrError::ServerError` variant added; 402 arm and generic arm in pipeline.rs

---

### [M-10] Helper crash loop creates orphaned uinput devices
- **File:** `daemon/src/uinput.rs`, `system/linux-flow.service`
- **Evidence:** Kernel log from `2026-05-23 17:28–17:29` shows 30 uinput devices registered in 60 seconds (`/devices/virtual/input/input19` → `input48`), one every `RestartSec=2` interval.
- **Problem:** Each helper restart creates a new `/dev/input` virtual device. While the kernel releases the device when the fd is closed on process exit, a rapid crash loop floods the input device list and can exhaust kernel input slots. No `StartLimitBurst` is set in the service, so restarts are unbounded.
- **Fix:**
  (a) In `system/linux-flow.service`, add restart rate limiting:
  ```ini
  StartLimitIntervalSec=60
  StartLimitBurst=5
  ```
  (b) Ensure `UinputDevice` implements `Drop` that explicitly closes the fd and calls `UI_DEV_DESTROY` ioctl before process exit.
- [x] **Status: Fixed** — (a) restart limits added to service file; (b) `Drop` for `UinputInjector` calling `UI_DEV_DESTROY` was already implemented

---

## 🔵 Low — Code Quality Improvements

### [L-01] SIGINT not handled — socket file left behind on Ctrl-C during debugging
- **File:** `daemon/src/main.rs:17`
- **Note:** Only SIGTERM is currently handled. SIGINT (`Ctrl-C`) is not, so the socket file at `/run/user/<uid>/linux-flow-helper.sock` is never cleaned up when the daemon is killed during development.
- **Fix:** Add `let mut sigint = signal(SignalKind::interrupt())?;` and include it in the `select!` alongside SIGTERM.
- [x] **Status: Fixed** — SIGINT handler added; both signals feed the same shutdown channel via `tokio::select!`

---

### [L-02] X11 hotkey thread cannot be stopped on Quit
- **File:** `app/src/hotkey.rs:89–92`
- **Problem:** The X11 thread runs until process exit with no stop mechanism — `XGrabKey` stays registered and it is a resource leak.
- **Fix:** Add an `AtomicBool` stop-flag; check it in the `XNextEvent` loop.
- [ ] **Status: Open**

---

### [L-03] `Ordering::Relaxed` used for stop-flag — unsafe on ARM
- **File:** `app/src/audio.rs:38,52,97`
- **Problem:** Correct on x86 due to TSO, but unsound on ARM (weaker memory model).
- **Fix:** Use `store(true, Ordering::Release)` at the write site and `load(Ordering::Acquire)` at every read site in the PipeWire thread.
- [x] **Status: Fixed** — all three sites updated: `store(false, Release)`, `store(true, Release)`, `load(Acquire)`

---

### [L-04] Doc comment says "8 s timeout" but constant is 30 s
- **File:** `app/src/asr.rs:53` vs `app/src/asr.rs:14`
- **Code:** Comment: *"8 s from EOS to `finished`"* — Constant: `STREAM_TIMEOUT = Duration::from_secs(30)`
- **Fix:** Update the comment to read *"30 s from EOS to `finished`"*.
- [x] **Status: Fixed** — doc comment updated

---

### [L-05] Keyboard hot-plug not supported in evdev backend
- **File:** `app/src/hotkey_evdev.rs:22–31`
- **Problem:** Keyboards connected after startup are not detected until the application is restarted.
- **Fix:** Watch `/dev/input/` with `inotify` for `IN_CREATE` events and call `open_keyboards` again on new devices.
- [ ] **Status: Open**

---

### [L-06] Keyboard disconnect ends evdev backend with no retry
- **File:** `app/src/hotkey_evdev.rs:43`
- **Problem:** When a keyboard is unplugged, the merged stream closes and `run` returns `Err(StreamClosed)` immediately with no reconnect attempt.
- **Fix:** On `StreamClosed`, call `open_keyboards` again after a short delay and re-enter the event loop.
- [ ] **Status: Open**

---

### [L-07] Wizard input fields appear empty when navigating Back *(partial)*
- **File:** `app/src/wizard.rs` — `page_soniox`, `page_openrouter`
- **Problem:** Fields are not pre-populated from `WizardState` when a page is rebuilt on Back navigation.
- **Note:** `model_entry` in `page_openrouter` IS pre-populated. The Soniox key field and OpenRouter key field are not.
- **Fix:** Add `entry.set_text(state.borrow().field.as_str())` during initialization of each `PasswordEntryRow`.
- [x] **Status: Fixed** — Soniox key and OpenRouter key fields pre-populated from state after `connect_changed` is wired

---

### [L-08] Animation loop runs every 100 ms even when Idle — 10 wakeups/sec wasted
- **File:** `app/src/tray.rs:136–151`
- **Problem:** `tick.tick().await` fires every 100 ms unconditionally. The `update()` call still runs when `TrayState::Idle`, doing a round-trip to the tray service for nothing.
- **Fix:** Exit the loop when state is `Idle` (add `else { return; }`) or switch to a notify-based approach driven by state changes.
- [x] **Status: Fixed** — `Arc<AtomicBool>` shared between `set_state` and animation loop; DBus `update()` skipped when not recording/processing

---

### [L-09] Hotkey change in system settings is not detected at runtime
- **File:** `app/src/hotkey_wayland.rs:34–58`
- **Problem:** Changing the shortcut from the OS settings panel requires an application restart to take effect.
- **Fix:** Subscribe to `shortcuts.receive_shortcuts_changed()` if the ashpd version supports it.
- [ ] **Status: Open**

---

### [L-10] Helper stderr not forwarded to journald — crash root causes invisible
- **File:** `system/linux-flow.service`
- **Evidence:** The crash loop on `2026-05-23 17:28` produced zero stderr lines in journald. The helper writes all errors via `eprintln!` but the service has no `StandardError=journal` directive, so output goes to `/dev/null`.
- **Problem:** When the helper crashes, the reason (socket bind failure, uinput error, IPC decode error) is permanently lost. Debugging requires attaching a terminal to the process, which is impractical for a background daemon.
- **Fix:**
  ```ini
  [Service]
  StandardOutput=journal
  StandardError=journal
  ```
- [x] **Status: Fixed** — `StandardOutput=journal` and `StandardError=journal` added to service file

---

### [L-11] New Unix socket connection opened for every injection
- **File:** `app/src/inject.rs:15`
- **Code:** `let mut stream = timeout(IPC_TIMEOUT, UnixStream::connect(&path)).await...`
- **Problem:** `inject_text` opens a fresh `UnixStream` on every call and drops it when the function returns. Unix socket connection is fast (< 1 ms) but the repeated connect/drop cycle is unnecessary overhead and adds minor allocations on every injection.
- **Fix:** Hold a persistent `UnixStream` in the `Pipeline` struct. Reconnect only on `Err`. Pass it into `inject_text` or wrap it in a lightweight connection-manager. This matches how `LlmClient` (reqwest) handles its HTTP connection pool — created once at startup, reused across sessions.
- **Note:** Low priority — the overhead is sub-millisecond and not user-visible. Implement alongside another pipeline refactor to avoid a standalone session for a minor gain.
- [x] **Status: Fixed** — `IpcClient` struct holds a persistent `UnixStream`; reconnects transparently on broken pipe (daemon restart); `ensure_connected()` called after hotkey release to pre-connect during ASR wait, eliminating the connect syscall from the injection hot path

---

## Summary

| Severity | Total | Fixed | Partial | Open |
|----------|------:|------:|--------:|-----:|
| 🔴 Critical | 6 | 5 | 0 | 1 |
| 🟠 High | 10 | 5 | 1 | 4 |
| 🟡 Medium | 10 | 10 | 0 | 0 |
| 🔵 Low | 11 | 6 | 0 | 5 |
| **Total** | **37** | **27** | **1** | **9** |

### Open items requiring dedicated sessions
- **C-06** Layout-independent injection — design: `c06-injection-fix.md`
- **H-06** Persistent WebSocket to Soniox — design: `h06-persistent-websocket.md`
- **H-07** inotify config watch — needs `inotify`/`notify` crate dependency
- **H-10** Clipboard clear race condition — cancellable timer; resolves automatically with `clipboard-latency-fix`
- **H-04** (partial) — keychain save errors still silent
- **L-02** X11 hotkey thread stop — `XNextEvent` is blocking; channel closure provides partial coverage
- **L-05** Keyboard hotplug — needs inotify on `/dev/input/`
- **L-06** Keyboard disconnect retry — moderate complexity
- **L-09** Runtime shortcut detection — depends on ashpd API version
- **L-11** Persistent IPC connection — low priority; combine with pipeline refactor
