# v0.8.0 Migration — Journey Status

> **Snapshot 2026-07-15.** High-level position only — the operational sources of truth are
> `docs/tasks/ongoing/upstream-v0.8.0-migration/EXECUTION-HANDOFF.md` (latest SESSION UPDATE
> banner), `docs/FORK_DELTA.md` + `docs/RELEASE.md` (on `core/v0.8.0`), and the bead tracker
> (`tbd list`). When this file and those disagree, trust those.

## The destination

**Arc 1 — SHIP:** the sovereign v0.8.0 runtime serving production clawcraft pods, reached
through a clean, reproducible dev→prod path (rendered config, smoke-gated SHA-only image,
human-gated cutover).

**Arc 2 — CODIFY:** a durable, CI-enforced fork-management standard ("fork management as
code"), proven by the v0.8.0 → v0.8.2 catch-up rebase — so the 84-commit, two-minor-version
dig this campaign escaped can never recur.

## Behind us ✅

The hard engineering is done. Epic `zc-137m` is closed:

- The fork delta was **re-homed onto upstream v0.8.0** as a linear, themed commit series on
  `core/v0.8.0` — every commit bound to the 13-row `FORK_DELTA.md` ledger by a
  `Fork-Delta:` trailer bijection (currently 18 commits ⇄ 13 rows, enforced by
  `fork-delta-check.yml`, green in CI).
- The **Laminar observability layer was rebuilt and live-proven** against dev ClickHouse
  (battery green for every clause not blocked by the config renderer).
- The **release pipeline exists**: `clawcraft-v*` tag → build → smoke-before-push →
  SHA-only image → human-gated Convex bump (`docs/RELEASE.md`).
- Arc-1 kickoff landed: the **prod-gating scrubber fix** (`zc-1qoq`, value-form Bearer +
  bare-prefix tokens) and the **repo transfer to `soulbound-labs/zeroclaw`**, which retired
  the cross-org praxis PAT entirely (ambient `GITHUB_TOKEN` now suffices).

## Where we are — Arc 1, ~60%, at the repo boundary

Everything executable *from this repo* is done. What remains lives in clawcraft or in
one-time provisioning:

| Step | What | Status |
|---|---|---|
| 1 | **`zc-zb2t`** (P1, **prod boot blocker**): clawcraft config renderer still emits the 0.6.9 schema — port to v0.8.0 schema-v3, fan provider keys, map tool auto-approval. Also unlocks the last two battery clauses (triggers 4 & 5). | open, clawcraft-side |
| 2 | Provisioning: praxis package Actions-access grant (UI toggle) · WIF Terraform PR in clawcraft (widen provider condition + `zeroclaw-image-pusher` SA, fully specified in the handoff banner) · 4 repo variables (values pinned in the banner) | open, ~1 hour |
| 3 | `zc-n6so`/`zc-8zdt` Docker smokes → **dev exit gate** (pod boots from rendered config, battery 5/5) | blocked on 1–2 |
| 4 | §3.8 prereqs in clawcraft (**workspace-pin render FIRST**), tag `clawcraft-v1`, CI push, `convex env set CLAW_DOCKER_IMAGE --prod` → pods roll | blocked on 3 |

Step 4 completing **is Arc 1 done**.

## Ahead — Arc 2, designed, not started

- The design brief exists (the "Sovereign Delta Standard": declared ledger, enforced
  bijection, observed drift via `conflict-canary.yml`, scheduled reconciliation) — next
  stop `/substrate:architect-spec` after ship.
- Its proof exercise is filed: **`zc-owuk`**, the v0.8.0 → v0.8.2 catch-up rebase.
  Deliberately post-ship, and honestly scoped: 364 upstream commits touching every hot
  fork file — a real rebase, not a formality. Precondition: land or re-base the
  observability-epic worktrees first.
- Then the 7 queued doctrine amendments batch into clawcraft (source of truth) and
  re-snapshot here, closing the loop (`doctrine-amendments.md`).

## One line

The code is shipped to the launchpad; what separates us from prod is one clawcraft
renderer port and about an hour of provisioning — and the sustainability machinery is
designed, filed, and waiting its turn.
