# تقرير فحص المشروع — مخالفات DESIGN.md

> فُحص المشروع بثلاثة sub-agents متوازية. النتائج التالية مُتحقَّق منها بقراءة مباشرة للملفات.

---

## المخالفات المؤكدة (2)

### 1. `app/src/asr.rs` — السطران 14 و20
- **المخالف:** `STREAM_TIMEOUT = Duration::from_secs(30)`
- **المطلوب:** "stream timeout: **8s** after EOS"
- **الإصلاح:**
  - السطر 14: `from_secs(30)` → `from_secs(8)`
  - السطر 20: رسالة الخطأ `"(30 s)"` → `"(8 s)"`

### 2. `app/src/llm.rs` — السطر 9
- **المخالف:** `REQUEST_TIMEOUT = Duration::from_secs(20)`
- **المطلوب:** "Timeout **5s**" للـLLM
- **الإصلاح:** `from_secs(20)` → `from_secs(5)`

---

## إيجابيات زائفة (ليست مخالفات)

| الملف | الملاحظة | السبب |
|---|---|---|
| `daemon/src/uinput.rs:103` | `ioctl` مُتجاهَل في `Drop` | `Drop` في Rust لا يمكنه إرجاع خطأ — حتمي |
| `app/src/wizard_polkit.rs:12` | `_nav` غير مُستخدَم | آخر صفحة wizard لا تحتاج push — `_` بادئة Rust لإسكات التحذير، التوقيع مطابق للمطلوب |

---

## الملفات التي اجتازت الفحص بنجاح

`audio.rs`, `pipeline.rs`, `secrets.rs`, `daemon/server.rs`, `daemon/uinput_inner.rs`,
`daemon/clipboard.rs`, `shared/ipc_proto.rs`, `tray.rs`, `wizard.rs`, `wizard_hotkey.rs`,
`hotkey.rs`, `hotkey_wayland.rs`, `hotkey_x11.rs`, `hotkey_evdev.rs`, `settings.rs`,
`shared/config.rs`, `install.sh`
