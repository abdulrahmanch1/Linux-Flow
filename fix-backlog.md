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
- [ ] **Status: Open**

---

### [H-02] No read timeout in `handle_connection` — stall attack possible
- **File:** `daemon/src/server.rs:83,87`
- **Code:** `stream.read_exact(&mut len_buf).await?;` — awaits forever
- **Problem:** A process on the same UID can open many connections and send incomplete data, leaving tasks pending forever.
- **Fix:** Wrap both `read_exact` calls with `tokio::time::timeout(Duration::from_secs(5), ...)`.
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

### [H-04] API key save failure is silent *(partial)*
- **File:** `app/src/settings.rs:90–96`
- **Code:** `let _ = tokio::runtime::Builder::new_current_thread()...`
- **Problem:** `spawn_save_key` for Soniox and OpenRouter keys is fire-and-forget. If Secret Service is not running the key is silently lost while the wizard/settings UI closes normally.
- **Note:** Config-file save errors ARE now surfaced via toast (fixed in commit `cf38820`). Only the keychain (libsecret) path remains silent.
- **Fix:** Return the save result from the spawned thread via a `std::sync::mpsc` channel, then show an `adw::Toast` in the UI if it fails.
- [~] **Status: Partially fixed** — `config.toml` errors shown; keychain errors still silent

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
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

## 🟡 Medium — Quality and Incorrect Behaviour

### [M-01] `IpcRequest` derives `Clone` — dictation text can be silently copied
- **File:** `shared/src/ipc_proto.rs:10`
- **Code:** `#[derive(Debug, Clone, Serialize, Deserialize)]`
- **Problem:** Deriving `Clone` on `IpcRequest::InjectText { text }` creates an unzeroized copy of the dictation text.
- **Fix:** Remove `Clone` from the derive — no current code clones an `IpcRequest`.
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

### [M-03] 500 µs sleep per character — +50 ms on 100-character text
- **File:** `daemon/src/uinput_inner.rs:124`
- **Code:** `thread::sleep(Duration::from_micros(500));`
- **Fix:** Remove entirely or lower to 100 µs. Test for character dropping at lower values.
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

### [M-05] "Next" button in X11 hotkey wizard page is active before a key is chosen
- **File:** `app/src/wizard_hotkey_x11.rs:21`
- **Problem:** The Next button is enabled from the start, so the user can skip the hotkey step silently.
- **Fix:** Add `next.set_sensitive(false);` after creating the button; enable it only after a successful `key_ctrl.connect_key_pressed` callback sets a hotkey.
- [ ] **Status: Open**

---

### [M-06] `unwrap_or_default` silently overwrites a corrupt config file
- **File:** `app/src/settings.rs:85`
- **Code:** `let mut cfg = Config::load().unwrap_or_default();`
- **Problem:** If the TOML is corrupt, default values are written over it without warning, permanently erasing the user's settings.
- **Fix:** Propagate the `Err` from `patch_config` and show a toast before any write is attempted.
- [ ] **Status: Open**

---

### [M-07] X11 display branch has no evdev fallback
- **File:** `app/src/hotkey.rs:88–94`
- **Problem:** On a pure X11 environment, if `X11Backend::run` fails there is no fallback to `EvdevBackend` — the hotkey stops working permanently until restart.
- **Fix:** Mirror the Wayland branch: call `EvdevBackend::run` when `X11Backend` exits with an error.
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

## 🔵 Low — Code Quality Improvements

### [L-01] SIGINT not handled — socket file left behind on Ctrl-C during debugging
- **File:** `daemon/src/main.rs:17`
- **Note:** Only SIGTERM is currently handled. SIGINT (`Ctrl-C`) is not, so the socket file at `/run/user/<uid>/linux-flow-helper.sock` is never cleaned up when the daemon is killed during development.
- **Fix:** Add `let mut sigint = signal(SignalKind::interrupt())?;` and include it in the `select!` alongside SIGTERM.
- [ ] **Status: Open**

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
- [ ] **Status: Open**

---

### [L-04] Doc comment says "8 s timeout" but constant is 30 s
- **File:** `app/src/asr.rs:53` vs `app/src/asr.rs:14`
- **Code:** Comment: *"8 s from EOS to `finished`"* — Constant: `STREAM_TIMEOUT = Duration::from_secs(30)`
- **Fix:** Update the comment to read *"30 s from EOS to `finished`"*.
- [ ] **Status: Open**

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
- [~] **Status: Partially fixed** — model field restored; key fields still empty on Back

---

### [L-08] Animation loop runs every 100 ms even when Idle — 10 wakeups/sec wasted
- **File:** `app/src/tray.rs:136–151`
- **Problem:** `tick.tick().await` fires every 100 ms unconditionally. The `update()` call still runs when `TrayState::Idle`, doing a round-trip to the tray service for nothing.
- **Fix:** Exit the loop when state is `Idle` (add `else { return; }`) or switch to a notify-based approach driven by state changes.
- [ ] **Status: Open**

---

### [L-09] Hotkey change in system settings is not detected at runtime
- **File:** `app/src/hotkey_wayland.rs:34–58`
- **Problem:** Changing the shortcut from the OS settings panel requires an application restart to take effect.
- **Fix:** Subscribe to `shortcuts.receive_shortcuts_changed()` if the ashpd version supports it.
- [ ] **Status: Open**

---

## Summary

| Severity | Total | Fixed | Partial | Open |
|----------|------:|------:|--------:|-----:|
| 🔴 Critical | 5 | 5 | 0 | 0 |
| 🟠 High | 7 | 0 | 1 | 6 |
| 🟡 Medium | 8 | 0 | 0 | 8 |
| 🔵 Low | 9 | 0 | 2 | 7 |
| **Total** | **29** | **5** | **3** | **21** |
