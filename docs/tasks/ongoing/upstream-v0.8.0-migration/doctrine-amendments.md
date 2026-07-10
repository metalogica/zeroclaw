# Doctrine amendments — upstream-v0.8.0 migration (zc-garf, Phase 8)

**Review scope:** all code written by the `upstream-v0.8.0-migration` spec, against the six
migration-relevant doctrine snapshots registered in `docs/doctrine/doctrine-manifest.yaml`
(praxis, observability, claw, claw-state-machine, methodology, infra).

> **⚠ These doctrines are SNAPSHOTS, not canonical** (see `docs/doctrine/SNAPSHOT-PROVENANCE.md`;
> source of truth = clawcraft `docs/doctrine/` @ `7f70c20`). **Do NOT edit the snapshot bodies
> in this repo.** Every amendment below is routed to **clawcraft as a cross-repo follow-up**
> (filed in clawcraft `docs/cross-repo-followups.md` at rollout, alongside the spec §3.8 items),
> then re-snapshotted. Each amendment is also queued as a `doctrine-amendment` bead for human
> triage (Step 8.2).

---

## Part A — Compliance review (MUST / MUST NOT) — ALL PASS

No doctrine violations were found in the migration code. The four spec-flagged high-risk checks:

| Check | Doctrine | Result | Evidence |
|-------|----------|--------|----------|
| §6.5 data-blindness — driver never reads `data` on a null `next_action` | praxis | **PASS** | `continuation.rs:102` `parse_envelope_next_action` returns terminal on `next_action.is_null()` without dereferencing `data`; regression-pinned by the `{"data":{"x":1}}` → terminal test (`continuation.rs:494`). |
| Resource-attr allowlist — `service.name` + `deployment.environment` only, zero PII/creds | observability | **PASS** | `otel.rs:130 build_otlp_resource` carries only `with_service_name` + `deployment.environment` (when configured), "kept deliberately minimal — resource attributes are process-global". session_id/user_id are span association-properties, never resource. |
| Never-`:latest` on the **published** image tag | infra | **PASS** | `release-clawcraft-image.yml:60` `IMAGE_URI = …:${{ github.sha }}`; `tags: ${{ env.IMAGE_URI }}` — SHA-only, never `:latest`. (Minor: `Dockerfile:177` uses `wolfi-base:latest` as a **build-stage base**, not the published tag — a reproducibility nit, not a violation; see A6 note.) |
| Trailer ↔ ledger bijection over `refs/heads/upstream..core/v0.8.0` | infra / methodology | **PASS (GREEN)** | 16 commits ⇄ 13 FD rows (FD-04 ×2, FD-07 ×3); all rows `private`; linear history. Recorded in the spec Post-execution notes final audit (zc-t0ii). |

---

## Part B — Amendments (doctrine is stale / missing coverage; code is compliant)

Category legend: **[outdated]** doctrine text no longer matches reality · **[new-pattern]** a
pattern the migration proved worth codifying · **[missing]** a decision the spec made by default
because the doctrine was silent.

### A1 — [outdated] claw §5.0: the 42617 gateway relay (`/api/chat`) is now a full agent-loop path

- **Doctrine:** `architecture/claw-doctrine.md` §5.0 (Trap: `/webhook` overloaded across 42617/42618).
- **Finding:** §5.0 correctly distinguishes 42617 (`{message}`, gateway) from 42618 (`{sender,content}`, channel). FD-05 restored `POST /api/chat` as a **thin alias of the full `handle_webhook` loop**, registered on the **600s sub-router** (the default 30s `TimeoutLayer` would kill multi-step turns; relay budget 300s). The doctrine's "different agent-loop semantics" bullet should record that the 42617 relay path now drives the full tool-call loop (multi-step continuation), not a single-shot reply.
- **Proposed change:** add to §5.0 that the gateway relay (`/api/chat`, FD-05) runs the full agent loop on the long-running sub-router; body `{message, context?}` (`context` tolerated-and-ignored), reply `{response, model}`; `session_id: None` unless `X-Session-Id`.

### A2 — [outdated] observability §7.1: extend the span-attribute allowlist with the fork's Laminar attrs

- **Doctrine:** `architecture/observability-doctrine.md` §7.1 (dual-emit resource+span; zero-credential allowlist).
- **Finding:** FD-07 re-homes the Laminar layer, which stamps span attrs the doctrine's allowlist does not yet enumerate: `lmnr.association.properties.session_id` / `lmnr.association.properties.tags`, and the turn-outcome attrs `agent.turn.exit_reason` / `agent.turn.iterations`. These are all non-PII/non-credential (session_id is a non-PII correlation key; user_id only past the `CLAW_USER_ID` 32-char gate) and are consistent with §7.1's zero-credential rule — but the allowlist text should acknowledge them so a future audit doesn't flag them as unknown.
- **Proposed change:** add the four attrs above to the §7.1 span-attr allowlist, noting they are association-properties / turn-outcome (span-scope, never resource-scope).

### A3 — [outdated] infra §6.9: the doctrine's own build example pushes `:latest` (violates never-`:latest`) and uses the wrong trigger shape

- **Doctrine:** `architecture/infra/infra-doctrine.md` §6.9 (Docker Image Build (CI)), lines ~750–752.
- **Finding:** the §6.9 example does `docker build … -t ${IMAGE}:${GIT_SHA} -t ${IMAGE}:latest` **and** `docker push ${IMAGE}:latest` — the doctrine example itself violates the never-`:latest` rule the migration's release workflow correctly follows (SHA-only, `IMAGE_URI:${{ github.sha }}`). Separately, §6.9 frames the trigger as merge/push-driven ("Push → CI triggers"), whereas the migration ships a **tag-driven** publish (`clawcraft-v*` tag + `workflow_dispatch`, per FD-11 / spec §3.6).
- **Proposed change:** (i) remove the `:latest` build-tag and the `docker push …:latest` line from the §6.9 example; (ii) update the trigger shape to tag-driven (`clawcraft-v*`) + dispatch, SHA-only push, smoke-before-push.

### A4 — [outdated] claw-state-machine §2: startup/config-pipeline diagram for v0.8.0

- **Doctrine:** `architecture/claw-system-state-machine-doctrine.md` §2 (Configuration Pipeline).
- **Finding:** two v0.8.0 deltas the §2 startup-env diagram/text should reflect: (i) `deployment.environment` is currently **env-sourced** (`ZEROCLAW_/OTEL_DEPLOYMENT_ENVIRONMENT`) because v0.8.0 config lacks the field (0.6.9 carried it in config) — see FD-07 follow-up; (ii) `RUST_LOG` module filters use the renamed crate paths (`zeroclaw::gateway` → `zeroclaw_gateway`) after the multi-crate workspace split.
- **Proposed change:** update the §2 startup-env pipeline to note the env-sourced `deployment.environment` (pending the config-field restore) and the crate-rename `RUST_LOG` filters.

### A5 — [new-pattern] methodology: tolerant-reader config-compat deserializer (string-or-map)

- **Doctrine:** `architecture/methodology-doctrine.md` (peer to §3 Transitional schema discipline).
- **Finding:** FD-04 added an `otel_headers` deserializer that accepts **both** the legacy string form and the new map form, so the clawcraft-rendered config migrates with **zero breaking change** — a golden test pins the rendered shape. This is a reusable transitional-schema pattern (a "tolerant reader" that widens the accepted input while the writer catches up), distinct from §2 (no aspirational config) and worth a worked example under §3.
- **Proposed change:** add a §3 worked example — the string-or-map config-compat deserializer — codifying: widen the reader to accept legacy ∪ new, pin the rendered shape with a golden test, and record the narrowing (drop legacy) as an end-state follow-up.

### A6 — [new-pattern] infra: the coldstart harness as a release gate

- **Doctrine:** `architecture/infra/infra-doctrine.md` (§6.9 CI / release-gate area).
- **Finding:** the migration added `dev/hotswap/verify-coldstart.sh` — a fresh-container boot-to-healthy proof used as a phase gate (it catches the class of failure where a cold container never reaches `/health`, invisible to a warm-cache dev loop). This is a durable release-gate pattern worth codifying alongside smoke-before-push. (Also note here: digest-pin `wolfi-base` rather than `:latest` for the release base image — the Part-A minor nit — as a reproducibility recommendation.)
- **Proposed change:** document `verify-coldstart.sh` as a recommended release/CI gate (fresh-container boot-to-healthy), and add a SHOULD to digest-pin the release base image.

### A7 — [missing] praxis: cancel-vs-guard precedence

- **Doctrine:** `architecture/praxis-doctrine.md` (§6.5 NextAction contract / continuation guard; §10.9 waiting_for safety net).
- **Finding:** the continuation termination guard refuses turn-end while `has_pending()`, but the spec decided **by default** (no doctrine rule existed) that a **user cancel wins over the guard**: cancel fires the §10.9 `praxis update … waiting_for` safety net rather than being suppressed by the pending-continuation guard. This precedence is a real invariant the code now encodes and the doctrine is silent on.
- **Proposed change:** codify in the praxis doctrine that **user-cancel takes precedence over the termination guard** — cancel is honored immediately and triggers the §10.9 `waiting_for` safety net; the guard only blocks a *model-initiated* turn-end while a continuation is pending.

---

## Routing

All seven amendments are **cross-repo follow-ups to clawcraft** (source of truth), to be filed in
clawcraft `docs/cross-repo-followups.md` at rollout and then re-snapshotted into this repo. This
extends spec §3.8 item 4 (which pre-identified A1–A4); A5–A7 were surfaced by this review.
Each is queued as a `doctrine-amendment` bead for human triage (Step 8.2).
