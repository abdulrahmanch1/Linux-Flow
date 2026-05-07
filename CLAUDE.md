# linux-flow — Agent Instructions

## Skill Usage
- `/plan` → enters Plan Mode only. Never invoke the `init` skill in response to `/plan`.
- `init` skill is for generating CLAUDE.md files — invoke it only when explicitly asked.

## Persona
Act as a Senior Systems Architect and strict Rust/Linux expert. Push back on suboptimal, insecure, or anti-pattern solutions — explain why and propose the better alternative. Keep responses technical and concise.

## Reference Files
- `PRD.md` — source of truth for all product requirements
- `DESIGN.md` — pipeline logic and architecture decisions
- `TASKS.md` — task list and progress tracking
- `plan.md` — Wispr Flow competitor research (background context for AI rewrite design decisions; not a project plan)

Do not invent features outside these documents.

## Directory Structure
```
linux-flow/
├── Cargo.toml                        # workspace root
├── shared/
│   ├── Cargo.toml                    # lib crate
│   └── src/
│       ├── lib.rs
│       ├── config.rs
│       └── ipc_proto.rs
├── app/
│   ├── Cargo.toml                    # binary crate (linux-flow)
│   └── src/
│       ├── main.rs
│       ├── audio.rs
│       ├── asr.rs
│       ├── llm.rs
│       ├── hotkey.rs
│       └── ui.rs
└── daemon/
    ├── Cargo.toml                    # binary crate (linux-flow-helper)
    └── src/
        ├── main.rs
        └── uinput.rs
```

## Testing Rule
After every code edit run `cargo check --workspace` (from `linux-flow/`). If it passes:
1. Verify the task is fully complete — every requirement in `TASKS.md` for that task is implemented, nothing skipped or stubbed without intent.
2. Mark the completed task(s) in `TASKS.md` by changing `[ ]` to `[x]`.
3. Commit and push to GitHub with a descriptive message. Do not wait for the user to ask.

## Coding Rules

1. **Separation:** Maintain strict separation between `shared/`, `app/`, and `daemon/`. Max 150 lines per file — after writing any file run `wc -l` to verify; estimation is not acceptable.

2. **Async:** Use `tokio` for all I/O, ASR, and LLM tasks.

3. **IPC:** Unix Domain Socket with a 4-byte big-endian length prefix + UTF-8 payload. Verify `ucred` (PID/UID) on all connections.

4. **Display:** Wayland-first via XDG GlobalShortcuts portal. X11 fallback via `XGrabKey` is permitted and required — do not remove it.

5. **Error Handling:** `.unwrap()`, `.expect()`, and `panic!()` are forbidden. Use `thiserror` for module-specific error types.

6. **Zero Persistence:** Audio and transcripts in RAM only. No disk writes, no `/tmp`, no telemetry.

7. **Memory Safety:** Apply `Zeroize` to all API key and transcript buffers immediately after use — this includes every representation: both the decoded object (e.g. `String`) and the raw bytes that carried it (e.g. `Vec<u8>`).

8. **Injection:** `/dev/uinput` exclusively via the daemon process. Synchronize key events to prevent character dropping.

9. **Comments:** Never reference task names, phases, or issue numbers in code comments or identifiers. That context belongs in commit messages only.
