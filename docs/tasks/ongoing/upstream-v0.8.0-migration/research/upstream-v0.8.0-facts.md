# Upstream v0.8.0 — verified facts (self-contained; no web calls needed)

> **Purpose:** everything you need to know about the upstream **v0.8.0** target to make
> extraction/rework decisions **locally**. Every claim here was verified against the `v0.8.0`
> git tag, which is already present in this repo (`git ls-tree v0.8.0 …`, `git show v0.8.0:…`).
> You do **not** need to fetch or browse the web.
>
> **Anchor:** the `v0.8.0` tag (released 2026-06-12). Verified 2026-07-09.
> **Freshness caveat:** the local `upstream/main` ref appears stale (behind the `v0.8.0` tag);
> run `git fetch upstream --tags` at *execution* time to confirm nothing newer than v0.8.0
> landed. The spec targets v0.8.0 regardless — it's the concrete, fully-local anchor.

---

## 1. The gating fact — v0.8.0 is a multi-crate workspace

v0.6.9 was a single `src/` tree. v0.8.0 split it into a **Cargo workspace**. Cherry-pick /
rebase across `main..HEAD` is intractable because git keys 3-way merges on path, and nearly
every fork edit lands on a file that **moved** `src/X` → `crates/zeroclaw-*/src/X`.

**Verified crate list** (`git ls-tree v0.8.0 --name-only -- crates/`):

```
crates/aardvark-sys              crates/zeroclaw-log
crates/robot-kit                 crates/zeroclaw-macros
crates/zeroclaw-api              crates/zeroclaw-memory
crates/zeroclaw-channels         crates/zeroclaw-plugins
crates/zeroclaw-config           crates/zeroclaw-providers
crates/zeroclaw-gateway          crates/zeroclaw-runtime
crates/zeroclaw-hardware         crates/zeroclaw-spawn
crates/zeroclaw-infra            crates/zeroclaw-tool-call-parser
                                 crates/zeroclaw-tools
```

That's **15 `zeroclaw-*` crates** (+ `aardvark-sys`, `robot-kit` for robot hardware).
Apps (`git ls-tree v0.8.0 --name-only -- apps/`): `apps/tauri`, `apps/zerocode`.

> **Correction vs `migration-playbook.md` §2:** the playbook's crate list is correct **plus
> two it missed** — `zeroclaw-spawn` (subagent spawning) and `zeroclaw-tool-call-parser`.

---

## 2. Where our touched modules land in v0.8.0

Verified directory contents at the tag. Directories our real work lives in (`agent/`,
`observability/`) **kept their internal shape** — that's what makes Themes A/B salvageable.

| Fork path (v0.6.9) | v0.8.0 destination | Move type |
|--------------------|--------------------|-----------|
| `src/agent/agent.rs` | `crates/zeroclaw-runtime/src/agent/agent.rs` | 1:1 relocation |
| `src/agent/loop_.rs` | `crates/zeroclaw-runtime/src/agent/loop_.rs` | 1:1 relocation |
| `src/agent/tool_execution.rs` | `crates/zeroclaw-runtime/src/agent/tool_execution.rs` | 1:1 relocation |
| `src/agent/personality.rs` | `crates/zeroclaw-runtime/src/agent/personality.rs` (+ `personality_templates/` added upstream) | relocation |
| `src/agent/continuation.rs` | `crates/zeroclaw-runtime/src/agent/` (**new file**) | clean add |
| `src/observability/*` | `crates/zeroclaw-runtime/src/observability/*` | 1:1 relocation |
| `src/gateway/{mod,ws,sse}.rs` | `crates/zeroclaw-gateway/src/*` | split into own crate |
| `src/channels/{mod,webhook}.rs` | `crates/zeroclaw-channels/src/*` | split into own crate |
| `src/providers/openrouter.rs` | `crates/zeroclaw-providers/src/openrouter.rs` | split into own crate |
| `src/multimodal.rs` | `crates/zeroclaw-providers/src/multimodal.rs` | relocation |
| `src/config/schema.rs` | `crates/zeroclaw-config/src/*` (+ `multi_agent.rs`) | relocation + schema evolved |
| `src/tools/*` | `crates/zeroclaw-tools/src/*` | split into own crate |
| `src/util.rs` | (find home in `zeroclaw-runtime` or a shared crate) | relocation |
| `dev/hotswap/*`, `Justfile`, `Dockerfile` | unchanged top-level paths | **path-stable** |

**Verified v0.8.0 `crates/zeroclaw-runtime/src/agent/` contents** (`git ls-tree v0.8.0 …`):
`agent.rs, classifier.rs, context_analyzer.rs, context_compressor.rs, cost.rs, dispatcher.rs,
eval.rs, history.rs, history_pruner.rs, loop_.rs, loop_detector.rs, memory_loader.rs,
memory_strategy.rs, mod.rs, personality.rs, personality_templates/, prompt.rs,
system_prompt.rs, tests.rs, thinking.rs, tool_execution.rs, tool_receipts.rs`
→ **no `continuation.rs`** (confirms Theme B is a clean add).

**Verified v0.8.0 `crates/zeroclaw-runtime/src/observability/` contents:**
`dora.rs, log.rs, mod.rs, multi.rs, noop.rs, otel.rs, prometheus.rs, runtime_trace.rs,
traits.rs, verbose.rs` → note upstream **added** `log.rs`, `dora.rs`, `prometheus.rs`,
`noop.rs`, `verbose.rs`; our fork's `active.rs` and `identity.rs` are **not** present upstream
(net-new fork files to re-home).

**Agent turn location in v0.8.0:** the turn is driven from
`crates/zeroclaw-runtime/src/agent/loop_.rs` and `crates/zeroclaw-runtime/src/rpc/turn.rs`
(`execute_turn()`; `TurnAttribution` carries `session_key`, `agent_alias`, `model_provider`,
`model`, `channel`). This is the single source of truth for spawn/drain/cancel.

---

## 3. The open questions — RESOLVED against v0.8.0 source

Each verdict is evidence-backed. Re-confirm with the cited `git` command if a decision hinges on it.

### Q1 — logging pipeline: does upstream now cover our OTel work? → **PARTIALLY. Salvage only Laminar.**
- ✅ `crates/zeroclaw-log` **exists**, with a `record!` macro
  (`git show v0.8.0:crates/zeroclaw-log/src/macro.rs`) emitting structured `zc_name` /
  `zc_action` / `zc_outcome` fields via `tracing`.
- ❌ **No** `gen_ai.*` semantic conventions and **no** authenticated-OTLP (`otel_headers`) in
  `zeroclaw-log` (`git grep -in 'otel_headers\|gen_ai\.\|OTLP' v0.8.0 -- crates/zeroclaw-log/`
  → nothing). OTel support lives in `zeroclaw-runtime/src/observability/otel.rs` but without
  auth-header passing.
- **Implication:** our **generic** OTel/gen_ai/exit-drain work now **overlaps** upstream's
  pipeline → drop it. The **Laminar-specific layer is still ours** and missing upstream:
  `lmnr.span.input/output`, association-property `session_id`/`user_id` columns, turn-outcome
  stamping. Salvage that subset, re-homed onto upstream's `record!`/otel pipeline.

### Q2 — NextAction / continuation: is it upstream already? → **NO. Clean net-new differentiator.**
- ❌ `git grep -il 'NextAction\|next_action\|continuation' v0.8.0 -- crates/` finds only
  incidental "line continuation" strings — **no** NextAction concept, no continuation auto-drive.
- ✅ v0.8.0 **does** have multi-agent via a `spawn_subagent` **tool**
  (`crates/zeroclaw-runtime/src/tools/spawn_subagent.rs`) with a depth-1 cap
  (`crates/zeroclaw-config/src/multi_agent.rs`; "SubAgents must not spawn further subagents").
  This is a *tool*, not a turn-level continuation.
- **Implication:** Theme B (`continuation.rs` + the `turn/turn_streamed` guard) is the fork's
  **keystone differentiator** — re-home `continuation.rs` into `zeroclaw-runtime/src/agent/`
  and re-wire the call-sites into v0.8.0's `execute_turn`/`loop_.rs`. It must honor the
  praxis-doctrine §6.5 NextAction contract exactly (see `clawcraft-integration-contract.md`).
- ⚠️ Note: NextAction was on upstream's own radar — a v0.8.0 commit punts
  `zeroclaw agents create/delete/list` to v0.8.1. Check whether v0.8.1+ introduces an upstream
  continuation concept before re-adding ours (upstream-first bias).

### Q3 — OpenRouter: what's already upstream? → **Image: yes. `user` field + audio: no.**
- ✅ v0.8.0 `crates/zeroclaw-providers/src/openrouter.rs` **already** parses image markers
  (`MessagePart::ImageUrl`; `multimodal::parse_image_markers()` for
  `[IMAGE:data:image/png;base64,…]`; test
  `to_message_content_converts_image_markers_to_openai_parts`).
- ❌ `ChatRequest` has **no `user` field** (only `model`, `messages`, `temperature`,
  `max_tokens`) — our pod-`user`-id passthrough (`b46a7e01e`, `ceadd3143`) is **still ours**.
- ❌ **No audio** handling — our `[AUDIO:]` marker (`17345a002`, also `multimodal.rs`) is
  **still ours**.
- **Implication:** drop our SSE-streaming/reasoning-alias work as likely-redundant (verify
  against v0.8.0's openrouter which already has streaming); re-apply only the still-unique
  bits: pod-`user`-id passthrough + `[AUDIO:]` marker. `multimodal.rs` relocates to
  `zeroclaw-providers/`.

### Q4 — `ZEROCLAW_SYSTEM_DIR` split-mount: still needed? → **Likely OBSOLETE.**
- ❌ `git grep -n 'ZEROCLAW_SYSTEM_DIR' v0.8.0 -- crates/` → **no matches**. v0.8.0 uses
  `ZEROCLAW_CONFIG_DIR` + `ZEROCLAW_DATA_DIR` (and deprecated `ZEROCLAW_WORKSPACE`).
- ✅ v0.8.0 makes the **per-agent workspace dir** the security boundary natively
  (`<install>/agents/<alias>/workspace/`, cross-agent access via
  `[agents.<alias>.workspace.access]` with a Read/Write/ReadWrite `AccessMode`).
- **Implication:** Theme G's split-mount fs-security (`a0d1a8fbd`) is probably **superseded** →
  drop unless a clawcraft-specific need remains. Keep preshared-token + supervised-webhook
  intent only if not already upstream.

### Q5 — session model: how to re-express clawcraft threadId continuity? → **Rebuild on ACP sessions.**
- v0.8.0 replaced the old session model with a **per-agent ACP session** layer:
  `crates/zeroclaw-channels/src/orchestrator/acp_server.rs`,
  `crates/zeroclaw-infra/src/acp_session_store.rs`, SQLite-backed `SessionBackend`
  (`crates/zeroclaw-infra/src/session_backend.rs`), sessions keyed by `session_key` on
  `TurnAttribution`.
- **Implication:** Theme E (`session_key_for_thread` / `replace_history` /
  per-thread hydration / thread-switch detection) **no longer fits** — the gateway also split
  into `zeroclaw-gateway`. Re-express the clawcraft-threadId continuity **intent** against the
  new per-agent ACP session layer from scratch (KILL & REWORK), preserving the pod-API contract
  clawcraft depends on (claw-doctrine).

---

## 4. Quick verdict table (your baseline to re-derive against — do not trust blindly)

| Theme (fork) | v0.8.0 status | Likely disposition |
|--------------|---------------|--------------------|
| A — Connected observability (OTel + Laminar), ~33 commits | generic OTel now upstream; Laminar layer missing | REWORK — salvage Laminar subset onto upstream pipeline |
| B — Praxis NextAction continuation auto-drive | absent upstream; `spawn_subagent` tool exists | EXTRACT — clean add; keystone; honor §6.5 |
| C — Hotswap dev tooling (`dev/hotswap/*`, `Justfile`) | path-stable | EXTRACT — re-point binary-extract at multi-crate build |
| D — OpenRouter enhancements | image ✓ upstream; `user`/audio ✗ | REWORK (thin) — keep pod-`user`-id + `[AUDIO:]` only |
| E — Gateway threadId continuity | session model replaced by per-agent ACP | KILL & REWORK on ACP sessions |
| F — Docker/Wolfi packaging | Dockerfile path-stable; build now multi-crate + lean bundle | REWORK — re-apply praxis-in-Wolfi + otel-feature intent |
| G — Misc/security (`ZEROCLAW_SYSTEM_DIR`, preshared token, webhook) | split-mount obsolete; token/webhook maybe still needed | REWORK / partial KILL |
| H — README + tbd/tooling noise (~25 commits) | n/a | DROP — regenerate; re-add tooling fresh |

**Net (playbook's estimate, re-verify):** ~2 extract clean (B), ~6 extract w/ path-rewrite
(C + A-salvage), ~10 rework, ~50+ drop. Durable surface = **continuation auto-drive + thin
Laminar layer + hotswap**.
