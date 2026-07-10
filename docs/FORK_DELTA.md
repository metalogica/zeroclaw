# FORK_DELTA.md — sovereign-delta ledger

This file is the single source of truth for every way the `core/v0.8.0` sovereign series
diverges from upstream. One row per **squashed theme-commit** on the series.

## Branch semantics

Branch `upstream` = the exact upstream ref the sovereign series is currently rebased onto; it
advances **only** in the same operation that rebases the series (ff-only). The sovereign series
`core/v0.8.0` carries the fork's durable delta as squashed-by-theme commits, each tagged with a
`Fork-Delta: FD-NN` trailer that maps to exactly one row below.

## Ledger schema

Columns: `id (FD-NN) | title | bead | crate(s) | disposition | rationale [| end-state | removal-ref]`

- **No SHA column** — SHAs are rebase-unstable. The commit↔row mapping is carried by the
  `Fork-Delta: FD-NN` commit trailer, enforced as a bijection over `upstream..HEAD` by
  `.github/workflows/fork-delta-check.yml` (bijection + §3-field completeness).
- **disposition** enum:
  - `private` — a permanent fork-local divergence, not intended to upstream.
  - `upstreaming` — headed upstream; **requires an open PR URL** in the rationale/end-state.
  - `transitional` — temporary; **requires `end-state` + `removal-ref`** (the condition and the
    tracking ref under which the row is deleted).

## Maintenance protocol

- **Rows land in the same commit as the divergence they describe** — never bulk-seeded
  (methodology §2). A theme-commit and its ledger row are one atomic change.
- The `Fork-Delta: FD-NN` trailer is **mandatory** on every sovereign-only commit and must match
  exactly one row here (and vice-versa).
- The ledger records **live divergences only**. Drops (things deliberately not carried forward)
  live in the migration spec + the archived playbook, not here.
- **CI workflow files get their own FD rows** (they are themselves fork divergence).

## Rebase cadence

- **Fetch weekly** (remote-tracking ref only — never auto-advancing `upstream`).
- **Rebase** the sovereign series onto the new upstream ref at least **every upstream minor
  release**; the `upstream` branch advances (ff-only) in that same operation.
- **Escalation:** conflicts surfaced by `conflict-canary.yml` that persist **past one upstream
  minor release** flip the tracking bead into a rebase-sprint.

## Ledger

| id (FD-NN) | title | bead | crate(s) | disposition | rationale | end-state | removal-ref |
|------------|-------|------|----------|-------------|-----------|-----------|-------------|
| FD-00 | Fork-delta ledger infrastructure (this file + trailer protocol) | zc-d5i0 | — | private | Establishes the sovereign-delta ledger, `Fork-Delta:` trailer protocol, branch semantics, and rebase cadence. Self-describing row so the trailer↔row bijection holds from the first commit. | — | — |
| FD-01 | Wolfi+praxis release image over v0.8.0 multi-crate builder | zc-n6so | Dockerfile | private | Fork ships a Wolfi runtime with a bundled praxis 0.10.0 sidecar (node) instead of upstream's distroless image; builder narrowed to `-p zeroclawlabs --bin zeroclaw`, web/zerocode stages dropped. | — | — |
| FD-02 | Hotswap dev-loop tooling re-pointed at multi-crate build | zc-8zdt | dev/hotswap | private | Fork's fast incremental dev-swap (named-volume caches, stdout binary extraction); build line re-pointed to `-p zeroclawlabs --bin zeroclaw`, RESET_VOLUMES one-time reset, self-verify marker → `lmnr.span.input`. | — | — |
| FD-03 | Praxis NextAction continuation auto-drive (keystone) | zc-pauf, zc-rfri, zc-n1mx | zeroclaw-runtime | private | Net-new upstream differentiator: `ContinuationDriver` FSM (`agent/continuation.rs`) — mechanical drive of `kind:"call"` chains with zero model round-trips, forcing-directive + bounded re-drive for `agent_work_then_call`, termination guard refusing turn-end while `has_pending()`, user-cancel precedence firing the §10.9 `praxis update … waiting_for` safety net, unknown-kind fail-open WARN. Contract values `MAX_AUTO_DRIVE_CALLS=32` / `MAX_AGENT_WORK_REDRIVES=3` co-designed with praxis park-chunking (§10.5). Wired into `Agent::turn`/`turn_streamed_with_steering_state` (agent.rs) AND the shared `run_tool_call_loop` (which `process_message` → `POST /api/chat` + 42618 webhook, channels orchestrator, and CLI `run` all delegate through) so the mimo stall cannot reach prod through any front door; guard is inert without a praxis envelope. `spawn_subagent` left as a depth-1-capped tool whose output flows through the parent driver (documented scope-out). | — | — |
| FD-04 | Gateway pre-shared-token auth + clawcraft config-compat | zc-2sw0, zc-qwhe | zeroclaw-config, zeroclaw-gateway | private | Fork adds `GatewayConfig.pre_shared_token`, seeded into `PairingGuard` (plaintext hashed-on-load) so clawcraft's rendered token authenticates pod `/api/*` + the 42618 webhook (Bearer); plus an `otel_headers` string-or-map deserializer and a golden clawcraft-config test pinning the rendered shape. Webhook-only supervision invariant is covered by upstream's `webhook_only_config_is_supervised` (daemon/mod.rs:1641; `has_supervised_channels`→`ChannelsConfig::has_any_enabled()` includes webhook, schema.rs:11089) — c276ffe6a absorbed upstream, no fork test added. Per-boot legacy TOML migration is upstream behavior, not fork delta. | — | — |
| FD-05 | POST /api/chat restore (pod-API relay endpoint) | zc-qwhe | zeroclaw-gateway | private | Thin alias of the full-loop `handle_webhook` path registered on the long-running 600s sub-router (the default 30s TimeoutLayer would kill multi-step turns; relay budget 300s). Body `{message, context?}` (`context` tolerated-and-ignored, regression-pinned so a future `deny_unknown_fields` can't break prod); reply `{"response","model"}` satisfies clawcraft's `content ?? reply ?? response` parse order; `session_id: None` unless `X-Session-Id`. Latent-gap fix: the route never existed at fork HEAD or v0.8.0; claw-doctrine §4.1/§5 mandate it. | — | — |
| FD-06 | ws threadId → thread:<id> per-message session key | zc-nvqs | zeroclaw-gateway | private | Clawcraft ws clients send a typed `threadId`; map it to a `thread:<id>` SessionBackend key per message (legacy `[conversationId:]` prefix fallback), preserving upstream connection-key behavior for other clients. No rehydration/switch-detection. | — | — |
