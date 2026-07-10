# Upstream v0.8.0 Migration & Fork-Sync System: Technical Specification

**Version**: 1.0.0
**Status**: Draft
**Author**: Architect Agent (Fable 5, /substrate:architect-spec)
**Date**: 2026-07-09
**Brief**: `docs/tasks/ongoing/upstream-v0.8.0-migration/upstream-v0.8.0-migration-brief.md`
**Research pack**: `docs/tasks/ongoing/upstream-v0.8.0-migration/research/` (fork-delta-commits.md, upstream-v0.8.0-facts.md, clawcraft-integration-contract.md)
**Supersedes (as execution plan)**: `migration-playbook.md` (retained as archived analysis; see §3.9 contradictions)

---

## 1. Overview

### 1.1 Objective

Re-home the sovereign fork's durable delta (84 commits over `v0.6.9`) onto upstream's **v0.8.0 multi-crate workspace** (15 `zeroclaw-*` crates), and stand up a durable upstream-sync + dev→prod release system so the fork never again accumulates a two-minor-version rebase backlog. The durable surface is small: **praxis NextAction continuation auto-drive** (keystone), a **thin Laminar observability layer**, the **hotswap dev loop**, plus thin pod-API-compat and provider patches. Everything else is now-upstream or noise.

### 1.2 Constraints (binding, from brief + doctrines)

- **MUST** anchor on the local `v0.8.0` git tag. Every upstream claim in this spec is pinned to a file path at that tag (methodology §5).
- **MUST** preserve the praxis **NextAction `{data, next_action}` contract exactly** (praxis-doctrine §6.5): `next_action: null` = unconditional turn-end, runtime **never inspects `data`** on null; PARK = `null` + `data.parked`; auto-drive of `agent_work_then_call` is a doctrine MUST (empirical finding 2026-06-04, traces `f3246a74`/`e54257b3`; cross-repo bead `rnk-h6g3`).
- **MUST** keep the dev hotswap loop yielding a runnable `clawcraft-claw-runtime:dev` — landed early so every later theme is locally testable.
- **MUST NOT** break clawcraft prod: pod API (`POST /api/chat`, `GET /health`, 42618 webhook, `/ws/chat`), brain.db at `/zeroclaw-data/workspace/memory/brain.db`, workspace files, tool policy; SHA-pinned `CLAW_DOCKER_IMAGE` → GCP Artifact Registry (never `:latest`); `Dockerfile ARG PRAXIS_VERSION` praxis bundling (pin stays **0.10.0**; bump = rnk-pzbs, out of scope); `config.toml [observability]` Laminar config-carrier (no `OTEL_*` env).
- **MUST** land each theme as its own reviewable commit behind a green gate: `cargo build --all-targets --message-format=short`, then pinned `cargo +1.93.0 fmt --check` / clippy **neutrality** vs. the Phase-1 baseline (per CLAUDE.md).
- **MUST** be upstream-first: DROP anything now covered upstream (generic OTel/gen_ai, OpenRouter SSE streaming, webhook-only supervision fix, `ZEROCLAW_SYSTEM_DIR`).
- **MUST NOT** land unlabeled divergence: every sovereign-only commit carries a `Fork-Delta: FD-NN` trailer matching a `docs/FORK_DELTA.md` row (methodology §2/§4).

### 1.3 Success Criteria (binary)

- [ ] `upstream` branch exists at the `v0.8.0` tag; sovereign branch `core/v0.8.0` carries the delta as squashed-by-theme commits, each with a `Fork-Delta:` trailer.
- [ ] `cargo build --all-targets --message-format=short` green on the sovereign tip.
- [ ] All Theme B contract tests pass (§4.2 invariants 1–6) on both `turn` and `turn_streamed` paths.
- [ ] `just claw-hotswap` produces a runnable `clawcraft-claw-runtime:dev`; `curl -sf localhost:42617/health` → 200 after `docker compose --profile claw up -d --force-recreate`.
- [ ] `dev/hotswap/verify-coldstart.sh` passes all checks (headless boot, token 200/401 matrix, webhook-only supervision, brain.db resume + single-backup idempotency).
- [ ] Laminar live battery passes (§4.4): typed `user_id`/`session_id` columns populated, root input/output non-empty, `agent.turn.exit_reason` stamped, probe secret redacted, negative control (blank headers → spans dropped).
- [ ] `docs/FORK_DELTA.md` exists with one row per live divergence; `fork-delta-check.yml` enforces the trailer↔ledger bijection.
- [ ] `conflict-canary.yml` runs on schedule and reports per-FD conflict attribution.
- [ ] `release-clawcraft-image.yml` builds+smokes+pushes a SHA-tagged image to GCP AR via WIF on `workflow_dispatch`.
- [ ] `docs/RELEASE.md` documents the dev→prod loop incl. rollback and cross-repo prerequisites.

---

## 2. Scope

| In Scope | Out of Scope |
|----------|--------------|
| Branch model (mirror `upstream` + sovereign `core/v0.8.0`), archive tag | Robot-hardware crates (`aardvark-sys`, `robot-kit`), `apps/{tauri,zerocode}` |
| Theme B: continuation auto-drive re-home (keystone) | Praxis 0.10.0→0.11.0 bump (rnk-pzbs) |
| Theme C+F: hotswap re-point + Wolfi/praxis Dockerfile re-apply | Re-homing README/tbd/tooling noise (Theme H — regenerate fresh) |
| Theme E+G: `POST /api/chat` restore, `pre_shared_token`, ws threadId→session_key, cold-start harness | Generic OTel/gen_ai instrumentation (now upstream `zeroclaw-log`/`otel.rs`) |
| Theme A: Laminar-specific salvage + `[observability]` config compat | OpenRouter SSE streaming / reasoning alias (upstream has streaming — verify then DROP) |
| Theme D: pod-`user`-id + `[AUDIO:]` marker (thin) | `ZEROCLAW_SYSTEM_DIR` split-mount (superseded by per-agent workspaces — DROP) |
| Fork-sync system: FORK_DELTA.md, trailer CI, conflict canary, release workflow, runbook, rebase cadence | Refreshing doctrine snapshots against clawcraft (noted as cross-repo follow-ups) |
| | Clawcraft-side code changes (rendered-config pin, relay_logs check — cross-repo follow-ups only) |

**Auth boundary**: pod `/api/*` + 42618 webhook = Bearer `pre_shared_token`; `/health` unauthenticated. CI→GCP = WIF only (infra §15.4), no JSON keys. npm token for praxis = BuildKit secret, never ARG (infra §15.23).

---

## 3. Architecture

### 3.1 Verified target facts (pinned to the `v0.8.0` tag)

- **Workspace**: 15 `zeroclaw-*` crates + `aardvark-sys`/`robot-kit`; root package `zeroclawlabs` still builds bin `zeroclaw` (`default-run`); default features `agent-runtime, default-channels, acp-bridge, gateway, observability-prometheus, schema-export`; `observability-otel` opt-in. Fork features `channel-lark, whatsapp-web, rag-pdf, observability-otel` all exist by the same names at v0.8.0 (root `Cargo.toml`); the fork's `skill-creation` feature is gone upstream.
- **Turn flow**: `crates/zeroclaw-runtime/src/rpc/turn.rs` `execute_turn()` → `Agent::turn_streamed_with_steering_state()` (`agent/agent.rs:~2390`); `turn()` (~2081) and `turn_streamed()` (~2357) still exist. `TurnAttribution { session_key, agent_alias, model_provider, model, channel }`. **No continuation concept upstream** (`git grep -il 'next_action\|NextAction' v0.8.0 -- crates/` → incidental only); `spawn_subagent` is a depth-1-capped *tool*, not turn-level.
- **Gateway**: `crates/zeroclaw-gateway`; routes at `src/lib.rs:1498–1786`. **No `POST /api/chat`**. 42617 `/webhook` is now a **full tools-enabled agent loop** (`handle_webhook` → `run_gateway_chat_with_tools` → `zeroclaw_runtime::agent::process_message`, lib.rs:2341–2607), body `{message}`, Bearer pairing auth, optional `?agent=`/`X-Session-Id`, reply `{"response","model"}` — §4.2-parse-order compatible. Gateway-wide 30s `TimeoutLayer`; long-running sub-router (600s) exists (`/api/cron/{id}/run`). Auth: `PairingGuard` accepts **plaintext tokens hashed on load** (`crates/zeroclaw-config/src/pairing.rs:74–96`); `GatewayConfig.paired_tokens: Vec<String>` (schema.rs:5622); **no `pre_shared_token` key**. `[gateway].host` is honored (`v0.8.0:src/main.rs:3556`) — rnk-3m71 workaround retire-able.
- **Sessions**: per-agent ACP sessions keyed by `session_uuid` (`crates/zeroclaw-channels/src/orchestrator/acp_server.rs`, `crates/zeroclaw-infra/src/{acp_session_store,session_backend}.rs`); ACP store at `<data_dir>/sessions/acp-sessions.db`. **No thread_id concept.**
- **Dirs**: `ZEROCLAW_WORKSPACE` survives as deprecated alias of `ZEROCLAW_DATA_DIR` (schema.rs:14766–14775); brain.db at `<data_dir>/memory/brain.db` (`crates/zeroclaw-memory/src/sqlite.rs:111`) → `/zeroclaw-data/workspace/memory/brain.db` holds **iff** the image keeps the workspace env pin and never sets `ZEROCLAW_CONFIG_DIR` (which would re-pin data under `<config_dir>/data`, schema.rs:14700–14760). Per-agent workspace defaults under the **install root** (`<config_dir>/agents/<alias>/workspace`, schema.rs:3584–3612) = the 10Mi emptyDir unless pinned via `[agents.<alias>.workspace] path`.
- **Config migration**: clawcraft's rendered legacy config (no `schema_version`) auto-migrates in memory on every boot (schema.rs:15241–15279): `[channels_config.webhook]` alias-wraps to `channels.webhook.default`; `[autonomy] non_cli_excluded_tools` folds to `excluded_tools`/`risk_profiles` (schema/v2.rs:148–153); `[agents.default]` synthesized (missing `model_provider` → hard error, agent.rs:1170–1183). `migrate_sqlite_memory_to_v3` runs on every brain.db open, writes one `brain.db.backup-*`, **one-way**.
- **Observability**: `ObservabilityConfig { backend: String, otel_endpoint: Option<String>, otel_service_name: Option<String>, otel_headers: Option<HashMap<String,String>> }`; OTLP/HTTP exporter at `crates/zeroclaw-runtime/src/observability/otel.rs` (endpoint base + `/v1/traces`; header application to be re-verified at execution — see §3.9-3). `crates/zeroclaw-log` `record!` macro exists. **No Laminar/lmnr references upstream**; fork's `active.rs`/`identity.rs` are net-new.
- **OpenRouter**: `crates/zeroclaw-providers/src/openrouter.rs` — SSE streaming **exists** (`supports_streaming()`, `stream_chat()`); `ChatRequest`/`NativeChatRequest` have **no `user` field**; **no `[AUDIO:]`** handling; `multimodal.rs` exists with `[IMAGE:]` parsing.
- **Supervision**: `has_supervised_channels` → `ChannelsConfig::has_any_enabled()` includes webhook (schema.rs:11089); upstream regression test `webhook_only_config_is_supervised` (daemon/mod.rs:1641–1662) — c276ffe6a **absorbed**.

### 3.2 Domain — praxis NextAction contract (praxis-doctrine §6.5, §10.5, §10.9)

The re-homed `crates/zeroclaw-runtime/src/agent/continuation.rs` (751 LOC + 16 unit tests, from fork `src/agent/continuation.rs`) carries the `ContinuationDriver` FSM: `observe_tool_result` / `observe_driven_result` / `register_driven` / `try_redrive` / `directive_to_inject` / `safety_net_command`. Contract values (**not tuning knobs** — co-designed with praxis park-chunking, §10.5): `MAX_AUTO_DRIVE_CALLS = 32`, `MAX_AGENT_WORK_REDRIVES = 3`. Safety net: `praxis update <id> --state waiting_for --assignee user --notes <reason> --json`.

Binding runtime rules:
- `next_action: null` → immediate legal turn-end; the driver MUST NOT read, branch on, or log-parse any `data` key on a null envelope (data-blindness = praxis-version-agnosticism; why the 0.10.0 binary and 0.11.0 doctrine coexist).
- Closed union: parse exactly `call` / `agent_work_then_call` / `null`. **Decision (default)**: unknown `kind` → treat as no-continuation + WARN log (fail-open matches envelope tolerance; avoids deploy-order coupling on future praxis bumps).
- Mechanical drive of `kind:"call"` chains with zero model round-trips; forcing-directive + bounded re-drive for `agent_work_then_call`; termination guard refuses turn-end while `has_pending()` — wired into **both** `turn()` and `turn_streamed_with_steering_state()` **and** the `process_message` path (the restored `/api/chat` + 42618 webhook route through it — guarding only the WS/ACP paths reproduces the mimo stall through prod's front door).
- Verifier-failure continuity (§10.5): a failed-verifier `update` envelope still carrying the execute continuation is driven; the walk never aborts on verifier failure.
- **Decision (default)**: user cancel/steer **wins** over the termination guard; the driver fires the §10.9 safety net on the pending bead (reason "user cancelled mid-continuation") before yielding.
- e2e tests assert *runtime* invariants with synthetic envelopes — never 0.11.0-only emitter shapes against the live 0.10.0 binary.

### 3.3 Backend — pod API & sessions (claw-doctrine §4.1, §4.2, §5.x; state-machine §1–§5)

- **`POST /api/chat` (restore)**: thin alias of the full-loop `handle_webhook` path, registered on the **long-running (600s) sub-router** (default 30s TimeoutLayer would kill multi-step turns; relay client budget is 300s). Body `{message, context?}` — `context` tolerated-and-ignored (regression-pinned so a future `deny_unknown_fields` can't break prod); reply `{"response","model"}` satisfies clawcraft's `content ?? reply ?? response` parse order. `session_id: None` unless `X-Session-Id` present (claw §4.1 MUST NOT couple relay to a session — global memory recall). Agent alias: legacy default pick (`?agent=` override else `resolved_runtime_agent_alias()`); align `/ws/chat` to the same default pick.
  > **Insight (verified)**: no `/api/chat` route exists at fork HEAD *or* v0.8.0 — only on unmerged 0.1.8-era branches. This is a **latent prod gap**, not a regression risk; the doctrine mandates the endpoint either way. Cross-repo follow-up: check prod `relay_logs` for `http_error` on that path.
- **`pre_shared_token` (Option A now, B later)**: re-add `GatewayConfig.pre_shared_token: Option<String>` and seed it into `PairingGuard::new`'s token list (~15 lines; fork `src/gateway/mod.rs:694–706` is the template; PairingGuard hashes plaintext on load). Headless cold start preserved: zero clawcraft change, byte-identical rendered config across the image swap. End-state follow-up: clawcraft renders `paired_tokens = ["<token>"]` (both keys during transition), then the fork key drops.
- **threadId → session_key (KILL & REWORK)**: in `crates/zeroclaw-gateway/src/ws.rs`, keep only the envelope-parsing half (typed `threadId`, legacy `[conversationId:]` fallback); map per message: `session_key = "thread:<id>"`, falling back to the connection session key when absent. SessionBackend owns persistence — **no** `replace_history`/hydration/switch-detection machinery. Never surface the prefixed key as `X-Session-Id` (v0.8.0 header validation rejects `:`, lib.rs:214–218).
- **Cold-start invariants (state-machine §1/§2/§4)**: image keeps `ENV ZEROCLAW_DATA_DIR=/zeroclaw-data/workspace` (+ legacy `ZEROCLAW_WORKSPACE` during transition; DATA_DIR wins with WARN), `HOME=/zeroclaw-data`; **never** sets `ZEROCLAW_CONFIG_DIR`. First v0.8.0 boot on an existing PVC = a schema-migration event: one `brain.db.backup-*`, per-boot TOML migration; rollback = restore backup + re-pin previous image SHA. `/health` stays unauthenticated 200 (body enriched; probes are status-code-only).
- **Webhook channel (42618)**: upstream implements the `{sender, content}` async contract incl. `auth_header` → `/container-webhook`. Deliverable = verification + a fork regression test pinning webhook-only supervision (c276ffe6a intent; upstream absorbed).

### 3.4 Backend — observability salvage (observability-doctrine §2–§10)

Salvage = **attribute-mirroring on upstream's spans, never a parallel span tree**:
- Re-home `identity.rs` (`pod_user_id()` gated on `^[a-z0-9]{32}$` from `CLAW_USER_ID`; `tag_user_id` dual-emit `user.id` + `lmnr.association.properties.user_id`, root only, absence-not-empty; `tag_channel` + `lmnr.association.properties.tags` array; `session_id` association from `TurnAttribution.session_key`) and `active.rs` (`stamp_turn_exit` → `agent.turn.exit_reason` ∈ {`final_answer`,`max_iterations`,`error`} + `agent.turn.iterations`, plus native OTel Status).
- `lmnr.span.input/output` mirroring on `llm.call`-equivalents **and** the activation root, every value `scrub_credentials` + `truncate_with_ellipsis(·, 16_000)` once per site; tool-call-only iterations keep the `name(args)` summary (no blank rows).
- **Decision (mediated)**: the `deployment.environment` every-span stamp is **retained** in the salvage (doctrine v1.6.10 binds it; dropping it would need a clawcraft-side doctrine amendment).
- Single exporting observer across `zeroclaw-gateway`/`zeroclaw-channels`/runtime (§4.1, §10#13 — the 2026-06-02 incident class); one `service.name=zeroclaw`; resource attrs exactly `{service.name, deployment.environment}`.
- **Config compat (must-land-before-boot-tests)**: custom serde deserializer for `otel_headers` accepting **string** `"k=v[,k=v]"` (split on **first** `=` only — values contain spaces, keys may carry `=` padding) *and* map form; empty ⇒ inert. Tolerate/re-add the fork-extra fields (`otel_deployment_environment`, `runtime_trace_*`); backend literal must match what clawcraft actually renders (`"otel"` per `claw-config.ts` — golden-test the verbatim rendered block). Verify upstream exporter applies headers; wire `.with_headers()` if absent (silent 100% ingest drop otherwise, §10#12).
- Content-emission gate: preserve **current fork behavior exactly** — neither widen nor narrow prod content emission; inventory the existing gate in fork `otel.rs` during execution.
- Root-span check before wiring: if v0.8.0 wraps turns in a parent span, association properties must land on the **trace root** or Laminar's typed columns silently empty.

### 3.5 Backend — provider thin patches (Theme D)

- Pod-`user`-id passthrough: add `user` to OpenRouter request structs, sourced from validated `CLAW_USER_ID` (strip `claw-` prefix; 32-char lowercase alnum), re-homed to `crates/zeroclaw-providers/src/openrouter.rs`.
- `[AUDIO:]` marker: mirror of `[IMAGE:]` in `crates/zeroclaw-providers/src/multimodal.rs` + `input_audio` parts in openrouter + `max_audio_files`/`max_audio_size_mb` config keys (land with their consumer, methodology §2) + the 24 fork tests.
- Verify-then-DROP at execution: SSE streaming + `reasoning` alias (upstream has streaming; check alias), image-generation *output* extraction (`images` field — upstream has `[IMAGE:]` *input* parsing; output extraction unverified), Thinking-event emission (upstream has `agent/thinking.rs` — check TurnEvent parity). Each verdict recorded in the theme commit body.

### 3.6 Infra — image, hotswap, release (infra-doctrine §2.3, §6.9, §6.10, §15.4, §15.23, §15.24)

- **Dockerfile**: start from upstream v0.8.0's builder (keep `# syntax=docker/dockerfile:1.7-labs`, `COPY --parents` stub-then-real, cache mounts — bump cache ids to `*-v080`; keep `locales.toml`, ≥1MB size guard). DROP web-node/web-builder stages (+ `-p zerocode`, `g++`) — dashboard is fs-served, `web_dist_dir` defaults `None`, `embedded-web` not in our features. REPLACE distroless release stage with the fork's `praxis-install` (node:20-alpine, `ARG PRAXIS_VERSION=0.10.0`, BuildKit `npm_token` secret, scoped `.npmrc`) + Wolfi release (`cgr.dev/chainguard/wolfi-base` + ca-certificates/bash/coreutils/vim/git/nodejs, `/opt/praxis` + symlink, `USER 65534`, `HEALTHCHECK ["zeroclaw","status","--format=exit-code"]` — §6.10 no curl/wget, `ENTRYPOINT ["zeroclaw"] CMD ["daemon"]`). Env pins per §3.3. Builder features ARG defaults to the shared string `channel-lark,whatsapp-web,rag-pdf,observability-otel` (additive over v0.8.0 defaults).
- **Hotswap**: `cargo build --locked -p zeroclawlabs --bin zeroclaw --features "$FEATURES"`; one-time named-volume reset + `hotswap-base` re-pin after the new Dockerfile lands; self-verify marker → `lmnr.span.input`. `Dockerfile.builder` (rust:1.94-slim + mold) unchanged.
- **Release workflow** (`release-clawcraft-image.yml`): trigger = `clawcraft-v*` tag + `workflow_dispatch`; WIF auth (`GCP_WORKLOAD_IDENTITY_PROVIDER`/`GCP_DEPLOY_SA_EMAIL` repo vars, AR-writer-scoped SA); `--platform linux/amd64`; smoke (run + host-curl `/health` + `zeroclaw status` + `praxis --version` + `ldd` clean) **before** push; push `…/clawcraft-claw-runtime:<git-sha>` only — **no `:latest`**; step summary emits URI+digest+bump command. `CLAW_DOCKER_IMAGE` bump = documented human-gated `convex env set … --prod` (deployment colorful-rook-584).
- **Conflict canary** (`conflict-canary.yml`): weekly `git merge-tree --write-tree` walk per theme-commit cumulative tip vs `upstream/main`, attributing conflicted files to `FD-NN` via trailers; **informational only** (single auto-updated tracking issue; never a permanently-red gate). Escalation: conflicts persisting past one upstream minor release flip the tracking bead to a rebase-sprint.
- **Rebase cadence**: fetch weekly (remote-tracking ref only); rebase the sovereign series onto the new upstream ref at least every minor release; the `upstream` branch advances **only** in the same operation that rebases the series (ff-only).

### 3.7 Cross-cutting — fork-delta ledger (methodology §1–§5)

`docs/FORK_DELTA.md`: one row per **squashed theme-commit** on the sovereign series; columns `id (FD-NN) | title | bead | crate(s) | disposition | rationale [| end-state | removal-ref]`; disposition enum `private | upstreaming (requires open PR URL) | transitional (requires end-state + removal-ref)`. **No SHA column** (rebase-unstable); commit↔row mapping via `Fork-Delta: FD-NN` trailers, enforced by `fork-delta-check.yml` (bijection over `upstream..HEAD` + §3-field completeness). Rows land **in the same commit** as the divergence they describe — never bulk-seeded. Ledger records live divergences only; drops live in this spec + the archived playbook. Header carries the branch-semantics sentence and the maintenance protocol (the durable graduate of playbook §7). CI workflow files get their own FD rows.

### 3.8 Cross-repo follow-ups (clawcraft-side; file in clawcraft `docs/cross-repo-followups.md` at rollout)

1. Render `[agents.default.workspace] path = "/zeroclaw-data/workspace"` in `buildConfigToml` + dev renderer **before** the image bump (old image warns-and-ignores; new image needs it or agent state lands in the 10Mi emptyDir).
2. Check prod `relay_logs` for `/api/chat` `http_error` (latent-gap confirmation).
3. Update `RUST_LOG` module filters in gke.ts (crate renames: `zeroclaw::gateway` → `zeroclaw_gateway`).
4. Doctrine reconciliations: claw §5.0 (42617 `/webhook` now full-loop), observability §7.1 allowlist (add `lmnr.association.properties.session_id`/`tags`, `agent.turn.exit_reason/iterations`), infra §6.9 trigger shape (tag vs merge) + `:latest` example removal, state-machine §2 startup-env diagram.
5. End-state token migration: render `paired_tokens` (transition: both keys), then drop `pre_shared_token`.
6. Later: v3-native config render (schema_version 3, `[channels_config.webhook.default]`, explicit `[agents.default] model_provider`) to retire per-boot TOML migration.
7. Retire the rnk-3m71 `--host 0.0.0.0` compose workaround (v0.8.0 honors `[gateway].host`).

### 3.9 Contradictions found (brief mandate: flag playbook/facts vs. re-derived evidence)

1. **Playbook §2** crate list missed `zeroclaw-spawn` + `zeroclaw-tool-call-parser` (already corrected in facts §1).
2. **Playbook §4 Theme D** ("port SSE streaming") contradicted: v0.8.0 openrouter already streams (`stream_chat()` verified) → streaming/reasoning-alias work is DROP-pending-verification, not extract.
3. **Facts §3 Q1** ("otel.rs … without auth-header passing") vs. direct source read (headers applied to exporters at otel.rs:52–54,71–73): unresolved discrepancy → encoded as an execution-time verify-and-wire step (Step 5.2), not assumed either way.
4. **Playbook §4 Theme C** ("likely `--no-default-features` / kernel binary") wrong: root package still builds `zeroclaw` with additive features.
5. **Playbook §8** ("Seed FORK_DELTA.md from §1+§4") violates methodology §2 (aspirational rows) → incremental per-theme seeding.
6. **Playbook §1 Theme F** ("packaging-only churn") understated: `1aac33081` (praxis-in-Wolfi) and `7e9d6fac8` (otel feature) are real product changes.
7. **Contract §1/§5** implies `/api/chat` exists today; verified it never existed on the deployed lineage (claw architect, route tables at HEAD and v0.8.0) → reframed as latent-gap fix.
8. **Infra vs state-machine** on `ZEROCLAW_DATA_DIR` value: `/zeroclaw-data/data` (infra draft) vs `/zeroclaw-data/workspace` (verified against gke.ts + dir-resolution code) → **workspace** wins.
9. **Contract §4 snippet** shows `backend = "otlp"`; clawcraft code renders `"otel"` → golden test pins the rendered literal.
10. **Doctrine snapshot (state-machine §2)** claims `ZEROCLAW_CONFIG_DIR=/zeroclaw-config` pod env; as-built gke.ts sets no `ZEROCLAW_*` env (image-baked) → snapshot drift, follow-up #4.

### 3.10 Re-derived disposition table (final)

| Theme | Commits (fork) | Disposition | One-line rationale |
|---|---|---|---|
| B — continuation auto-drive | 9b3cad4cb, 9970e5536 | **EXTRACT** (FD-03) | Net-new upstream; keystone; §6.5 mandate |
| C — hotswap | 52b5921e3, 839630740, ef85f4c23, 7ca7e5404, 843e34b55, 64bec054c | **EXTRACT** (FD-02) | Path-stable; `-p zeroclawlabs` + volume/base re-pin only |
| F — Docker/Wolfi | 1aac33081, 7e9d6fac8, f92efae55(docker part), praxis bumps | **REWORK** (FD-01) | Re-apply praxis+Wolfi intent over upstream's multi-crate builder |
| A — observability | ~30 commits (see playbook §1 list) | **REWORK — Laminar salvage only** (FD-07) | Generic OTel now upstream; lmnr layer is ours |
| E — threadId/pod-API | 8ee05706f, 03c739b41, b54bcc5de, 8ddbda837, bb99e49cf | **KILL & REWORK** (FD-05, FD-06) | Session model replaced; deliverable = /api/chat + thin thread mapping |
| G — misc/security | f92efae55(token), c276ffe6a, a0d1a8fbd | token **EXTRACT** (FD-04); c276ffe6a **DROP** (absorbed upstream, test-pinned); SYSTEM_DIR **DROP** (superseded) | Evidence: schema.rs:11089 + daemon test; per-agent workspace boundary |
| D — OpenRouter | b46a7e01e, ceadd3143, 17345a002 keep; 229ce9124, 54f466c5d, f8f267879, 28ed745ea, 68bcc3d4a, 02474cc03, 9969a35fd verify-then-drop | **REWORK (thin)** (FD-08) | `user` + audio still ours; streaming/images/thinking likely upstream |
| H — README/tbd noise (~21) | see playbook §1 | **DROP** | No product value; regenerate |

---

## 4. Implementation Details

### 4.1 File map (sovereign-series deliverables)

| Path | Theme | Content |
|---|---|---|
| `crates/zeroclaw-runtime/src/agent/continuation.rs` | B | ContinuationDriver FSM + 16 unit tests (moved intact) |
| `crates/zeroclaw-runtime/src/agent/{mod,agent,loop_}.rs` | B | module reg; guard wiring in `turn`/`turn_streamed_with_steering_state`/`process_message` path |
| `crates/zeroclaw-runtime/src/observability/{identity,active}.rs` | A | net-new re-homes |
| `crates/zeroclaw-runtime/src/observability/otel.rs` | A | headers verify/wire; lmnr mirrors; Status; turn-outcome |
| `crates/zeroclaw-config/src/schema.rs` | A,G | `otel_headers` string-or-map deserializer + extra fields; `GatewayConfig.pre_shared_token` |
| `crates/zeroclaw-gateway/src/lib.rs` | E,G | `/api/chat` on 600s router; token seeding into PairingGuard |
| `crates/zeroclaw-gateway/src/ws.rs` | E | threadId parse + `thread:<id>` session key |
| `crates/zeroclaw-runtime/src/util…` | A | `scrub_credentials`/`truncate_with_ellipsis` port |
| `crates/zeroclaw-providers/src/{openrouter,multimodal}.rs` | D | `user` field; `[AUDIO:]` |
| `Dockerfile`, `dev/hotswap/hotswap.sh`, `dev/hotswap/verify-coldstart.sh` | F,C,G | per §3.6, §3.3 |
| `docs/FORK_DELTA.md`, `docs/RELEASE.md` | sync | ledger + runbook |
| `.github/workflows/{release-clawcraft-image,conflict-canary,fork-delta-check}.yml` | sync | per §3.6/§3.7 |

### 4.2 Theme B behavioral invariants (each test-pinned on `turn` AND `turn_streamed` paths)

1. **Null data-blindness**: `{data:{parked:{…}}, next_action:null}` → clean turn-end, zero driven calls; poison-`data` variant (undeserializable payload) still ends cleanly.
2. **Mechanical chains**: N `kind:"call"` envelopes → N driven calls, zero model round-trips; call 33 not driven (budget 32).
3. **Forcing**: directive injected; turn cannot end while pending; after 3 re-drives, exactly the safety-net command fires.
4. **Verifier-failure continuity**: failed-verifier `update` envelope with execute continuation → driven.
5. **Envelope tolerance**: bare JSON/prose/chatter → no continuation, no panic; unknown `kind` → no continuation + WARN.
6. **Cancel precedence**: cancel during `has_pending()` → safety net fires, turn yields.

### 4.3 Cold-start harness checks (`dev/hotswap/verify-coldstart.sh`)

Headless boot (no pairing code printed); resolved dirs = `/zeroclaw-data/.zeroclaw` + `/zeroclaw-data/workspace`; `/health` 200 ≤10s; `/api/status` 200 with token / 401 without; `/api/chat` 200 `{message}` and `{message,context}` / 401 unauth; 42617 `/webhook` rejects `{sender,content}` with 400 (polysemy check); 42618 webhook-only supervision 2xx; brain.db resume: seeded row survives restart, exactly one `brain.db.backup-*` after first boot, zero new on second; `du -s /zeroclaw-data/.zeroclaw` ≪ 10Mi; no `config.toml` at PVC root (dir-resolution foot-gun); rollback drill documented.

### 4.4 Laminar live battery (dev Laminar via clawcraft compose profile; query ClickHouse, not the UI)

Pass iff: one `agent.activation` root per trigger per ingress surface; typed `user_id` **and** `session_id` columns populated; root input/output non-empty; every `llm.call` row non-blank incl. tool-call-only iterations; root `agent.turn.exit_reason` matches forced outcome; probe secret appears only as `*[REDACTED]`. **Negative control**: blank `otel_headers` → spans dropped at ingest.

---

## 5. Error Handling

| Error | Cause | Handling |
|---|---|---|
| Pod boots but 401s all authed calls | `pre_shared_token` key unknown to v0.8.0 schema | FD-04 re-add + coldstart harness token matrix (silently-degraded-`running` is undetectable by /health) |
| Config parse fail on boot | `otel_headers` string vs HashMap | custom deserializer + golden test on verbatim rendered block, lands before any boot test |
| Spans 100% dropped at Laminar | headers not applied by exporter | Step 5.2 verify-and-wire + negative control |
| brain.db corruption on rollback | one-way sqlite v3 migration | runbook: restore `brain.db.backup-*` + re-pin previous SHA |
| Turn killed at 30s | `/api/chat` on default router | register on 600s long-running sub-router + test pinning router membership |
| Stall reproduces via webhook/api path | guard wired only into WS/ACP turn paths | wire `process_message` path too (Step 3.3) |
| Hotswap phantom stale artifacts | v0.6.9 fingerprints in named volumes / cache mounts | one-time volume reset + cache-id bump `*-v080` |
| `cargo --locked` failures everywhere | lockfile drift | adopt v0.8.0 `Cargo.lock` wholesale; dep additions regenerate lock in their own commit |

---

## 6. Testing Strategy

| Layer | Focus | Command |
|---|---|---|
| Unit (runtime) | continuation FSM, identity guards, scrub/truncate | `cargo test -p zeroclawlabs continuation identity` (adjust to crate layout) |
| Unit (config) | otel_headers forms, golden rendered block, pre_shared_token round-trip, autonomy migration | `cargo test -p zeroclaw-config` |
| Unit (gateway) | /api/chat 200/401/context, router membership, threadId keys, token guard | `cargo test -p zeroclaw-gateway` |
| Integration | Theme B invariants on turn/turn_streamed/process_message | `cargo test -p zeroclawlabs --test '*'` (fork's 12 scenario tests ported) |
| Build gate | lib+tests+benches ripple | `cargo build --all-targets --message-format=short` |
| Neutrality | fmt/clippy vs Phase-1 baseline | `cargo +1.93.0 fmt --all -- --check` diff-count compare |
| System | cold start, resume, rollback | `bash dev/hotswap/verify-coldstart.sh` |
| Live | Laminar battery | §4.4 (manual gate, ClickHouse queries) |
| CI | image smoke, canary dry-run, trailer bijection | `workflow_dispatch` each |

---

## 7. Failure Modes (FMEA)

| # | Failure Mode | Severity | Mitigation |
|---|---|---|---|
| 1 | §6.5 contract drift in re-home (data inspected on null) | Critical | poison-data test (§4.2-1); doctrine-review phase |
| 2 | Prod pod boots into silently-degraded state (token/config) | Critical | coldstart harness end-to-end token+parse checks pre-release |
| 3 | brain.db one-way migration + bad rollout | Critical | runbook rollback drill (Step 4.5); backup-restore procedure; PVC snapshot note |
| 4 | Laminar typed columns silently empty (root-span mismatch) | High | root-span verification before wiring; live battery gates release |
| 5 | Double span emission (parallel tree) | High | mirror-on-upstream-spans rule; per-trigger span-count check |
| 6 | mimo stall reproduces via unguarded path (process_message / subagent) | High | Step 3.3 + subagent route check (test or documented scope-out) |
| 7 | Fork delta grows unlabeled again | Medium | trailer CI bijection; canary; cadence bead |
| 8 | Hotswap silently builds wrong features vs prod | Medium | single shared feature string; hotswap uses same ARG default |
| 9 | Canary noise trains ignoring | Medium | informational-only, per-FD attribution, single tracking issue |
| 10 | CI npm-token scope (fork org ≠ soulbound-labs) | Low | pre-flight org check in Step 7.3; PAT decision surfaced if needed |

---

## 8. Prompt Execution Strategy

<!-- PROTOCOL: docs/protocol/sdd/execution-format.md. Gates per CLAUDE.md: full-target build, then pinned-toolchain neutrality. -->

### Phase 1: Foundation — branches, baseline, upstream-first checks

#### Step 1.1: Archive and branch setup

In `/Users/reinova/code/forks/zeroclaw`: tag the old tip `git tag archive/0.6.9-alpha-p10.7 9970e5536` (verify that SHA is the tip of branch `0.6.9-alpha-p10.7` first with `git rev-parse 0.6.9-alpha-p10.7`). Create the mirror branch: `git branch upstream v0.8.0`. Create the sovereign branch: `git switch -c core/v0.8.0 v0.8.0`. Do NOT rebase or cherry-pick anything. Attempt `git fetch upstream --tags` (network); if it fails or is unavailable, proceed — the target is the local v0.8.0 tag. If newer tags arrive, run `git grep -il 'next_action\|NextAction\|continuation' <newest-tag> -- crates/ | grep -v -i 'line continuation'` and record the result; a hit triggers the §6.5-conformance checklist (praxis §6.5: null=unconditional turn-end + data never inspected + closed union + auto-drive mandate) — anything less than exact conformance means keep ours; partial adoption is a user decision, stop and surface it.

Tools to use: Bash

##### Verify

- `git rev-parse --verify archive/0.6.9-alpha-p10.7`
- `git rev-parse --verify upstream && test "$(git rev-parse upstream)" = "$(git rev-parse v0.8.0^{commit})"`
- `git rev-parse --abbrev-ref HEAD | grep -qx 'core/v0.8.0'`

#### Step 1.2: Clean-tree build + neutrality baseline

On the clean `core/v0.8.0` tip: run `cargo build --all-targets --message-format=short` (expect green; if upstream itself fails, STOP and surface — do not fix upstream in this phase). Install the pinned toolchain if absent: `rustup toolchain install 1.93.0 --component rustfmt clippy`. Capture baselines into `docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/`: `cargo +1.93.0 fmt --all -- --check 2>&1 | tee baseline/fmt-baseline.txt` (record diff count; non-empty is acceptable — it's the comparison anchor) and the clippy error-file set similarly. Every later theme gate compares against these files: equal counts/sets = neutral.

Tools to use: Bash, Write

##### Verify

- `cargo build --all-targets --message-format=short`
- `test -f docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

##### Timeout

600000

#### Step 1.3: Execution-time fact re-verification (Q1/Q3 residuals)

Verify at the tag and record in `baseline/facts-verified.md` with file:line evidence: (1) does `crates/zeroclaw-runtime/src/observability/otel.rs` apply `otel_headers` to BOTH trace and metric exporters (§3.9-3 discrepancy)? (2) does upstream openrouter accept a `reasoning` alias for `reasoning_content`? (3) does upstream extract generated images from the `images` response field? (4) does upstream emit Thinking TurnEvents (check `crates/zeroclaw-runtime/src/agent/thinking.rs` + TurnEvent enum)? (5) where does `process_message` live and does it run its own tool loop (`git grep -n 'fn process_message' v0.8.0 -- crates/zeroclaw-runtime/`)? These verdicts bind Steps 3.3, 5.2, and 6.3.

Tools to use: Bash, Write

##### Verify

- `test -f docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/facts-verified.md`
- `grep -c 'crates/' docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/facts-verified.md | awk '{exit ($1<5)}'`

#### Step 1.4: FORK_DELTA.md header (no rows)

Create `docs/FORK_DELTA.md` containing ONLY: the branch-semantics sentence ("branch `upstream` = the exact upstream ref the sovereign series is currently rebased onto; it advances only in the same operation that rebases the series (ff-only)"), the column schema + disposition enum from §3.7, the update protocol (row lands in the same commit as its divergence; `Fork-Delta: FD-NN` trailer mandatory), the rebase cadence (fetch weekly; rebase at least every upstream minor; escalate canary conflicts persisting past one minor), and an empty table. NO rows yet (methodology §2). Commit with trailer `Fork-Delta: FD-00` where FD-00 is the ledger-infrastructure row itself (the one row present).

Tools to use: Write, Bash

##### Verify

- `test -f docs/FORK_DELTA.md && grep -q 'FD-00' docs/FORK_DELTA.md`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-00'`

#### Gate

- `cargo build --all-targets --message-format=short`

### Phase 2: Dev loop — Dockerfile re-apply (F) then hotswap re-point (C)

#### Step 2.1: Dockerfile re-apply (FD-01)

Rewrite `Dockerfile` per §3.6: keep upstream v0.8.0's `# syntax=docker/dockerfile:1.7-labs` header, builder stage (`COPY --parents crates/*/Cargo.toml` stub-then-real, cache mounts with ids bumped to `zeroclaw-target-v080`/`zeroclaw-cargo-registry-v080`, `COPY locales.toml`, ≥1MB size guard), building only `-p zeroclawlabs --bin zeroclaw` with `ARG ZEROCLAW_CARGO_FEATURES="channel-lark,whatsapp-web,rag-pdf,observability-otel"`. DROP web-node/web-builder stages and every zerocode reference. Keep upstream's `dev` stage as-is. Replace the release stage with the fork's `praxis-install` stage (node:20-alpine, `ARG PRAXIS_VERSION=0.10.0`, `--mount=type=secret,id=npm_token`, scoped `.npmrc`, `rm -f ~/.npmrc`) + Wolfi release stage from the fork Dockerfile (wolfi-base, `apk add ca-certificates bash coreutils vim git nodejs`, `COPY --from=praxis-install /opt/praxis /opt/praxis` + `/usr/local/bin/praxis` symlink, `USER 65534`, `EXPOSE 42617`, `HEALTHCHECK CMD ["zeroclaw","status","--format=exit-code"]`, `ENTRYPOINT ["zeroclaw"] CMD ["daemon"]`). Env: `ENV ZEROCLAW_DATA_DIR=/zeroclaw-data/workspace` AND `ENV ZEROCLAW_WORKSPACE=/zeroclaw-data/workspace` (transition pair), `HOME=/zeroclaw-data`; NEVER set `ZEROCLAW_CONFIG_DIR`. Keep the builder's inline default-config block in upstream's v0.8.0 shape (`[risk_profiles.default]`). Use the fork Dockerfile at `archive/0.6.9-alpha-p10.7` as the source for the praxis/Wolfi stages (`git show archive/0.6.9-alpha-p10.7:Dockerfile`). Commit with `Fork-Delta: FD-01` + ledger row (crate(s): top-level; disposition: private).

Recovery: if `docker build` fails on a missing shared lib in Wolfi, `apk add` the specific lib (never curl/wget); if `--parents` fails, confirm the labs syntax line survived.

Tools to use: Read, Write, Edit, Bash

##### Verify

- `DOCKER_BUILDKIT=1 docker build --target release --platform linux/amd64 --secret id=npm_token,env=NPM_TOKEN -t clawcraft-claw-runtime:spec-smoke .`
- `docker run -d --rm --name zc-smoke clawcraft-claw-runtime:spec-smoke && sleep 8 && docker exec zc-smoke zeroclaw status --format=exit-code; RC=$?; docker exec zc-smoke praxis --version && docker exec zc-smoke sh -c 'ldd /usr/local/bin/zeroclaw | grep -c "not found" | grep -qx 0' && docker rm -f zc-smoke && exit $RC`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-01' && grep -q 'FD-01' docs/FORK_DELTA.md`

##### Timeout

600000

#### Step 2.2: Hotswap re-point (FD-02)

Edit `dev/hotswap/hotswap.sh`: build line → `cargo build --locked -p zeroclawlabs --bin zeroclaw --features "$FEATURES"` (FEATURES default unchanged: `channel-lark,whatsapp-web,rag-pdf,observability-otel`); add a `RESET_VOLUMES=1`-guarded block that runs `docker volume rm zeroclaw-hotswap-registry zeroclaw-hotswap-git zeroclaw-hotswap-target` and `docker rmi clawcraft-claw-runtime:hotswap-base`; switch the binary self-verify grep marker from `ws-activation-probe` to `lmnr.span.input` (or an env-overridable `HOTSWAP_MARKER`). `dev/hotswap/Dockerfile.builder` and `Justfile` unchanged. Document the one-time migration (volume reset + base re-pin from the new `:dev` image) in the commit body. Run the end-to-end: `RESET_VOLUMES=1 just claw-hotswap` against a running clawcraft dev stack if available; otherwise `docker build --target release … -t clawcraft-claw-runtime:dev .` + standalone container check. Commit `Fork-Delta: FD-02` + ledger row.

Tools to use: Read, Edit, Bash

##### Verify

- `bash -n dev/hotswap/hotswap.sh`
- `grep -q -- '-p zeroclawlabs --bin zeroclaw' dev/hotswap/hotswap.sh`
- `docker image inspect clawcraft-claw-runtime:dev --format '{{.Id}}'`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-02' && grep -q 'FD-02' docs/FORK_DELTA.md`

##### Timeout

600000

#### Gate

- `cargo build --all-targets --message-format=short`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 3: Theme B — praxis NextAction continuation auto-drive (FD-03)

#### Step 3.1: Re-home continuation.rs

Copy `git show archive/0.6.9-alpha-p10.7:src/agent/continuation.rs` → `crates/zeroclaw-runtime/src/agent/continuation.rs` intact (FSM, `MAX_AUTO_DRIVE_CALLS=32`, `MAX_AGENT_WORK_REDRIVES=3`, `safety_net_command`, all 16 unit tests). Register the module in `crates/zeroclaw-runtime/src/agent/mod.rs`. Fix only import paths (crate-relative), never behavior. Add the unknown-`kind` posture: unrecognized `kind` deserializes to no-continuation + `tracing::warn!` (decision §3.2). Do NOT add any `data`-inspection on null envelopes.

Tools to use: Bash, Write, Edit

##### Verify

- `cargo build -p zeroclawlabs --message-format=short`
- `cargo test -p zeroclawlabs continuation`

##### Timeout

600000

#### Step 3.2: Wire the guard into turn / turn_streamed

Port zc-g50j (`git show 9970e5536`) onto v0.8.0's `crates/zeroclaw-runtime/src/agent/agent.rs`: `drive_continuation_calls()` (resolve pending target → registered tool else shell; execute via the same tool-execution path as model-initiated calls for hook/span parity; emit `TurnEvent::ToolCall`/`ToolResult`), `inject_forcing_directive()`, `should_bypass_response_cache()` (first characterize v0.8.0's response cache — if none exists, drop the bypass with rationale in the commit body), forcing re-drive + exhaustion → safety net, termination guard in both `turn()` (~2081) and `turn_streamed_with_steering_state()` (~2390). Cancel precedence (decision §3.2): on cancel/steer-abort while `has_pending()`, fire the safety net on the pending bead then yield — add this to the existing cancel branch (agent.rs ~2476). Port the 12 scenario tests, adding the §4.2-6 cancel test and the §4.2-1 poison-data test.

Recovery: if line anchors moved, locate by symbol (`grep -n 'fn turn_streamed_with_steering_state\|fn turn(' crates/zeroclaw-runtime/src/agent/agent.rs`).

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclawlabs continuation`
- `cargo test -p zeroclawlabs turn`

##### Timeout

600000

#### Step 3.3: Wire the process_message path + subagent check

Using Step 1.3's verdict on `process_message` (the path `/api/chat` + 42618 webhook + gateway chat drive): if it runs its own tool loop (the v0.6.9 `run_tool_call_loop` descendant), port the rnk-h6g3 wiring (`git show 9b3cad4cb` loop_.rs hunks) there — per-turn driver, observe on every tool result, mechanical drive, forcing re-drive, termination guard; if it delegates to `Agent::turn*`, record that Step 3.2's wiring already covers it (no code) in the commit body. Then resolve R3: determine whether `spawn_subagent` children execute tools through a guarded `Agent::turn*` path (`crates/zeroclaw-runtime/src/tools/spawn_subagent.rs` + `subagent.rs`); if unguarded AND praxis is reachable by subagents, wire the same driver; if praxis is policy-excluded for subagents, document the scope-out with the policy line. Commit all of Phase 3 as ONE squashed commit `Fork-Delta: FD-03` + ledger row (private; rationale: §6.5 auto-drive mandate, absent upstream — evidence from Step 1.1 grep).

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclawlabs`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-03' && grep -q 'FD-03' docs/FORK_DELTA.md`

##### Timeout

600000

#### Gate

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclawlabs continuation`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 4: Pod-API compat — Themes E + G (FD-04, FD-05, FD-06)

#### Step 4.1: Config compat (FD-04 part 1)

In `crates/zeroclaw-config/src/schema.rs`: (a) add `pre_shared_token: Option<String>` to `GatewayConfig` (+ Default sites; template: `git show archive/0.6.9-alpha-p10.7:src/gateway/mod.rs` lines ~694–706 and fork schema); (b) custom deserializer for `ObservabilityConfig.otel_headers` accepting string `"k=v[,k=v]"` (first-`=` split) and map forms, empty ⇒ None; (c) confirm unknown-key tolerance for the fork-extra observability fields (`otel_deployment_environment`, `runtime_trace_mode/path/max_entries`) — if upstream uses `deny_unknown_fields` anywhere on these structs, re-add the fields; (d) golden test: a fixture reproducing clawcraft's verbatim rendered config (source: `/Users/reinova/code/soulbound-labs/clawcraft/apps/clawcraft/domain/claw-config.ts` `buildConfigToml` — copy the exact rendered shape incl. `[gateway] pre_shared_token`, `[channels_config.webhook]`, `[autonomy]`, `[observability]` with string `otel_headers` and `backend = "otel"`) parses + auto-migrates with webhook.default enabled/port/send_url intact, permissive tool policy, token present, otel backend selected.

Tools to use: Read, Edit, Write, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-config`

##### Timeout

600000

#### Step 4.2: Gateway — token seeding + POST /api/chat (FD-04 part 2, FD-05)

In `crates/zeroclaw-gateway/src/lib.rs`: (a) at the `PairingGuard::new(...)` construction (~1177), append `config.gateway.pre_shared_token` (plaintext; guard hashes on load) to the initial token list; port the fork's 4 token tests (`git show archive/0.6.9-alpha-p10.7:src/gateway/mod.rs` tests ~3840–3873). (b) Add `.route("/api/chat", post(handle_api_chat))` on the **long-running (600s) sub-router** next to `/api/cron/{id}/run`; `handle_api_chat` = the `handle_webhook` full-loop path (same body struct `{message}`, same bearer check, same `{"response","model"}` reply); body deserializer tolerates an optional `context` field (ignored); `session_id: None` unless `X-Session-Id` header present; agent alias = `?agent=` else `resolved_runtime_agent_alias()`; align `/ws/chat`'s alias resolution to the same default pick if it currently hard-requires one. Tests: 200 `{message}`, 200 `{message,context}`, 401 no-bearer, router-membership (timeout > 300s).

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-gateway`

##### Timeout

600000

#### Step 4.3: ws threadId → session_key (FD-06)

In `crates/zeroclaw-gateway/src/ws.rs`: port only the envelope-parsing half from the fork (`parse_thread_id`/`effective_thread_id` intent from `git show archive/0.6.9-alpha-p10.7:src/gateway/ws.rs` — typed `threadId` beats legacy `[conversationId:]` prefix); resolve the SessionBackend key **per message**: `thread:<id>` when threadId present, else the connection's session key (upstream behavior preserved for non-clawcraft clients). NO `replace_history`, NO hydration, NO switch-detection state. Never emit the prefixed key as an `X-Session-Id` value. Tests: typed-beats-legacy, deterministic key, fallback, distinct histories on thread switch.

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-gateway ws`

##### Timeout

600000

#### Step 4.4: Drops + regression pins

Add the webhook-only supervision regression pin ONLY if upstream's `webhook_only_config_is_supervised` test doesn't already cover the clawcraft-rendered shape (check `crates/zeroclaw-runtime/src/daemon/mod.rs:1641`; if covered, record the reference in the FD-04 ledger row rationale, no new code). Record DROPs in the spec's post-execution notes + theme commit body (NOT ledger rows): `a0d1a8fbd` ZEROCLAW_SYSTEM_DIR (superseded by per-agent workspace boundaries, zero matches at v0.8.0), `c276ffe6a` (absorbed: schema.rs:11089 + daemon test). Squash Phase 4 code into commits: FD-04 (config+token), FD-05 (/api/chat), FD-06 (ws threadId) — each with trailer + ledger row.

Tools to use: Read, Edit, Bash

##### Verify

- `cargo test -p zeroclawlabs daemon || cargo test webhook_only`
- `git log --format=%B -3 | grep -c 'Fork-Delta: FD-0[456]' | grep -qx 3`
- `grep -q 'FD-04' docs/FORK_DELTA.md && grep -q 'FD-05' docs/FORK_DELTA.md && grep -q 'FD-06' docs/FORK_DELTA.md`

#### Step 4.5: Cold-start harness

Write `dev/hotswap/verify-coldstart.sh` implementing §4.3 exactly: rebuild `:dev`, render a real dev config (`cd /Users/reinova/code/soulbound-labs/clawcraft && pnpm dev:claw:render` if the dev Convex stack is available; else a checked-in fixture matching the golden-test shape with a known token), `docker run` prod-shaped (config bind-mount at `/zeroclaw-data/.zeroclaw/config.toml`, named volume at `/zeroclaw-data/workspace`, image env only, `daemon`), then assert each check with explicit exit codes. Include the rollback drill as a documented (commented) section: boot the archived 0.6.9 image against the migrated volume, record the failure mode, document backup-restore. Rides the FD-04 commit or its own docs commit with the FD-04 trailer.

Tools to use: Write, Bash

##### Verify

- `bash -n dev/hotswap/verify-coldstart.sh`
- `bash dev/hotswap/verify-coldstart.sh`

##### Timeout

600000

#### Gate

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-gateway && cargo test -p zeroclaw-config`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 5: Theme A — Laminar salvage (FD-07)

#### Step 5.1: Re-home identity.rs, active.rs, util

Copy from `archive/0.6.9-alpha-p10.7`: `src/observability/identity.rs` and `src/observability/active.rs` → `crates/zeroclaw-runtime/src/observability/`; port `scrub_credentials` + `truncate_with_ellipsis` from fork `src/util.rs` into the runtime crate (or a shared home if one exists). Register modules; port the guard tests (unset/`claw-`-prefix/wrong-length ⇒ neither key; valid ⇒ both; `root_session_id_twin_follows_thread_id_guard_without_panic`). Preserve absence-not-empty semantics exactly.

Tools to use: Bash, Write, Edit

##### Verify

- `cargo build -p zeroclawlabs --message-format=short`
- `cargo test -p zeroclawlabs identity`

#### Step 5.2: Wire the pipeline

Per Step 1.3 verdict: if upstream's exporter doesn't apply `otel_headers`, wire `.with_headers()` on both exporters in `crates/zeroclaw-runtime/src/observability/otel.rs`; confirm endpoint = base URL + `/v1/traces`. Determine the trace-root span under the v0.8.0 turn model (the `execute_turn` info_span vs any ACP parent) and attach: `lmnr.span.input/output` mirrors on llm-call-equivalents + activation root (scrub+truncate once per site; tool-call-only summaries), `session_id`/`user_id`/`tags` association properties + `deployment.environment` stamp on every span (retained per mediation §3.4), `stamp_turn_exit` + OTel Status from the turn outcome. Audit single-exporting-observer across gateway/channels/runtime (grep for observer construction sites; assert one shared instance). Content gate: replicate the fork's current gating logic verbatim (inventory `git show archive/0.6.9-alpha-p10.7:src/observability/otel.rs` first). Squash into one commit `Fork-Delta: FD-07` + ledger row.

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclawlabs observability`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-07' && grep -q 'FD-07' docs/FORK_DELTA.md`

##### Timeout

600000

#### Step 5.3: Live Laminar battery (manual gate)

With the clawcraft dev Laminar profile up (`pnpm dev:laminar:up`) and a hotswapped `:dev` pod with valid `CLAW_USER_ID` + rendered config: drive one activation per ingress surface (/api/chat, 42618 webhook, /ws/chat) plus one multi-iteration tool loop and one forced-failure turn. Run the §4.4 ClickHouse checks + the blank-headers negative control. Record results in `docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/laminar-battery.md`. If the environment is unavailable, mark the step BLOCKED-manual and surface it — do not fake a pass.

Tools to use: Bash, Write

##### Verify

- `test -f docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/laminar-battery.md`

##### Timeout

600000

#### Gate

- `cargo build --all-targets --message-format=short`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 6: Theme D — OpenRouter thin (FD-08)

#### Step 6.1: Pod-user-id + [AUDIO:]

In `crates/zeroclaw-providers/src/openrouter.rs`: add `user: Option<String>` to `ChatRequest` and `NativeChatRequest` (serde skip-if-none), sourced from the validated `CLAW_USER_ID` helper (port from `git show ceadd3143`; strip `claw-` prefix, `^[a-z0-9]{32}$` gate — reuse/share the identity.rs validator, don't duplicate: single source per methodology §4). Port `[AUDIO:]` from `git show 17345a002` into `crates/zeroclaw-providers/src/multimodal.rs` (marker parsing, format detection) + openrouter `input_audio` parts + `max_audio_files`/`max_audio_size_mb` on the multimodal config (keys land WITH this consumer) + the 24 tests.

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-providers`

##### Timeout

600000

#### Step 6.2: Verify-then-drop verdicts

Per Step 1.3 verdicts, record in the FD-08 commit body: streaming (expect DROP — upstream `stream_chat()` verified), `reasoning` alias (drop if upstream accepts it; else port `git show f8f267879` thin), image-gen output extraction (port `git show 68bcc3d4a` thin ONLY if upstream lacks `images`-field extraction), Thinking events (drop if upstream TurnEvents cover reasoning_content on both paths; else port thin). Anything ported joins FD-08; anything dropped is recorded in the commit body + post-execution notes. Squash Phase 6 into one commit `Fork-Delta: FD-08` + ledger row.

Tools to use: Read, Edit, Bash

##### Verify

- `cargo build --all-targets --message-format=short`
- `cargo test -p zeroclaw-providers`
- `git log -1 --format=%B | grep -q 'Fork-Delta: FD-08' && grep -q 'FD-08' docs/FORK_DELTA.md`

#### Gate

- `cargo build --all-targets --message-format=short`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 7: Fork-sync system

#### Step 7.1: Trailer-bijection CI (FD-09)

Write `.github/workflows/fork-delta-check.yml`: on PR + push to `core/v0.8.0`, assert (i) every commit in `upstream..HEAD` carries a `Fork-Delta: FD-NN` trailer whose id exists in `docs/FORK_DELTA.md`; (ii) every ledger id matches ≥1 commit in that range; (iii) every `transitional` row has `end-state` + `removal-ref`. Implement as a ~40-line bash script step (git log --format + grep). Run the check locally against the series before committing. Commit `Fork-Delta: FD-09` + row.

Tools to use: Write, Bash

##### Verify

- `git log --format='%(trailers:key=Fork-Delta,valueonly)' upstream..HEAD | grep -v '^$' | sort -u | while read id; do grep -q "$id" docs/FORK_DELTA.md || exit 1; done`
- `test -f .github/workflows/fork-delta-check.yml`

#### Step 7.2: Conflict canary (FD-10)

Write `.github/workflows/conflict-canary.yml` per §3.6: weekly cron + `workflow_dispatch`; `fetch-depth: 0`; fetch upstream; walk `upstream..core/v0.8.0` commit-by-commit running `git merge-tree --write-tree $(git merge-base <tip> upstream/main) <cumulative-tip> upstream/main`; attribute each conflicted file to the commit's `Fork-Delta:` id; emit `GITHUB_STEP_SUMMARY` (upstream SHA · per-FD file list · total) and create/update one pinned tracking issue when total > 0. `permissions: {contents: read, issues: write}`. Informational only — the job itself always exits 0 on successful analysis. Dry-run the merge-tree walk locally. Commit `Fork-Delta: FD-10` + row.

Tools to use: Write, Bash

##### Verify

- `test -f .github/workflows/conflict-canary.yml`
- `git merge-tree --write-tree $(git merge-base HEAD upstream) HEAD upstream >/dev/null; test $? -le 1`

#### Step 7.3: Release workflow + runbook (FD-11)

Pre-flight: `git remote get-url origin` — if the fork is NOT under the soulbound-labs org, the praxis npm read needs a packages-read PAT (surface this; §15.23 avoids PATs). Write `.github/workflows/release-clawcraft-image.yml` per §3.6 (tag `clawcraft-v*` + dispatch; WIF; amd64; smoke-before-push; SHA-only tag; step summary). Write `docs/RELEASE.md`: the full dev→prod loop — sovereign commit → tag `clawcraft-v*` → CI builds+pushes `…:<git-sha>` → human runs `pnpm --filter @clawcraft/app exec convex env set CLAW_DOCKER_IMAGE "<uri>" --prod` (colorful-rook-584) → pods roll; rollback = re-pin previous SHA + brain.db `backup-*` restore procedure; the cross-repo prerequisite checklist from §3.8 (workspace pin render FIRST, RUST_LOG filters, relay_logs check, optional PVC snapshot); the rebase-cadence procedure. Note that the WIF pool extension for this repo is a clawcraft Terraform change (plan-reviewed). Commit `Fork-Delta: FD-11` + row.

Tools to use: Write, Bash

##### Verify

- `test -f .github/workflows/release-clawcraft-image.yml && test -f docs/RELEASE.md`
- `grep -q 'clawcraft-v' .github/workflows/release-clawcraft-image.yml && ! grep -q ':latest' .github/workflows/release-clawcraft-image.yml`
- `grep -q 'CLAW_DOCKER_IMAGE' docs/RELEASE.md && grep -q 'backup' docs/RELEASE.md`

#### Step 7.4: Archive transitional docs

The migration docs are transitional (methodology §3): move `migration-playbook.md` into `docs/tasks/ongoing/upstream-v0.8.0-migration/archive/` with a header banner "ARCHIVED — superseded by docs/FORK_DELTA.md (live ledger) + this spec"; verify FORK_DELTA.md's header carries the durable playbook-§7 content; append the DROP record (Themes H, SYSTEM_DIR, c276ffe6a, and Phase-6 drop verdicts) to this spec's post-execution notes section. List the §3.8 cross-repo follow-ups verbatim in `docs/RELEASE.md`'s prerequisites section (they're filed in clawcraft at rollout, not now). This commit carries the FD-00 trailer (ledger infrastructure).

Tools to use: Bash, Edit, Write

##### Verify

- `test -f docs/tasks/ongoing/upstream-v0.8.0-migration/archive/migration-playbook.md`
- `grep -q 'ARCHIVED' docs/tasks/ongoing/upstream-v0.8.0-migration/archive/migration-playbook.md`

#### Gate

- `cargo build --all-targets --message-format=short`
- `bash dev/hotswap/verify-coldstart.sh`
- `cargo +1.93.0 fmt --all -- --check 2>&1 | diff - docs/tasks/ongoing/upstream-v0.8.0-migration/baseline/fmt-baseline.txt`

### Phase 8: Doctrine Review

#### Step 8.1: Review Implementation Against Doctrines

Review all code written in this spec against the doctrines in `docs/doctrine/doctrine-manifest.yaml` (praxis, observability, claw, claw-state-machine, methodology, infra). For each: (1) Compliance — all MUST/MUST NOT followed? Pay specific attention to: §6.5 data-blindness (grep the driver for any `data.` access on null paths), observability resource-attr allowlist, never-`:latest`, trailer bijection. (2) New patterns worth doctrine (e.g. the string-or-map config-compat deserializer pattern; the coldstart harness). (3) Outdated rules found (the §3.8-4 doctrine reconciliations are pre-identified — record them). (4) Missing coverage (e.g. cancel-vs-guard precedence is a doctrine gap this spec decided by default). If ANY amendments needed, create `docs/tasks/ongoing/upstream-v0.8.0-migration/doctrine-amendments.md` in the template format. NOTE: doctrine bodies here are snapshots — amendments are routed to clawcraft (source of truth) as cross-repo follow-ups, per `docs/doctrine/SNAPSHOT-PROVENANCE.md`.

##### Verify

- `test -f docs/tasks/ongoing/upstream-v0.8.0-migration/doctrine-amendments.md && echo "Amendments documented" || echo "No amendments needed"`

#### Step 8.2: Queue Doctrine Amendments (if any)

If `doctrine-amendments.md` exists, queue each amendment as a bead for human triage, one bead per amendment:

```bash
tbd create -t chore -l doctrine-amendment --spec docs/tasks/ongoing/upstream-v0.8.0-migration/upstream-v0.8.0-migration-spec.md \
  -f docs/tasks/ongoing/upstream-v0.8.0-migration/doctrine-amendments.md "<amendment title>"
```

##### Verify

- `tbd list --label doctrine-amendment 2>/dev/null | grep -q . && echo "Amendments queued as beads" || echo "No amendments needed"`

---

## 9. Operational Queries

### Status check — series shape

```bash
git log --oneline upstream..core/v0.8.0            # the whole fork delta, at a glance
git log --format='%h %(trailers:key=Fork-Delta,valueonly) %s' upstream..core/v0.8.0
```

### Invariant audit — trailer↔ledger bijection (expected: no output)

```bash
git log --format='%(trailers:key=Fork-Delta,valueonly)' upstream..core/v0.8.0 | grep -v '^$' | sort -u \
  | while read id; do grep -q "$id" docs/FORK_DELTA.md || echo "MISSING ROW: $id"; done
```

### Recovery — conflict-canary manual run

```bash
git fetch upstream && git merge-tree --write-tree $(git merge-base core/v0.8.0 upstream/main) core/v0.8.0 upstream/main
```

### Prod verification — pod API (post-deploy)

```bash
curl -sf https://<pod>/health
curl -s -X POST https://<pod>/api/chat -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"message":"ping"}'
```

---

## 10. Spec Completeness Checklist

### Semantic Completeness
- [x] All data structures fully defined (FORK_DELTA schema §3.7; config fields §3.4; invariants §4.2)
- [x] All terms defined or linked (doctrine sections cited throughout)
- [x] State machines exhaustive (pod lifecycle referenced §3.3; ContinuationDriver FSM §3.2)
- [x] Enums closed (disposition enum §3.7; NextAction union §3.2; exit_reason set §3.4)
- [x] Nullability/defaults explicit (Option fields named; feature strings pinned)

### Verification Completeness
- [x] Each phase has executable verification + gates
- [x] Invariants have audit commands (§9)
- [x] Success criteria binary (§1.3)

### Recovery Completeness
- [x] FMEA table (§7)
- [x] Idempotency (single-backup check; per-boot migration idempotent; archive tag preserves everything)
- [x] Rollback defined (SHA re-pin + brain.db backup restore; Step 7.3 runbook)
- [x] Stuck-state recovery (§9 manual canary; BLOCKED-manual marking on live battery)

### Context Completeness
- [x] Brief linked (header)
- [x] Decision rationale captured (§3.2 decisions, §3.9 contradictions, Change Log defaults)
- [x] Change log present

### Boundary Completeness
- [x] Scope table (§2)
- [x] Auth explicit (§2 auth boundary)
- [x] External dependencies listed (clawcraft renderers, GCP AR, WIF, GitHub Packages, Laminar)
- [x] Interface contracts defined (pod API §3.3; NextAction §3.2; TOML shapes §3.4)

---

## 11. Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2026-07-09 | Initial spec. Defaults chosen autonomously (user ran headless): branch model = mirror `upstream` + sovereign `core/v0.8.0`, squashed-by-theme + FD trailers; cancel wins over termination guard (safety net fires); unknown NextAction.kind → no-continuation + WARN; token carrier Option A (re-home `pre_shared_token`), paired_tokens as follow-up; release trigger = `clawcraft-v*` tag + dispatch, no `:latest`, human-gated Convex bump; canary informational-only, escalate past one minor release; `deployment.environment` every-span stamp retained; `/api/chat` context field tolerated-and-ignored; `/ws/chat` alias aligned to legacy default pick; drops recorded in spec+archive, not ledger rows. |

---

### Post-execution notes

_(populated during execution — scope expansions per execution-format §7.1, drop verdicts from Steps 4.4/6.2, live-battery results.)_
