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

**FD ledger — ALL rows landed + bijection green (post zc-hnah squash @`05ce4c459`):** FD-00,01,02,03,04,05,06,07,08,11,12 — 11 rows ⇄ 11 trailered theme commits. (FD-08 authored during the squash; FD-12 = zc-ju48 OTel OnceLock, upstreaming-candidate.) No deferred rows remain.

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

> **SESSION UPDATE 2026-07-10b — zc-jfun + zc-hnah DONE; sovereign tip now `core/v0.8.0` @ `05ce4c459`.**
> - **zc-jfun (FD-07, Laminar re-home)** implemented by a bead-implementer worktree runner (3 plain commits), Adj A + Adj B satisfied, ff-merged; union re-gate @`d1db0c8fd` green (build + otel-feature nextest 2148 + clippy; runner also ran workspace nextest 8585). FD-07 + FD-12 (zc-ju48) rows landed. Runner notes: `deployment.environment` sourced from env (v0.8.0 config lacks the field — zc-t0ii follow-up); a stack-overflow regression on the oversized orchestrator loop future was fixed with `Box::pin`.
> - **zc-hnah squash-by-theme** rewrote `refs/heads/upstream(5fc9d3c38)..core/v0.8.0` → **11 dependency-ordered theme commits**, each atomic with its ledger row + `Fork-Delta: FD-NN` trailer. Per Fable's amended verification: code-tree assert (`git diff old..new -- ':!docs/FORK_DELTA.md'`) EMPTY, ledger set-equality PASS, bijection dry-run PASS (11⇄11), belt build 1.52s. Rollback tag `archive/pre-hnah-squash` = `e9201a7c1`.
> - **FD-08 row** (openrouter `user`/`[AUDIO:]` + provider verdicts, zc-ul1f/zc-qskr) was found MISSING (deferred at wave time) and authored+landed during the squash so the bijection holds — the sole ledger row-set addition.
> - **Empty-diff assertion amendment** (code-tree-identical + ledger-set-equality) approved by Fable in-thread; recorded in `.substrate/execution-state.json` deviations as the audit trail.
> - Wave 6 ran next (see below). Signing stays disabled until epic close.

> **SESSION UPDATE 2026-07-10c — Wave 6 DONE (file-disjoint fleet); sovereign tip `core/v0.8.0` @ `315220221`.**
> Two parallel windows off `05ce4c459`, merged linearly via cherry-pick (NO merge commits — a merge commit in `upstream..HEAD` carries no trailer and breaks the bijection):
> - **zc-vja9** (w6a) — `dev/hotswap/verify-coldstart.sh` (358-line cold-start parity harness), `bash -n` + shellcheck clean, rides `Fork-Delta: FD-04` (no new row). Live run is OUT-OF-BAND — batched into zc-b78l. `@f33832f3d`.
> - **zc-4leb** (w6b) — `.github/workflows/fork-delta-check.yml` bijection CI (FD-09), `refs/heads/upstream` explicit + pinned-base materialization on CI. `@c23ab7934`.
> - **zc-p7tx** (w6b) — `.github/workflows/conflict-canary.yml` weekly informational merge-tree walk (FD-10), always-exit-0. Corrected the `git merge-tree --write-tree` base to the git-2.47 `--merge-base=` flag form. `@315220221`.
> - **Final bijection GREEN** over `refs/heads/upstream..HEAD`: 14 commits ⇄ 13 rows (FD-00…FD-12; FD-04 carries 2 commits — theme + coldstart, allowed by criterion ii). Wave-6 delta is bash/yaml/docs only → no cargo re-gate. Beads zc-vja9/zc-4leb/zc-p7tx closed.
> - **REMAINING TAIL:** `zc-b78l` (manual live Laminar battery — batch with the coldstart run + the zc-n6so/zc-8zdt NPM_TOKEN docker smokes) → `zc-t0ii` (reduced: archive playbook + post-exec notes + final bijection audit) → `zc-garf` (doctrine review). `zc-mk2r` (P3, shutdown drain) is an independent follow-up. Epic close: restore signing (`git config --unset commit.gpgsign`), drop `archive/pre-hnah-squash`.

> **SESSION UPDATE 2026-07-10d — zc-b78l battery run PARTIAL; root-I/O gap found + fixed; sovereign tip `core/v0.8.0` @ `b4288dc0b`.**
> User drove the live battery (record: `baseline/laminar-battery.md`). FD-07 span production PROVEN across `/api/chat`, `/ws/chat`, 42618 webhook (roots + `llm.call` land in dev ClickHouse; typed `user_id`; `session_id` absence-not-empty per Adj B; `llm.call` I/O non-empty; `exit_reason=final_answer`; 0 `sk-or-` key leaks). Two findings, both adjudicated by the user (tech-lead) this session:
> - **Root input/output EMPTY (0/5 roots)** — hard §1.3/§4.4 clause. Root cause: the zc-jfun squash wired `lmnr.span.input/output` only on `llm.call` children, never the `agent.activation` root (root I/O IS in FD-07 scope per §3.4/Step 5.2). Ruling: **FIX NOW.** New bead **`zc-gnpx`** — `stamp_root_input`/`stamp_root_output` ambient helpers (active.rs; scrub+truncate once/site) called at loop entry + every final-answer terminal across `turn`/`turn_streamed`/`run_tool_call_loop`. Landed `@b4288dc0b` (`Fork-Delta: FD-07`, 2nd commit under that row). **Gate GREEN** (workspace build+clippy; runtime nextest `--features observability-otel` 2150 passed). **Bijection GREEN** (15 commits ⇄ 13 rows; FD-04 & FD-07 each carry 2). ⚠️ **LIVE RE-VERIFY PENDING** — rebuild `:dev` from the worktree + re-run A4 (expect `agent.activation with_input=with_output=n`, `attr_in=attr_out=1`).
> - **Triggers 4 & 5 BLOCKED-manual** (multi-iter tool loop + forced `max_iterations`) — tool auto-approval not honored post-migrate. Ruling: **defer.** New cross-repo bead **`zc-zb2t`** (clawcraft config renderer emits 0.6.9 schema; needs v0.8.0 schema-v3 + provider-key fan across 13 agent aliases + tool-approval mapping). This is the prod-rollout blocker: the pod could not run out of the box until the config was hand-migrated on the mount.
> - **STILL OPEN on zc-b78l:** (1) root-I/O A4 live re-verify, (2) negative control (blank `otel_headers` → dropped), (3) credential-shaped redaction probe (mind `zc-1qoq` value-form Bearer miss). zc-b78l `blocked-by zc-gnpx`. Then → `zc-t0ii` → `zc-garf` → epic close.

> **SESSION UPDATE 2026-07-10e — battery re-run (b); root I/O PROVEN; NEW ws-root gap found + fixed; sovereign tip `core/v0.8.0` @ `8e309c9a8`.** Record: `baseline/laminar-battery.md` §"Re-run 2026-07-10 (b)".
> - **zc-gnpx root I/O — FIXED & PROVEN** on chat + webhook (`agent.activation` `with_input=with_output=2`; was 0/0). `JSONHas=0` is Laminar promoting the attrs into the typed `input`/`output` columns. **Negative control PASS** (blank `otel_headers` → 0 spans; restore → 7). `user_id`/`session_id` (Adj B)/no-key-leak all PASS.
> - **NEW GAP `zc-a1bp` — `/ws/chat` minted no activation root** (2 roots for 3 surfaces; ws emitted only upstream-native `gen_ai.*`). `ws::handle_ws_chat` called `turn_streamed` with no `start_activation`/`scope_span` — the prod ingress owner FD-07/Adj A omitted (0.6.9 fork had it). Ruling: **FIX NOW.** Ported the root mint (`start_activation(WebChat, session_key)` + `tag_channel("web")`/`tag_user_id` + `scope_span`) → `@8e309c9a8` (`Fork-Delta: FD-07`, 3rd commit under the row; FD-07 ledger row corrected to include ws). Gate green (build + clippy + gateway ws nextest 40). **Bijection GREEN: 16 commits ⇄ 13 rows** (FD-04 ×2, FD-07 ×3).
> - **Redaction — conditional-pass, FD-07 faithful:** bare `sk-live-…` + `Bearer` value-form both raw. `scrub_credentials` is key-value-only (bare = out-of-scope by design; the `token:` form redacts live in the FD-07 mirror; Bearer value-form = `zc-1qoq`, bare-prefix note appended). Ported verbatim from 0.6.9 — not a regression. Ruling: **re-probe** `token:`-shaped next pass to positively confirm.
> - **REMAINING (one live pass → GREEN):** rebuild `:dev` from the worktree `@8e309c9a8`, then (1) drive `/ws/chat` → confirm an `agent.activation` root (typed `session_id`, non-empty root I/O, `llm.call` children); (2) `token: sk-live-…` redaction positive-confirm. Then flip `baseline/laminar-battery.md` to GREEN, close **zc-b78l** (+ zc-gnpx/zc-a1bp), → `zc-t0ii` → `zc-garf` → epic close. Triggers 4&5 stay BLOCKED-manual (`zc-zb2t`).

> **SESSION UPDATE 2026-07-10f — zc-b78l battery GREEN (FD-07 scope); zc-b78l/zc-gnpx/zc-a1bp CLOSED; sovereign tip `core/v0.8.0` @ `8e309c9a8` (no new code — verification only).** Record: `baseline/laminar-battery.md` §"Final pass (c)".
> - Final live pass @`8e309c9a8` verified every FD-07-scoped §4.4 clause: **3 roots / 3 surfaces** (ws no longer hollow — sid `gw_37602bf5…`, non-empty root I/O, matching `llm.call`), **root + `llm.call` I/O 3/3**, typed `user_id`, `session_id` absence-not-empty, `final_answer` stamped, **redaction fires on the mirror** (`api_key: sk-l*[REDACTED]` on root + `llm.call`), negative control drops all spans, 0 key leaks.
> - **Carve-out (not a faked pass):** triggers 4 & 5 (tool-loop `max_iterations` exit + tool-call-only `llm.call`) BLOCKED-manual behind the tool-approval gap → **folded into `zc-zb2t`'s scope** (re-verify once the clawcraft config-renderer fix lands). The only two unverified §4.4 clauses; explicitly deferred, tracked.
> - Doc-accuracy note for **zc-t0ii**: the 42618 webhook is `[channels.webhook.default]` → body `{"sender","content"}` (NOT `{"message"}`, which is `/api/chat`). Optional non-gating enhancement: give `/ws/chat` a distinct `trigger`/`surface` attr (chat & ws currently share `web_chat`/`web`, split only by `session_id`).
> - **NEXT: `zc-t0ii`** (reduced finalize — archive `migration-playbook.md`, `FORK_DELTA.md` §7 durable header, post-exec notes incl. the above + zc-mk2r + FD-07 `deployment.environment` env-source follow-up, final bijection audit) → **`zc-garf`** (doctrine review) → **epic close** (`git config --unset commit.gpgsign`; drop `archive/pre-hnah-squash`). Independent: `zc-zb2t` (prod-rollout), `zc-n6so`/`zc-8zdt` (NPM_TOKEN docker smokes), `zc-mk2r` (P3 drain). Bijection GREEN: 16 commits ⇄ 13 rows.
>
> **SESSION UPDATE 2026-07-10g — zc-t0ii CLOSED (reduced finalize); control-plane @`e93644791`; sovereign tip unchanged `core/v0.8.0` @ `8e309c9a8` (no sovereign code — docs + audit only).**
> - Archived `migration-playbook.md` → `archive/migration-playbook.md` with the ARCHIVED banner (superseded by `docs/FORK_DELTA.md` live ledger + this spec). Verified the durable playbook-§7 graduate already lives in the `docs/FORK_DELTA.md` header, and `docs/RELEASE.md` §3 reproduces spec §3.8 verbatim — so the sovereign side was **verify-only, no FD-00 commit needed** (the two branches hold disjoint doc sets; the "single FD-00 commit" the bead assumed is a control-plane docs commit here, no trailer).
> - Spec **Post-execution notes** appended: Phase-6 (Theme A) provider verdicts (reasoning-alias PORTED→FD-08; streaming/image-gen-extraction/Thinking DROPPED); Theme H (~21 README/tbd) DROP; carried follow-ups (zc-mk2r drain, FD-07 `deployment.environment` env-source, image-to-disk standalone, runtime-trace JSONL/zc-rfri, 42618 webhook body-shape doc-fix, optional ws surface-attr); and the **final bijection AUDIT — 16 commits ⇄ 13 rows GREEN** (FD-04 ×2, FD-07 ×3; all rows `private`, no transitional/upstreaming fields outstanding; linear history).
> - **NEXT: `zc-garf`** (doctrine review across the 6 doctrines; amendments routed to clawcraft per SNAPSHOT-PROVENANCE) → **epic close** (`git config --unset commit.gpgsign`; drop `archive/pre-hnah-squash`; restore signing). Independent tracks unchanged: `zc-zb2t` (prod-rollout + battery triggers 4&5 re-verify), `zc-n6so`/`zc-8zdt` (NPM_TOKEN docker smokes), `zc-mk2r` (P3 drain).
>
> **SESSION UPDATE 2026-07-10h — 🏁 EPIC CLOSED (`zc-137m`); migration critical path COMPLETE. Control-plane @`2d86c32c9`; sovereign tip `core/v0.8.0` @ `8e309c9a8`.**
> - **zc-garf DONE** (@`2d86c32c9`): doctrine review across all 6 snapshots — **compliance GREEN, no violations** (§6.5 data-blindness, resource-attr allowlist, never-`:latest` published tag, trailer bijection all PASS). **7 amendments** authored in `doctrine-amendments.md` + queued as `doctrine-amendment` beads, all routed cross-repo to clawcraft: A1 claw §5.0 [`zc-vjkd`], A2 observability §7.1 allowlist [`zc-xfh6`], A3 infra §6.9 `:latest`-in-example + trigger [`zc-vpe0`], A4 state-machine §2 startup-env [`zc-wuu7`], A5 methodology §3 string-or-map config-compat [`zc-x1xa`], A6 infra coldstart-harness gate [`zc-35uy`], A7 praxis cancel-vs-guard precedence [`zc-brho`]. (A1–A4 pre-identified by spec §3.8-4; A5–A7 review-surfaced.)
> - **Epic-close ritual done:** signing restored (`git config --unset commit.gpgsign` → falls back to global `true`, SSH key `id_ed25519_soulbound`); rollback tag `archive/pre-hnah-squash` (`e9201a7c1`) dropped. **Migration series is now the permanent sovereign base — no rollback tag remains.**
> - **Standalone open follow-ups (NOT migration blockers; survive epic close):** `zc-n6so`/`zc-8zdt` — out-of-band Docker smokes for FD-01/FD-02; **code already landed + ledgered**, smokes need `NPM_TOKEN` for the private `@soulbound-labs/praxis` npm (run out-of-band, then close). `zc-zb2t` — prod-rollout config-renderer port (carries the battery triggers 4&5 re-verify). `zc-1qoq` — `scrub_credentials` value-form Bearer gap. `zc-mk2r` — P3 exit-time telemetry drain. Plus the 7 `doctrine-amendment` beads (human-triage → clawcraft). None block the migration; all tracked independently.
> - **Bijection GREEN: 16 commits ⇄ 13 rows** (FD-04 ×2, FD-07 ×3). Spec left in `docs/tasks/ongoing/` (NOT moved to `completed/`) so the still-open follow-ups' `spec_path` references stay valid.
>
> **SESSION UPDATE 2026-07-11i — Arc-1 SHIP kickoff (post-epic finish plan): D1–D3 ruled, zc-1qoq CLOSED; sovereign tip `core/v0.8.0` @ `29011528c` (first push to origin — fork-delta-check CI GREEN on its first real run).**
> - **Rulings (user-confirmed):** D1 — `zc-zb2t` promoted **P2→P1**, explicit HARD GATE on the first prod image bump. D2 — v0.8.0→v0.8.2 catch-up rebase runs POST-ship; bead **`zc-owuk`** filed now with measured scope (upstream delta = **364 commits / 630 files**, overlapping ALL 16 hot fork files — NOT trivial; precondition: land-or-rebase the observability-epic worktrees). D3 — the 7 doctrine amendments batch as one clawcraft PR at rollout (with the §3.8 items), then re-snapshot here.
> - **zc-1qoq DONE** (@`29011528c`, rides FD-07 as 4th commit, ledger row updated atomically): `scrub_credentials` hardened to three passes — KV (+`authorization` key, JSON stray-quote fix), value-form `Bearer <token>`, bare well-known-prefix (`sk-`,`ghp_`,`xoxb-`,`AKIA`); 4-char context prefix everywhere; redacted output is a fixed point. Gates GREEN (1.93.0): build, clippy `-D warnings`, workspace nextest **8590**, otel-feature nextest **2153**, fmt-neutral. **Bijection GREEN: 17 commits ⇄ 13 rows** (FD-04 ×2, FD-07 ×4). Live Bearer/`token:` re-probe rides the zc-zb2t battery re-verify. Gate note: bare `cargo nextest run` at the workspace root covers only the root package (~900 tests) — union re-gates MUST use `--workspace` (~8.6k).
> - **⚠ Secrets/infra gaps found (prod-bump prerequisites, all human-provisioned):** repo Actions **secrets AND variables are EMPTY** — no `PRAXIS_PACKAGES_READ_PAT`, none of the 4 `GCP_*` WIF vars (and the WIF pool trust extension = a clawcraft Terraform change, likely unfiled — longest-lead item); `NPM_TOKEN` unset locally (blocks zc-n6so/zc-8zdt smokes). 1Password-brokered commit signing needs the app unlocked (one commit attempt failed until the user unlocked).
> - **NEXT (Arc-1 Phase 1.2+, all outside this repo or user-gated):** `zc-zb2t` [clawcraft] renderer schema-v3 port → §3.8 prereqs [clawcraft; workspace-pin render FIRST] → `zc-n6so`/`zc-8zdt` smokes [needs NPM_TOKEN] → dev exit gate (battery 5/5) → prod cutover per RELEASE.md. Arc-2 (version-control-as-code standard + zc-owuk as its proof) goes to `/substrate:architect-spec` after ship.
> - **REPO TRANSFERRED metalogica → soulbound-labs (2026-07-11, same day):** now `soulbound-labs/zeroclaw` (fork lineage to zeroclaw-labs preserved; git redirects live; local origins re-pointed). **`PRAXIS_PACKAGES_READ_PAT` retired** @`26dff3879` (FD-11 2nd commit): same-org → ambient `GITHUB_TOKEN` `packages: read`; one-time grant instead = praxis package → Manage Actions access → add `zeroclaw`. Bijection GREEN: **18 ⇄ 13** (FD-04 ×2, FD-07 ×4, FD-11 ×2); fork-delta-check green on the new org. WIF prerequisites unchanged in shape, repo string now `soulbound-labs/zeroclaw`: clawcraft Terraform (github-actions.tf) needs (i) provider `attribute_condition` widened to include it, (ii) new least-privilege SA `zeroclaw-image-pusher` + `workloadIdentityUser` principalSet + `artifactregistry.writer` on `clawcraft-images` (existing deployer SA only writes `ledger`). Repo variables to set: `GCP_WORKLOAD_IDENTITY_PROVIDER=projects/660322167379/locations/global/workloadIdentityPools/github-actions-pool/providers/github-actions-provider`, `GCP_DEPLOY_SA_EMAIL=<new SA>@clawcraft-489901.iam.gserviceaccount.com`, `GCP_AR_REGISTRY=northamerica-northeast1-docker.pkg.dev`, `GCP_AR_REPOSITORY=clawcraft-489901/clawcraft-images`.

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
