# Upstream v0.8.0 Migration & Fork-Maintenance Playbook

> ⚠️ **PRIOR ANALYSIS — CROSS-CHECK ONLY, DO NOT TRUST BLINDLY.** This is a first-pass planning
> ledger from 2026-06-16. The handover to Fable requires **re-deriving** the commit clustering
> and each per-theme disposition from the raw ledger (`research/fork-delta-commits.md` +
> `git show <sha>`) and the verified facts (`research/upstream-v0.8.0-facts.md`). Use this
> document to cross-check; **flag any place its claims contradict the verified facts.** Known
> corrections already found: v0.8.0 has **15** `zeroclaw-*` crates (this doc's §2 list missed
> `zeroclaw-spawn` and `zeroclaw-tool-call-parser`); the open questions Q1–Q5 in §6 are now
> **resolved** in `research/upstream-v0.8.0-facts.md §3`.
>
> **Status:** planning ledger — durable, iterate in place.
> **Fork tip:** `0.6.9-alpha-p10.7` (84 commits on top of `v0.6.9`).
> **Target:** upstream `metalogica/zeroclaw` **v0.8.0** (2026-06-12).
> **Owner:** rei nova. **Last updated:** 2026-06-16.

This document is the single source of truth for (1) re-homing our fork delta onto the
v0.8.0 multi-crate tree, and (2) the standing playbook that keeps us from ever
accumulating an 84-commit, two-minor-version backlog again.

---

## 0. The augmented prompt (kept for provenance)

The work below was scoped from this brief. Original intent preserved; sharpened into
acceptance criteria so it's re-runnable.

> **Original:** review commit history on tip `0.6.9-alpha-p10.7`; cluster into themes;
> per theme list explicit commits; evaluate a basic extraction mechanism; decide
> extract-vs-kill-and-rework after pulling; write to `docs/` for durable iteration.
> Goal: update repo to latest release incl. the major crate change. Follow industry
> best practice for (a) making this massive change and (b) a new playbook for rapid
> updates so we don't repeat this.

> **Augmented (acceptance criteria):**
> 1. **Inventory** — all 84 commits since `v0.6.9` clustered into ≤8 themes; each theme
>    lists its explicit commit shas, net-new files, and the modules it touches.
> 2. **Target mapping** — each theme's files mapped to their v0.8.0 crate destination
>    (the crate split is the gating fact, so this must precede any extraction call).
> 3. **Extraction evaluation** — cherry-pick / rebase-onto / format-patch+path-rewrite /
>    logical-reimplementation assessed against the path moves; pick a default and name
>    where it breaks.
> 4. **Per-theme disposition** — EXTRACT vs KILL-AND-REWORK vs DROP, each with a
>    one-line rationale tied to (a) does upstream now do this? (b) did the file move
>    crates? (c) is it our durable differentiator?
> 5. **Execution strategy** — the best-practice sequence for the big change, in
>    dependency order, with a per-step verification gate.
> 6. **Standing playbook** — concrete cadence + artifacts so fork delta stays small and
>    rebasable going forward; include an automated "conflict canary".
> 7. **Open questions** — explicitly flag every claim about upstream behavior that must
>    be verified against the v0.8.0 source before acting on it.

---

## 1. Inventory — 84 commits clustered into 8 themes

Totals: `72 files changed, +10,061 / −1,149`. Churn is concentrated:
`README.md` (20), `agent/loop_.rs` (17), `agent/agent.rs` (17), `gateway/ws.rs` (14),
`gateway/mod.rs` (12), `providers/openrouter.rs` (11), `Dockerfile` (11).

### Theme A — Connected agentic observability (OTel + Laminar)  ⟵ dominant, ~33 commits
The bulk of the fork's real engineering. Net-new files: `src/observability/active.rs`,
`src/observability/identity.rs`, `docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md`.
Touches `agent/{agent,loop_,tool_execution}.rs`, every provider, `gateway/{mod,ws,sse}.rs`, `channels/mod.rs`.

Commits: `c52786374` (spec) · `99d51c41d` (Phase 1 recursive Span + OtelSpan) ·
`2110d18f8` (Phase 2 core ambient span) · `197515895` (docs) · `bb1b3f179` (Phase 2 wire WS/webhook) ·
`07002a4fd` (reasoning + Composio metadata) · `3fd09e639` · `cd4ddeb24` (tool.output/error) ·
`c00663a69` · `827f7c0b1` · `9fd8c0e3c` (gen_ai.prompt/completion) · `434ce0294` (user.id) ·
`ad73cc01f` (OTLP drain on exit) · `7a47624b7` (OTLP auth headers, shared instance) ·
`c51c166c9` (activation spans for all WS turns) · `245e7277c` (deployment.environment) ·
`948328a82` (root Status) · `c15c1f1dd` (lmnr.span.input/output) · `242b6fa97` (tool.input scrubbed) ·
`c034820c2` (delivery span) · `16722ec21` · `6c887fa5e` (retry/fallback/exception events) ·
`7ce12af71` · `658fcf95e` (Laminar user_id column) · `26e97d230` · `892bec179` `b54052c0e` `bcf111e8b` (merges) ·
`0c1dfd419` (session_id + tags assoc props) · `53c23b2e7` (turn-outcome) · `411c27945` (finish_reason + tool_call_count) ·
`074b84201` (usage + gen_ai.system on streaming) · `7e9d6fac8` (build: observability-otel feature).

### Theme B — Praxis NextAction continuation auto-drive  ⟵ our differentiator
Net-new: `src/agent/continuation.rs`. Self-contained, runtime-local.
Commits: `9b3cad4cb` (rnk-h6g3 runtime auto-drive: `continuation.rs` + `loop_.rs` + `mod.rs`) ·
`9970e5536` (zc-g50j port guard to turn/turn_streamed: `agent.rs`).
Related: `02474cc03` `9969a35fd` (emit Thinking events).

### Theme C — Hotswap dev tooling (path-independent)
Net-new: `dev/hotswap/hotswap.sh`, `dev/hotswap/Dockerfile.builder`, `Justfile`.
Commits: `52b5921e3` (claw-hotswap) · `839630740` (mold linker) · `ef85f4c23` (bake into image) ·
`7ca7e5404` · `843e34b55` (zc-i4gm stdout stream) · `64bec054c` (zc-pije docker commit).

### Theme D — OpenRouter provider enhancements
Commits: `28ed745ea` (modalities + image-gen) · `68bcc3d4a` (extract images field) ·
`229ce9124` (SSE streaming + incremental reasoning/tool calls) · `54f466c5d` (declare streaming caps) ·
`f8f267879` (`reasoning` alias) · `b46a7e01e` (pod user id as `user`) · `ceadd3143` (CLAW_USER_ID env) ·
`17345a002` (`[AUDIO:]` multimodal marker — also `multimodal.rs`, `config/schema.rs`).

### Theme E — Gateway thread/session continuity (clawcraft threadId)
Commits: `8ee05706f` (B2 bind WS to threadId) · `03c739b41` (Step 1 session_key_for_thread + replace_history) ·
`b54bcc5de` (Step 2) · `8ddbda837` (Step 3 hydration) · `bb99e49cf` (Step 4 thread-switch detection).

### Theme F — Docker / Wolfi runtime packaging
Commits: `f92efae55` (preshared token + wolfi) · `b4bba8d97` (Stripe link-cli p7) · `e1bb3452d` (tbd p8) ·
`026694cd8` (git p9) · `1aac33081` (praxis in Wolfi; drop link-cli + get-tbd) · `7e9d6fac8` (otel feature build) ·
plus packaging-only README/Dockerfile bumps: `f95420a99` `f9443fb9c` `def049394` `28b11deb7` and praxis bumps `ce0ea2a69` `50564e311`.

### Theme G — Misc capability / security
Commits: `a0d1a8fbd` (ZEROCLAW_SYSTEM_DIR split-mount fs security) · `c276ffe6a` (webhook in supervised detection) ·
`f92efae55` (preshared token, shared w/ F).

### Theme H — Repo tooling & README noise  ⟵ DROP
`d4cf6e6b2` (tbd install). Plus ~20 README-only commits (`dbee54f5b` `d6ab72723` `0d8ae2916` `7acee7e90`
`93269b493` `d0e810339` `6b0430efa` `d7fac90d3` `5174d738e` `d8a78c021` `3d3d11500` `de6a39b9d` `098fb4932` `6b7f72bff` …).

---

## 2. Target mapping — where each theme lands in v0.8.0

v0.8.0 is a **12+ crate workspace** (`crates/zeroclaw-{api,runtime,gateway,channels,tools,memory,providers,infra,config,log,plugins,hardware,macros,spawn}`, plus `apps/{zerocode,tauri}`). Confirmed destinations for our touched modules:

| Fork path (v0.6.9)            | v0.8.0 destination                                  | Move type |
|-------------------------------|-----------------------------------------------------|-----------|
| `src/agent/agent.rs`          | `crates/zeroclaw-runtime/src/agent/agent.rs`        | **1:1 relocation** |
| `src/agent/loop_.rs`          | `crates/zeroclaw-runtime/src/agent/loop_.rs`        | **1:1 relocation** |
| `src/agent/tool_execution.rs` | `crates/zeroclaw-runtime/src/agent/tool_execution.rs` | **1:1 relocation** |
| `src/agent/personality.rs`    | `crates/zeroclaw-runtime/src/agent/personality.rs`  | relocation + templates dir added upstream |
| `src/agent/continuation.rs`   | `crates/zeroclaw-runtime/src/agent/` (new file)     | **clean add** |
| `src/observability/*`         | `crates/zeroclaw-runtime/src/observability/*`        | **1:1 relocation** (mod/otel/traits/multi/runtime_trace all exist upstream) |
| `src/gateway/{mod,ws,sse}.rs` | `crates/zeroclaw-gateway/src/*`                      | split into own crate |
| `src/channels/{mod,webhook}.rs`| `crates/zeroclaw-channels/src/*`                    | split into own crate |
| `src/providers/openrouter.rs` | `crates/zeroclaw-providers/src/*`                    | split into own crate |
| `src/multimodal.rs`           | `crates/zeroclaw-providers/src/multimodal.rs`        | relocation |
| `src/config/schema.rs`        | `crates/zeroclaw-config/src/*` (+ `multi_agent.rs`)  | relocation + schema V3 |
| `src/tools/*`                 | `crates/zeroclaw-tools/src/*`                        | split into own crate |
| `dev/hotswap/*`, `Justfile`, `Dockerfile` | unchanged top-level paths                | **path-stable** |

**Decisive fact:** Themes A & B (our real value) live almost entirely in
`zeroclaw-runtime/src/{agent,observability}/` — directories that **kept their internal
shape** upstream. Themes D/E/F touch the modules that *did* split into new crates and
that upstream rewrote hardest (providers, gateway multi-agent dispatch, lean Docker).

---

## 3. Extraction mechanism — evaluation

| Mechanism | Verdict for this migration |
|-----------|----------------------------|
| `git cherry-pick v0.6.9..HEAD` onto v0.8.0 | **Fails.** 3-way merge keys on path; every `src/X` → `crates/zeroclaw-*/src/X` move makes git treat our edits as edits to deleted files → conflict on nearly all 84. |
| `git rebase --onto v0.8.0 v0.6.9` | **Fails worse.** Same path problem, replayed 84× with compounding conflicts. Do not attempt. |
| `git format-patch` + `git apply --directory=crates/zeroclaw-runtime` (path rewrite via `-p`/`--directory`) | **Partial win for A & B only.** Because `agent/` and `observability/` relocate 1:1, a path-rewritten patch applies with manageable fuzz. Useless for D/E/F where content also changed. |
| **Logical re-implementation** guided by `git show <sha>` per theme | **Default for everything except A/B.** Re-apply the *intent* directly in the new crate against current upstream code. Slower but the only correct path where upstream rewrote the surface. |

**Chosen strategy:** *hybrid.* Patch-rewrite the relocations (A observability core, B
continuation); logically re-implement the crate-split + upstream-rewritten themes (D, E,
F). This is encoded per-theme in §4.

---

## 4. Per-theme disposition (extract / rework / drop)

Each call answers: does upstream now cover it? did the file change crates? is it our durable differentiator?

| Theme | Disposition | Rationale |
|-------|-------------|-----------|
| **A — Observability** | **REWORK (salvage core)** | v0.8.0 shipped its **own** unified logging pipeline (`zeroclaw-log` + `record!` macro, `gen_ai.tool.*` conventions, authenticated OTLP `otel_headers`, `/api/logs`). Our generic OTel/gen_ai/exit-drain work now **overlaps upstream** → drop those. **Salvage only the Laminar-specific layer** (`lmnr.span.input/output`, association-property `session_id`/`user_id` columns, turn-outcome) re-homed onto upstream's pipeline. Files relocate 1:1, so patch-rewrite the salvaged subset. Keep the spec doc. ⚠️ Verify §6-Q1. |
| **B — Continuation auto-drive** | **EXTRACT** | Net-new, runtime-local, self-contained, and our genuine differentiator (praxis NextAction). `continuation.rs` is a clean add into `zeroclaw-runtime/src/agent/`; re-wire the `loop_.rs`/`agent.rs` call-sites into the **new multi-agent turn**. Highest-priority, highest-value. ⚠️ Verify §6-Q2. |
| **C — Hotswap** | **EXTRACT** | Path-stable (`dev/hotswap/*`, `Justfile`, `Dockerfile`), near-zero conflict. Only edit: point the binary-extract at the new multi-crate build output (kernel binary, `--no-default-features`). |
| **D — OpenRouter** | **REWORK (thin)** | Crate moved to `zeroclaw-providers`; upstream added native extended thinking + OpenRouter prompt caching + SSE. Our streaming/reasoning-alias work is likely **now redundant** → drop. Re-apply only the still-unique bits: pod-`user`-id passthrough, image-field extraction, `[AUDIO:]` marker. ⚠️ Verify §6-Q3. |
| **E — Gateway threadId** | **KILL & REWORK** | Gateway split into `zeroclaw-gateway` **and** the whole session model was replaced by per-agent dispatch + ACP sessions in v0.8.0. Our `session_key_for_thread`/`replace_history` design no longer fits. Re-express the clawcraft-threadId continuity intent against the new per-agent session layer from scratch. |
| **F — Docker/Wolfi** | **REWORK** | `Dockerfile` path-stable but build is now multi-crate + **lean default bundle**. Re-apply intent (bundle praxis in Wolfi runtime, otel feature) against the new build; drop dead p5–p9 packaging churn and the link-cli/get-tbd bits already removed. |
| **G — Misc/security** | **REWORK / partial KILL** | `ZEROCLAW_SYSTEM_DIR` split-mount may be **obsolete** — v0.8.0 makes per-agent workspace dir the security boundary natively. Keep preshared-token + supervised-webhook intent only if not already upstream. ⚠️ Verify §6-Q4. |
| **H — README/tbd noise** | **DROP** | ~20 README-only + tbd-scaffold commits carry no product value. Regenerate README against v0.8.0; re-add `.tbd`/`.claude` tooling fresh if wanted. |

**Net:** of 84 commits, ~2 extract cleanly (B), ~6 extract with path-rewrite (C + A-salvage),
~10 rework, ~50+ drop. The fork's *durable* surface is **continuation auto-drive + a thin
Laminar observability layer + hotswap** — everything else is either now upstream or noise.

---

## 5. Execution strategy — the big change (best practice)

**Do not rebase the branch.** Reset onto upstream and re-land themes as focused commits.

1. **Set up.** `git remote add upstream` (done), `git fetch upstream --tags`. Create
   integration branch `git switch -c v0.8.0-integration v0.8.0`.
2. **Archive the old tip.** Tag `archive/0.6.9-alpha-p10.7` so nothing is lost; this doc
   is the index into it.
3. **Land themes in dependency order**, each its own commit/PR, each behind a green gate:
   - **C Hotswap** first (path-stable, unblocks fast iteration on the new build).
   - **B Continuation** (clean add; our keystone feature) — re-home `continuation.rs`,
     re-wire into the new turn, port the `turn/turn_streamed` guard.
   - **A Observability-salvage** (Laminar layer on upstream's pipeline).
   - **D / F** (provider + docker thin reworks).
   - **E / G** last (highest rework, lowest residual value — decide keep-or-cut after B/A land).
4. **Verification gate per step:** `cargo build --all-targets --message-format=short`
   (lib **+ tests + benches** — see CLAUDE.md: lib-only hides ripple sites), then the
   pinned `cargo +1.93.0 fmt --check` / clippy neutrality check from CLAUDE.md.
5. **Each theme = one reviewable PR** with its bead ID, referencing this doc's section.

---

## 6. Open questions — verify against v0.8.0 source before acting

- **Q1 (A):** Exactly what does `zeroclaw-log` + `record!` already emit? Which of our
  spans (`gen_ai.prompt/completion`, `tool.input/output`, retry/fallback events,
  `deployment.environment`) are now built-in vs still missing? Diff our
  `observability/otel.rs` against `crates/zeroclaw-runtime/src/observability/otel.rs`.
- **Q2 (B):** Does upstream already have a NextAction/continuation concept in the new
  multi-agent turn or `spawn_subagent`? If so, adapt onto it rather than re-adding.
  (See memory `project_praxis_continuation_autodrive` — clawcraft doctrine is the contract.)
- **Q3 (D):** Does v0.8.0 OpenRouter already pass a `user` field / extract image output /
  handle `[AUDIO:]`? Read `crates/zeroclaw-providers/src/` openrouter module.
- **Q4 (G):** Is `ZEROCLAW_SYSTEM_DIR` superseded by per-agent workspace boundary?
- **Q5 (E):** Map the new per-agent session/ACP model before re-expressing threadId continuity.

---

## 7. Standing playbook — rapid updates so this never recurs

The root cause was **84 commits accumulating across two minor versions with no rebase
cadence and no delta ledger**. Fixes:

1. **Delta ledger — `docs/FORK_DELTA.md`.** Every intentional divergence in one table:
   commit/bead, target crate, *upstream-or-private* disposition, rationale. A patch that
   isn't in the ledger is a bug. This file is the thing you rebase, not 84 commits.
2. **Upstream-first bias.** Default to contributing features upstream (observability,
   provider tweaks are prime candidates). Truly-private patches stay a *thin, labeled*
   series. Smaller delta = trivial rebase.
3. **Rebase cadence, not big-bang.** `git fetch upstream` weekly; rebase the private
   series onto `upstream/main` at least every **minor release**. Never let delta exceed
   ~1 release of drift.
4. **Conflict canary (CI).** Scheduled job: trial `git rebase --onto upstream/main` (or
   `git merge-tree`) on a throwaway branch; report conflict count to a dashboard/issue.
   Turns "surprise 84-commit migration" into an early weekly signal.
5. **Topic branches, squashed-by-theme.** One branch per theme (the §1 clustering is the
   template), squashed to a few semantic commits with bead IDs — so future extraction is
   `git cherry-pick <theme-branch>`, not archaeology.
6. **No README-churn commits on the feature line.** Docs/tag bumps go in dedicated
   commits or are generated; they were ~25% of the backlog noise here.
7. **Release-watch.** Subscribe to upstream releases; on each minor, open a tracking bead
   that points at this playbook and budgets a short rebase sprint.

---

## 8. Next actions

- [ ] Verify Q1–Q5 against v0.8.0 source (spawn a read-only audit per crate).
- [ ] `git tag archive/0.6.9-alpha-p10.7 HEAD`; create `v0.8.0-integration`.
- [ ] Land Theme C, then B, with green gates.
- [ ] Seed `docs/FORK_DELTA.md` from §1 + §4.
- [ ] Add the conflict-canary CI job.
