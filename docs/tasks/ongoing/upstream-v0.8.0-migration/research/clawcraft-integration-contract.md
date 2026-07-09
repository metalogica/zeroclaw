# Clawcraft integration contract — the "don't break prod" constraints

> **Purpose:** the hard constraints the migration MUST respect because clawcraft consumes this
> fork in production. Sourced from the clawcraft repo (`/Users/reinova/code/soulbound-labs/clawcraft`)
> and its doctrines. The binding doctrines are snapshotted into `docs/doctrine/` of *this* repo
> (see `docs/doctrine/SNAPSHOT-PROVENANCE.md`) so the SDD tooling can read them locally.
>
> **Rule of thumb:** if a migration step would change any interface in this document, it is a
> **breaking change to clawcraft prod** and must be called out explicitly in the spec.

---

## 1. How prod consumes the fork (the SHA-pin flow)

```
clawcraft (control plane, Convex)                zeroclaw fork (compute plane, the pod image)
────────────────────────────────                 ─────────────────────────────────────────────
CLAW_DOCKER_IMAGE  ─── Convex env, --prod ──►  GCP Artifact Registry image:
  (colorful-rook-584 deployment)                northamerica-northeast1-docker.pkg.dev/
                                                clawcraft-489901/clawcraft-images/
                                                clawcraft-claw-runtime:<fork-git-sha>
```

- **Prod pin is by git SHA of the fork commit** (e.g. `…:50564e311`) — **never `:latest`**
  (infra-doctrine tag-driven / build-time pinning). Pods roll automatically when
  `CLAW_DOCKER_IMAGE` changes.
- The image is built from this fork's `Dockerfile` (Wolfi runtime). CI:
  `.github/workflows/release-beta-on-push.yml` builds binaries + `Dockerfile.ci` /
  `Dockerfile.debian`, tags `ghcr.io/zeroclaw-labs/zeroclaw:<tag>` upstream-style; the clawcraft
  prod image is the SHA-tagged push to GCP AR.
- **Constraint:** the migration MUST keep producing a runnable single-binary image whose
  **pod API surface** (claw-doctrine: `/api/chat`, brain.db, workspace files, tool policy) is
  unchanged, so the existing Convex `CLAW_DOCKER_IMAGE` swap keeps working. A new dev→prod
  release loop is a **desired output** of the spec — but it must remain SHA-pinned and
  tag-driven.

## 2. The dev hotswap loop (MUST be preserved)

- Fork side: `dev/hotswap/hotswap.sh`, `dev/hotswap/Dockerfile.builder`, `Justfile`,
  `Dockerfile` — build a local binary and bake it into `clawcraft-claw-runtime:dev`.
- Clawcraft side: `pnpm dev:claw:render && pnpm dev:claw:up` runs the docker-compose stack
  against the `:dev` image; `pnpm dev:claw:watch-praxis` rebuilds praxis in parallel.
- Bind mounts (per-user dev workspace): `/opt/praxis` ← `packages/praxis/` (ro);
  `/zeroclaw-data/workspace` and `/zeroclaw-data/.zeroclaw` ← `infra/local/claw-workspace/<userId>/…`.
- **Constraint:** after the migration, `hotswap.sh` must extract the correct **multi-crate
  build output** (the kernel binary; likely `--no-default-features` / a specific bin target) and
  still produce a runnable `clawcraft-claw-runtime:dev`. This is Theme C, and it's the *first*
  thing to land so all subsequent migration work can be tested locally.

## 3. Praxis (bundled dependency, not a submodule)

- Praxis = `@soulbound-labs/praxis`, published to GitHub Packages from clawcraft
  (`packages/praxis/`) on a `praxis-v<semver>` tag.
- The fork **consumes it at image-build time** via `Dockerfile ARG PRAXIS_VERSION` (currently
  `0.10.0`) → `npm install -g @soulbound-labs/praxis@${PRAXIS_VERSION}` →
  `/usr/local/bin/praxis`. One-way registry coupling only; no submodules.
- **Constraint:** the new v0.8.0 `Dockerfile` MUST keep the `PRAXIS_VERSION` ARG and bundle
  praxis identically (Theme F).

## 4. Laminar / observability (config carrier, NOT env)

- Laminar is the OTel-native trace backend (self-hosted dev via docker-compose `laminar`
  profile; managed Laminar Cloud in prod, live since 2026-06-02).
- **Config is carried in `config.toml [observability]`**, NOT pod env vars:
  ```toml
  [observability]
  backend       = "otlp"           # or "" to disable
  otel_endpoint = "…"
  otel_headers  = "Authorization=Bearer <project-api-key>"
  ```
  (rendered by clawcraft: prod `apps/clawcraft/convex/clients/gke.ts`, dev
  `infra/scripts/dev/render-claw-config.ts`).
- **Constraint (observability-doctrine):** zeroclaw is the **sole trace emitter**; trace scope =
  agent intent, originating at pod channel ingress; resource attrs limited to `service.name` +
  `deployment.environment` (**zero PII/creds**). The salvaged Laminar layer (Theme A) must emit
  onto this same config-carried pipeline — do not reintroduce `OTEL_*` env-var config.

## 5. Binding doctrines (source of truth = clawcraft; snapshot in `docs/doctrine/`)

### praxis-doctrine §6.5 — the NextAction contract (the fork's keystone constraint)
The agent-facing continuation envelope is `{ data, next_action }`:
- `next_action = null` is an **UNCONDITIONAL turn-end**; the runtime **NEVER inspects `data`**
  when `next_action: null`.
- **PARK** shape = `next_action: null` + `data.parked` (emitted **only** by `execute`, never by
  `update`). The agent reads `data` to disambiguate park kind (timer / event / human-needs).
- Four execution-path emitters, priority order: (1) ready non-empty → `agent_work_then_call`;
  (2) all closeable → `call` praxis verify; (3) ready empty ∧ (parked|needs_user) non-empty →
  PARK; (4) wedge (blocked, non-closeable) → `null` + `data.parked` with blocker details.
- `update --output` emits an execute-continuation **unconditionally** (safety net, even on
  auto-verifier failure).
- **Constraint:** Theme B's continuation auto-drive (`continuation.rs` + turn/turn_streamed
  guard) implements this contract. The v0.8.0 re-home MUST preserve it byte-for-behavior. The
  runtime does **not** auto-drive `agent_work_then_call` beyond what the doctrine specifies.

### claw-doctrine — control plane ↔ compute plane
Pod API (`/api/chat`), brain.db, workspace files, tool policy define the boundary Convex
depends on. Gateway/threadId continuity (Theme E) must be re-expressed on v0.8.0's per-agent
ACP session model **without changing the pod API contract**.

### infra-doctrine — tag-driven publish
SHA-pinned images, tag-driven publish, no `:latest` in prod. The new upstream-sync + release
system must fit this (mirror branch → SHA-tagged image → `CLAW_DOCKER_IMAGE` bump).

### methodology-doctrine — how to sequence
Anti-duplication (single source of truth — hence the `FORK_DELTA.md` ledger idea), no
aspirational config, transitional-schema discipline, inventory-as-debugging. Governs the
migration sequencing and keeps the fork delta small and rebasable.

## 6. Cross-repo follow-ups

Clawcraft tracks zeroclaw-fork work in `docs/cross-repo-followups.md` (~35 entries). The
continuation auto-drive originated as cross-repo follow-up `rnk-h6g3`. When the migration lands,
reconcile the relevant follow-ups there.

---

## 7. Checklist — interfaces a migration step MUST NOT silently break

- [ ] Pod image is a runnable single binary with the **same pod API** (`/api/chat`, brain.db,
      workspace, tool policy) → `CLAW_DOCKER_IMAGE` swap still works.
- [ ] `Dockerfile ARG PRAXIS_VERSION` still bundles `/usr/local/bin/praxis`.
- [ ] Observability still config-carried via `config.toml [observability]` (no `OTEL_*` env).
- [ ] Laminar `session_id` / `user_id` / turn-outcome association properties still emitted.
- [ ] praxis NextAction §6.5 contract preserved exactly (null = turn-end, PARK shape, etc.).
- [ ] `hotswap.sh` still yields a runnable `clawcraft-claw-runtime:dev` from the new build.
- [ ] Image remains **SHA-pinned / tag-driven** (never `:latest`) in prod.
