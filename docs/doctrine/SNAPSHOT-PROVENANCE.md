# Doctrine Snapshot Provenance

**These doctrines are SNAPSHOTS, not canonical.**

- **Source of truth:** `/Users/reinova/code/soulbound-labs/clawcraft/docs/doctrine/`
- **Snapshot taken from clawcraft @** `7f70c20`
- **Snapshot date:** 2026-07-09
- **Reason:** so the substrate SDD tooling (`/substrate:architect-spec` and its
  `doctrine-architect` subagents) can discover and read the binding doctrines *inside this
  repo* while planning the upstream-v0.8.0 migration — without cross-repo path reads or web
  calls.

## Rules

- **Do NOT edit these files here.** Fixes go to clawcraft, then re-snapshot.
- Treat them as **binding constraints** on any migration spec, not as zeroclaw-owned docs.
- They will drift from clawcraft over time. Before trusting a fine detail, diff against the
  clawcraft canonical copy. The standing fork-maintenance playbook should include a
  doctrine-refresh step.

## Snapshotted files (migration-relevant subset)

| File | Governs |
|------|---------|
| `architecture/praxis-doctrine.md` | Praxis task-tracker CLI; **NextAction `{data,next_action}` contract (§6.5)** |
| `architecture/observability-doctrine.md` | Agentic trace contract (Laminar/OTel); zeroclaw = sole emitter |
| `architecture/claw-doctrine.md` | Control-plane ↔ compute-plane (pod API, brain.db, workspace, tool policy) |
| `architecture/claw-system-state-machine-doctrine.md` | Agent lifecycle state machine |
| `architecture/methodology-doctrine.md` | Cross-cutting design principles (anti-duplication, no aspirational config, …) |
| `architecture/infra/infra-doctrine.md` | GCP infra; SHA-pinned tag-driven publish workflow |

The web-app (`frontend`/`backend`/`domain`/`style`/`3d-engine`) and `treasury` doctrines were
intentionally **not** snapshotted — they don't govern the runtime/infra migration.
