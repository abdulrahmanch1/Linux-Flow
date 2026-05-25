# Clipboard Latency Fix Design — إزالة الـ 80ms Sleep

> وثيقة تصميم للمراجعة. لا يُكتب كود حتى يُوافَق على القرارات في §11.
> كل قرار مُعلَّم بـ DESIGN.md مقابل نص القيد المباشر.

---

## 1. المشكلة — أين تذهب الـ 80ms؟

### المسار الحالي (كامل)

```
المستخدم يرفع الزر
        │
        ▼ ~200ms  ASR
        ▼ ~200ms  LLM
        ▼ ~200ms  Network
        │
        ▼  app → IPC → helper
        │
inject_via_clipboard()        ← نحن هنا
├── clipboard_write()          wl-copy subprocess  ~20–60ms
├── thread::sleep(80ms)        ← المشكلة
├── key_event(CTRL down)       12ms sleep
├── key_event(V down)          12ms sleep
├── key_event(V up)            12ms sleep
└── key_event(CTRL up)         (C-06 يستبدل هذا الجزء)
```

**المجموع الكلي بعد IPC:** ما بين 136ms و 176ms — وهاد **أكبر من ميزانية
الـ injection كلها** في الـ 700ms target.

---

## 2. لماذا الـ 80ms موجودة أصلاً؟

### الـ Clipboard على Wayland — كيف يشتغل؟

على Wayland، الـ clipboard ما هو ذاكرة مشتركة. هو **اتفاقية بين ثلاثة أطراف:**

```
الطرف 1: daemon (linux-flow-helper)
    "أنا عندي نص، أنا المالك الحالي للـ clipboard"

الطرف 2: Clipboard Manager (جزء من COSMIC/GNOME/KDE)
    "حسناً، سأحتفظ بنسخة منه حتى يطلبه أحد"

الطرف 3: التطبيق (VS Code, Firefox, Terminal)
    "أريد محتوى الـ clipboard"
```

`wl-copy` يقوم بدور الطرف 1 — يعلن أنه مالك الـ clipboard ثم **ينتظر** حتى
Clipboard Manager يأخذ نسخة منه، ثم يخرج.

### لماذا الـ 80ms إضافية إذاً؟

```
clipboard_write() ← تنتهي حين يخرج wl-copy
                    أي: حين الـ clipboard manager أخذ الملكية

لكن...

بعد خروج wl-copy ← يبقى الـ Wayland socket يتدفق داخلياً
                     الكومبوزيتور لم يُفرغ buffer events بعد
                     التطبيق المستهدف لم يتلقَّ إشعار "clipboard تغيّر" بعد

لو أرسلنا Ctrl+V الآن ← التطبيق يطلب clipboard فيجد إما:
                          - محتوى قديم (قبل النص الجديد)
                          - لا شيء (race condition)
```

الـ 80ms هي هامش أمان تجريبي — لا يوجد إشعار مضمون بأن الكومبوزيتور
"انتهى من استيعاب تغيير الـ clipboard"، فاخترنا رقماً يعمل على COSMIC
بناءً على اختبار يدوي.

---

## 3. السبب الجذري

```
الاستخدام الحالي:   daemon → subprocess (wl-copy) → clipboard manager
المشكلة:            الـ daemon فقد السيطرة — لا يعرف متى "الـ clipboard جاهز"
الحل:               الـ daemon يصبح هو مالك الـ clipboard مباشرة
```

بمجرد أن الـ daemon يكون هو الطرف 1 (المالك المباشر)، لا يحتاج لانتظار
أي شيء — هو يعرف بالضبط متى تطلب التطبيق البيانات ويعطيها له فوراً.

---

## 4. الحل المقترح — `wl-clipboard-rs`

### الفكرة

بدلاً من:
```
daemon يشغّل wl-copy (subprocess) → ينتظر → subprocess ينتهي → ننتظر 80ms
```

نفعل:
```
daemon يفتح Wayland socket مباشرة → يعلن نفسه مالك الـ clipboard
→ يرسل paste trigger فوراً
→ حين التطبيق يطلب البيانات، يُجيبه الـ daemon مباشرة
```

### لماذا هذا آمن؟

الـ daemon موجود طالما التطبيق المستهدف يطلب البيانات — لأننا نُرسل
paste trigger قبل أن نتركه، وعملية الـ paste تحدث قبل أن يتوقف الـ daemon
عن خدمة البيانات.

### مخطط المسار الجديد

```
inject_text(text)
      │
      ▼
WaylandClipboard::set(text)      ← في الذاكرة، فوري — 0ms
      │
      ▼
PasteTrigger::fire()             ← (من C-06: zwp/zwlr virtual keyboard)
      │
      ▼  [async — يحدث في الخلفية]
التطبيق يرسل طلب "أعطني محتوى الـ clipboard"
      │
      ▼
WaylandClipboard::serve(request) ← الـ daemon يُجيب مباشرة
      │
      ▼
النص يظهر في التطبيق ✓
```

**النتيجة:** الـ 80ms تصبح 0ms. لا subprocess، لا انتظار.

---

## 5. مسار X11

على X11، الـ clipboard يشتغل بنفس المبدأ (Selection Protocol) لكن عبر
Xlib. بدلاً من `xclip` subprocess:

```
xclip subprocess (حالياً)     →   arboard crate (مباشر)
"أنا شغّل xclip وانتظر"         "أنا أخدم الـ clipboard مباشرة"
```

`arboard` يدعم Wayland و X11 بنفس الـ API — لكن لأسباب تُذكر في §9،
نستخدمهما منفصلَين.

---

## 6. مقارنة المسارين قبل وبعد

### Wayland

| الخطوة | قبل | بعد |
|--------|-----|-----|
| كتابة الـ clipboard | wl-copy subprocess | `wl-clipboard-rs` in-process |
| وقت الكتابة | ~20–60ms (fork + exec) | ~0ms (memory write) |
| انتظار بعد الكتابة | 80ms (sleep ثابت) | 0ms (لا انتظار) |
| paste trigger | uinput Ctrl+V (C-06 يُصلح) | zwp/zwlr virtual keyboard |
| **المجموع** | **~136–176ms** | **~12–24ms** |

### X11

| الخطوة | قبل | بعد |
|--------|-----|-----|
| كتابة الـ clipboard | xclip subprocess | `arboard` in-process |
| وقت الكتابة | ~10–30ms | ~0ms |
| انتظار بعد الكتابة | 80ms | 0ms |
| paste trigger | uinput Ctrl+V | xdotool (C-06) |
| **المجموع** | **~114–152ms** | **~10–20ms** |

---

## 7. المسار الكامل بعد كلا الإصلاحَين (C-06 + هذا الملف)

```
المستخدم يرفع الزر
        │
        ▼ ~200ms  ASR (Soniox)
        ▼ ~200ms  LLM (OpenRouter)
        ▼ ~200ms  Network
        │
        ▼  app → IPC → helper  ~2ms
        │
inject_text()
├── WaylandClipboard::set()      ~0ms   (memory)
└── PasteTrigger::fire()         ~12ms  (timestamp delta, no sleep)
        │
        ▼
النص يظهر ✓

المجموع الكلي (p99 estimate): ~614ms  ← أقل من 700ms target
```

---

## 8. ماذا عن AT-SPI؟ (تجاوز الـ clipboard كلياً)

AT-SPI هو بروتوكول إمكانية الوصول — يسمح بإدخال النص مباشرةً في موضع
الـ cursor بدون clipboard ولا paste trigger. صفر تأخير، يعمل على Wayland
و X11 بدون فرق.

### ⚠ تعارض مع DESIGN.md — يستوجب قرار المالك

**نص DESIGN.md الحرفي (الخطوة 6):**
> `Cleaned text → helper over Unix socket → uinput → focused window`

AT-SPI **لا يستخدم uinput** — يستخدم D-Bus مباشرةً إلى التطبيق.
هذا تعارض صريح مع الخطوة 6 في DESIGN.md.

**لذلك:** AT-SPI مُستبعَد من هذه الوثيقة. إذا أراد المالك إضافته كمسار
أول في المستقبل فيجب تحديث DESIGN.md أولاً قبل أي كود.

| مسار | يتطابق مع DESIGN.md؟ |
|------|---------------------|
| `wl-clipboard-rs` + virtual keyboard | ✅ نعم — الـ helper لا يزال يحقن عبر uinput/virtual keyboard |
| `arboard` + xdotool | ✅ نعم |
| AT-SPI | ❌ يتعارض مع "uinput → focused window" |

---

## 9. لماذا لا نستخدم `arboard` للـ Wayland أيضاً؟

`arboard` هو مكتبة cross-platform ممتازة لكنها تُشغّل event loop داخلي
في thread منفصل للـ Wayland. هذا يتعارض مع هيكل الـ daemon:

- الـ daemon يعمل على tokio runtime
- `arboard` يشغّل thread إضافي مع wayland-client event loop خاص فيه
- نتيجة: thread-ين لـ Wayland connection على نفس البروسيس

`wl-clipboard-rs` مصمم خصيصاً للـ clipboard وله دعم async يتكامل مع tokio.

---

## 10. الملفات المتأثرة

| الملف | طبيعة التغيير |
|-------|--------------|
| `daemon/src/clipboard.rs` | استبدال `pipe("wl-copy", ...)` بـ `WaylandClipboard::set()` و `pipe("xclip", ...)` بـ `arboard` |
| `daemon/src/uinput_inner.rs` | حذف `thread::sleep(Duration::from_millis(80))` |
| `daemon/src/wayland_clipboard.rs` | **ملف جديد** — `WaylandClipboard` struct تغلّف `wl-clipboard-rs` |
| `daemon/src/uinput.rs` | تهيئة `WaylandClipboard` عند الـ startup؛ تمريره إلى `inject_via_clipboard` |
| `daemon/Cargo.toml` | إضافة `wl-clipboard-rs` و `arboard` |

**الملفات التي لا تتغير:**
`daemon/src/keys.rs`، كل `shared/`، كل `app/`، IPC protocol، polkit rules.

---

## 11. التبعيات الجديدة

| Crate | Version | License | الغرض |
|-------|---------|---------|-------|
| `wl-clipboard-rs` | `0.8` | MIT | Wayland clipboard owner in-process |
| `arboard` | `3` | MIT / Apache-2.0 | X11 clipboard owner in-process |

كلاهما MIT-compatible مع GPL-3.0 للمشروع.
لا يوجد runtime binary جديد — كل شيء داخل الـ daemon process.

---

## 12. القرارات المطلوبة قبل التنفيذ

| # | السؤال | التوصية | توافق DESIGN.md |
|---|--------|---------|-----------------|
| D-1 | استخدام `wl-clipboard-rs` للـ Wayland بدلاً من `wl-copy`؟ | ✅ نعم | متوافق — الـ helper لا يزال يتحكم بالـ injection |
| D-2 | استخدام `arboard` للـ X11 بدلاً من `xclip`؟ | ✅ نعم | متوافق |
| D-3 | هل يُنفَّذ هذا الإصلاح بعد C-06 أم بالتوازي؟ | بعد C-06 — يعتمد على الـ paste trigger الجديد | — |

---

## 13. قائمة الاختبار

- [ ] نص عربي، Wayland، COSMIC — يُلصق صح بدون تأخير ملحوظ
- [ ] نص عربي، Wayland، GNOME — يُلصق صح
- [ ] نص إنجليزي، Wayland — لا regression
- [ ] نص عربي، X11 — يُلصق صح
- [ ] نص 500 حرف — لا فقدان أحرف
- [ ] لصق متتالي سريع (3 إملاءات خلال 5 ثواني) — لا تداخل بين الـ clipboard sessions
- [ ] daemon يخرج خلال clipboard serve — لا crash في التطبيق المستهدف
- [ ] p99 latency بعد الإصلاحَين — قياس فعلي، الهدف أقل من 700ms

---

## 14. ما لا يتغير

- IPC بين app و helper — لا يتغير
- منطق الـ virtual keyboard (C-06) — يبقى كما هو
- uinput device registration — لا يتغير
- Polkit rules، install.sh — لا يتغير
- Zero persistence rule — لا كتابة على الدسك
