# xvw — Agent Instructions

This file is the canonical project guidance. Direct user requests take
precedence over it.

## Working rules

- Inspect `git status` and the relevant files before editing.
- Preserve existing user changes and unrelated work.
- Use `apply_patch` for source and documentation edits.
- Do not use `git reset`, `git checkout --`, or broad/destructive deletion to
  discard work.
- Keep changes focused, then run the narrowest relevant checks followed by the
  full checks below.

## Project shape

`xvw` is a binary-only Rust 2024 application built with GPUI and Tokio.

- `src/main.rs`: application entry point, Tokio runtime, GPUI application setup,
  and global key bindings.
- `src/core/`: editor engine and deterministic logic (buffer, document, editor,
  history, search, diff, formatting, and Kaitai parsing).
- `src/service/`: orchestration and file/editor services.
- `src/ui/`: GPUI components, panels, panes, and workspace.
- `src/actions.rs`: action definitions; bindings may be registered by `main`,
  the workspace, or the owning component.
- `themes/`: theme data; `assets/`: embedded application assets.
- `testdata/`: repository-owned test fixtures.

Kaitai structure definitions use `.ksy` YAML. Runtime loading is exposed by
`LoadStructureDefinition` (`cmd-shift-s` on macOS, `ctrl-shift-s` elsewhere).
Supported syntax is documented in `docs/structure-definition-spec.md`.

## Commands

```text
cargo run -- [file_or_folder_path]
cargo build --release
cargo fmt --all -- --check
cargo test
cargo test --no-run
cargo clippy --all-targets
```

Use `cargo fmt --all` to apply formatting. `rustfmt.toml` is authoritative for
formatting settings. Run a focused test with a Cargo name filter when iterating,
then run the full suite before handoff.

## Implementation conventions

### GPUI

- Create entities with `cx.new` and use `cx.observe`/`cx.subscribe` for
  reactive relationships.
- Call `cx.notify()` after state mutations that affect rendering.
- Use `cx.spawn` for application tasks and `cx.spawn_in` when a task must remain
  associated with a window; use a background executor or `spawn_blocking` for
  file I/O and CPU-heavy work.
- Do not block the UI thread or hold `RwLock`/`Mutex` guards across `.await`.
- Prefer pure helpers for layout, formatting, and state transitions so they can
  be tested without starting a GPUI window.

### Rust

- Follow Rust 2024 idioms and the existing module boundaries.
- Use `Result` propagation for recoverable errors; use `expect` only when an
  invariant is established and the message explains it.
- Keep ownership and lock scope explicit. Avoid cloning or allocating in hot
  paths without a reason.
- Add `///` documentation to new public APIs and keep names idiomatic
  (`snake_case`, `CamelCase`, and `SCREAMING_SNAKE_CASE` for constants).

## Testing policy

- The default suite is deterministic unit testing inside the binary target.
- Keep small private-API tests in the implementation module; put large suites
  in a sibling test file declared with `#[cfg(test)] mod tests;`.
- Keep pure UI/layout helper tests separate from GPUI runtime tests.
- Load fixtures from `testdata/` with `include_str!`/`include_bytes!`; never use
  developer-specific absolute paths or silently skip a missing fixture.
- Do not put wall-clock performance assertions in `cargo test`. Use a separate,
  explicitly opted-in benchmark target or performance job.
- Do not add `src/lib.rs` or `tests/*.rs` solely to test this binary. A public
  integration-test surface requires an intentional library/UI split; see
  `docs/testing.md`.

## Domain gotchas

- Custom breaks, joins, and empty lines affect `Editor::line_starts`; changes
  must preserve their interactions and existing tests.
- GPUI state flow is entity/subscription based and is not React/Elm state
  management.
- Structure parsing may be long-running or cancellable; keep parsing off the UI
  thread and preserve progress/cancellation behavior.
- File paths are canonicalized by file services; account for that in tests.

## Generated and local files

Do not commit build or IDE metadata: `target/`, `.serena/`, `.vscode/`,
`.idea/`, `.gemini/`, and `.DS_Store` are ignored. Do not modify or remove
unrelated local fixtures or user changes.
