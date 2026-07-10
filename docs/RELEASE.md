# RELEASE.md — Clawcraft dev → prod release runbook (sovereign fork)

> **Scope.** How a sovereign-fork change becomes a running clawcraft prod pod,
> and how to roll it back. The image build/smoke/push is automated
> (`.github/workflows/release-clawcraft-image.yml`, FD-11); the prod cutover is
> **human-gated** (a `convex env set … --prod`). Nothing here mutates prod
> automatically.
>
> Related: `docs/FORK_DELTA.md` (the live sovereign-delta ledger + rebase
> cadence), the fork's `Dockerfile` (`release` target = Wolfi + praxis sidecar,
> FD-01), and the migration spec under
> `docs/tasks/ongoing/upstream-v0.8.0-migration/`.

---

## 0. Prerequisites (READ BEFORE THE FIRST RELEASE)

### 0.1 Packages-read PAT — REQUIRED for this fork (metalogica org)

This fork's `origin` is **`git@github.com:metalogica/zeroclaw.git`** — the repo
lives under the **`metalogica`** org, **NOT `soulbound-labs`**. The release
image bundles the **private** `@soulbound-labs/praxis` CLI, installed from GitHub
Packages during the Docker build.

**A repo under the `soulbound-labs` org could authenticate that npm read with the
ambient `GITHUB_TOKEN`. This fork cannot** — `GITHUB_TOKEN` does not carry
cross-org `read:packages`. Therefore the workflow consumes a dedicated
**`secrets.PRAXIS_PACKAGES_READ_PAT`**:

- A GitHub **Personal Access Token** (classic or fine-grained) with **`read:packages`**
  scope, authorized against the `soulbound-labs` org (SSO-authorized if the org
  enforces SAML SSO).
- Stored as the repo secret **`PRAXIS_PACKAGES_READ_PAT`** (Settings → Secrets and
  variables → Actions).
- The token is passed to BuildKit **as a `--secret` (`id=npm_token`)**, never as an
  `ARG`/`ENV`, so it never persists in an image layer (see the `praxis-install`
  stage in `Dockerfile`).

If this secret is missing/empty, the workflow **fails fast** with a message
pointing here — it does not silently fall back to `GITHUB_TOKEN`.

### 0.2 WIF (Workload Identity Federation) to GCP — no JSON keys

CI authenticates to GCP Artifact Registry via **WIF** (`google-github-actions/auth`),
never a downloaded JSON service-account key. Required repo **variables** (`vars.*`):

| variable | meaning |
|----------|---------|
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | full WIF provider resource name |
| `GCP_DEPLOY_SA_EMAIL` | AR-writer-scoped service-account email |
| `GCP_AR_REGISTRY` | Artifact Registry host, e.g. `us-docker.pkg.dev` |
| `GCP_AR_REPOSITORY` | `<project>/<repo>` path of the AR docker repository |

> **The WIF pool must be extended to trust THIS repo's OIDC subject.** That pool
> extension is a **clawcraft Terraform change (plan-reviewed)** — it is filed and
> reviewed in the clawcraft infra repo, **not** provisioned by this workflow.
> Until it lands, `google-github-actions/auth` fails with a trust error; that is
> the Terraform gate, not a workflow bug.

The service account must be scoped to **Artifact Registry writer** only.

---

## 1. The dev → prod loop

```
  sovereign commit on core/v0.8.0
            │
            ▼
  git tag clawcraft-v<N>   (or Actions → Run workflow)
            │
            ▼
  release-clawcraft-image.yml
    build (amd64, Dockerfile `release` target, praxis npm secret)
            │
            ▼
    SMOKE  (status + /health + praxis --version + ldd clean)  ── red ─▶ NO push, job fails
            │ green
            ▼
    push  <GCP_AR_REGISTRY>/<repo>/clawcraft-claw-runtime:<git-sha>   (SHA-only; NO :latest)
            │
            ▼
    step summary: image URI + digest + the bump command
            │
            ▼
  HUMAN: convex env set CLAW_DOCKER_IMAGE "<uri>" --prod   (deployment colorful-rook-584)
            │
            ▼
  clawcraft pods roll onto the new SHA
```

### 1.1 Cut a release

1. Land the change on the sovereign branch `core/v0.8.0` (each squashed
   theme-commit carries its `Fork-Delta: FD-NN` trailer + ledger row — see
   `docs/FORK_DELTA.md`).
2. Tag it and push the tag:

   ```sh
   git tag clawcraft-v<N>        # e.g. clawcraft-v12
   git push origin clawcraft-v<N>
   ```

   The `clawcraft-v*` tag push triggers `release-clawcraft-image.yml`. Or trigger
   manually: **Actions → Release Clawcraft Image → Run workflow** (`workflow_dispatch`).

3. Watch the run. On green it pushes exactly one immutable tag:

   ```
   <GCP_AR_REGISTRY>/<GCP_AR_REPOSITORY>/clawcraft-claw-runtime:<git-sha>
   ```

   **There is no `:latest`** — prod pins an immutable SHA so rollback is a re-pin.

4. Read the run's **step summary** for the exact image URI, the digest, and the
   copy-paste bump command.

### 1.2 Smoke gate (why a red smoke means no push)

The build loads the image locally and runs, **before any push**:

- `zeroclaw status --format=exit-code` (daemon reaches healthy status),
- host-`curl` of the unauthenticated `/health` endpoint (port 42617),
- `praxis --version` (the bundled sidecar is invocable),
- an `ldd` check that `/usr/local/bin/zeroclaw` has **no unresolved dynamic links**
  (a broken link would crash the pod at boot).

Only if all pass does the push step run. A red smoke fails the job with **nothing
pushed** — prod is never offered a broken SHA.

### 1.3 Cut over prod (human-gated)

Point clawcraft prod at the new SHA. Convex deployment: **`colorful-rook-584`**.

```sh
pnpm --filter @clawcraft/app exec convex env set CLAW_DOCKER_IMAGE "<uri>" --prod
```

where `<uri>` is the SHA-tagged URI from the step summary. The pods then roll onto
the new image. Verify:

- pods reach Ready and `/health` returns 200,
- `praxis --version` inside a pod matches the pinned praxis (0.10.0),
- brain.db resumed cleanly (see §2).

---

## 2. Rollback

Rollback is a **re-pin of the previous SHA** plus, if the bad image ran long
enough to migrate the database, a **brain.db backup restore**.

### 2.1 Re-pin the previous image SHA

```sh
pnpm --filter @clawcraft/app exec convex env set CLAW_DOCKER_IMAGE "<previous-sha-uri>" --prod
```

Because every image is SHA-tagged and immutable, the previous good SHA is always a
valid target — this is the whole reason there is no `:latest`. Pods roll back onto
the previous image.

### 2.2 Restore the brain.db backup (schema-migration rollback)

The **first v0.8.0 boot on an existing PVC is a schema-migration event**: the
runtime writes exactly one `brain.db.backup-*` snapshot (at
`/zeroclaw-data/workspace/memory/brain.db.backup-<timestamp>`) before migrating,
then migrates the live `brain.db` in place. A newer schema is **not** readable by
the older (rolled-back) image, so re-pinning the previous SHA alone is not enough —
you must also restore the pre-migration `backup-*`:

1. Scale the deployment down (or cordon the pod) so nothing is writing brain.db.
2. On the PVC, list the backups:
   ```sh
   ls -1 /zeroclaw-data/workspace/memory/brain.db.backup-*
   ```
3. Restore the snapshot taken **immediately before** the bad image's first boot:
   ```sh
   cp /zeroclaw-data/workspace/memory/brain.db \
      /zeroclaw-data/workspace/memory/brain.db.pre-rollback   # keep the bad state for forensics
   cp /zeroclaw-data/workspace/memory/brain.db.backup-<timestamp> \
      /zeroclaw-data/workspace/memory/brain.db
   ```
4. Scale back up on the **previous** image SHA (§2.1). The rolled-back image now
   reads a schema it understands.

> **Guard rails:** `ZEROCLAW_DATA_DIR=/zeroclaw-data/workspace` and
> `HOME=/zeroclaw-data` are baked into the image; **`ZEROCLAW_CONFIG_DIR` is never
> set** (setting it would re-pin data under `<config_dir>/data` and orphan the
> existing `/zeroclaw-data/workspace` PVC — and its brain.db backups).

---

## 3. Cross-repo prerequisites (clawcraft-side — filed in clawcraft AT ROLLOUT, not now)

These are **clawcraft-side** follow-ups tracked in clawcraft
`docs/cross-repo-followups.md`. They are recorded here so the release operator
sees them, but they are filed and executed **in clawcraft at rollout**, not in
this fork. Reproduced verbatim from the migration spec §3.8:

1. **Render `[agents.default.workspace] path = "/zeroclaw-data/workspace"`** in
   `buildConfigToml` + the dev renderer **BEFORE** the image bump (the old image
   warns-and-ignores; the new image needs it or agent state lands in the 10Mi
   emptyDir). **This is FIRST** — it must precede the `CLAW_DOCKER_IMAGE` bump.
2. **Check prod `relay_logs`** for `/api/chat` `http_error` (latent-gap
   confirmation).
3. **Update `RUST_LOG` module filters** in `gke.ts` (crate renames:
   `zeroclaw::gateway` → `zeroclaw_gateway`).
4. **Doctrine reconciliations:** claw §5.0 (42617 `/webhook` now full-loop),
   observability §7.1 allowlist (add `lmnr.association.properties.session_id`/`tags`,
   `agent.turn.exit_reason/iterations`), infra §6.9 trigger shape (tag vs merge) +
   `:latest` example removal, state-machine §2 startup-env diagram.
5. **(Optional) PVC snapshot** before the cutover, as belt-and-suspenders over the
   brain.db `backup-*` mechanism, for the schema-migration boot (§2.2).

> Ordering matters: **(1) workspace-pin render FIRST**, then the image bump, then
> the remaining checks. Item (5) is optional but recommended for the first
> v0.8.0 boot on any long-lived PVC.

---

## 4. Rebase cadence (keeping the fork current)

The sovereign series must not accumulate a multi-minor-version rebase backlog.
The durable procedure (see `docs/FORK_DELTA.md` header for the authoritative copy):

- **Fetch weekly** — a remote-tracking ref only. **Never auto-advance the
  `upstream` branch.** (`upstream` = the exact upstream ref the sovereign series
  is currently rebased onto; it advances **only** in the same operation that
  rebases the series, ff-only.)

  ```sh
  git fetch upstream --tags        # updates remote-tracking refs only
  ```

- **Rebase** the sovereign series (`core/v0.8.0`) onto the new upstream ref **at
  least every upstream minor release**. The `upstream` branch advances (ff-only)
  **in that same operation** — never separately.

- **Escalation:** conflicts surfaced by `conflict-canary.yml` (informational,
  never a permanently-red gate) that **persist past one upstream minor release**
  flip the tracking bead into a **rebase-sprint**.

- After any rebase, re-cut a `clawcraft-v*` tag and run through §1 again — SHAs are
  rebase-unstable, so the image is re-built and re-pinned from the new tip.

---

## 5. Quick reference

| action | command / location |
|--------|--------------------|
| Trigger release | push tag `clawcraft-v<N>` **or** Actions → Run workflow |
| Image tag pushed | `…/clawcraft-claw-runtime:<git-sha>` (SHA-only, no `:latest`) |
| Bump prod | `pnpm --filter @clawcraft/app exec convex env set CLAW_DOCKER_IMAGE "<uri>" --prod` |
| Convex prod deployment | `colorful-rook-584` |
| Rollback | re-pin previous SHA (§2.1) + restore brain.db `backup-*` (§2.2) |
| Praxis pin | `0.10.0` (`Dockerfile ARG PRAXIS_VERSION`; bump is out of scope for this workflow) |
| PAT secret | `PRAXIS_PACKAGES_READ_PAT` (read:packages; metalogica org can't use `GITHUB_TOKEN`) |
