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
