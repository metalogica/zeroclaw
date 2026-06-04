# CLAUDE.md — ZeroClaw (Claude Code)

> **Shared instructions live in [`AGENTS.md`](./AGENTS.md).**
> This file contains only Claude Code-specific directives.

## Claude Code Settings

Claude Code should read and follow all instructions in `AGENTS.md` at the repository root for project conventions, commands, risk tiers, workflow rules, and anti-patterns.

## Workflow gotchas (learned the hard way)

- **Never run `cargo fmt --all` (or `cargo fmt` without a path) to format your own edits.** Local rustfmt is newer than the CI-pinned toolchain (**Rust 1.93.0**, see `.github/workflows/ci-run.yml`), so a full format reformats ~96 lines of *pre-existing* drift across files you never touched (`openrouter.rs`, `multimodal.rs`, `personality.rs`, `providers/mod.rs`, `otel.rs`) and pollutes your diff. Format only the hunks you changed, or hand-write added code in canonical style. To verify your change is fmt-neutral, install the pinned toolchain (`rustup toolchain install 1.93.0 --component rustfmt`) and diff `cargo +1.93.0 fmt --all -- --check` against a `git stash`ed clean tree — equal diff counts = you added nothing.
- **The repo currently fails `cargo +1.93.0 fmt --check` on a clean checkout** (pre-existing drift, above). Don't try to "fix" it inside a feature PR — it's out of scope and balloons the diff. A standalone `cargo +1.93.0 fmt --all` commit is the place for it.
- **Adding/removing a field on a widely-constructed struct (e.g. `ChatResponse`) or changing an enum variant's shape (e.g. `StreamEvent::Final`) ripples to 60+ literal sites** across providers, tools, and tests — Rust requires every literal to list the field. Enumerate them all in one pass with `cargo build --all-targets --message-format=short` (compiles lib **+ tests + benches**); the lib-only build hides test/bench sites until the lib succeeds, causing slow discover-fix waves. Script the mechanical `field: None,` insertions (e.g. insert after the last existing field), keyed off the compiler's `missing field` error lines, then re-run until clean.
- **Clippy `-D warnings` locally surfaces ~41 lints absent in CI** (newer local toolchain: `large_futures`, `&Option<T>`, `cannot test inner items`). Prove your change is clippy-neutral the same way — `git stash` baseline vs your tree, compare the error-file sets — rather than chasing pre-existing lints.

## Hooks

_No custom hooks defined yet._

## Slash Commands

_No custom slash commands defined yet._
