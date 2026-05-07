# Wispr Flow — Investigation Findings

## 1. Why Wispr Flow Beats Competitors

Wispr Flow's edge is not raw transcription — it's the **AI rewrite layer that sits between speech and the cursor**. Users describe output as "scarily human" — like thinking out loud rather than dictating.

### Five durable differentiators

1. **Auto-edit pipeline, not verbatim transcription.** A multi-stage cloud pipeline (Whisper-based ASR → fine-tuned Llama 3.1 on Baseten + GPT/Claude → polish) strips fillers ("um/uh/like"), false starts, and self-corrections, and reflows grammar — all under a ~700ms p99 budget. OS dictation, Dragon, MacWhisper, Aiko, and Otter all transcribe verbatim.
2. **Context-aware tone per app.** Detects active app and adapts register: formal in Gmail/Docs, casual in Slack/iMessage (strips trailing periods), code-aware in Cursor/VS Code/Windsurf (camelCase, file tagging). Competitors require manual "modes."
3. **Cross-platform parity.** Mac (Oct 2024), Windows (Mar 2025), iOS keyboard (Jun 2025), Android floating bubble via Accessibility Service (Feb 2026). Superwhisper/MacWhisper/Aiko are Mac-only; Dragon is Windows-centric and dated.
4. **Multilingual + Hinglish code-switching.** 100+ languages, auto-detect across 99, dedicated Hinglish model. Tier-1 quality in 7 languages.
5. **Zero-friction onboarding.** Hold Fn → speak → release. No model downloads, no voice training, no scripting (vs. Talon/Dragon).

### Where competitors win
Superwhisper/VoiceInk/MacWhisper run **fully local** (privacy + offline + lifetime pricing). Wispr Flow is **cloud-only with no offline mode**. The Feb 2026 viral Reddit thread exposed silent active-window screenshots, autostart re-enablement, and ~800 MB RAM / ~8% idle CPU. The CTO publicly apologized; training is now opt-in. Trustpilot sits at **2.7/5** despite glowing Product Hunt reviews — there is a real "trust gap."

---

## 2. Complete Feature Map

### Dictation Core
- Hotkey-triggered dictation (configurable, up to 4 bindings × 3 keys, mouse buttons OK)
- Push-to-talk (hold) and toggle (tap-to-start/stop) modes
- Whisper Mode — transcribes whispered/quiet speech
- System-wide cursor injection in any text field
- Cloud streaming ASR (no offline); ~96–97% accuracy on wired mic, ~88% noisy
- ~220 WPM marketed; 150–184 WPM independently verified
- ~700 ms–2 s end-to-end latency (network-dependent)
- 100+ languages, auto-detect across 99 (not on Android)
- Tier-1 quality: English, Spanish, French, German, Hindi, Italian, Portuguese, Thai
- Dedicated Hinglish (Hindi-English) code-switching model

### AI Editing ("Auto-Edits")
- Filler-word removal (um/uh/like, false starts, repetitions)
- Backtrack / self-correction detection ("actually, scratch that")
- Auto-punctuation, capitalization, smart spacing
- Paragraph & line-break detection ("new paragraph", "new line")
- List detection (numbered, bulleted, ordinal-spoken)
- Code formatting (camelCase, snake_case, CLI commands, technical jargon)
- Context-aware tone (Personalized Style) per app category
- Smart Formatting on by default
- Commonsense word substitution

### Command Mode (the "Whispers" system)
- Default shortcut: Fn+Ctrl (Mac) / Ctrl+Win+Alt (Windows)
- Free-form natural-language LLM commands (no fixed verbs)
- Built-in: "make this formal/friendlier/concise/assertive"
- "Turn into bullet points", "summarize", "expand outline into essay"
- "Translate to Spanish/Polish/…" (100+ languages)
- Web Q&A via Perplexity when no selection ("who won the Giants game?")
- Keystroke commands ("press enter")
- iPhone Shortcuts: take/dictate/quick-dictate Flow note

### Personalization
- Custom Dictionary (60-char entries, syncs across devices)
- Misspelling replacement rules
- Auto-learning from user corrections (proper-noun filtered)
- Snippets — trigger phrase → expanded text/code block
- Language-specific acronym expansions
- Personalized Style per app
- Cross-device sync (desktop ↔ iOS ↔ Android)

### Integrations
- Communication: Slack, Discord, Teams, Zoom, WhatsApp, Instagram DMs, Gmail
- Docs/Notes: Google Docs, Word, Notion, Apple Notes
- Productivity/CRM: Asana, Linear, HubSpot, Salesforce
- Developer (deep): VS Code, Cursor, Windsurf — reads editor context, file tagging in AI chat panels
- Design: Figma comments
- AI: ChatGPT
- Universal: any web form / text field across browsers

### Platforms
- macOS 12–26 (Apple Silicon + Intel; 8 GB RAM)
- Windows 10/11 x64 (8 GB RAM)
- iPhone iOS 18.3+ (system keyboard extension; Lock Screen widget; Siri Shortcuts)
- Android 13–16 (floating bubble via Accessibility Service; accelerometer required)
- Web demo (marketing only)
- Coming soon: iPad, Linux, Apple Watch
- Not supported: ChromeOS, VMs, browser extensions

### Privacy & Compliance
- Privacy Mode (audio discarded, no training, syncs across devices)
- Training opt-in, off by default
- US + EU (London) servers, fastest-region routing
- SOC 2 Type II (audited)
- ISO 27001:2022 (Enterprise)
- HIPAA + BAA on all plans (locks Privacy Mode permanently when signed)
- Encryption in transit + at rest
- Trust center: trust.wispr.ai

### Team / Enterprise
- SSO/SAML, SCIM provisioning, domain auto-invite, SSO enforcement
- Free IT admin seats (Enterprise)
- Admin console (admin.wisprflow.ai): seat management, billing, policy
- Shared team dictionary + shared snippets
- Aggregated + per-member usage analytics, export
- MDM: Kandji, Jamf, Mosyle, Intune
- macOS signed PKG with PPPC profiles; Windows WiX MSI with stable upgrade GUID
- Universal binary, auto-update via dl.wisprflow.com
- Bulk discounts, custom invoicing
- No on-prem option

### Pricing
- **Free (Basic):** 2,000 words/wk Mac/Win · 1,000/wk iPhone · unlimited Android · 14-day Pro trial, no card
- **Pro:** $15/mo or $12/mo annual (~$144/yr, 20% off) — unlimited words, Command Mode, priority support
- **Teams:** $12/user/mo (3+ seats)
- **Enterprise:** custom — adds SOC 2/ISO/HIPAA enforcement, SSO/SAML, dashboards, dedicated support
- **Students/educators/nonprofits:** ~50% off + 90-day trial

### Company
- **Wispr AI, Inc.**, San Francisco, founded 2021
- Founders: Tanay Kothari (CEO, ex-Cerebra), Sahaj Garg (CTO, ex-Luminous Computing)
- ~50 employees, ~$10M ARR
- $81M total funding (Series A $30M Menlo Ventures Jun 2025; $25M ext. led by Notable Capital Nov 2025; investors incl. NEA, 8VC, Evan Sharp, Henry Ward)
- 270+ Fortune 500 customers
- Mission: build the "Voice OS"

### Tech Stack
- ASR: Whisper-based + proprietary context-conditioned models
- Editing LLM: fine-tuned Llama 3.1 on Baseten via TensorRT-LLM (100+ tokens <250 ms) + OpenAI/Anthropic/Cerebras APIs
- Orchestration: Baseten Chains
- Infra: AWS (S3, SQS, Secrets), Supabase (auth), Redis, ClickHouse
- p99 budget: <200 ms ASR + <200 ms LLM + <200 ms network

---

## 3. How Wispr Achieves Non-English Accuracy (Technical Deep Dive)

Short answer: **they don't actually train their own multilingual ASR.** Non-English accuracy comes from a layered system splitting work across multiple third-party ASRs and a proprietary LLM correction layer.

### 3.1 Ensemble routing, not a single model
Wispr's research page (`/research/supporting-languages`) admits Whisper underperforms on Asian languages, so Flow **routes each utterance to the best ASR per language** — explicitly naming **ElevenLabs Scribe and Google Gemini** as backends alongside Whisper. Language auto-detect picks the engine. The multilingual edge is mostly *engine selection + glue*, not a custom acoustic model.

### 3.2 Proprietary layer = LLM post-correction, not ASR
The only model Wispr actually trains is a **fine-tuned Llama 3.1** served on Baseten via TensorRT-LLM (≤200 ms inference, ≤700 ms p99 end-to-end). It does formatting, filler removal, and "personalized LLM formatting" at token level. Wispr's CTO explicitly notes LLMs are "high recall, low precision" for style — so the LLM is constrained, not freely rewriting.

### 3.3 Context-conditioned decoding ("Voice OS")
Each transcription is conditioned on:
- the **active app** (Slack vs. Gmail vs. Cursor),
- **topic history** for the user,
- the user's **custom dictionary** (names, jargon),
- **voice embeddings** of the user.

The biasing mechanism is not disclosed, but their warning that "very large dictionaries hurt accuracy" strongly implies **shallow-fusion / logit biasing at decode** rather than just prompt-stuffing the LLM.

### 3.4 Hinglish — the one place they did real ASR work
Standout multilingual achievement. Two key choices:
- **Romanized Latin script output** ("tum kya kar rahe ho"), not Devanagari — sidesteps script-switching entirely.
- A **specialized training set** for mixed Hindi-English conversations (per press; not detailed on their own site). Almost certainly a fine-tune of a third-party ASR rather than from scratch.

### 3.5 Accent handling = n-best rescoring
"Accent confidence scoring" runs multiple transcriptions across the ensemble and picks the highest-likelihood candidate. The rescorer is not specified but is consistent with n-best reranking using the LLM or a language-model score.

### 3.6 Personalization via RL on formatting
A **local RL policy** is trained per-user to align formatting/style with that user's edit history. This is on the formatter, not the ASR — but compounds with non-English accuracy because formatting/punctuation conventions vary heavily across languages.

### 3.7 Documented vs. inferred
- **Documented:** ensemble of third-party ASRs (ElevenLabs Scribe, Gemini, Whisper); Llama post-processor; Hinglish romanization; dictionary biasing; ≤700 ms p99; ~1B words/month scale; "more than half" WER reduction in internal tests.
- **Not documented:** per-language WER, exact biasing mechanism, which ASR backs Hinglish, diarization details.
- **No published WER benchmarks per language** — closest claim is that pushing base ASR from 3% → 2% WER "hardly made a dent" on their hard sets, telling us the bottleneck is *contextual* errors (names, jargon, code-switching), not acoustic accuracy.

### 3.8 The bigger insight
Wispr's "exceptional non-English accuracy" is largely a **systems-engineering win, not an ML breakthrough**: pick the best ASR per language, bias it with user context, post-correct with a tightly-constrained LLM, and route everything in under 700 ms. Competitors like Superwhisper/MacWhisper run a single local Whisper model and lose on every axis — engine choice, context biasing, and post-correction — simultaneously.

---

## Primary Sources

- wisprflow.ai (homepage, features, pricing, use-cases, integrations)
- docs.wisprflow.ai (help center, MDM, SSO, privacy mode)
- wisprflow.ai/research/supporting-languages
- wisprflow.ai/post/technical-challenges
- wisprflow.ai/post/speech-recognition-challenges
- wisprflow.ai/post/transcription-quality
- baseten.co/resources/customers/wispr-flow/
- TechCrunch (Series A funding; iOS launch; Android launch)
- Crunchbase, PitchBook, Tracxn (company/funding)
- Trustpilot, Product Hunt, App Store, Google Play (sentiment)
- Reddit threads + Medium (Ryan Shrott "Trust Gap" critique)
- Independent reviews: Zack Proser, Willow Voice, tldv, Voibe, eesel AI, ClickUp, LumeVoice
