# Session Plan — ترتيب الجلسات وتوزيع المهام

> كل جلسة = commit منفصل + PR منفصل.
> الترتيب إلزامي للمجموعات 1 و 2 بسبب تشارك الملفات. الباقي مرن.
> لتحديد جلسة كمكتملة: غيّر `⬜ مفتوحة` إلى `✅ مكتملة` في سطر الحالة.

---

## المجموعة 1 — Daemon Injection (ترتيب إلزامي)

### جلسة 1 — C-06

| | |
|--|--|
| **الحالة** | ✅ مكتملة |
| **ملف التصميم** | `c06-injection-fix.md` |
| **لوحدها أو مع غيرها؟** | **لوحدها** |
| **السبب** | تُنشئ ملفين جديدين وتُعدّل 3 ملفات في daemon — كبيرة بما يكفي |

**الملفات:**
```
daemon/src/uinput_inner.rs   ← يُعدَّل (يُستبدل بلوك Ctrl+V)
daemon/src/paste_trigger.rs  ← جديد
daemon/src/wayland_vk.rs     ← جديد
daemon/src/uinput.rs         ← يُعدَّل (تهيئة VirtualKeyboard)
daemon/Cargo.toml            ← يُعدَّل (wayland-client, wayland-protocols, wayland-protocols-wlr)
```

---

### جلسة 2 — clipboard-latency-fix + H-10

| | |
|--|--|
| **الحالة** | ✅ مكتملة |
| **ملف التصميم** | `clipboard-latency-fix.md` |
| **لوحدها أو مع غيرها؟** | **مع H-10** — H-10 يختفي تلقائياً في هاد الجلسة |
| **السبب** | clipboard-latency-fix يحذف الـ deferred clear بالكامل فيحل H-10 بدون كود إضافي |
| **شرط** | لا تبدأ قبل اكتمال جلسة 1 — كلتاهما تُعدّل `uinput_inner.rs` و `uinput.rs` |

**الملفات:**
```
daemon/src/clipboard.rs          ← يُعدَّل (subprocess → wl-clipboard-rs)
daemon/src/uinput_inner.rs       ← يُعدَّل (حذف 80ms sleep + deferred clear)
daemon/src/wayland_clipboard.rs  ← جديد
daemon/src/uinput.rs             ← يُعدَّل (تهيئة WaylandClipboard)
daemon/Cargo.toml                ← يُعدَّل (wl-clipboard-rs, arboard)
```

**المهام المُغلقة في هاد الجلسة:** clipboard-latency-fix ✓  H-10 ✓

---

## المجموعة 2 — App Pipeline (ترتيب إلزامي داخلياً)

### جلسة 3 — H-06

| | |
|--|--|
| **الحالة** | ✅ مكتملة |
| **ملف التصميم** | `h06-persistent-websocket.md` |
| **لوحدها أو مع غيرها؟** | **لوحدها** |
| **السبب** | تُعيد هيكلة `asr.rs` بالكامل — كبيرة وحساسة |
| **شرط** | يجب التحقق من Soniox protocol قبل البدء (D-1 في ملف التصميم) |

**الملفات:**
```
app/src/asr.rs       ← يُعدَّل (SonioxConnection struct + reconnect)
app/src/pipeline.rs  ← يُعدَّل (إضافة soniox_conn للـ Pipeline struct)
app/Cargo.toml       ← يُعدَّل (rustls إذا اخترنا Option B)
```

---

### جلسة 4 — H-07 + L-11

| | |
|--|--|
| **الحالة** | ✅ مكتملة |
| **ملف التصميم** | لا يوجد |
| **لوحدها أو مع غيرها؟** | **معاً إلزامياً** |
| **السبب** | كلاهما يُعدّل `pipeline.rs` — لو نُفّذا في جلستين منفصلتين حصل merge conflict |
| **شرط** | بعد جلسة 3 لأن H-06 أيضاً يُعدّل `pipeline.rs` |

**الملفات:**
```
app/src/pipeline.rs  ← يُعدَّل (inotify watch بدل Config::load + persistent IPC conn)
app/src/inject.rs    ← يُعدَّل (persistent UnixStream بدل connect per call)
app/Cargo.toml       ← يُعدَّل (notify crate لـ inotify)
```

**المهام المُغلقة في هاد الجلسة:** H-07 ✓  L-11 ✓

---

## المجموعة 3 — مستقلة (أي ترتيب)

### جلسة 5 — H-04

| | |
|--|--|
| **الحالة** | ✅ مكتملة |
| **ملف التصميم** | لا يوجد |
| **لوحدها أو مع غيرها؟** | **لوحدها** (صغيرة) |

**الملفات:**
```
app/src/settings.rs  ← يُعدَّل (keychain error → toast)
```

---

### جلسة 6 — L-05 + L-06

| | |
|--|--|
| **الحالة** | ⬜ مفتوحة |
| **ملف التصميم** | لا يوجد |
| **لوحدها أو مع غيرها؟** | **معاً إلزامياً** |
| **السبب** | كلاهما في `hotkey_evdev.rs` — hotplug و disconnect وجهان لنفس المشكلة |

**الملفات:**
```
app/src/hotkey_evdev.rs  ← يُعدَّل (inotify للـ hotplug + reconnect للـ disconnect)
```

**المهام المُغلقة في هاد الجلسة:** L-05 ✓  L-06 ✓

---

### جلسة 7 — L-02 + L-09

| | |
|--|--|
| **الحالة** | ⬜ مفتوحة |
| **ملف التصميم** | لا يوجد |
| **لوحدها أو مع غيرها؟** | **معاً مُفضَّل** |
| **السبب** | كلاهما في ملفات hotkey منفصلة لكن لا تعارض — يوفران جلسة |

**الملفات:**
```
app/src/hotkey.rs         ← يُعدَّل (L-02: AtomicBool stop-flag للـ X11 thread)
app/src/hotkey_x11.rs     ← يُعدَّل (L-02: فحص الـ flag في حلقة XNextEvent)
app/src/hotkey_wayland.rs ← يُعدَّل (L-09: shortcuts_changed subscription)
```

**المهام المُغلقة في هاد الجلسة:** L-02 ✓  L-09 ✓

---

## ملخص الجلسات

| # | الحالة | المهام | الملفات الرئيسية | وحدها؟ | شرط |
|---|--------|--------|-----------------|--------|-----|
| 1 | ✅ | C-06 | `uinput_inner`, `paste_trigger`*, `wayland_vk`* | وحدها | — |
| 2 | ✅ | clipboard-fix + H-10 | `clipboard`, `uinput_inner`, `wayland_clipboard`* | وحدها | بعد 1 |
| 3 | ✅ | H-06 | `asr`, `pipeline` | وحدها | Soniox check |
| 4 | ✅ | H-07 + L-11 | `pipeline`, `inject` | معاً ⚠ | بعد 3 |
| 5 | ✅ | H-04 | `settings` | وحدها | — |
| 6 | ⬜ | L-05 + L-06 | `hotkey_evdev` | معاً ⚠ | — |
| 7 | ⬜ | L-02 + L-09 | `hotkey`, `hotkey_x11`, `hotkey_wayland` | مُفضَّل | — |

`*` ملف جديد
