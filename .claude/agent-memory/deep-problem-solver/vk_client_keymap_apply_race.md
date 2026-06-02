---
name: vk-client-keymap-apply-race
description: roundtrip على Vk queue يؤكّد معالجة الـ compositor فقط، لا تطبيق العميل للخريطة — مصدر سباق متكرّر
metadata:
  type: project
---

في wayland_vk.rs: عند رفع keymap جديد عبر zwp_virtual_keyboard_v1، الـ `queue.roundtrip` يؤكّد أنّ **الـ compositor** عالج الأمر، **لا** أنّ التطبيق المُركَّز (عميل Wayland منفصل) طبّق الخريطة. التطبيق يستقبل الخريطة لا تزامنيّاً. لهذا وُجِد `settle_ms` (sleep حقيقي) + warmup key قبل أوّل حرف حقيقي — وإلا يُحلّ أوّل حرف ضدّ الخريطة القديمة (sudo → §). موثّق في vk_util.rs تعليق detect_gap والسطور 19-24.

**Why:** أي كود جديد يحقن مفتاحاً عبر VK ويعتمد على roundtrip وحده لضمان أنّ التطبيق رآه بالخريطة الصحيحة سيفشل صامتاً تحت سباق. المسار ASCII (type_base) لا يرفع keymap على المسار الحارّ ويعتمد base_keymap المرفوع عند البدء.

**How to apply:** أي مفتاح اصطناعي يُحقَن قبل النص (إلغاء/إحماء) يجب أن يأتي **بعد** warmup+settle لا قبله، وإلا يُحلّ ضدّ خريطة قديمة أو يُبتلَع كـ «أوّل-مفتاح-مُسقَط» الذي يصفه تعليق warmup_and_settle. هذا قيد توقيت يصطدم بقاعدة الكمون CLAUDE.md #12. راجِع [[ptyxis-compose-injection]].
