# Upstream v0.8.0 Migration & Fork-Sync System Brief

**Author**: rei nova
**Date**: 2026-07-09
**Status**: Draft

---

## User Story

As the maintainer of the sovereign `zeroclaw` fork that clawcraft runs on,
I want to re-home our 84-commit fork delta onto upstream's v0.8.0 multi-crate workspace **and**
stand up a durable upstream-sync + dev→prod release system,
so that clawcraft gets upstream's latest without breaking prod, and we never again accumulate a
two-minor-version, 84-commit rebase backlog.

---

## Context (read these first — everything is local, no web calls)

- `research/fork-delta-commits.md` — the complete raw 84-commit ledger (`main..HEAD`), the
  **primary source** for re-deriving the change clustering + per-theme disposition.
- `research/upstream-v0.8.0-facts.md` — verified facts about the v0.8.0 target (the 15-crate
  layout, module destinations, and the RESOLVED open questions Q1–Q5).
- `research/clawcraft-integration-contract.md` — the hard "don't break prod" constraints.
- `migration-playbook.md` — **prior analysis, cross-check only** — re-derive, don't trust.
- Binding doctrines snapshotted in `docs/doctrine/` (praxis §6.5, observability, claw, infra,
  methodology, claw-state-machine). See `docs/doctrine/SNAPSHOT-PROVENANCE.md`.

The gating fact: v0.8.0 split the single `src/` tree into a Cargo workspace of 15 `zeroclaw-*`
crates, so cherry-pick/rebase across the path moves is intractable. Our durable value is a small
surface — praxis NextAction continuation auto-drive, a thin Laminar observability layer, and the
hotswap dev loop; most of the 84 commits are now-upstream or noise.

---

## Constraints

- **MUST** anchor the migration on the **`v0.8.0` git tag** (fully local, every path
  verifiable). Design the standing sync system to absorb future releases; a
  `git fetch upstream --tags` at execution time may reveal newer tags but the target is v0.8.0.
- **MUST** preserve the praxis **NextAction `{data,next_action}` contract exactly**
  (praxis-doctrine §6.5): `next_action:null` = unconditional turn-end, runtime never inspects
  `data` on null; PARK = `null` + `data.parked`. Theme B (`continuation.rs` + turn/turn_streamed
  guard) is the keystone deliverable, re-homed onto v0.8.0's `execute_turn`/`loop_.rs`.
- **MUST** keep the dev **hotswap** loop working against the new multi-crate build — land it
  first so every later step is locally testable; it must still yield a runnable
  `clawcraft-claw-runtime:dev`.
- **MUST NOT** break clawcraft prod: the pod API (`/api/chat`, brain.db, workspace, tool policy),
  the SHA-pinned `CLAW_DOCKER_IMAGE` → GCP Artifact Registry flow, the `Dockerfile
  ARG PRAXIS_VERSION` praxis bundling, and the `config.toml [observability]` Laminar
  config-carrier (no `OTEL_*` env). See the §7 checklist in the integration contract.
- **MUST** land each theme as its own reviewable commit/PR behind a green gate
  (`cargo build --all-targets --message-format=short`, then the pinned
  `cargo +1.93.0 fmt --check` / clippy neutrality per CLAUDE.md).
- **MUST** re-derive the commit clustering and disposition from the raw ledger + verified facts;
  treat `migration-playbook.md` as cross-check only and flag contradictions.
- **SHOULD** be upstream-first: for generic features already covered or coverable upstream
  (generic OTel/gen_ai, OpenRouter streaming), **drop** our version rather than re-home it, to
  shrink the fork delta. Keep only genuinely-private differentiators.
- **SHOULD** express the migration as a sequence that lets high-value/low-risk themes (C, then B,
  then A-salvage) land before high-rework/low-residual themes (E, G).

---

## References

- Doctrines: `docs/doctrine/architecture/praxis-doctrine.md` (§6.5),
  `observability-doctrine.md`, `claw-doctrine.md`, `infra/infra-doctrine.md`,
  `methodology-doctrine.md`, `claw-system-state-machine-doctrine.md`.
- SDD protocol: `docs/protocol/sdd/{_SPEC-STANDARD.md,execution-format.md}`.
- Research pack: `research/*.md` (this folder).

---

## Acceptance Criteria

- [ ] An `upstream`/mirror branch tracks the v0.8.0 tag; the fork's durable delta is re-landed on
      the sovereign/core branch clawcraft SHA-pins in prod.
- [ ] `cargo build --all-targets` is **green** on the v0.8.0 multi-crate tree with our delta applied.
- [ ] The praxis NextAction continuation auto-drive works on v0.8.0's turn, verified against the
      §6.5 contract (null turn-end, PARK shape, verifier-failure continuity).
- [ ] `hotswap.sh` produces a runnable `clawcraft-claw-runtime:dev` from the new build.
- [ ] The thin Laminar layer (`lmnr.span.input/output`, `session_id`/`user_id` association
      properties, turn-outcome) emits onto upstream's logging pipeline via `config.toml`.
- [ ] A **`docs/FORK_DELTA.md` ledger** exists: every intentional divergence in one table
      (commit/bead, target crate, upstream-or-private disposition, rationale).
- [ ] A **conflict-canary CI job** exists (scheduled trial rebase/merge-tree → conflict count).
- [ ] A documented **dev→prod release loop** into clawcraft exists (mirror branch → SHA-tagged
      image → `CLAW_DOCKER_IMAGE` bump), fitting infra-doctrine (tag-driven, never `:latest`).
- [ ] A **rebase cadence** is documented (fetch weekly; rebase the private series onto
      `upstream/main` at least every minor release) so delta never exceeds ~1 release of drift.

---

## Out of Scope

- Re-homing the ~25 README/tbd/tooling-noise commits (Theme H) — DROP; regenerate fresh.
- Generic OTel/gen_ai instrumentation now covered by upstream `zeroclaw-log` — DROP.
- OpenRouter SSE-streaming/reasoning-alias work if redundant with v0.8.0's provider — verify then DROP.
- Robot-hardware crates (`aardvark-sys`, `robot-kit`) and the `apps/{tauri,zerocode}` surface.
- Refreshing the doctrine snapshots against clawcraft (note it in the standing playbook instead).

---

## Open Questions (for the architect to resolve during spec authoring)

1. **Branch model.** Confirm the exact two-branch shape: a pure mirror (`upstream`/`vendor`)
   tracking the v0.8.0 tag + a sovereign prod branch that carries the thin private series. How
   does the private series get expressed — a rebasable topic-branch stack, or squashed-by-theme
   commits keyed to `FORK_DELTA.md`?
2. **Theme B re-wire.** Exactly where do `continuation.rs`'s call-sites attach in v0.8.0's
   `execute_turn`/`loop_.rs`, and does any v0.8.1+ upstream continuation concept exist to build
   on instead (upstream-first check before re-adding ours)?
3. **Theme E.** Map v0.8.0's per-agent ACP session model, then decide: re-express clawcraft
   threadId continuity on it, or is it now redundant given per-agent sessions?
4. **Theme G.** Is `ZEROCLAW_SYSTEM_DIR` fully superseded by per-agent workspace boundaries
   (drop), and are the preshared-token + supervised-webhook bits still needed vs already upstream?
5. **Release automation.** How much of the dev→prod loop (mirror sync, image build, SHA bump)
   should be CI-automated now vs documented-manual, given clawcraft's Convex `CLAW_DOCKER_IMAGE` step?
