---
name: ptyxis-compose-injection
description: لماذا تظهر § في بداية النص المحقون في Ptyxis، ولماذا الإلغاء عبر VK keysym رهان غير مؤكَّد
metadata:
  type: project
---

المشكلة: hotkey = Right Ctrl، وخريطة المستخدم بها `compose:rctrl` فيصير Right Ctrl = `Multi_key`. ضغط الـ hotkey يفتح تأليف Compose معلّقاً في طبقة إدخال Ptyxis (GTK4+VTE)، فيُكمِله أوّل النص المحقون (`Multi_key`+s+o → §). الجذر خارج الحقن تماماً.

نقطة جوهريّة غير بديهية: حالة Compose مملوكة لـ **GtkIMContext الخاص بالتطبيق**، لا للـ compositor. الدليل الوحيد المُثبَت لما يُلغيها هو `im_context_reset()` المُستدعى من حدث focus/pointer (النقر بالفأرة يصلحها). لا دليل أنّ keysym مُحقَن عبر zwp_virtual_keyboard_v1 (Escape/Multi_key) يكافئ هذا الإعادة-ضبط.

**Why:** الحلول المقترَحة عبر حقن «مفتاح إلغاء» بخريطة VK خاصّة تقف على هذه الفرضيّة غير المؤكَّدة. ملفّ المشكلة يوثّق أنّ warmup keys ومفاتيح uinput الاصطناعيّة **لا تُلغي** Compose — فمفتاح إلغاء عبر نفس VK يكرّر فئة فاشلة.

**How to apply:** أي حلّ لهذه المشكلة يجب أن يُثبِت آليّة الإلغاء يدويّاً (اضغط RCtrl ثم المرشّح ثم اكتب so) **قبل** كتابة أي كود. Escape يخاطر بالتسرّب المُتلِف للـ PTY (\e)؛ Multi_key المُعاد يخاطر بإعادة فتح التأليف وعودة §. كلاهما غير مقبول دون إثبات. راجِع [[vk-client-keymap-apply-race]].

تحقّق keycode: build_keymap يربط evdev i+1 → XKB i+9 (XKB=evdev+8). warmup=evdev247→XKB255. cancel evdev246→XKB254 صحيح حسابيّاً.
