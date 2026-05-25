# H-06 Fix Design — Persistent WebSocket to Soniox

> وثيقة تصميم للمراجعة. لا يُكتب كود حتى يُوافَق على القرارات في §10.
> كل قرار مُعلَّم بـ DESIGN.md مقابل نص القيد المباشر.

---

## 1. المشكلة

في `app/src/asr.rs:61`:

```rust
let (ws, _) = timeout(CONNECT_TIMEOUT, connect_async(SONIOX_URL)).await...
```

هذا السطر يُنشئ اتصال WebSocket جديداً — بما يتضمن TLS handshake كامل —
**في كل مرة يُفرج المستخدم عن زر الإملاء.** الكلفة: 100–300 ms تُستهلك
من ميزانية الـ 200 ms المخصصة لـ ASR قبل أن تصل أول بايت صوت إلى Soniox.

---

## 2. لماذا TLS مكلف؟

```
TCP Handshake:      ~20–50ms   (SYN → SYN-ACK → ACK)
TLS 1.3 Handshake:  ~50–150ms  (ClientHello → ServerHello → Finished)
WebSocket Upgrade:  ~10–30ms   (HTTP 101 Switching Protocols)
─────────────────────────────────────────────────────
الإجمالي:           ~80–230ms  قبل إرسال أي صوت
```

هاد يعني أن الـ 200ms ASR budget تُهدر نصفها أو أكثر في مجرد فتح الاتصال.
Wispr Flow وأدوات مشابهة تتجنب هاد بإبقاء الاتصال مفتوحاً بين الجلسات.

---

## 3. الفرق بين اتصال جديد ومستمر

```
الحالة الحالية:
────────────────────────────────────────────────────
hotkey released
    │
    ▼ ~150ms  TLS handshake + WebSocket upgrade
    ▼ ~50ms   إرسال config frame (مع API key)
    ▼         بث الصوت...
    ▼         استقبال "finished"
    └─ إغلاق الاتصال

بعد الإصلاح:
────────────────────────────────────────────────────
[startup]
    │
    ▼ ~150ms  TLS handshake + WebSocket upgrade  ← مرة واحدة فقط
    └─ الاتصال يبقى مفتوحاً

hotkey released
    │
    ▼ ~50ms   إرسال config frame (مع API key)
    ▼         بث الصوت...
    ▼         استقبال "finished"
    └─ الاتصال يبقى مفتوحاً للجلسة التالية
```

---

## 4. ⚠ قيد حرج — يجب التحقق قبل التنفيذ

**السؤال الأساسي:** هل يدعم Soniox إعادة استخدام نفس WebSocket لجلسات متعددة؟

أي: بعد استقبال `"finished": true`، هل يمكن إرسال `config` frame جديد
على نفس الاتصال لبدء جلسة جديدة؟

| السيناريو | الحل المناسب |
|-----------|-------------|
| ✅ نعم — Soniox يدعم multi-session per WS | Option A: اتصال واحد دائم |
| ❌ لا — Soniox يغلق الاتصال بعد "finished" | Option B: إعادة استخدام TLS session |

⚠ **DECISION D-1:** يجب قراءة توثيق Soniox وتجربة إرسال config جديد بعد "finished"
قبل كتابة أي كود. هاد غير قابل للتخمين.

---

## 5. خيارات التنفيذ

### Option A — اتصال WebSocket دائم (الأمثل)

**المتطلب:** Soniox يدعم multi-session per connection.

```
الـ Pipeline يحتفظ بـ SonioxConnection دائمة
    ├── عند الـ startup: يُنشئ الاتصال ويُرسل ping كل 30s للإبقاء عليه
    ├── عند hotkey: يُرسل config frame جديد على نفس الاتصال
    ├── بعد "finished": الاتصال يبقى مفتوحاً
    └── عند خطأ: reconnect تلقائي (back-off: 1s, 2s, 4s, max 30s)
```

**الكسب:** يُلغي TLS handshake من كل الجلسات اللاحقة. الجلسة الأولى
فقط تدفع التكلفة.

### Option B — إعادة استخدام TLS Session (Fallback)

**المتطلب:** لا يشترط دعم Soniox لأي شيء إضافي.

TLS 1.3 يدعم **Session Resumption** عبر PSK (Pre-Shared Key) — الاتصال
الثاني يستأنف الـ TLS session القديمة بدون handshake كامل:

```
الاتصال الأول:  ~150ms (TLS handshake كامل)
الاتصال الثاني: ~20–40ms (TLS resumption فقط)
```

للتفعيل: استخدام `rustls` بدلاً من `native-tls` مع تفعيل session tickets.
`tokio-tungstenite` يدعم كلاهما.

**الكسب:** ~110–260ms محفوظة في كل جلسة لاحقة. أبسط من Option A
ولا يعتمد على بروتوكول Soniox.

### المقارنة

| | Option A | Option B |
|--|----------|----------|
| الكسب بعد الجلسة الأولى | ~150ms | ~110–260ms |
| يعتمد على Soniox protocol | نعم | لا |
| تعقيد التنفيذ | عالي | منخفض |
| خطر الـ reconnect | موجود | لا (اتصال جديد دائماً) |
| التوافق مع DESIGN.md | ✅ | ✅ |

---

## 6. توافق DESIGN.md

| القيد في DESIGN.md | الحالة |
|--------------------|--------|
| ASR عبر Soniox WebSocket | ✅ محافظ عليه في كلا الخيارين |
| Zeroize على API key بعد الإرسال | ✅ لا تغيير — config frame يُرسل ويُمحى كما هو |
| لا persistence على الدسك | ✅ الاتصال في الذاكرة فقط |
| Async عبر tokio | ✅ كلا الخيارين tokio-native |

لا تعارض مع DESIGN.md في كلا الخيارين.

---

## 7. هيكل الكود المقترح (Option A)

```
app/src/asr.rs
    ├── SonioxConnection struct
    │       ├── WebSocket sink/stream
    │       ├── ping task handle
    │       └── reconnect logic
    └── transcribe(conn: &mut SonioxConnection, ...) ← يستقبل connection جاهزة

app/src/pipeline.rs
    └── Pipeline struct
            └── soniox_conn: SonioxConnection  ← يُنشأ في Pipeline::new()
```

بدلاً من الحالة الحالية حيث `transcribe` تُنشئ اتصالها الخاص.

---

## 8. هيكل الكود المقترح (Option B)

```
app/Cargo.toml
    └── tokio-tungstenite = { features = ["rustls-tls-webpki-roots"] }
        (بدلاً من native-tls)

app/src/asr.rs
    └── لا تغيير في المنطق — فقط تغيير الـ TLS backend
        rustls يُفعّل session resumption تلقائياً
```

Option B أبسط بكثير ولا يحتاج إعادة هيكلة `pipeline.rs`.

---

## 9. Keepalive (Option A فقط)

Soniox لا يوثّق فترة idle timeout رسمياً. الممارسة الشائعة:

```rust
// Ping كل 25 ثانية (أقل من معظم server timeouts البالغة 30s)
tokio::time::interval(Duration::from_secs(25))
    → sink.send(Message::Ping(vec![]))
    → إذا فشل: reconnect
```

---

## 10. القرارات المطلوبة قبل التنفيذ

| # | السؤال | كيف يُحسم |
|---|--------|-----------|
| D-1 | هل Soniox يدعم multi-session per WebSocket؟ | قراءة توثيق Soniox + اختبار عملي |
| D-2 | Option A أم Option B؟ | بعد حسم D-1 |
| D-3 | هل نستخدم `rustls` بدلاً من `native-tls` حتى في Option A؟ | توصيتي نعم — rustls أكثر قابلية للتدقيق الأمني |

---

## 11. الملفات المتأثرة

### Option A

| الملف | طبيعة التغيير |
|-------|--------------|
| `app/src/asr.rs` | إعادة هيكلة — `SonioxConnection` struct + reconnect logic |
| `app/src/pipeline.rs` | إضافة `soniox_conn` للـ `Pipeline` struct؛ تمريره لـ `transcribe` |
| `app/Cargo.toml` | لا تغيير في الـ dependencies |

### Option B

| الملف | طبيعة التغيير |
|-------|--------------|
| `app/Cargo.toml` | تغيير feature flag لـ `tokio-tungstenite` من `native-tls` إلى `rustls` |
| `app/src/asr.rs` | لا تغيير في المنطق |

---

## 12. قائمة الاختبار

- [ ] الجلسة الأولى — latency مع TLS handshake كامل (baseline)
- [ ] الجلسة الثانية — latency بدون TLS handshake (الكسب الفعلي)
- [ ] الاتصال ينقطع أثناء الإملاء — reconnect تلقائي بدون crash
- [ ] Soniox يُعيد خطأ 402 — يظهر للمستخدم فوراً (لا انتظار 30s)
- [ ] idle لـ 5 دقائق ثم إملاء — الاتصال لا يزال حياً (ping فعّال)
- [ ] p99 latency بعد الإصلاح — قياس فعلي، الهدف: ASR < 200ms
