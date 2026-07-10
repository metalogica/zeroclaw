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

> **SESSION UPDATE 2026-07-09 (pause @ `core/v0.8.0` `ac3b2df27`, 33 off v0.8.0):** Wave 5 + zc-ju48 are **DONE** (union re-gates green; per-bead detail in `.substrate/execution-state.json`). **zc-jfun is now UNBLOCKED and is the next task on resume.** Paused at user checkpoint before the Laminar re-home. zc-n1mx's continuation guard was wired into the shared `run_tool_call_loop` (covers process_message/`/api/chat`/webhook/channels/CLI — verified no double-drive vs agent.rs `turn*`). New follow-up bead **zc-mk2r** (P3): wire `shutdown_shared_providers` into exit-time drain.

## GATE DECISIONS 2026-07-10 (Fable, tech-lead — resume rulings; supersede open questions)

**Decision 1 — zc-jfun = GO now**, single fleet worktree, 4-step plan confirmed, FD-07 = **private**, with two scope adjustments (both recorded in the zc-jfun bead notes):
- **Adjustment A (step 3 was under-scoped — the §7 root-span risk / FMEA #4):** `execute_turn` is the ACP/rpc path ONLY. Prod front doors (POST `/api/chat`, 42618 webhook channel, channels orchestrator, CLI run) delegate through the shared `run_tool_call_loop` / `process_message`, NOT `execute_turn`. Step 3 MUST mint activation roots at those ingress owners (gateway `run_gateway_chat_with_tools`, webhook, channels orchestrator, CLI) **alongside** `execute_turn`; step 4 mirrors + `stamp_turn_exit` MUST land at BOTH `agent.rs` turn/turn_streamed AND `loop_.rs` `run_tool_call_loop` sites — else prod-relay traces are hollow (typed `session_id`/`user_id`/turn-outcome silently empty in prod).
- **Adjustment B (port guards exactly; do NOT synthesize):** `session_id` absence-not-empty (None on `/api/chat` per claw §4.1); `user_id` only when `CLAW_USER_ID` passes the 32-char gate. Port the 0.6.9 guards verbatim. Carry per-span `deployment.environment`, scrub+truncate once per site, tool-call-only `name(args)` summaries.
- **Gate change (zc-ju48 lesson):** bead gate + union re-gate MUST add `cargo nextest run -p zeroclaw-runtime --features observability-otel` (default `gate.test` under-covers feature-gated otel).

**Decision 2 — bijection = (a)-variant:** squash-by-theme is a dedicated **orchestrator** step, **bead `zc-hnah`**, run immediately after zc-jfun merges, **BEFORE Wave 6** — pulled OUT of zc-t0ii. Rewrite `refs/heads/upstream..core/v0.8.0` → ~12 trailered theme commits; assert `git diff old..new` empty; refresh worktree. `zc-ju48` kept as its own commit trailered `Fork-Delta: FD-12` (new PRIVATE row, upstreaming-candidate). **MUST use `refs/heads/upstream` explicitly** (ambiguous refname — bites the workflow too). `zc-hnah` blocks `zc-4leb`; `zc-t0ii` reduced to archive + post-exec notes + final bijection **audit** (no history rewrite).

**Decision 3 — zc-jfun trailer:** runner stays **plain-commit + defer**. Orchestrator lands the FD-07 row (private) at merge; the `zc-hnah` squash materializes the trailer commit minutes later. One protocol for all runners, one history rewrite total.

> **RESUME SEQUENCE (Fable):** zc-jfun window → merge + union re-gate (incl. `--features observability-otel`) + land FD-07 row (private) + FD-12 row (zc-ju48) → **zc-hnah squash-by-theme** → Wave 6 (zc-vja9, zc-4leb, zc-p7tx; post-squash commits carry trailers at commit time) → zc-b78l manual battery (batch with the zc-n6so/zc-8zdt NPM_TOKEN docker smokes) → zc-t0ii (reduced) → zc-garf doctrine review.

## Remaining waves (resume order)

- **Wave 5 — ✅ DONE:** `zc-ju48` (OTel OnceLock, @`c5977885b`), `zc-n1mx` (continuation on `run_tool_call_loop`, FD-03 row, @`ea310963f`+`ace744dc7`), `zc-e6t0` (FD-04/05 rows + drop records, @`ac3b2df27`). Trailer-squash for all three themes deferred to zc-t0ii.
- **NEXT — zc-jfun window (Laminar / FD-07):** the 4-step re-home (§"THE BIG REMAINING PIECE"), now unblocked (zc-ju48 closed → shared-provider OnceLock in place). Own window; touches zeroclaw-api + otel.rs + rpc/turn.rs + agent.rs. Do NOT re-wire `.with_headers()` (already applied upstream).
- **Pre-Wave-6 — `zc-hnah` squash-by-theme (orchestrator, single-writer):** runs the moment zc-jfun merges. Rewrite `refs/heads/upstream..core/v0.8.0` → ~12 trailered theme commits (zc-ju48 → own FD-12 private commit); assert `git diff old..new` empty; refresh worktree. This is where the bijection gets reconciled — NOT zc-t0ii (Fable Decision 2).
- **Wave 6:** `zc-vja9` (coldstart harness `dev/hotswap/verify-coldstart.sh`, needs zc-e6t0; commit carries `Fork-Delta: FD-04`), `zc-4leb` (fork-delta-check.yml — trailer↔ledger bijection CI, FD-09; validate GREEN against the **squashed** tree; use `refs/heads/upstream` explicitly), `zc-p7tx` (conflict-canary.yml, FD-10). All post-squash commits carry their trailers at commit time.
- **Wave 7:** `zc-b78l` — live Laminar battery (MANUAL gate, needs clawcraft dev ClickHouse; blocked on zc-jfun).
- **Wave 8:** `zc-t0ii` (REDUCED — squash moved to zc-hnah) — archive `migration-playbook.md`, finalize ledger, final bijection **audit** (no rewrite), post-execution notes (drops: Themes H, SYSTEM_DIR, c276ffe6a, Phase-6 verdicts; + zc-mk2r/image-to-disk/runtime-trace follow-ups).
- **Wave 9:** `zc-garf` — doctrine review across the 6 doctrines; queue amendments as beads (routed to clawcraft per SNAPSHOT-PROVENANCE.md).

## Resume protocol

1. `git config commit.gpgsign false` (re-disable for fleet worktree commits; restore at true/unset at end).
2. Confirm integration worktree at `…/zeroclaw-worktrees/core-v0.8.0` (branch `core/v0.8.0` @ `ac3b2df27` as of the 2026-07-09 pause); recreate with `git worktree add` if gone.
3. Read `.substrate/execution-state.json` for the full outcome/re-gate ledger.
4. Group-runner protocol used: implement per inlined spec-step + env-resolved gate (nextest, 1.93.0); **commit plainly — NO `Fork-Delta:` trailers, NO `FORK_DELTA.md` edits** (orchestrator lands ledger rows single-writer, and squashes-by-theme at finalization to avoid parallel-append conflicts). Resolve `-p zeroclawlabs` test gates to the owning crate (`-p zeroclaw-runtime` etc.).
5. Per wave: dispatch file-disjoint windows → merge on green → union re-gate the integrated tip (`cargo build --all-targets` + `cargo nextest run --workspace` + clippy + fmt-vs-baseline) → record into execution-state.

## Follow-ups surfaced during the run (for zc-t0ii post-execution notes / new beads)

- OpenRouter **image-to-disk** persistence: upstream removed the substrate; if the fork still wants it, file a standalone feature bead (per zc-qskr — not thin-portable).
- Continuation trace events route via `zeroclaw_log::record!` (no v0.8.0 `runtime_trace` JSONL writer) — possible follow-up (per zc-rfri).
- The **single-observer `OnceLock` regression** (6 racing observer sites) is real and independent of Laminar — worth its own bug bead even if zc-jfun is deferred.
- `apps/zerocode` needs a workspace-member stub in the Dockerfile builder (per zc-n6so) — consider removing it from `Cargo.toml` members (separate bead).
