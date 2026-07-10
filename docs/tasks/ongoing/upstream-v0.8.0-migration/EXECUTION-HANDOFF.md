# Upstream v0.8.0 migration — execution handoff (paused)

Paused mid-run via `/substrate:orchestrate` (hybrid mode). This captures live state so a fresh
session can resume cold. Authoritative machine state: `.substrate/execution-state.json`
(run-id `upstream-v0.8.0-migration-20260709-1622`).

## Branch topology (all created, durable)

- `archive/0.6.9-alpha-p10.7` → tag @ `9970e5536` (old single-crate fork tip).
- `upstream` (local branch) == `v0.8.0` tag (`5fc9d3c38`) — the rebase anchor (ff-only advance).
- **`core/v0.8.0`** — the sovereign integration branch (off the `v0.8.0` tag). **All product work lands here.** Tip at pause: `b2ba48608`.
- `docs/v0.8.0-migration-handoff` — the **control-plane** branch (current checkout). Holds `.tbd/` tracker, the spec, `substrate.yaml`, `docs/scripts/`, baselines, this handoff. **Never switch this checkout to core/v0.8.0** (would lose the tracker).
- Integration worktree kept warm at `/Users/reinova/code/forks/zeroclaw-worktrees/core-v0.8.0` (has a warm `target/` cache — reuse on resume).

Upstream advanced to **v0.8.1 / v0.8.2** during the run (continuation STOP-check negative — 0 NextAction envelopes; base held at v0.8.0 per the locked spec decision). A future rebase-cadence pass may retarget.

## Infrastructure installed (this fork wasn't orchestrate-ready)

- `substrate.yaml` — gate pinned to **Rust 1.93.0** (`RUSTUP_TOOLCHAIN`), `compile`=`cargo build --all-targets --locked`, `test`=`cargo nextest run --locked` (process isolation — avoids the `content_search`/personality parallel flakes), `lint`=`cargo clippy --all-targets -D warnings`. fmt **excluded** (known-red on clean tree). `worktree-seed`=`.env`/`.secret_key`; `toolchain-pin.install` installs components + nextest.
- `docs/scripts/bead-graph.sh` (+ doctrine-lint, bead-tui) — DAG reader.
- **NOT** a full `/substrate:adopt` (repo already had custom `docs/doctrine/` + real AGENTS.md/CLAUDE.md — surgical install only). `core.hooksPath` left on `.git/hooks` (tbd hooks).
- `cargo-nextest` 0.9.140 installed globally (`~/.cargo/bin`).

## Beads done (14) — all union-re-gated green (Batch-A nextest: 8567 tests, 0 fail)

| Bead | State | Commit | FD |
|------|-------|--------|-----|
| zc-7c42 | closed | (git topology) | — |
| zc-d5i0 | closed | ba9a0e4 | FD-00 |
| zc-h1l5 | closed | 64a29a1 (baselines: fmt/clippy drift 0) | — |
| zc-m67s | closed | 64a29a1 (facts-verified.md) | — |
| zc-pauf | closed | ec8d9ea | (FD-03) |
| zc-2sw0 | closed | eedfb7e | (FD-04) |
| zc-nvqs | closed | ca7d428 | FD-06 |
| zc-yz6h | closed | b922a63 | (FD-07) |
| zc-ul1f | closed | 2e30b15 | (FD-08) |
| zc-rfri | closed | 0896b6b (KEYSTONE continuation) | (FD-03) |
| zc-qwhe | closed | f01b964 | (FD-04/05) |
| zc-qskr | closed | 7d00334 | (FD-08) |
| zc-1g6p | closed | e7a0944 | FD-11 |
| **zc-n6so** | **open** | 81bbc3a | FD-01 |
| **zc-8zdt** | **open** | 688424 | FD-02 |

**FD ledger landed:** FD-00, 01, 02, 06, 11. **Deferred** (land at theme-completion beads): FD-03 (zc-n1mx), FD-04/05 (zc-e6t0), FD-07 (zc-jfun), FD-08 (finalization / zc-t0ii).

## Out-of-band (merged but OPEN — need `NPM_TOKEN` for the private praxis npm package)

- `zc-n6so` — Dockerfile release smoke: `docker build --secret id=npm_token,env=NPM_TOKEN --target release -t clawcraft-claw-runtime:spec-smoke .` then the §Step-2.1 smoke.
- `zc-8zdt` — `RESET_VOLUMES=1 just claw-hotswap` (or `docker build --target release … -t clawcraft-claw-runtime:dev .`) then `docker image inspect`.

Close these once the smokes pass (two-stage gate: merge already unblocked dependents).

## THE BIG REMAINING PIECE — zc-jfun (Laminar / FD-07): scope gap

v0.8.0 **deleted the span-*producing* half** of observability. zc-jfun is a subsystem re-home, not thin wiring. Runner `a5437e9ec184969ac` gave this 4-step plan (write-scope must span 3+ crates):

1. **`crates/zeroclaw-api/src/observability_traits.rs`** — re-add `Observer::start_activation(&self, Trigger, Option<&str>) -> Box<dyn Span>` with a `NoopSpan` default (port archive `traits.rs:295`).
2. **`crates/zeroclaw-runtime/src/observability/otel.rs`** — port `OtelSpan` (root/child, `lmnr.association.properties.session_id`, per-span `deployment.environment` stamp), override `start_activation`, and **restore the `OTEL_PROVIDERS` `OnceLock` + `shutdown_shared_providers`** so the **6** observer construction sites share ONE exporter (currently they race the OTel globals — a real bug). Sites: gateway `lib.rs:1417`/`sse.rs:531`, channels `orchestrator/mod.rs:8153`, runtime `agent.rs:1141`/`loop_.rs:3378`/`loop_.rs:4829`/`daemon/mod.rs:647`.
3. **`crates/zeroclaw-runtime/src/rpc/turn.rs`** (`execute_turn`, ~L49/69) — mint the activation root via `observer.start_activation(...)` and wrap the turn in `observability::scope_span(root, …)`. (Plus any CLI/webhook owners that bypass `execute_turn`.)
4. **`crates/zeroclaw-runtime/src/agent/agent.rs`** + a new in-scope `observability/` helper for `llm_call_input_delta`/`llm_call_output_summary` — then llm.call attachment + `lmnr.span.input/output` mirrors + `stamp_turn_exit` become live. LLM-call sites: `turn` agent.rs:2593–2665/2668+/2789; `turn_streamed` agent.rs:3025/3253–3396/3417+/3541/3800.

Headers are ALREADY applied on both exporters upstream (verified) — do NOT re-wire `.with_headers()`. zc-jfun **blocks** zc-b78l (live battery — manual gate), zc-4leb, zc-p7tx (the latter two edges are likely conservative — CI-workflow beads don't consume Laminar code).

## GATE DECISIONS (2026-07-09, tech-lead review — supersedes the zc-jfun "blocked" state below)

1. **zc-jfun = option (a): re-scope + execute the 4-step plan.** Laminar emission is a §7
   integration-contract item (prod Laminar Cloud live since 2026-06-02); deferring doesn't take it
   off the release critical path — it just ships identity/active (zc-yz6h, merged) as inert dead
   code and moves first verification to prod. Recon is done (attach points line-pinned), so the
   residual is execution, not discovery. Bead re-opened with expanded write-scope (recorded in the
   bead notes — not silent). Runs as its **own window after Wave 5** (agent.rs adjacency vs
   zc-n1mx). Conservative edges **dropped**: zc-4leb/zc-p7tx no longer wait on it (they consume no
   Laminar code); zc-b78l edge retained — the manual battery stays gated on real span production.
2. **OTel-globals race = new P1 bug bead `zc-ju48`** (window-5), extracted from zc-jfun step 2 so
   the fix lands even if the Laminar window stalls again. Sequenced **before** zc-jfun
   (`zc-jfun blocked-by zc-ju48` — both edit otel.rs; no co-edited files in one wave). Does NOT
   gate Wave 5 (zc-n1mx/zc-e6t0 are verify+document+ledger; file-disjoint) — dispatchable in
   parallel with it. `upstreaming` candidate: the race exists at v0.8.0 upstream; PR it after
   landing (ledger row stays `private` until a PR URL exists).

Resume order becomes: **Wave 5** (zc-n1mx, zc-e6t0) ∥ **zc-ju48** → **zc-jfun window** →
Wave 6 (zc-vja9, zc-4leb, zc-p7tx) → zc-b78l (manual) → zc-t0ii → zc-garf.

## Remaining waves (resume order)

- **Wave 5 — UNBLOCKED (do first on resume):**
  - `zc-n1mx` (Step 3.3) — wire `process_message`/subagent continuation coverage (fact: `process_message` delegates to shared `agent_turn`, so Step 3.2's wiring likely already covers it — verify + document), then **squash Theme B as `Fork-Delta: FD-03`** + ledger row.
  - `zc-e6t0` — record drops (SYSTEM_DIR, c276ffe6a, webhook-supervision pin) + **land FD-04/05/06 rows** (FD-06 already landed; add FD-04/05).
- **Wave 6:** `zc-vja9` (coldstart harness `dev/hotswap/verify-coldstart.sh`, needs zc-e6t0), `zc-4leb` (fork-delta-check.yml — trailer↔ledger bijection CI, FD-09), `zc-p7tx` (conflict-canary.yml, FD-10). NOTE: run `zc-4leb`'s bijection check locally against `upstream..core/v0.8.0` — every commit needs a `Fork-Delta:` trailer OR the check must scope to squashed theme-commits. **Current per-bead commits on core/v0.8.0 are NOT squashed-by-theme yet** — the finalization (zc-t0ii) or a pre-4leb squash pass must reconcile this so the bijection holds.
- **Wave 7:** `zc-b78l` — live Laminar battery (MANUAL gate, needs clawcraft dev ClickHouse; blocked on zc-jfun).
- **Wave 8:** `zc-t0ii` — archive `migration-playbook.md`, finalize ledger, **squash-by-theme the core/v0.8.0 series** (each theme → one commit + `Fork-Delta:` trailer, so the bijection CI passes), post-execution notes (drops: Themes H, SYSTEM_DIR, c276ffe6a, Phase-6 verdicts).
- **Wave 9:** `zc-garf` — doctrine review across the 6 doctrines; queue amendments as beads (routed to clawcraft per SNAPSHOT-PROVENANCE.md).

## Resume protocol

1. `git config commit.gpgsign false` (re-disable for fleet worktree commits; restore at true/unset at end).
2. Confirm integration worktree at `…/zeroclaw-worktrees/core-v0.8.0` (branch `core/v0.8.0` @ `b2ba48608`); recreate with `git worktree add` if gone.
3. Read `.substrate/execution-state.json` for the full outcome/re-gate ledger.
4. Group-runner protocol used: implement per inlined spec-step + env-resolved gate (nextest, 1.93.0); **commit plainly — NO `Fork-Delta:` trailers, NO `FORK_DELTA.md` edits** (orchestrator lands ledger rows single-writer, and squashes-by-theme at finalization to avoid parallel-append conflicts). Resolve `-p zeroclawlabs` test gates to the owning crate (`-p zeroclaw-runtime` etc.).
5. Per wave: dispatch file-disjoint windows → merge on green → union re-gate the integrated tip (`cargo build --all-targets` + `cargo nextest run --workspace` + clippy + fmt-vs-baseline) → record into execution-state.

## Follow-ups surfaced during the run (for zc-t0ii post-execution notes / new beads)

- OpenRouter **image-to-disk** persistence: upstream removed the substrate; if the fork still wants it, file a standalone feature bead (per zc-qskr — not thin-portable).
- Continuation trace events route via `zeroclaw_log::record!` (no v0.8.0 `runtime_trace` JSONL writer) — possible follow-up (per zc-rfri).
- The **single-observer `OnceLock` regression** (6 racing observer sites) is real and independent of Laminar — worth its own bug bead even if zc-jfun is deferred.
- `apps/zerocode` needs a workspace-member stub in the Dockerfile builder (per zc-n6so) — consider removing it from `Cargo.toml` members (separate bead).
