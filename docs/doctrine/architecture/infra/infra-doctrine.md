# GCP Infrastructure Doctrine

**Version**: 2.51.0
**Status**: Binding
**Date**: 2026-07-06
**App**: Clawcraft (Managed ZeroClaw Hosting Platform)
**Runtime**: ZeroClaw v0.6.9-alpha-p10 (Rust core + Wolfi-base release stage with bash/coreutils/nodejs for `@soulbound-labs/praxis`; multi-stage build, ~398MB image). The "distroless, ~76MB single-binary" framing from v0.1.x is obsolete — see §6.10 and §15.24.

---

## 1. Authority

This document is **Binding**. Violations are architectural bugs.

Keywords MUST, MUST NOT, SHOULD, MAY follow RFC 2119.

**Reference Implementation**: `infra/terraform/` (this directory)

---

## 2. Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         Clawcraft SYSTEM ARCHITECTURE                           │
│                                                                              │
│  ┌────────────────────┐         ┌──────────────────────────────────────────┐ │
│  │   User (Browser)   │         │     GCP: northamerica-northeast1        │ │
│  │   Vite Web App     │         │            (Montreal)                    │ │
│  │   on Vercel        │         │                                          │ │
│  └────────┬───────────┘         │  ┌────────────────────────────────────┐  │ │
│           │                     │  │     GKE Autopilot Cluster          │  │ │
│           │ WebSocket/HTTP      │  │     "clawcraft-claw-cluster"          │  │ │
│           ▼                     │  │                                    │  │ │
│  ┌────────────────────┐        │  │  ┌──────────────────────────────┐ │  │ │
│  │   Convex Backend   │◄───────┤  │  │ NS: clawcraft-system         │ │  │ │
│  │   (Control Plane)  │        │  │  │  ┌────────────────────────┐  │ │  │ │
│  │                    │        │  │  │  │ Nginx WS Gateway       │  │ │  │ │
│  │  • User management │        │  │  │  │ gw.clawcraft.ca        │  │ │  │ │
│  │  • Stripe billing  │        │  │  │  │ auth_request → Convex  │  │ │  │ │
│  │  • Pod lifecycle   │────────┼──┤  │  │ proxy → pod /ws/chat   │  │ │  │ │
│  │  • Config gen      │        │  │  │  └────────────────────────┘  │ │  │ │
│  │  • Health checks   │        │  │  │  Placeholder Pod (warm node) │ │  │ │
│  │  • WS auth         │        │  │  └──────────────────────────────┘ │  │ │
│  └────────────────────┘        │  │                                    │  │ │
│                                 │  │  ┌──────────────────────────────┐ │  │ │
│                                 │  │  │  NS: claw-{userId}           │ │  │ │
│                                 │  │  │  ┌────────────┐              │ │  │ │
│                                 │  │  │  │ ZeroClaw   │ ◄── ConfigMap│ │  │ │
│                                 │  │  │  │ Pod 0 or 1 │    (per-user)│ │  │ │
│                                 │  │  │  │ :42617 gw  │              │ │  │ │
│                                 │  │  │  │ :42618 wh  │              │ │  │ │
│                                 │  │  │  └──────┬─────┘              │ │  │ │
│                                 │  │  │         │                    │ │  │ │
│                                 │  │  │  ┌──────▼─────┐             │ │  │ │
│                                 │  │  │  │ ClusterIP  │             │ │  │ │
│                                 │  │  │  │ Service    │             │ │  │ │
│                                 │  │  │  └────────────┘             │ │  │ │
│                                 │  │  │                              │ │  │ │
│                                 │  │  │  NetworkPolicy: ports 42617, │ │  │ │
│                                 │  │  │  42618 (allows clawcraft-    │ │  │ │
│                                 │  │  │  system namespace only)      │ │  │ │
│                                 │  │  └──────────────────────────────┘ │  │ │
│                                 │  └────────────────────────────────────┘  │ │
│                                 │                                          │ │
│                                 │  ┌────────────────────────────────────┐  │ │
│                                 │  │  Artifact Registry: clawcraft-images  │  │ │
│                                 │  │  clawcraft-claw-runtime:{tag}         │  │ │
│                                 │  └────────────────────────────────────┘  │ │
│                                 └──────────────────────────────────────────┘ │
│                                                                              │
│  ZeroClaw Pod ──► OpenRouter (LLM) ──► External channels (Telegram, etc.)   │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Component Responsibilities

| Component | Role | Owns |
|---|---|---|
| **Convex** | Control plane | Users, billing, pod lifecycle, config generation, WS auth, task relay |
| **ZeroClaw pod** | Execution engine | LLM inference (via OpenRouter), tools (including Composio integrations), memory, WS chat |
| **Nginx WS Gateway** | WebSocket proxy | Auth + route browser WS to per-user pod (`clawcraft-system` namespace) |
| **Vite/Vercel** | Frontend | UI, Google OAuth, chat interface, dashboard |
| **GKE Autopilot** | Compute | Pod scheduling, node management, scaling |
| **Artifact Registry** | Image store | ZeroClaw container images |

### 2.2 What Was Removed (vs v1.0)

| Removed | Reason |
|---|---|
| Vertex AI | LLM calls handled by ZeroClaw → OpenRouter. No GCP LLM dependency. |
| Ghost chat | Onboarding uses real ZeroClaw instance. No separate bot handoff. |
| `clients/vertex.ts` | No longer needed. |
| Workload Identity (Vertex binding) | Replaced: Workload Identity re-enabled for GCS Fuse — pods authenticate to GCS via per-namespace KSA `gcs-reader` bound to `clawcraft-gcs-reader` GSA. |
| PersistentVolumeClaims | Stateless pods. Convex is source of truth. SQLite ephemeral in-pod. |
| `clawcraft-claw-pod` service account | Replaced: pods now use `gcs-reader` KSA for GCS Fuse Workload Identity. |

### 2.3 Boundary Rules

- MUST deploy all GCP resources in `northamerica-northeast1` (Montreal). No exceptions.
- MUST use x86_64 (AMD64) architecture for ALL workloads in the cluster. ARM (T2A/Arm64) nodes are NOT available in `northamerica-northeast1` as of March 2026. All container images MUST be built for `linux/amd64`. All deployments MUST include `nodeSelector: { "kubernetes.io/arch": "amd64" }` to prevent scheduling failures if GKE Autopilot introduces mixed-arch node pools in the future. No ARM images, no multi-arch manifests, no `arm64` targets.
- MUST use Autopilot mode for GKE.
- MUST NOT call LLM APIs from Convex. All LLM inference happens inside ZeroClaw pods.
- Web chat uses WS gateway: User → Nginx → ZeroClaw pod (streaming). Scheduled tasks use Convex relay: Convex → ZeroClaw pod → Convex.
- MUST use GCS Fuse CSI driver for user file access in pods. Mounts MUST be `readOnly: true`, except the `/media` mount which is `readOnly: false` to allow agent-generated content output. Kernel-level `readOnly: true` on `user-storage` and `conversation-attachments` mounts provides defense in depth.
- **GCS bucket is the canonical storage backend.** Convex `_storage` is **deprecated** and migration-targeted. Two consumer surfaces still use `_storage` today (multimedia attachments via `/api/media-url`; email-ingest attachments via Cloudflare Worker → Convex action). Migration tracked at `docs/tasks/ongoing/storage-migrate-to-gcs/`. Access mechanism splits by consumer type:
  - **Pod consumers** read via the GCS Fuse mount (`/zeroclaw-data/workspace/{user-storage,conversation-attachments,media}/`). Fuse is unavailable to non-pod runtimes (V8 isolates, Convex Node actions in lightweight mode).
  - **Non-pod writers** (Cloudflare Workers, Convex `"use node"` actions) MUST use the GCS SDK with the `clawcraft-gcs-reader` service account (or an equivalent SA scoped to the relevant bucket prefix). Workers cannot mount Fuse; they go through a Convex action that does the SDK write.
  - **Dev environment**: docker volume mounts at the same paths replicate the prod Fuse abstraction. Same code on both sides; only the backend differs.
- MUST NOT introduce new uses of Convex `_storage` for user-attachable content. Code paths that legitimately need ephemeral, single-request blob storage (rare; usually a sign that the boundary is wrong) MUST justify the choice in a code comment with a doctrine pointer.

---

## 3. Developer Workflow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      DEVELOPMENT & DEPLOY FLOW                          │
│                                                                         │
│  TWO REPOS:                                                             │
│                                                                         │
│  ┌─────────────────────────┐    ┌────────────────────────────────────┐  │
│  │  zeroclaw (thin fork)   │    │  clawcraft (main repo)                │  │
│  │                         │    │                                    │  │
│  │  • ZeroClaw source      │    │  convex/        Control plane      │  │
│  │  • Dockerfile           │    │  src/            Vite frontend     │  │
│  │  • Pre-shared token     │    │  terraform/          Terraform + k8s   │  │
│  │    auth patch (~10 LOC) │    │  .github/        CI for Clawcraft     │  │
│  │  • .github/             │    │                                    │  │
│  │    CI: build + push     │    │  Only coupling: image tag string   │  │
│  └───────────┬─────────────┘    └──────────────────┬─────────────────┘  │
│              │                                      │                    │
│              ▼                                      │                    │
│  ┌─────────────────────────┐                       │                    │
│  │  ZEROCLAW IMAGE UPDATE  │                       │                    │
│  │                         │                       │                    │
│  │  1. git fetch upstream  │                       │                    │
│  │  2. git rebase onto     │                       │                    │
│  │     clawcraft branch       │                       │                    │
│  │  3. Push → CI triggers  │                       │                    │
│  │  4. CI: docker build    │                       │                    │
│  │     --target release    │                       │                    │
│  │  5. CI: docker push to  │──────────────────┐    │                    │
│  │     Artifact Registry   │                  │    │                    │
│  │  6. CI: update image    │                  │    │                    │
│  │     tag in Convex env   │                  ▼    ▼                    │
│  └─────────────────────────┘    ┌──────────────────────────────────┐   │
│                                 │   Artifact Registry (Montreal)   │   │
│                                 │   clawcraft-claw-runtime:{sha}      │   │
│                                 └──────────────────────────────────┘   │
│                                                                         │
│  INFRASTRUCTURE SETUP (one-time + maintenance):                         │
│                                                                         │
│  1. terraform apply          → GKE cluster, IAM, Artifact Registry     │
│  2. Deploy placeholder pod   → Keeps one node warm (~$30/mo)           │
│  3. ./terraform/setup-convex-env.sh [--prod]                           │
│                              → Sets GKE credentials from TF outputs    │
│  4. Manual: set secrets      → Docker image tag, ENCRYPTION_KEY, etc.  │
│                                                                         │
│  RUNTIME (automatic):                                                   │
│                                                                         │
│  • User opens chat page     → ws-auth wakes pod if reaped (scale 0→1) │
│  • Pod starts in 3-5s       → Image cached, node warm, binary instant  │
│  • Idle > 20min             → reapIdlePods cron scales 1→0 (§6.8)      │
│  • Any inbound wakes it      → web / email / Telegram → scaleUp        │
│  • New pods get latest image tag automatically                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Pod Lifecycle

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        POD LIFECYCLE (per user)                          │
│                                                                         │
│  FIRST TIME (signup):                                                   │
│                                                                         │
│  Google OAuth ──► Convex creates user ──► Create:                       │
│                                            • Namespace                  │
│                                            • ConfigMap                  │
│                                            • NetworkPolicy              │
│                                            • Deployment (replicas: 1)   │
│                                            • Service                    │
│                                           ──► Pod ready in ~3-5s        │
│                                           ──► User lands in chat        │
│                                                                         │
│  RETURNING USER:                                                        │
│                                                                         │
│  Opens chat page ──► Convex checks pod state                            │
│                      │                                                  │
│                      ├─ replicas: 1 ──► Already running, relay message  │
│                      │                                                  │
│                      └─ replicas: 0 ──► Patch to 1 (predictive)        │
│                                         Pod ready in ~3-5s              │
│                                         User types, pod is live         │
│                                                                         │
│  IDLE-REAP POLICY (§6.8):                                               │
│                                                                         │
│  Pods scale to 0 after CLAW_IDLE_REAP_SECONDS idle (prod 20min).        │
│  Any inbound (web/email/Telegram) wakes them; PVC + brain.db persist.   │
│  Unset CLAW_IDLE_REAP_SECONDS ⇒ reaper inert (always-on fallback).      │
│                                                                         │
│  *** Same mechanism for onboarding AND returning users ***              │
│  *** One code path. No ghost chat. No handoff. ***                      │
│                                                                         │
│  STATES:                                                                │
│                                                                         │
│   [not exists] ──signup──► [replicas: 1] ──idle>20min──► [replicas: 0] │
│                                   ▲                          │          │
│                                   └──── inbound wakes ───────┘          │
│                                   (web / email / Telegram → scaleUp)    │
│                                                                         │
│   [replicas: 0] ──cancel + 30 days──► [namespace deleted]              │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.1 Pod State Rules

- Pods are reaped to `replicas: 0` after `CLAW_IDLE_REAP_SECONDS` of inactivity (prod 1200 = 20 min; unset disables the reaper → always-on fallback). Idle is measured by `users.lastActivityAt`. See §6.8.
- MUST wake a `scaled_down` pod on **any** inbound activity: web chat (`ws-auth` + `persistUserMessage`), email (`insertInboundEmail`), Telegram (`telegramRelay`). Each schedules `scaleUp`.
- MUST scale to 1 when the WS gateway auth (`GET /api/ws-auth`) sees a `scaled_down` pod — it schedules `scaleUp` and denies the connect (503); the browser reconnects once `podState` flips to `running`.
- MUST NOT reap a pod mid-turn: `lastActivityAt` is bumped on assistant replies (`persistAssistantMessage`, `insertAssistantMessage`) and on every `ws-auth` connect, so active sessions and long agent turns stay alive.
- MUST only delete namespace 30 days after subscription cancellation.
- MUST NOT scale above 1 replica.
- MUST queue messages received during 0→1 transition and deliver when healthy (`deliverQueued` on `pollHealth` success).
- User-controlled sleep (manual scale-down from dashboard) is available via `podControls.pauseInstance`; the automatic reaper is independent of it.

---

## 5. Structural Conventions

### 5.1 Directory Layout

```
terraform/
├── main.tf                           # Provider, project, region lock
├── gke.tf                            # Autopilot cluster
├── gcs.tf                            # GCS bucket for user file storage (GCS Fuse)
├── iam.tf                            # Service accounts + bindings (incl. compute SA, GCS Fuse WI)
├── artifact-registry.tf              # Image repository
├── variables.tf                      # Environment-specific vars
├── outputs.tf                        # Cluster endpoint, SA emails
├── setup-convex-env.sh               # Post-apply: sets Convex env vars from TF outputs
├── clawcraft-convex-key.json         # SA key (gitignored, generated by gcloud)
├── k8s-templates/
│   ├── placeholder-pod.yaml          # Warm node placeholder (PriorityClass + NS + Deployment)
│   ├── namespace.yaml.tmpl           # claw-{userId} namespace
│   ├── deployment.yaml.tmpl          # ZeroClaw pod spec
│   ├── service.yaml.tmpl             # ClusterIP service (internal, gateway routes to it)
│   ├── configmap.yaml.tmpl           # Per-user config.toml
│   └── networkpolicy.yaml.tmpl       # Allow TCP 42617, 42618 from clawcraft-system namespace only
│
terraform/
├── cloudflare.tf                     # DNS records (gateway, frontend, email MX/SPF)
├── ws-gateway.tf                     # Nginx WS gateway (Terraform-managed K8s resources)
│                                     # Namespace, ConfigMap, Deployment, Service, BackendConfig

workers/
└── email-ingest/                     # Cloudflare Worker: CF Email Routing → Convex email proxy
    ├── src/index.ts                  # Parse email, attachment upload, JSON forward
    ├── wrangler.toml                 # Worker config (CONVEX_URL var, secrets)
    ├── package.json                  # Zero prod deps
    └── tsconfig.json                 # ES2022, @cloudflare/workers-types

local/                                # Local-equivalent of the prod stack
├── docker-compose.yml                # Pinned images + opt-in profiles: cloudflared (tunnel), claw, laminar
├── laminar/                          # Self-hosted Laminar assets (§15.25)
│   └── clickhouse-profiles-config.xml  # Vendored from upstream lmnr; CH date_time_input_format
└── .gitignore                        # .data/

scripts/                              # Operational scripts run interactively from a laptop
├── README.md                         # Conventions
├── lib/common.sh                     # Shared helpers (PROJECT_ID, REGION, log/warn/error, require_cmd)
├── ledger/get-auth-prod.sh           # Mint identity token for prod ledger calls
├── dev/bootstrap-tunnel.sh           # Per-dev Cloudflare tunnel bootstrap (§15.20)
├── dev/bootstrap-laminar.sh          # Self-hosted Laminar secret generation (§15.25)
└── dev/reset-laminar.sh              # Wipe the dev Laminar stack + volumes (§15.25)

.env.dev                              # Dev secrets (gitignored, mode 0600); see §15.21
.env.prod                             # Prod-equivalent secrets (gitignored, mode 0600); see §15.21
.envrc                                # direnv loader: sources .env.dev on `cd infra`
```

### 5.2 Naming Conventions

| Resource | Pattern | Example |
|---|---|---|
| GCP Project | `clawcraft-{id}` | `clawcraft-489901` |
| GKE Cluster | `clawcraft-claw-cluster` | — |
| Namespace | `claw-{userId}` | `claw-jk7a8b9c2d3e` |
| Deployment | `claw-{userId}` | `claw-jk7a8b9c2d3e` |
| Service | `claw-{userId}-svc` | `claw-jk7a8b9c2d3e-svc` |
| ConfigMap | `claw-{userId}-config` | `claw-jk7a8b9c2d3e-config` |
| NetworkPolicy | `claw-{userId}-netpol` | `claw-jk7a8b9c2d3e-netpol` |
| Docker Image | `clawcraft-claw-runtime:{sha}` | `clawcraft-claw-runtime:abc123f` |
| WS Gateway NS | `clawcraft-system` | — |
| WS Gateway Deployment | `ws-gateway` | — |
| WS Gateway Service | `ws-gateway-svc` | — |
| WS Gateway DNS | `gw.clawcraft.ca` | A-record → LoadBalancer IP |
| Email DNS | `agent.clawcraft.ca` | MX auto-managed by CF Email Routing |
| Email Worker | `email-ingest` | Cloudflare Worker |
| GCS Bucket (user data) | `clawcraft-489901-user-data` | `northamerica-northeast1` |
| SA (Convex) | `clawcraft-convex` | — |
| SA (GCS Fuse reader) | `clawcraft-gcs-reader` | — |
| KSA (GCS Fuse) | `gcs-reader` (per-namespace) | — |

### 5.3 Resource Labels

All K8s resources created by `gke.ts` MUST carry a standard label set via `resourceLabels()`:

| Label | Value | Purpose |
|---|---|---|
| `app` | `clawcraft` (namespace, PVC, ConfigMap, NetworkPolicy, Service) or `claw-{userId}` (Deployment, pod template — required by selector) | Resource ownership |
| `env` | `CLAWCRAFT_ENV` env var, defaults to `"dev"` | Dev/prod separation |
| `user` | `{userId}` | Per-user filtering |

**Deployment exception**: The Deployment and pod template use `app: claw-{userId}` (not `clawcraft`) because the pod selector (`matchLabels`) requires it. Selectors are immutable — changing them would break existing deployments. The `env` and `user` labels are added alongside `app` but NOT included in `matchLabels`.

**Rollout**: Labels are applied incrementally on the next lifecycle event (`provisionUser`, `reconcileDeployment`, `scaleUp`). No migration needed for existing resources.

**Setup**: Prod Convex deployment MUST set `CLAWCRAFT_ENV=prod`. Dev defaults to `"dev"` when unset.

```bash
# kubectl filtering examples
kubectl get ns -l env=prod              # All prod namespaces
kubectl get pods -A -l env=dev          # All dev pods
kubectl get all -A -l user=kd76q47...   # All resources for one user
kubectl delete ns -l env=dev            # Clean up all dev resources
```

---

## 6. Core Patterns

### 6.1 Terraform — Region Lock

```hcl
# terraform/main.tf
provider "google" {
  project = var.project_id
  region  = "northamerica-northeast1"
}

locals {
  region = "northamerica-northeast1"
  zone   = "northamerica-northeast1-a"
}
```

Rules:
- MUST hardcode region in `locals`. Never parameterize it.
- MUST NOT use `var.region`. The region is an invariant, not a variable.
- Rationale: Canadian data sovereignty.

### 6.2 Terraform — GKE Autopilot

```hcl
# terraform/gke.tf
resource "google_container_cluster" "claw_cluster" {
  name     = "clawcraft-claw-cluster"
  location = local.region

  enable_autopilot = true
  ip_allocation_policy {}

  release_channel {
    channel = "REGULAR"
  }
}
```

Rules:
- MUST set `enable_autopilot = true`.
- MUST use `REGULAR` release channel.
- MUST NOT create node pools (Autopilot manages them).
- No Workload Identity needed (pods don't call GCP APIs).
- MUST NOT add a `monitoring_config` block that disables Managed Prometheus or trims components — **Autopilot rejects it** (`--disable-managed-prometheus` errors; component trim fails `DeployPatch`). The ~$33/mo `Prometheus Samples Ingested` is a forced Autopilot cost; reduce it via GMP `OperatorConfig` filtering or a Standard migration only. See `infra-billing.md` §4.3.

### 6.3 Terraform — IAM (Simplified)

```hcl
# terraform/iam.tf

# SA for Convex actions (manages containers only)
resource "google_service_account" "convex" {
  account_id   = "clawcraft-convex"
  display_name = "Clawcraft Convex Backend"
}

resource "google_project_iam_member" "convex_container_admin" {
  project = var.project_id
  role    = "roles/container.admin"
  member  = "serviceAccount:${google_service_account.convex.email}"
}

resource "google_project_iam_member" "convex_ar_reader" {
  project = var.project_id
  role    = "roles/artifactregistry.reader"
  member  = "serviceAccount:${google_service_account.convex.email}"
}
```

Rules:
- `clawcraft-convex` MUST have `container.admin` + `artifactregistry.reader` + `storage.objectAdmin` on `clawcraft-489901-user-data` bucket.
- `clawcraft-gcs-reader` MUST have `storage.objectViewer` on `clawcraft-489901-user-data` bucket. Workload Identity binding: `[*/gcs-reader]` (all namespaces).
- MUST NOT grant broader roles (editor, owner).
- Default compute service account (`{PROJECT_NUMBER}-compute@developer.gserviceaccount.com`) MUST have `artifactregistry.reader` so GKE nodes can pull images from Artifact Registry. This is NOT managed by Terraform — it must be granted manually or via `gcloud` after project creation.
- Pod service account: `gcs-reader` KSA in each `claw-{userId}` namespace, annotated for Workload Identity to `clawcraft-gcs-reader` GSA. Provides read-only GCS access via GCS Fuse.

### 6.4 Container Provisioning (Convex Client)

```typescript
// convex/clients/gke.ts

// 1. Create API clients (authenticates via SA key → JWT → OAuth2 → GKE discovery)
const clients = await createK8sClients({
  clusterName: "clawcraft-claw-cluster",
  projectId: "clawcraft-489901",
  region: "northamerica-northeast1",
  serviceAccountKey: process.env.GKE_SERVICE_ACCOUNT_KEY!,
});

// 2. Provision all resources (idempotent, check-before-create)
//    Order: Namespace → [PVC, ConfigMap, NetworkPolicy, Service] (parallel) → Deployment
//    Returns { endpoint: null } — endpoint is deterministic ClusterIP DNS.
//    pollHealth polls at 1s intervals (500ms initial delay) until /health responds.
await provisionUser({
  clients,
  userId: user._id,
  plan: user.plan,
  openRouterApiKey: apiKey,
  preSharedToken: user.preSharedToken,
  convexUrl: process.env.CONVEX_URL!,
  convexServiceToken: process.env.CONTAINER_SERVICE_TOKEN!,
  dockerImage: process.env.CLAW_DOCKER_IMAGE!,
});

// 3. Declarative deployment reconciliation
//    Every operation that touches the deployment sends the full desired spec.
//    K8s diffs and applies: no-op if unchanged, rolling update if changed.
const deploymentSpec = buildDeploymentSpec({
  userId: user._id,
  dockerImage: process.env.CLAW_DOCKER_IMAGE!,
  convexUrl: process.env.CONVEX_URL!,
  convexServiceToken: process.env.CONTAINER_SERVICE_TOKEN!,
  replicas: 1,
  includeBootstrap: !persona?.onboardingComplete,
});
await reconcileDeployment({ clients, deploymentSpec, userId });

// 4. Scale up (self-healing: detects missing namespace, falls back to provision)
//    Checks getDeploymentStatus() first:
//    - If namespace/deployment exists: reconcile full resource set
//      (PVC, NetworkPolicy, Service in parallel) + updateConfigMap + reconcileDeployment
//    - If not_found: provisionUser (full reprovision — Namespace → PVC → ConfigMap → etc.)
await scaleUp({ clients, userId });
// This eliminates the "not_found" dead state — any caller gets resilient behavior.

// 5. Scale down (only operation that patches instead of reconciling)
await scaleDown({ clients, userId });   // replicas: 1 → 0
// Drift correction happens on the next reconcileDeployment (scale-up, restart, etc.)
```

Rules:
- MUST create resources in order: namespace → [PVC, ConfigMap, NetworkPolicy, Service, ServiceAccount] (parallel) → Deployment.
- MUST be idempotent (check-before-create on every resource).
- MUST NOT create more than one replica per user.
- MUST generate a pre-shared bearer token per user and store in Convex.
- MUST use `reconcileDeployment` (full spec replace) for all operations that start or restart a pod. MUST NOT use imperative patches (e.g., patch replicas, patch annotation) except for `scaleDown`.
- `scaleUp` and `restartPodForIntegration` MUST reconcile the full K8s resource set (PVC, NetworkPolicy, Service in parallel, then ConfigMap + Deployment) before starting or restarting a pod. This ensures port changes, NetworkPolicy updates, and new resources propagate on every lifecycle event — not only on initial provisioning. Previously only `provisionUser` ran the full set; `scaleUp` and `restartPodForIntegration` skipped Service and NetworkPolicy.
- `scaleUp` MUST check `getDeploymentStatus()` before patching. If namespace/deployment is `not_found`, MUST fall back to `provisionUser()` (full reprovision). This prevents the "not_found dead state" where a pod with a missing namespace has no recovery path.
- `ensureService` MUST use `patchNamespacedService` to update ports on existing services. The previous `replace` approach had an early-return bug — if the Service already existed as ClusterIP, it returned without updating ports. Patch is also required because `clusterIP` is immutable and must be preserved (replace would fail). New services are created normally.
- `scaleUp` MUST pass `restartAnnotation: new Date().toISOString()` to `buildDeploymentSpec`. This forces a K8s rolling update even when the spec is otherwise identical, resetting CrashLoopBackOff timers and ensuring a fresh pod on every scale-up.

### 6.5 K8s Template — ConfigMap

```yaml
# terraform/k8s-templates/configmap.yaml.tmpl
apiVersion: v1
kind: ConfigMap
metadata:
  name: claw-{{USER_ID}}-config
  namespace: claw-{{USER_ID}}
data:
  config.toml: |
    api_key = "{{OPENROUTER_API_KEY}}"
    default_provider = "openrouter"
    default_model = "anthropic/claude-sonnet-4"
    default_temperature = 0.7

    [gateway]
    port = 42617
    host = "0.0.0.0"
    pre_shared_token = "{{PRE_SHARED_TOKEN}}"
    allow_public_bind = true

    [memory]
    backend = "sqlite"
    auto_save = true

    [autonomy]
    level = "supervised"                # valid: readonly | supervised | full
    workspace_only = true
    max_actions_per_hour = {{MAX_ACTIONS}}
    max_cost_per_day_cents = {{MAX_COST_CENTS}}
    allowed_commands = []
    forbidden_paths = []
    non_cli_excluded_tools = [{{NON_CLI_EXCLUDED_TOOLS}}]

    [http_request]
    enabled = true
    timeout_secs = 120
    allowed_domains = ["{{CONVEX_DOMAIN}}", "{{CONVEX_SITE_DOMAIN}}"]

    [web_search]
    enabled = true
    timeout_secs = 120

    [web_fetch]
    timeout_secs = 120

    [identity]
    format = "openclaw"

    # NOTE: [channels_config.telegram] removed — Telegram is handled by
    # Convex webhook mode (POST /telegram-webhook). Pod has zero Telegram
    # awareness. Bot token lives in Convex DB, not ConfigMap.

    {{#if WHATSAPP_ENABLED}}
    [channels_config.whatsapp]
    access_token = "{{WHATSAPP_TOKEN}}"
    phone_number_id = "{{WHATSAPP_PHONE_ID}}"
    verify_token = "{{WHATSAPP_VERIFY}}"
    allowed_numbers = [{{WHATSAPP_ALLOWED}}]
    {{/if}}

  IDENTITY.md: |
    {{IDENTITY_MD}}
  SOUL.md: |
    {{SOUL_MD}}
  AGENTS.md: |
    {{AGENTS_MD}}
  TOOLS.md: |
    {{TOOLS_MD}}
  BOOTSTRAP.md: |
    {{BOOTSTRAP_MD}}
```

Rules:
- MUST mount `config.toml` via init container into a writable emptyDir at `/zeroclaw-data/.zeroclaw/`. Dockerfile defaults handle config resolution — no `ZEROCLAW_CONFIG_DIR` env var needed. The emptyDir is writable — ZeroClaw creates `.secret_key`, `daemon_state.json`, and `otp-secret` alongside `config.toml` at runtime.
- MUST mount system files (IDENTITY.md, SOUL.md, AGENTS.md, TOOLS.md, BOOTSTRAP.md) as individual ConfigMap subPath mounts at `/zeroclaw-data/workspace/system/*.md` with `readOnly: true`. `ZEROCLAW_SYSTEM_DIR=/zeroclaw-data/workspace/system` env var tells ZeroClaw where to find them.
- BOOTSTRAP.md is always included in the ConfigMap. It contains an idempotency guard ('if USER.md exists, skip onboarding') that makes it safe to include after onboarding completes.
- USER.md and MEMORY.md are agent-created on the workspace PVC, not ConfigMap-injected.
- MUST set `defaultMode: 0644` on volume mount (container runs as non-root; 0600 causes permission denied).
- MUST regenerate ConfigMap when user changes integrations in dashboard.
- MUST regenerate ConfigMap with workspace files on every `scaleUp` (persona + memories may have changed).
- MUST restart deployment (annotation-based rollout restart) after ConfigMap update — ZeroClaw reads `config.toml` once at startup.
- Integration-triggered ConfigMap update flow: `integrations.ts` mutation → schedules `integrationActions.restartPodForIntegration` → reads current DB state → reconciles full K8s resource set (NetworkPolicy, Service, ConfigMap in parallel) → `reconcileDeployment` → `pollHealth`.
- `non_cli_excluded_tools` field controls which tools are disabled on non-CLI channels. MUST be derived from ZeroClaw's actual `default_non_cli_excluded_tools()` in schema.rs (25 tools), minus deliberately enabled tools. Safe policy enables `http_request`, `image_info`, `memory_store`, and `memory_forget` (21 excluded). Full policy enables all (0 excluded).
- `[http_request]` section MUST always be present — `convexUrl` is a required parameter. Restricts agent HTTP requests to the Convex URL domain only (prevents data exfiltration). This is a security invariant.
- `[web_search]` section enables DuckDuckGo-based web search (no API key required).
- `[channels_config]` section is conditional — present when Telegram integration is connected. Contains `cli = true` (required by ZeroClaw's `ChannelsConfig` deserializer), `message_timeout_secs = 300`, and `[channels_config.telegram]` sub-table with decrypted bot token and `allowed_users = ["*"]`. The bot token is plaintext in the ConfigMap (deliberate trade-off — ZeroClaw's TOML parser has no env var interpolation).
- `[identity]` section sets the OpenClaw identity format for workspace file discovery.
- Plan-based limits:

| Plan | `max_actions_per_hour` | `max_cost_per_day_cents` |
|---|---|---|
| Trial | 10 | 100 |
| BYOK | 50 | 1000 |
| Pro | 100 | 2500 |

### 6.6 K8s Template — Deployment

```yaml
# terraform/k8s-templates/deployment.yaml.tmpl
apiVersion: apps/v1
kind: Deployment
metadata:
  name: claw-{{USER_ID}}
  namespace: claw-{{USER_ID}}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: claw-{{USER_ID}}
  template:
    metadata:
      labels:
        app: claw-{{USER_ID}}
    spec:
      securityContext:
        fsGroup: 65534
      nodeSelector:
        kubernetes.io/arch: amd64
      initContainers:
        - name: config-init
          image: {{INIT_CONTAINER_IMAGE}}
          command: ["sh", "-c", "cp /config-source/config.toml /zeroclaw-data/.zeroclaw/config.toml"]
          resources:
            requests:
              cpu: "10m"
              memory: "16Mi"
            limits:
              cpu: "10m"
              memory: "16Mi"
          volumeMounts:
            - name: config
              mountPath: /config-source
              readOnly: true
            - name: zeroclaw-config
              mountPath: /zeroclaw-data/.zeroclaw
      containers:
        - name: claw
          image: {{DOCKER_IMAGE}}
          workingDir: /zeroclaw-data/workspace
          ports:
            - containerPort: 42617
              name: gateway
            - containerPort: 42618
              name: webhook
          env:
            - name: CLAW_USER_ID
              value: "{{USER_ID}}"
            - name: CLAW_CONVEX_URL
              value: "{{CONVEX_URL}}"
            - name: CLAW_CONVEX_TOKEN
              value: "{{CONVEX_TOKEN}}"
            - name: ZEROCLAW_SYSTEM_DIR
              value: "/zeroclaw-data/workspace/system"
          resources:
            requests:
              cpu: "100m"
              memory: "128Mi"
            limits:
              cpu: "500m"
              memory: "512Mi"
          volumeMounts:
            - name: zeroclaw-config
              mountPath: /zeroclaw-data/.zeroclaw
            - name: data
              mountPath: /zeroclaw-data/workspace
            - name: config
              mountPath: /zeroclaw-data/workspace/system/IDENTITY.md
              subPath: IDENTITY.md
              readOnly: true
            - name: config
              mountPath: /zeroclaw-data/workspace/system/SOUL.md
              subPath: SOUL.md
              readOnly: true
            - name: config
              mountPath: /zeroclaw-data/workspace/system/AGENTS.md
              subPath: AGENTS.md
              readOnly: true
            - name: config
              mountPath: /zeroclaw-data/workspace/system/TOOLS.md
              subPath: TOOLS.md
              readOnly: true
            - name: config
              mountPath: /zeroclaw-data/workspace/system/BOOTSTRAP.md
              subPath: BOOTSTRAP.md
              readOnly: true
          readinessProbe:
            httpGet:
              path: /health
              port: 42617
            periodSeconds: 10
            failureThreshold: 3
          livenessProbe:
            httpGet:
              path: /health
              port: 42617
            periodSeconds: 30
            failureThreshold: 3
          startupProbe:
            httpGet:
              path: /health
              port: 42617
            periodSeconds: 1
            failureThreshold: 10
      volumes:
        - name: config
          configMap:
            name: claw-{{USER_ID}}-config
            defaultMode: 0644
        - name: data
          persistentVolumeClaim:
            claimName: claw-{{USER_ID}}-data
        - name: zeroclaw-config
          emptyDir:
            sizeLimit: "10Mi"
```

Notes vs v1.0:
- PVC per user (`claw-{userId}-data`, 1Gi, `standard-rwo`) mounted at `/zeroclaw-data/workspace` for durable workspace data (brain.db, USER.md, MEMORY.md, agent-created files). Survives pod restarts and scale-down/up cycles. Created during provisioning after namespace, before ConfigMap.
- `securityContext.fsGroup: 65534` ensures the non-root container (nobody/65534) can write to the PVC.
- `config-init` init container ALSO chowns `/zeroclaw-data/workspace` to `65534:65534` and mounts the workspace PVC for that purpose. The kubelet creates the PVC mount root as `root:nogroup` (uid 0, gid 65534 via `fsGroup`); `fsGroup` handles the group bit but not the owner. Without the chown, Git's dubious-ownership check (Git ≥2.35) refuses every command on the worktree-root-owned-by-root workspace — silently breaking the bootstrap's `git config` and every subsequent praxis auto-sync, with no projection ever reaching Convex. The chown is non-recursive on purpose: only the worktree root triggers Git's check; everything inside is created by the main container as 65534 or covered by `fsGroup`. Init containers default to root → can chown without a `securityContext` override. See `praxis-doctrine.md §7.6` for the full failure mode this prevents.
- `serviceAccountName: gcs-reader` — pods authenticate to GCS via Workload Identity for GCS Fuse mounts.
- Pod template annotation `gke-gcsfuse/volumes: "true"` triggers GCS Fuse sidecar injection on Autopilot.
- Three GCS Fuse CSI volumes: `gcs-user-storage` (only-dir=`{userId}/user-storage`, `readOnly: true`), `gcs-conversation-attachments` (only-dir=`{userId}/conversation-attachments`, `readOnly: true`), and `gcs-media` (only-dir=`{userId}/media`, `readOnly: false`). All use `implicit-dirs` mount option.
- Three GCS Fuse volume mounts: `/zeroclaw-data/workspace/user-storage` (`readOnly: true`), `/zeroclaw-data/workspace/conversation-attachments` (`readOnly: true`), and `/zeroclaw-data/workspace/media` (`readOnly: false`).
- Resources reduced: `100m/128Mi` request (was `250m/512Mi`).
- Startup probe tightened: `periodSeconds: 1, failureThreshold: 10` (10s max, was 120s). Readiness probe added (`periodSeconds: 10`).
- Two ports: `42617` (ZeroClaw gateway, handles API and health) and `42618` (webhook channel, receives inbound channel messages).
- System files (IDENTITY.md, SOUL.md, AGENTS.md, TOOLS.md, BOOTSTRAP.md) are individual ConfigMap subPath mounts at `/zeroclaw-data/workspace/system/*.md` with `readOnly: true`. SubPath mounts overlay onto the PVC without hiding existing PVC contents.
- BOOTSTRAP.md is always included (idempotency guard makes it safe post-onboarding).
- Two directories: `/zeroclaw-data/workspace` (PVC, writable workspace) and `/zeroclaw-data/.zeroclaw` (emptyDir, writable config). `/system` directory mount is gone — system files are subPath overlays inside the PVC at `/zeroclaw-data/workspace/system/`.
- Env vars: only `ZEROCLAW_SYSTEM_DIR=/zeroclaw-data/workspace/system`. `ZEROCLAW_CONFIG_DIR` and `ZEROCLAW_WORKSPACE` are removed — Dockerfile defaults are used.
- `workingDir: /zeroclaw-data/workspace` sets the container's working directory explicitly.
- Deployment is declarative: `buildDeploymentSpec` produces the complete desired spec, `reconcileDeployment` sends it to K8s. All three lifecycle paths (`provisionUser`, `scaleUp`, `restartPodForIntegration`) reconcile the full K8s resource set (PVC, NetworkPolicy, Service, ConfigMap, Deployment), preventing config drift. Only `scaleDown` uses an imperative replica patch (no drift risk when pod is off).

### 6.7 Wake-Up & Recovery (Convex)

A reaped pod (`scaled_down`, §6.8) is woken by inbound activity, not predictively. The web path is the subtle one: the browser connects through the Nginx WS gateway, which cannot proxy to a pod at `replicas: 0`. So the gateway auth endpoint owns the wake:

- `GET /api/ws-auth` calls `internal.users.resolveWsAuth`, which bumps `lastActivityAt`, and — if `podState === "scaled_down"` — schedules `internal.podActions.scaleUp`. It then **denies** the connect (503) for any non-`running` state so Nginx never proxies to a dead upstream.
- The frontend watches `podState` reactively (`usePodStatus`); the WS manager auto-reconnects, and once the pod flips to `running` the connect is granted (200, with `X-Pod-Upstream` / `X-Pod-Token`). Cold start is ~3–5s.
- **Hidden-tab reconnect suppression** (`src/lib/wsManager.ts` `scheduleReconnect`): the WS manager MUST NOT reconnect while `document.hidden`. Without it, a backgrounded tab whose pod reaps would immediately reconnect → `ws-auth` → wake, so it would bounce (reap → auto-wake) forever and never save cost. With the guard a hidden tab's pod reaps and **stays** `scaled_down`; `onVisibilityChange` reconnects (and `ws-auth` wakes the pod) the instant the user returns. A **foreground** tab still auto-reconnects, so it stays warm by design — savings come from closed/hidden tabs. Assistant replies persist to Convex regardless of socket state, so suppressing reconnect while hidden loses no messages.

Recovery from `error`/`not_found` is handled by the `healthCheckAll` cron (auto-restart up to 3×, then scale down). `stale-container-cleanup` handles stuck provisioning.

### 6.8 Idle-Pod Reaper

Tenant pods scale to `replicas: 0` after `CLAW_IDLE_REAP_SECONDS` of inactivity and wake on demand. This is the primary GKE cost lever (idle tenants otherwise bill ~$6/mo each indefinitely — see `docs/tasks/ongoing/gcp-cost-assessment/`). It replaces the earlier always-on policy, which had removed an even earlier `scaleDownIdle` cron; the reaper reinstates idle scale-down but with safe wake-on-inbound on every channel (the gap that justified "always-on" before).

**Mechanism:**
- `idle-pod-reaper` cron (`crons.ts`, 60s) → `internal.podActions.reapIdlePods`. It is **inert unless `CLAW_IDLE_REAP_SECONDS > 0`** — unset (the default, and the prod fallback if disabled) means no reaping at all.
- `reapIdlePods` reads `getRunningUsers` (only `podState === "running"`), skips `deletedAt`, and schedules `scaleDown` for any user whose `lastActivityAt` is older than the cutoff.
- `scaleDown` patches `replicas: 0` (no deletion — namespace, PVC, ConfigMap survive). `brain.db` persists on the PVC.

**Idle signal — `users.lastActivityAt`** is bumped by: web user message (`persistUserMessage`), assistant reply (`persistAssistantMessage` / `insertAssistantMessage` — mid-turn protection), verified inbound email (`insertInboundEmail`), and every `ws-auth` connect. A pod is reaped only when none of these has fired within the threshold.

**Wake paths** (all schedule `scaleUp`, then `pollHealth` flips `running` and runs `deliverQueued`):
- Web — `ws-auth` (§6.7) + `persistUserMessage`.
- Email — `insertInboundEmail` (verified inbound); the pending email batch-delivers on wake via `emailRelay`/`deliverQueued`.
- Telegram — `telegramRelay` (`skipped` outcome → `scaleUp`).
- *(Linq wake is deferred to a follow-up; until then Linq inbound queues until the next wake.)*

**Configuration:** prod sets `CLAW_IDLE_REAP_SECONDS=1200` (20 min). Local dev uses a short value via the pod-control sidecar (`docs/tasks/ongoing/idle-pod-reaper/`). `CLAW_LOCAL_POD_CONTROL_URL` MUST be unset in prod so the GKE scale path is used, never the local docker sidecar.

**User-controlled sleep** (`podControls.pauseInstance`) is independent of the automatic reaper.

### 6.9 Docker Image Build (CI)

```bash
GIT_SHA=$(git rev-parse --short HEAD)
IMAGE="northamerica-northeast1-docker.pkg.dev/clawcraft-489901/clawcraft-images/clawcraft-claw-runtime"

docker build --target release -t ${IMAGE}:${GIT_SHA} -t ${IMAGE}:latest .
docker push ${IMAGE}:${GIT_SHA}
docker push ${IMAGE}:latest

# check it exists
gcloud artifacts docker tags list northamerica-northeast1-docker.pkg.dev/clawcraft-489901/clawcraft-images/clawcraft-claw-runtime

# set convex env (full image URI including tag)
pnpx convex env set CLAW_DOCKER_IMAGE "northamerica-northeast1-docker.pkg.dev/clawcraft-489901/clawcraft-images/clawcraft-claw-runtime:${GIT_SHA}"
```

Rules:
- MUST tag with git SHA. Deploy using SHA tag, not `latest`.
- MUST store in Montreal Artifact Registry.
- MUST automate via GitHub Actions on merge to `main` on ZeroClaw fork.
- MUST update Convex env var `CLAW_DOCKER_IMAGE` (full URI with tag) after successful push.

### 6.10 Healthcheck overrides on Wolfi-base images

The ZeroClaw runtime's Stage 3 release image is `cgr.dev/chainguard/wolfi-base`,
which ships **without `wget` or `curl`**. Compose-level `healthcheck:` overrides
that call either binary will silently fail forever (the container holds the
`unhealthy` state while the underlying service is fine — diagnosable only via
`docker inspect --format='{{.State.Health.Log}}'`).

Rule: any `healthcheck:` override on a Wolfi-based image MUST use a binary
the image already exposes — for the ZeroClaw image that means
`["CMD", "zeroclaw", "status", "--format=exit-code"]` (the same command the
image's own `HEALTHCHECK` directive runs, just with tighter dev intervals).
Adding `wget`/`curl` to the image solely to satisfy a healthcheck is a
spec violation: the prod runtime is intentionally minimal.

Applies to: every consumer of `clawcraft-claw-runtime:*`. Reference
implementation: §15.24 dev compose declaration.

---

## 7. Warm Node Strategy

A single placeholder pod keeps one GKE Autopilot node alive at all times, ensuring sub-5-second pod scheduling for all users.

```yaml
# terraform/placeholder-pod.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: clawcraft-placeholder
  namespace: clawcraft-system
spec:
  replicas: 1
  selector:
    matchLabels:
      app: clawcraft-placeholder
  template:
    metadata:
      labels:
        app: clawcraft-placeholder
    spec:
      containers:
        - name: pause
          image: registry.k8s.io/pause:3.9
          resources:
            requests:
              cpu: "50m"
              memory: "64Mi"
            limits:
              cpu: "50m"
              memory: "64Mi"
      priorityClassName: low-priority
```

Rules:
- MUST use `pause` image (does nothing, minimal resources).
- MUST use low priority so it gets evicted first when real user pods need the node.
- SHOULD be removed when organic user load keeps nodes warm naturally (~50+ active users).
- Cost: ~$30/mo.

---

## 7.1 Nginx WebSocket Gateway

A single Nginx reverse proxy in `clawcraft-system` namespace routes browser WebSocket connections and relay traffic to per-user ZeroClaw pods. Resources are Terraform-managed in `infra/terraform/ws-gateway.tf` (namespace, configmap, deployment, service, BackendConfig).

### 7.1.1 Architecture

```
# WebSocket chat (browser → pod)
Browser → wss://gw.clawcraft.ca/ws/chat?token={JWT}
  → Cloudflare (orange cloud, Let's Encrypt TLS, HTTP/2)
    → GCP LoadBalancer (static IP 34.47.6.138, BackendConfig: idle timeout 3600s)
      → Nginx :8443 (Origin CA TLS, Full Strict)
        → auth_request → Convex GET /api/ws-auth (validates JWT, returns X-Pod-Upstream)
          → proxy_pass → http://{X-Pod-Upstream}/ws/chat (ClusterIP, no TLS)

# HTTP relay (Convex → gateway → pod /webhook)
Convex → POST https://gw.clawcraft.ca/relay/{userId}
  → Nginx validates X-Relay-Token header
    → proxy_pass → http://claw-{userId}-svc.claw-{userId}:42617/webhook
      (adds X-Webhook-Secret header from gateway_relay_token)

# WebSocket relay fallback (Convex → gateway → pod /ws/chat)
Convex → wss://gw.clawcraft.ca/ws/relay/{userId}
  → Nginx validates X-Relay-Token header
    → proxy_pass → ws://claw-{userId}-svc.claw-{userId}:42617/ws/chat
```

### 7.1.2 Resources

| Resource | Spec |
|---|---|
| Image | `nginx:1.27-alpine` |
| CPU request/limit | `100m` / `100m` |
| Memory request/limit | `64Mi` / `64Mi` |
| Replicas | 1 |
| Namespace | `clawcraft-system` |
| Node selector | `kubernetes.io/arch: amd64` |

### 7.1.3 DNS & TLS

- **DNS**: `gw.clawcraft.ca` A-record → static IP `34.47.6.138` (Cloudflare orange-cloud proxied, Terraform-managed via `google_compute_address` + `cloudflare_record`)
- **TLS (browser → Cloudflare)**: Cloudflare terminates with Let's Encrypt cert (auto-managed, HTTP/2, TLSv1.3). Browser sees trusted green lock.
- **TLS (Cloudflare → Nginx)**: Cloudflare Origin CA cert (RSA, 15yr validity, created manually in dashboard). Nginx terminates on port 8443. SSL mode: **Full (Strict)** — Cloudflare validates the Origin CA cert.
- **TLS resources**: Private key + Origin CA cert PEM stored as `kubernetes_secret` (`ws-gateway-tls`), mounted at `/etc/nginx/tls/`. Cert/key values passed via `terraform.tfvars` (gitignored).
- **Health check**: Separate plain HTTP server block on port 8080 (probes don't need TLS).

### 7.1.4 GCP BackendConfig

```yaml
apiVersion: cloud.google.com/v1
kind: BackendConfig
metadata:
  name: ws-gateway-backend-config
  namespace: clawcraft-system
spec:
  timeoutSec: 3600    # 1-hour idle timeout for WebSocket connections
```

The BackendConfig is referenced by the Service via annotation `cloud.google.com/backend-config: '{"default":"ws-gateway-backend-config"}'`. This prevents GCP's default 30-second idle timeout from closing long-lived WebSocket connections.

### 7.1.5 NetworkPolicy Integration

Per-user pod NetworkPolicy (`claw-{userId}-netpol`) is updated to allow ingress from `clawcraft-system` namespace in addition to existing rules:

```yaml
ingress:
  - from:
      - namespaceSelector:
          matchLabels:
            kubernetes.io/metadata.name: clawcraft-system
    ports:
      - port: 42617
        protocol: TCP
      - port: 42618
        protocol: TCP
```

### 7.1.6 Rules

- Gateway MUST be in `clawcraft-system` namespace (shared infrastructure, not per-user).
- Gateway MUST use `auth_request` to validate every WebSocket upgrade via Convex `GET /api/ws-auth`.
- Gateway MUST NOT store or cache auth tokens — stateless proxy only.
- Gateway MUST set `proxy_read_timeout` and `proxy_send_timeout` to at least 3600s for WebSocket keepalive.
- Gateway MUST forward `Upgrade` and `Connection` headers for WebSocket protocol upgrade.
- Gateway `/relay/{userId}` and `/ws/relay/{userId}` location blocks MUST validate `X-Relay-Token` header against the `gateway_relay_token` Terraform variable. Requests without a valid token MUST be rejected with 401.
- Gateway `/relay/{userId}` MUST proxy to pod `/webhook` endpoint, adding `X-Webhook-Secret` header (pre-shared token from gateway config).
- Gateway `/ws/relay/{userId}` MUST proxy WebSocket to pod `/ws/chat` endpoint.
- Pod NetworkPolicy MUST allow ingress from `clawcraft-system` namespace on ports 42617 and 42618.
- TLS termination MUST use two layers: Cloudflare (browser-facing, Let's Encrypt) + Nginx (Cloudflare-facing, Origin CA on port 8443). SSL mode MUST be Full (Strict).
- **Channel-specific inbound webhooks (Telegram, Linq, email) bypass the Nginx gateway entirely** — the external provider POSTs directly to a Convex `httpAction` (`/email-webhook`, `/linq-webhook`; Telegram native uses long-poll, no inbound webhook). The gateway is on the *outbound* (Convex → pod relay) hop only. MUST NOT add a per-user Nginx gateway route for a channel webhook (e.g. no `/linq/{userId}`). (add-linq)

---

## 8. Trust Boundaries

| Boundary | Enforcement |
|---|---|
| Convex → GKE API | SA `clawcraft-convex` with `container.admin`. Key as Convex env var. |
| Convex → ZeroClaw pod | Pre-shared bearer token. Generated per user, stored in Convex DB. |
| ZeroClaw → OpenRouter | API key in ConfigMap. Per-user (BYOK) or platform key. |
| ZeroClaw → Convex | Container service token in pod env. Webhook callbacks. |
| Convex → Telegram API | Bot token in Convex DB (encrypted). `setWebhook`, `deleteWebhook`, `sendMessage` from Convex actions. Pod never contacts Telegram. |
| Linq cloud → Convex `/linq-webhook` | HMAC-SHA256 over `${X-Webhook-Timestamp}.${raw_body_bytes}`, secret `LINQ_WEBHOOK_SIGNING_SECRET` (Convex env). Verified Convex-side; pod never sees the secret. (add-linq) |
| Convex (`linqRelay.sendOutbound`) → Linq Partner V3 | `Authorization: Bearer ${LINQ_PLATFORM_API_TOKEN}`. **Current outbound path** (Convex-side stopgap, claw-doctrine §3.5). (add-linq) |
| Pod (`LinqChannel::send`) → Linq Partner V3 | Same bearer, ConfigMap-loaded (`[channels_config.linq] api_token`). **Preferred target, NOT yet shipped** (sovereign-fork PR unmerged). (add-linq) |
| ~~Convex → Slack API~~ | Removed (2026-03-31). Slack integration will be re-added in a future release. |
| ZeroClaw → External channels | Channel credentials in ConfigMap (future: WhatsApp). Outbound only. |
| User pod → User pod | **Blocked.** NetworkPolicy restricts ingress to ports 42617 and 42618 only; cross-namespace blocked by selector (except `clawcraft-system`). |
| `clawcraft-system` → Pod (WS chat) | **Allowed on port 42617.** NetworkPolicy permits ingress from Nginx WS gateway namespace. No auth (ClusterIP, `require_pairing=false`). |
| `clawcraft-system` → Pod (relay) | **Allowed on ports 42617 and 42618.** Gateway `/relay/{userId}` proxies to pod `/webhook` on port 42618 with `X-Webhook-Secret` header. Gateway `/ws/relay/{userId}` proxies to pod `/ws/chat` on port 42617. |
| Convex → Nginx gateway (relay) | `X-Relay-Token` header on `/relay/{userId}`. Token from Convex env var `GATEWAY_RELAY_TOKEN`, validated by Nginx against `gateway_relay_token` Terraform var. |
| Pod → GCS (Fuse) | Workload Identity: KSA `gcs-reader` → GSA `clawcraft-gcs-reader` (`storage.objectAdmin`). User-uploaded file mounts read-only at kernel level; `/media` mount read-write for agent-generated content. |
| Pod → Convex (praxis projection) | Per-user `preSharedToken` via `Authorization: Bearer …` to `POST /api/praxis/projection`. Validated by `convex/lib/httpAuth.ts:authenticatePod`. Same token credential as `/api/emails`, `/api/media-url`, `GET /api/ws-auth`. Per-pod traffic only; no shared global secret. |
| Internet → Pod | **Blocked.** Per-user services are ClusterIP (no external IP). All traffic routes through the WS gateway in `clawcraft-system` namespace. Convex relay uses `containerEndpoint` (legacy LB IPs for existing users) or deterministic ClusterIP DNS. |

---

## 9. Error Handling

**Pattern:** Idempotent provisioning. Every step check-before-creates. Retry skips completed steps.

| Failure | Action | Recovery |
|---|---|---|
| GKE API 429 | Retry with backoff (1s, 2s, 4s, max 3) | `ctx.scheduler.runAfter` |
| GKE API 5xx | Mark pod `error` | Stale-cleanup cron retries after 5 min |
| Pod OOM killed | Kubernetes auto-restarts | 3+ restarts in 10 min → mark `error` |
| OpenRouter 429 | ZeroClaw retries internally | User sees "thinking..." |
| OpenRouter 5xx | Return error to user | Suggest retry |
| Scale 0→1 timeout | `pollHealth` polls the pod `/health` (dev fast path) AND, on failure, reconciles from K8s Deployment status every 3rd attempt (prod path — pod ClusterIP DNS is unreachable from Convex Cloud, so `/health` always fails there; `readyReplicas > 0` ⇒ running). Resolves prod wakes in seconds vs. the 60s `healthCheckAll` cron. | On the 90th attempt, re-read the user and mark `error` **only if still `starting`** — never stomp a pod the cron or K8s fallback already flipped to `running`. Show retry button. |
| Message during 0→1 | Queue in Convex | Deliver when health check passes |
| Namespace deleted | `scaleUp` detects `not_found` via `getDeploymentStatus` | Falls back to `provisionUser` (full reprovision) |
| Pod in `error` or `not_found` | `onPageLoad` schedules `scaleUp` on next chat page load | Self-healing: `scaleUp` reprovisions if namespace gone, reconciles if present |
| PVC missing (pre-v2.7 user) | `scaleUp` calls `ensurePVC()` before `reconcileDeployment()` | Idempotent: creates PVC if missing, no-op if exists |
| CrashLoopBackOff | `scaleUp` forces rolling update via `restartAnnotation` | Resets CrashLoopBackOff timer, fresh pod on every scale-up |
| Pod endpoint resolution | `provisionUser` returns immediately after ClusterIP service creation (no LB wait). Endpoint is deterministic: `ClawPodIdentity.endpoint` computes `claw-{userId}-svc.claw-{userId}.svc.cluster.local:42617`. | `pollHealth` uses deterministic DNS as fallback when `containerEndpoint` is null. No LB IP resolution needed. |
| Pod stuck in error (>10 min) | `healthCheckAll` auto-restarts via `scaleUp` (up to 3x) | `podRestartCount` on user record tracks attempts; reset on successful health |
| Pod fundamentally broken | `healthCheckAll` detects `podRestartCount >= 3` | Scales down to stop wasting resources; user can manually restart from dashboard |
| Running pod idle > `CLAW_IDLE_REAP_SECONDS` | `reapIdlePods` cron (60s) schedules `scaleDown` | Pod scales to `replicas: 0`; any inbound (web/email/Telegram) wakes it via `scaleUp`. Inert when the env var is unset. §6.8. |

---

## 10. Invariants

1. **Region lock**: Every GCP resource in `northamerica-northeast1`. Violation = data left Canadian soil.
2. **One pod per user**: `replicas` is 0 or 1. Never 2+.
3. **Network isolation**: Pods only accept ingress on ports 42617 (gateway) and 42618 (webhook channel) from `clawcraft-system` namespace (WS gateway). No external ingress. No cross-tenant pod-to-pod access.
4. **Stateful memory**: PVC at `/zeroclaw-data/workspace/memory/` persists `brain.db` across restarts. Convex `messages` table is the UI source of truth; `brain.db` is the agent's working memory. Dual store, complementary.
5. **One code path**: Onboarding and returning users use identical wake-up mechanism.
6. **Immutable deploys**: SHA-tagged images preferred. `CLAW_DOCKER_IMAGE` env var contains full image URI with tag. Rollback = point to previous image URI.
7. **Warm node**: Placeholder pod always running. Sub-5s scheduling guaranteed.
8. **Pre-shared auth**: Every pod has a unique bearer token. No pairing dance at runtime.
9. **Declarative deployments**: Every operation that starts or restarts a pod MUST reconcile the full K8s resource set (PVC, NetworkPolicy, Service, ConfigMap, Deployment) and send the full deployment spec via `reconcileDeployment`. No imperative patches (except `scaleDown`). This prevents config drift between `gke.ts` code and live K8s state. `buildDeploymentSpec` is the single source of truth for the deployment spec. All three lifecycle paths (`provisionUser`, `scaleUp`, `restartPodForIntegration`) reconcile the same resource set.
10. **Self-healing scaleUp**: `scaleUp` MUST check `getDeploymentStatus()` before patching. If namespace/deployment is gone (`not_found`), it falls back to `provisionUser()` (full reprovision). `healthCheckAll` cron handles error recovery. Pods are reaped when idle (§6.8) and woken on inbound activity — never reaped mid-turn (`lastActivityAt` covers user sends, assistant replies, and ws-auth connects).
13. **Deterministic pod endpoint**: Pod endpoints are computed deterministically via `ClawPodIdentity.endpoint` (`claw-{userId}-svc.claw-{userId}.svc.cluster.local:42617`). No LoadBalancer IPs, no async resolution. `provisionUser` creates a ClusterIP service and returns immediately. `ws-auth` always uses `ClawPodIdentity.endpoint` for upstream routing (the Nginx gateway resolves cluster-internal DNS). **`pollHealth`'s `/health` fetch against this endpoint is the dev-only fast path** — cluster-internal DNS is NOT resolvable from Convex Cloud, so in prod the fetch always fails and `pollHealth` reconciles health from the K8s Deployment status (`getDeploymentStatus`) instead. `containerEndpoint` (legacy override) still takes precedence over the deterministic endpoint when set.
11. **OAuth redirect allowlist**: OAuth callback handlers MUST encode the return URL in the state parameter and validate it against `ALLOWED_ORIGINS` in `http.ts`. This makes redirects environment-aware (dev/prod/CA) without relying on env vars. `APP_URL` env var is a fallback only, not required. New domains MUST be added to the `ALLOWED_ORIGINS` array in `http.ts`.
12. **Prerequisite ensuring**: `scaleUp` and `restartPodForIntegration` MUST call all idempotent `ensure*` functions (`ensurePVC`, `ensureNetworkPolicy`, `ensureService`) before `reconcileDeployment`. The deployment spec references resources that may not exist for users provisioned before those resources were added. Every `ensure*` is idempotent — create if missing, patch/update if exists.
13. **Forced restarts**: `scaleUp` MUST pass `restartAnnotation` to `buildDeploymentSpec`. Without it, K8s treats an identical spec as a no-op and CrashLoopBackOff pods are never restarted. The annotation changes `spec.template.metadata.annotations`, which triggers a rolling update.
14. **Bounded pod state TTL**: Every pod state has a bounded time-to-live before something fixes it or kills it. `healthCheckAll` auto-restarts error pods (up to `MAX_AUTO_RESTARTS = 3`, tracked by `podRestartCount` on the user record), then scales down. `reapIdlePods` scans only `running` pods (skips `deletedAt`) and reaps the idle ones; reaped pods wake on inbound, so no dead state — a reaped active session reconnects via ws-auth within ~3–5s rather than being stranded. No infinite loops, no dead states.
15. **Standard resource labels**: Every K8s resource created by `gke.ts` MUST carry `app`, `env`, and `user` labels via `resourceLabels()`. `env` is derived from `CLAWCRAFT_ENV` env var (defaults to `"dev"`). Prod MUST set `CLAWCRAFT_ENV=prod`. Labels enable `kubectl` filtering by environment and user, and safe bulk operations like `kubectl delete ns -l env=dev`.
16. **GCS Fuse access control**: GCS Fuse mounts for `user-storage/` and `conversation-attachments/` MUST be `readOnly: true` at the kernel level. The `/media` mount is `readOnly: false` to allow agent-generated content output (images, documents). The pod's GSA has `storage.objectAdmin`, but kernel-level `readOnly` on user-uploaded file mounts provides defense in depth.

---

## 11. Cost Model

### Per-User Container (GKE Autopilot, scale-to-zero)

Assuming average 2 hours active per day:

| Resource | Request | Active hours/mo | Monthly |
|---|---|---|---|
| CPU | 100m | ~60h | ~$0.24 |
| Memory | 128Mi | ~60h | ~$0.03 |
| Storage | None (stateless) | — | $0 |
| **Per-user total** | | | **~$0.27** |

### Always-on costs

| Resource | Monthly |
|---|---|
| Placeholder pod (warm node) | ~$30 |
| GKE cluster management fee | $0 (Autopilot, no fee) |
| Artifact Registry | ~$1 |
| **Fixed total** | **~$31** |

### LLM (OpenRouter, platform key)

| Model | Input/1M tokens | Output/1M tokens |
|---|---|---|
| Claude Sonnet 4 | ~$3.00 | ~$15.00 |
| Gemini 2.0 Flash | ~$0.10 | ~$0.40 |

Est. per user: ~500K tokens/mo = ~$2-5/mo (model dependent).

### Unit Economics

| Plan | Price | Infra | LLM | Placeholder (amortized at 50 users) | Margin |
|---|---|---|---|---|---|
| **Pro ($39)** | $39 | $0.27 | ~$2-5 | $0.60 | **~85-93%** |
| **BYOK ($25)** | $25 | $0.27 | $0 | $0.60 | **~96%** |
| **Trial (free)** | $0 | $0.27 | ~$0.50 | $0.60 | **-$1.37** |

### Comparison to v1.0

| Metric | v1.0 (OpenClaw) | v2.0 (ZeroClaw) | Improvement |
|---|---|---|---|
| Per-user infra (always-on) | $10.34/mo | $0.27/mo | **97% reduction** |
| Free trial burn | -$10.84/user | -$1.37/user | **87% reduction** |
| Pro margin | ~62-69% | ~85-93% | **+20-25 pts** |
| Pod memory request | 512Mi | 128Mi | **75% reduction** |
| Container image size | ~500MB+ | ~72MB | **85% reduction** |
| Cold start (container) | ~10-30s | ~0.6s | **~95% faster** |

---

## 12. Testing Expectations

| Layer | Test Focus | Tool |
|---|---|---|
| Terraform | `plan` output review, no out-of-region resources | `terraform plan` + review |
| `clients/gke.ts` | API call construction, idempotency, error handling | Vitest + mocked K8s API |
| Provisioning flow | Resource creation order, ConfigMap content | Integration test on staging |
| NetworkPolicy | Cross-namespace blocked | `kubectl exec` pod A, `curl` pod B |
| Docker image | Builds, health endpoint responds, status works | CI: build + run + curl |
| Scale 0→1 | Time from patch to healthy | Staging benchmark |
| Predictive wake-up | Page load triggers scale-up | E2E test |
| Cost | Per-pod matches estimate | GCP Billing after 24h staging |

---

## 13. Change Protocol

- Modifications REQUIRE:
  - `terraform plan` review before any `apply`.
  - Docker rebuild + push for any ZeroClaw fork change.
  - ConfigMap update for per-user config changes.

- Security review required when:
  - IAM roles added or modified.
  - NetworkPolicy rules change.
  - New secrets added to ConfigMap or pod env.
  - Any resource created outside Montreal.

- Backwards compatibility:
  - ZeroClaw gateway API (`/webhook`, `/health`, `/pair`) MUST be backwards compatible.
  - ConfigMap schema changes MUST be deployable without restarting existing pods.

---

## 14. Later Improvements

1. **Static binary optimization**: Recompile ZeroClaw against `musl` (instead of `glibc`) to produce a fully statically linked binary. This allows using `distroless/static` (~2MB) or `scratch` as the base image instead of `distroless/cc` (~30MB), reducing container image from ~72MB to ~40-45MB. Marginal gain — only pursue when image pull time becomes a measurable bottleneck.

2. **ARM nodes**: GKE Autopilot supports ARM (`cloud.google.com/compute-class: Scale-Out`) at ~20% cost savings, but ARM (T2A) is **not available in `northamerica-northeast1` (Montreal)** as of March 2026. x86_64/AMD64 is a binding constraint (§2.3). If ARM becomes available in Montreal, migration requires: cross-compiling ZeroClaw for `aarch64-unknown-linux-gnu`, publishing multi-arch images, updating `nodeSelector` in §6.6, and updating §2.3.

3. **Memory sync**: Implement periodic SQLite snapshot to GCS bucket for durable memory across pod restarts. Enables ZeroClaw's native memory compaction and learned preferences. Only needed if "remembers across sessions" becomes a user-facing feature.

4. **Drop placeholder pod**: Once organic user load keeps nodes warm (~50+ concurrent active users), remove the placeholder pod and save ~$30/mo.

5. **Upstream convergence**: If ZeroClaw accepts the pre-shared token auth feature upstream, migrate from thin fork to base image approach (no fork maintenance burden).

---

## 15. Treasury Service (Cloud Run + Cloud SQL)

The workspace package is `@clawcraft/treasury` at `apps/treasury/`. The deployed Cloud Run service is still named `ledger` and the Artifact Registry repo is still `clawcraft-ledger` — GCP identifiers do not rename with the app.

### 15.1 Control-plane services run on Cloud Run, NOT GKE

GKE Autopilot hosts ZeroClaw compute-plane pods only. HTTP services that own
control-plane state (users, billing, ledger) deploy to Cloud Run in
`northamerica-northeast1`. Rationale: scale-to-zero, managed TLS, native Cloud
SQL connector, and no pollution of the compute cluster with control-plane
concerns. The `@clawcraft/treasury` Hono service is the first of these; future
control-plane services (auth, webhooks aggregator, etc.) follow the same
pattern with their own Artifact Registry repos, Cloud Run services, and IAM.

### 15.2 Postgres parity: declarative version pinning

Dev uses `postgres:16-alpine` in `infra/local/docker-compose.yml`.
Prod uses `google_sql_database_instance.database_version = "POSTGRES_16"`
with tier supplied per-environment from `infra/terraform/environments/*.tfvars`
(no default in `variables.tf` — divergence must be version-controlled).

**Never** push a custom Postgres image to Artifact Registry — Cloud SQL cannot
consume it, and divergence becomes invisible. Extensions live in
`apps/<svc>/db/init.sql`, applied by both compose initdb.d mount and by CI
via `cloud-sql-proxy` + `psql` before each Cloud Run deploy.

**Shared-core → dedicated-core is planned downtime.** `db-f1-micro` (the
current `prod.tfvars` value) cannot be live-resized to `db-custom-*`. Before
the first real traffic, bump the tier, schedule a window, take a manual
backup. Runbook: `infra/terraform/environments/README.md`.

The same parity discipline applies to every other image in
`infra/local/docker-compose.yml`. `cloudflare/cloudflared` is currently
pinned to a CalVer tag (`2026.3.0`); never `:latest`. Floating tags let two
devs running `docker compose pull` weeks apart end up on different connector
versions, with one silently broken. Bumps update the compose file in the
same PR as any code that depends on the new version.

The six **self-hosted Laminar** images (`laminar` profile, §15.25) are each
pinned to an exact `tag@sha256:<digest>` — the strongest form of this rule.
Upstream's lightweight `docker-compose.yml` ships floating tags
(`ghcr.io/lmnr-ai/*` untagged, `clickhouse-server:latest`) plus
`pull_policy: always`; both are re-pinned here. The `lmnr-ai/{app-server,
frontend,query-engine}` triplet is pinned to one cohesive release
(`v0.1.46`) so query-engine/app-server schema versions never skew, and
ClickHouse is pinned to the `25.8` LTS line because upstream notes that
ClickHouse 26.3 breaks the `spans_v0` view (ClickHouse/ClickHouse#101218).

### 15.3 Per-service Artifact Registry repo

One Docker repo per deployable: `clawcraft-images` for ZeroClaw,
`clawcraft-ledger` for the ledger, `clawcraft-<svc>` for each future service.
Never cross-pollinate. Image tags are the git SHA plus `latest` on every
successful deploy.

### 15.4 GitHub Actions authenticates via WIF, never JSON keys

`google_iam_workload_identity_pool.github_actions` + attribute-condition
`assertion.repository == "<owner>/<repo>"` scopes token exchange to this repo
only. Two GitHub repo variables (`GCP_WORKLOAD_IDENTITY_PROVIDER`,
`GCP_DEPLOY_SA_EMAIL`) are the full wiring. No service-account JSON anywhere.

Minting an ID token in the workflow for the post-deploy smoke goes through the
`auth@v2` action's `token_format: id_token` + `id_token_audience: <URL>`
path — NOT `gcloud auth print-identity-token --audiences=...`, which refuses
federated credentials. See PR #9 for the specific failure mode.

### 15.5 Migrations run in CI, not on pod startup

Cloud Run revisions are immutable. CI's `deploy-treasury.yml` runs `init.sql`
and `drizzle migrate` through `cloud-sql-proxy` before issuing
`gcloud run deploy`. The app never migrates itself. If migrations fail, the
new image stays in Artifact Registry but Cloud Run is not updated.

### 15.6 Terraform is manual; CI deploys application code only

`terraform apply` is a human action. CI owns: image build, push, migrate,
Cloud Run revision update. Cloud Run service definition has
`ignore_changes = [template[0].containers[0].image]` so CI's image update
does not drift against Terraform. Exceptions (IaC from day 1, not deferred):
branch protection (`github_repository_ruleset`), Cloud Run IAM bindings, API
enablement (`google_project_service`).

### 15.7 Cloud Run ingress is IAM-gated; `allUsers` is forbidden

`ingress = "INGRESS_TRAFFIC_ALL"` with NO `allUsers` invoker binding. Every
request presents an identity token validated by Cloud Run before Hono sees a
byte. Authorized invokers are named explicitly at the service level: the
runtime SA, the GHA deployer SA (for CI smoke tests), and
`convex-ledger-caller` (stable identity target for Convex). Adding `allUsers`
is a spec violation — Terraform review must reject it.

### 15.8 Convex → Ledger auth is an unresolved BLOCKER for Part 2

`convex-ledger-caller` SA exists today with zero permissions beyond
service-level `run.invoker` on the ledger. How Convex acquires a token to
impersonate it is deliberately deferred:

- **Option A (pragmatic)**: Convex holds a long-lived SA key for
  `convex-ledger-caller`, mints identity tokens via IAM Credentials API,
  rotates quarterly.
- **Option B (preferred when available)**: Convex → WIF federation via
  outbound OIDC, mirroring the GHA pattern.

**Part 2 cannot deploy any Convex-callable ledger endpoint until one of these
is chosen and wired.** This is a binding prerequisite, not a nice-to-have.

### 15.9 Cloud Run → Secret Manager IAM propagation

Cloud Run's pre-flight validator for `value_source.secret_key_ref` runs
synchronously against the runtime SA's IAM, which is eventually consistent.
Terraform surfaces this as `FAILED_PRECONDITION` on the first apply if the
Cloud Run service is created in the same batch as its Secret Manager IAM
binding. Workaround: `time_sleep.wait_for_ledger_run_iam` (30s) between the
IAM member and the Cloud Run service, with explicit `depends_on`. Must be
replicated for any future Cloud Run service that consumes Secret Manager.

### 15.10 Required-reviewer escalation trigger

The moment the first migration that **writes** to any `ledger_*` table lands
on `main`, a GitHub `production` Environment with required-reviewer
protection becomes mandatory on `deploy-treasury.yml`. Until that point, a bad
revision is a `gcloud run services update-traffic` rollback. After that
point, a bad migration is restore-from-backup. Tied to migration SQL
content, not a calendar date.

### 15.11 Schema ownership: `ledger` Postgres schema, Drizzle DDL, init.sql extensions-only

The ledger's relational model lives in a dedicated `ledger` Postgres **schema**
(not to be confused with Drizzle's TypeScript schema file). The boundaries:

- **`init.sql`** — extensions only (`CREATE EXTENSION IF NOT EXISTS pgcrypto`).
  No DDL. Mounted read-only into the local container on volume bootstrap and
  run by CI against Cloud SQL via `cloud-sql-proxy` before every deploy.
- **`apps/treasury/db/schema.ts`** — Drizzle table definitions using
  `pgSchema("ledger")`. Source of truth for all tables, columns, CHECK
  constraints, indexes. Edits generate migrations via `drizzle-kit generate`.
- **`apps/treasury/migrations/`** — autogenerated SQL (`0000_*.sql`) plus
  hand-written trigger migrations (`0001_triggers.sql`, etc.). Triggers live
  here because Drizzle cannot model `CREATE TRIGGER`. All DDL lives in
  migrations; `init.sql` never ships DDL.

Treasury-specific doctrine (journal invariants, system accounts, transaction
patterns, HTTP surface, client consumption) lives in
`docs/doctrine/architecture/treasury-doctrine.md`. Treat that file as binding
for any change under `apps/treasury/`.

### 15.12 Version-drift gotchas worth codifying

- **Container Node vs host Node**: the ledger image ships Node 20-alpine.
  Developer machines run whatever Node. `new URL(...)`, `fetch`, `URL`
  parser strictness, and a handful of other built-ins differ by Node
  version. Any change touching DB connection strings, URL construction, or
  Node built-ins should be validated against the **built image**, not
  `tsx`/`pnpm dev`. Tracked as bead `rnk-c2ww`.
- **pnpm nodeLinker: hoisted**: this repo's `pnpm-workspace.yaml` sets
  `nodeLinker: hoisted`, so `apps/<svc>/node_modules/` is empty and the
  actual packages live at the root. Dockerfiles must copy `/repo/node_modules`
  and rely on Node's upward module resolution — do NOT try to copy
  `/repo/apps/<svc>/node_modules`.

### 15.13 Cloud Tasks as durable async substrate

Treasury's billing-critical webhook processing decouples via Google Cloud
Tasks queues in `northamerica-northeast1`. **Three distinct IAM bindings
must all be granted** — missing any one fails at a different observable
layer, all surfacing only when real webhook traffic flows:

| # | Direction / purpose | Permission | Bound on | Granted to | Failure mode |
|---|--------------------|------------|----------|------------|--------------|
| 1 | Service → Queue (enqueue) | `roles/cloudtasks.enqueuer` (`cloudtasks.tasks.create`) | each queue | runtime SA (`clawcraft-ledger-run`) | `client.createTask` returns `7 PERMISSION_DENIED ... cloudtasks.tasks.create` |
| 2 | Service → invoker SA (delegate identity) | `roles/iam.serviceAccountUser` (`iam.serviceAccounts.actAs`) | the invoker SA | runtime SA (`clawcraft-ledger-run`) | `client.createTask` returns `7 PERMISSION_DENIED ... iam.serviceAccounts.actAs` |
| 3 | Queue → Service (dispatch) | `roles/run.invoker` | the Cloud Run service | dedicated invoker SA (`treasury-tasks-invoker`) | Cloud Tasks dispatch attempt 401s on the `/internal/tasks/*` route |

The dispatch direction (#3) is the one operators usually remember (it's
the "interesting" one — Google-issued OIDC tokens, audience checks). The
enqueue direction (#1) and the delegation binding (#2) are silent on
success and produce runtime `PERMISSION_DENIED` only when an actual
webhook fires. **All three must be in Terraform from day one.**

Why #2 is needed even though #3 already exists: Cloud Tasks mints the
OIDC token at *dispatch* time using the invoker SA's identity, but the
runtime SA must have permission to *configure* the dispatch with that
invoker SA in `oidcToken.serviceAccountEmail` at *enqueue* time. Two
separate operations, two separate IAM checks. Three queues exist:

| Queue | Producer | Consumer |
|-------|----------|----------|
| `treasury-stripe-ingest` | `POST /webhooks/stripe` | `POST /internal/tasks/stripe` |
| `treasury-openrouter-ingest` | `POST /webhooks/openrouter` | `POST /internal/tasks/openrouter` |
| `treasury-projections-out` | Treasury outbox push (`makeConvexProjectionPush` enqueue-on-failure path) | `POST /internal/tasks/projection` |

`treasury-stripe-ingest` carries **live Stripe traffic** once the operator
creates the Stripe webhook endpoints (treasury-stripe-integration spec
§9) — it is no longer a test-mode-only queue.

Terraform: queues live in `infra/terraform/cloud-tasks.tf`. `prevent_destroy = true`
on every queue — in-flight tasks are lost on destroy, and rollback requires a
manual `gcloud tasks queues purge` + wait-for-empty before `terraform destroy`.

Retry/rate tuning: GCP defaults (100 attempts, 0.1s→3600s exponential backoff).
Tune per-queue once production error data exists; ad-hoc tuning is out-of-scope
for the initial substrate.

### 15.14 Route namespace split for Cloud Run services with async ingest

Cloud Run services that both receive external provider webhooks and process
them durably MUST split routes into two namespaces:

- `/webhooks/*` — **public**. No IAM invoker gate (external provider hits it
  unauthenticated over the public internet). Per-route signature verification
  is the ONLY auth. Handler MUST be enqueue-only: verify signature, build
  payload (including `rawBody` + `signature` for re-verification on consume),
  enqueue to Cloud Tasks, return 200 within 500ms p95. NO direct DB writes.
  NO Convex calls.
- `/internal/tasks/*` — **OIDC-gated**. Cloud Tasks dispatches here with a
  Google-minted OIDC token in `Authorization: Bearer <id-token>`. Treasury's
  `oidcMiddleware` (in `src/middleware/oidc.ts`) validates signature, issuer
  `https://accounts.google.com`, audience equal to the Cloud Run service URL,
  and `email` claim equal to the dedicated invoker SA. Cloud Run's own IAM
  gate is defense-in-depth.

The existing X-Ledger-Token auth middleware skip list (`src/middleware/auth.ts`)
MUST skip both `/webhooks/*` and `/internal/*` — they have their own auth.

The Stripe webhook endpoints point at the direct `*.run.app` service URL
until the queued `treasury-cloud-armor` work fronts the service with an
HTTPS LB + Cloud Armor (§15.19) and re-points them. Until then the
per-route auth layers are the only defense (interim risk signed off
2026-07-05; spec F10).

Public webhook endpoints MAY NOT share middleware with the X-Ledger-Token
protected surface. Bugs that let the two auth paths cross produce critical
vulnerabilities (unauthenticated write access via webhook; or 400 rejections
on valid provider calls).

Sister-service endpoints (Convex → treasury, treasury → Convex) MAY use a
third namespace with bearer-token auth. Treasury → Convex projection push
hits `https://<deployment>.convex.site/treasury/projection` with
`Authorization: Bearer ${CONVEX_PROJECTION_TOKEN}` — a shared secret in
GCP Secret Manager + Convex env. The token MUST live in both stores; CI
guard rejects asymmetric rotation. See `treasury-doctrine.md` §4.7 for the
full producer-side ordering and `backend-doctrine.md` §6.5.x for the
consumer-side pattern.

### 15.15 Cloud Tasks queue naming convention

Format: `<service>-<domain>-<direction>`.

- `<service>` is the workspace package base name (`treasury`, not `ledger`).
- `<domain>` is the event source or target (`stripe`, `openrouter`, `projections`).
- `<direction>` is `ingest` (producer ext → us), `out` (us → ext), or `internal`
  (us → us, async).

Examples: `treasury-stripe-ingest`, `treasury-openrouter-ingest`,
`treasury-projections-out`. Future queues for auth webhooks would follow as
`auth-<provider>-ingest`.

The name is the Terraform resource's `name` attribute AND the `QueueName`
string literal in the domain layer (`apps/treasury/src/domain/queue-name.ts`).
Compile-time safety: a typo fails `tsc` because `QueueName` is a union of
string literals, not `string`.

### 15.16 Per-capability invoker SA, co-located in service Terraform

Each distinct invocation path into a Cloud Run service gets its own service
account with `roles/run.invoker`. Never share the runtime SA with invokers;
never share an invoker across unrelated capabilities.

Current bindings on the `ledger` service:

| SA | Purpose | Source of token |
|----|---------|-----------------|
| `clawcraft-ledger-run` | Runtime (target, not invoker) | — |
| `clawcraft-gha-deployer` | CI smoke | WIF |
| `convex-ledger-caller` | Convex direct calls | Deferred (§15.8) |
| `treasury-tasks-invoker` | Cloud Tasks → `/internal/tasks/*` | Cloud Tasks' built-in `oidcToken` |

The invoker SA lives in the SAME Terraform file as the service it invokes
(`cloud-run-ledger.tf`), NOT in `iam.tf`. Co-location makes the blast radius
obvious: deleting a capability deletes the service binding and the SA in one
change. Generic-IAM files become dumping grounds otherwise.

Mirror `time_sleep.wait_for_*_iam` (30s) between the binding and any
downstream resource that requires the binding to be visible to Cloud Run's
pre-flight validator (FMEA #9 in the spec; same rationale as §15.9).

No long-lived SA JSON keys are created for Cloud Tasks invocation. Cloud
Tasks mints ID tokens internally via the `oidcToken.serviceAccountEmail`
dispatch field — Google-managed key material, never exposed.

### 15.17 OIDC verification conventions for internal task routes

Internal task routes (§15.14) verify:

1. Signature against Google's public JWKS at
   `https://www.googleapis.com/oauth2/v3/certs`. Cache via
   `jose.createRemoteJWKSet`; refresh on `kid` miss.
2. `iss === "https://accounts.google.com"`.
3. `aud === process.env.TREASURY_PUBLIC_URL` (the Cloud Run service URL that
   Cloud Tasks was configured to dispatch to — single source of truth
   between the queue target and the middleware).
4. `email === process.env.TREASURY_TASKS_INVOKER_SA` (the dedicated invoker
   SA email).
5. `email_verified === true`.
6. `exp > now` (enforced by `jwtVerify`).

On any failure: `401 UNAUTHORIZED` with no detail in the response body.
Details go to logs only.

Dev bypass: if `NODE_ENV !== "production"` AND `OIDC_DEV_BYPASS === "true"`,
middleware is a no-op. Both conditions must hold; a single env flag leak to
prod still fails closed because the prod env is also checked.

The dev bypass flag MUST NOT appear in the prod Cloud Run manifest. CI guard:
`.github/workflows/deploy-treasury.yml` greps `infra/terraform/cloud-run-ledger.tf`
for the literal and fails the job if present.

### 15.18 Public webhook ingress on services with per-route auth

§15.7 ("Cloud Run ingress is IAM-gated; `allUsers` is forbidden") was
authored when the Cloud Run IAM gate was the only auth layer. Services
that now ship per-route auth (X-Ledger-Token middleware, Google OIDC
middleware, provider HMAC signature verification) MAY allow `allUsers`
**when** they need to receive unauthenticated public webhooks (Stripe,
OpenRouter, GitHub, etc.).

The carve-out applies ONLY when ALL of the following hold:

1. Every authenticated route on the service has its own auth middleware
   that runs before any side-effecting code. Removing the Cloud Run IAM
   gate must not expose any sensitive operation that previously relied on
   it as the only check.
2. Every public route MUST do cryptographic verification (HMAC, signature)
   before any side effects (DB write, queue enqueue, external call).
3. Health probe surface is intentional and benign (`/health` returning a
   short status string, no auth state, no PII).
4. The service has a dead-letter or DLQ behavior — abusive traffic that
   passes signature verification (e.g. a leaked webhook secret) does not
   cascade into unbounded ledger mutations. Cloud Tasks handler-level
   idempotency (`UNIQUE(provider, provider_event_id)`) provides this.
5. A replacement outer defense (rate limiting / bot detection) is either
   in place or queued in a follow-up brief. See §15.19.

The Terraform binding is:

```hcl
resource "google_cloud_run_v2_service_iam_member" "<svc>_invoker_public" {
  name     = google_cloud_run_v2_service.<svc>.name
  location = google_cloud_run_v2_service.<svc>.location
  project  = var.project_id
  role     = "roles/run.invoker"
  member   = "allUsers"
}
```

A multi-line comment in the same file MUST justify the binding by listing
the per-route auth layers that replace the IAM gate. Code review rejects
an `allUsers` binding without that justification.

§15.7 still applies to services that have no public ingress need
(internal control planes, admin tools, etc.).

**Org policy override required on Workspace orgs.** GCP's org-level
`iam.allowedPolicyMemberDomains` constraint (Domain Restricted Sharing)
is enabled by default in Workspace organizations. It rejects any IAM
binding whose member is outside a permitted Workspace customer ID — and
that includes the special `allUsers` / `allAuthenticatedUsers` values.
Adding the binding fails with:

```
Error 400: One or more users named in the policy do not belong to a
permitted customer, perhaps due to an organization policy.
```

The override is project-scoped, not org-scoped (other projects keep
the restriction):

```hcl
resource "google_org_policy_policy" "allow_public_iam_members" {
  name   = "projects/${var.project_id}/policies/iam.allowedPolicyMemberDomains"
  parent = "projects/${var.project_id}"

  spec {
    inherit_from_parent = false
    rules {
      allow_all = "TRUE"
    }
  }
}
```

Plus enable `orgpolicy.googleapis.com` in `project-apis.tf` and add
`depends_on = [google_org_policy_policy.allow_public_iam_members]` to
the `allUsers` binding so the apply doesn't race the constraint.

The override changes only what GCP will let Terraform express; the
per-route auth justification block on the binding remains the code-
review guardrail for what we choose to express.

### 15.19 Edge rate limiting and bot defense for public surfaces

When §15.18 is invoked (a Cloud Run service is opened to `allUsers`), the
service MUST be fronted by an HTTPS Load Balancer with a Cloud Armor
security policy attached to its backend. Cloud Armor enforces only
when traffic flows through an LB — direct `*.run.app` URLs bypass it.

Required topology:

1. **Reserve a global static IP** for the LB.
2. **Serverless NEG** (`google_compute_region_network_endpoint_group`)
   pointing at the Cloud Run service.
3. **Backend service** (`google_compute_backend_service`) with
   `security_policy` referencing the Cloud Armor policy below.
4. **Cloud Armor policy** (`google_compute_security_policy`) with at
   least:
   - Per-IP rate limit on `/webhooks/*` paths (default v1: 100 req/min
     per IP, drop excess).
   - Default rule: ALLOW (do not block legitimate traffic by default).
   - Optional: known-bad IP/ASN deny list, geographic restriction if
     warranted.
5. **URL map** + **target HTTPS proxy** + **managed SSL cert** for the
   public webhook hostname.
6. **Lock Cloud Run ingress** to
   `INGRESS_TRAFFIC_INTERNAL_AND_CLOUD_LOAD_BALANCING` so the
   `*.run.app` URL stops accepting external traffic. The LB hostname
   becomes the only public path in.
7. **Update provider webhook URLs** (Stripe, OpenRouter, etc.) to point
   at the LB hostname before locking ingress in step 6.

The free Cloud Armor tier covers basic rules. Tier upgrade required for
ML-based bot detection or Adaptive Protection.

If `allUsers` is added to a service WITHOUT this LB+Cloud Armor topology
already in place, the same PR (or a paired follow-up brief tracked
explicitly) MUST stand it up. "We'll do it later" is not a valid state
for production traffic.

### 15.20 Per-dev Cloudflare Tunnel for control-plane services

Convex Cloud dev deployments cannot reach `localhost`. Any Convex action
that calls a host-native control-plane service (treasury, future webhooks
aggregator, etc.) needs an externally reachable hostname. Per-dev
Cloudflare-managed tunnels are the standard pattern.

**Hostname convention.** `dev-<service>-<name>.clawcraft.ca` where
`<service>` matches the Cloud Run service name (e.g., `ledger`) and
`<name>` defaults to `$USER`, normalized to `[a-z0-9-]{1,32}`. Example:
`dev-ledger-rei.clawcraft.ca`.

**Provisioning.** `pnpm infra:tunnel:bootstrap [name]` runs
`infra/scripts/dev/bootstrap-tunnel.sh`. The script is idempotent — it
reuses an existing tunnel + ingress + DNS by name, so re-running on the
same machine never creates duplicates. It MUST validate the
`CLOUDFLARE_API_TOKEN` and required scopes (Account → Cloudflare Tunnel:
Edit; Zone → DNS: Edit on the target zone) before any Cloudflare API
mutation.

**Resources per dev** (all created via Cloudflare API v4):

| Resource | Identifier | Reuse key |
|---|---|---|
| Tunnel | `dev-<service>-<name>` | tunnel name |
| Ingress configuration | sub-resource of tunnel | tunnel id |
| DNS CNAME | `dev-<service>-<name>.clawcraft.ca` → `<tunnel_id>.cfargotunnel.com` (proxied, ttl 1) | DNS name |
| Connector token | sub-resource of tunnel | tunnel id |

**Token storage.** Connector token written atomically to `infra/.env` (mode
0600) — see §15.21. The `pnpm infra:up:tunnel` script loads the token via
`docker compose --env-file infra/.env`. Never `apps/<svc>/.env.local` —
that file is for app-local config (`DATABASE_URL`, `PORT`), not infra
secrets.

**Out of scope for prod.** Per-dev tunnels are imperative dev tooling, NOT
in Terraform. Production exposes services via Cloud Run with IAM-gated
ingress (§15.7). The two paths never converge: prod traffic flows through
Cloud Run identity tokens; dev traffic flows through Cloudflare's edge to
the host process.

**Convex env mirror.** The bootstrap script also folds in the Convex-side
config that previously required manual `convex env set` calls: it sets
`TREASURY_BASE_URL=https://<hostname>` and `LEDGER_SERVICE_TOKEN=<value>`
in the dev deployment env via `pnpm --filter @clawcraft/app exec convex
env set`. `LEDGER_SERVICE_TOKEN` is read from `apps/treasury/.env.local`
if present, generated (random 32-byte hex) if absent, and written back
to `apps/treasury/.env.local` so both sides match. Idempotent: existing
tokens are reused. Set `SKIP_CONVEX_MIRROR=1` to opt out (rare — useful
when tunneling a non-Convex service for ad-hoc debugging).

**One tunnel per developer.** Two devs running `infra:tunnel:bootstrap`
concurrently MUST pass distinct `<name>` arguments. The script defaults to
`$USER`, which is sufficient under normal use. A name collision causes
both connector tokens to point at the same tunnel; the last writer wins,
silently breaking the loser's stack.

### 15.21 Local secrets storage convention

Two files at the `infra/` level, both gitignored, file-per-environment
(no comment-swap):

| File | Purpose | Loaded by |
|---|---|---|
| `infra/.env.dev` | Dev secrets — the default in `cd infra` | direnv (`infra/.envrc` and root `.envrc`); `pnpm dev:claw:*` and `pnpm infra:up:tunnel` via `--env-file` |
| `infra/.env.prod` | Prod-equivalent secrets (for ops scripts that read from prod) | explicit opt-in only (e.g. `source infra/.env.prod` in the ops script) — never auto-loaded |

**Symmetric schema.** The two files share key names; only the values
differ. Example: `CLOUDFLARE_TUNNEL_TOKEN=` lives in both — the dev tunnel
connector token in `.env.dev`, the prod tunnel connector token in `.env.prod`.
This symmetry is the load-bearing reason the bootstrap script writes to
`infra/.env.dev` rather than `apps/<svc>/.env.local`.

**File-per-environment, not comment-swap (binding).** A single key MUST
appear at most once in each file. Never represent the alternate
environment's value as a commented-out line in the active file. The May
2026 Composio dev outage was caused exactly by this pattern: the dev
`COMPOSIO_API_KEY` was commented out, the prod value was uncommented "for
a one-off test", the swap was never reverted, and every subsequently
rendered pod inherited the (later-revoked) prod key. Split by file, not
by comment. `infra/.envrc` refuses to load if a legacy `infra/.env`
exists alongside `.env.dev`, forcing a clean rename on first pull.

**Per-app secrets stay per-app.** `apps/<svc>/.env.local` holds
configuration the local app process reads at startup (`DATABASE_URL`,
`PORT`, `LEDGER_SERVICE_TOKEN` for the local treasury). Operational scripts
in `infra/scripts/` MUST consume secrets from `infra/.env*`, never from
per-app env files.

**Mode + atomic writes.** Any file under `infra/.env*` MUST be mode 0600.
Scripts that write secrets MUST use `umask 077` and atomic-rename
(`mktemp` in same directory + `mv`). Token values MUST never traverse
stdout, the shell history, or `set -x` traces.

**Bootstrap scripts may generate tokens on first run.** When a script's
job IS to manage a secret file (`infra/scripts/dev/bootstrap-tunnel.sh`,
`infra/scripts/dev/bootstrap-projection.sh`,
`infra/scripts/dev/bootstrap-stripe.sh`), reading the existing per-app
env file for idempotency is allowed and expected — that is the script's
purpose, not a violation of the "ops scripts MUST consume secrets from
`infra/.env*`" rule above (which targets scripts that USE secrets to do
their job, e.g., to make API calls). Generated dev tokens are random
32-byte hex strings prefixed with `dev-`. Re-running the script reuses
the existing value; it never silently overwrites a value that diverges
between the two mirrored locations (treasury `.env.local` vs Convex env)
— it fails fast and asks the operator to resolve. See §15.22 for the
manifest-driven sync that owns the rest of the Convex env tier.

**Divergence-policy variant: external-CLI authority.** The fail-fast
rule above assumes the script *generates* the secret, so neither mirror
outranks the other. `bootstrap-stripe.sh` manages a secret it does NOT
generate — the ONE `stripe listen` device whsec, obtained from
`stripe listen --print-secret`. There the Stripe CLI is the authority: a
mirror (treasury `.env.local` `STRIPE_WEBHOOK_SECRET` or Convex dev env
`STRIPE_LIFECYCLE_WEBHOOK_SECRET`) that differs from the CLI value is
overwritten, not treated as a conflict. It fails fast (exit 3) only when
both mirrors are set, disagree with each other, AND both differ from the
CLI — i.e., something rotated outside the script and the intended value
is ambiguous. Scripts that fetch a secret from an external
source-of-truth follow this variant; scripts that generate the secret
follow the fail-fast-on-any-divergence rule.

### 15.22 Dev secrets manifest (Convex env target)

`infra/secrets-manifest.yaml` is the single declarative model of every
dev secret reaching the Convex deployment env.
`infra/scripts/dev/pull-secrets.sh` (alias `pnpm infra:dev:secrets:pull`)
enforces the manifest's invariants.

**Authority is the load-bearing field.** Each entry declares one of two:

| `authority` | Source of truth | Populated by | Pull behavior |
|---|---|---|---|
| `manifest` | GCP Secret Manager (`gcp_secret`) | `pull-secrets.sh` | written iff convex value differs |
| `machine`  | local generator (`generator`) | `bootstrap-tunnel.sh`, `bootstrap-projection.sh`, `bootstrap-stripe.sh` | never overwritten |

**Schema discipline.** Only fields the script reads today are part of
the schema. `scope`, `canonical_store`, and per-secret `requires:`
ordering are deferred until a real consumer earns them — a new field
lands together with its consumer, never before.

**Transitional schema discipline.** Some schema additions are
explicitly transitional — they earn their place by the same rule
(consumed by the script today) but are scoped to a finite migration
window. The first such field is `audit_pending:` (a top-level flat
list of UPPER_SNAKE_CASE env var names; see Spec 1/2/3 split below).
The script consumes it as the sixth-bucket suppressor; it exists only
for the Spec 1→3 migration and is removed from the schema in Spec 3's
final commit. Future transitional fields follow the same pattern:
real consumer, named end state, removal commit pinned in advance.

**Drift buckets are derived, not declared.** Pull classifies each key
in `convex env list`:

- `managed` — `authority=manifest`, value matches GCP.
- `drift` — `authority=manifest`, value mismatched (rewritten this run).
- `missing` — `authority=manifest`, absent in target (written this run).
- `delegated` — `authority=machine`, present in target.
- `audit_pending` — in target, listed in `audit_pending:` block.
  Transitional bucket for the Spec 1→3 migration; expected to be empty
  by end of Spec 3. Suppressed from the `orphan` warning so the report
  stays signal-rich during the transition.
- `orphan` — in target, not in manifest, not in `audit_pending`. Warns;
  exit 0. **Non-zero is a real signal**: a key got into Convex env that
  isn't even inventoried. Known accepted orphans: dev `STRIPE_SECRET_KEY`
  and `STRIPE_PRICE_*` are operator-set by hand with deliberately NO
  manifest entry (a manifest entry would front-run secrets Spec 3);
  documented as accepted in the manifest's Stripe comment block.

**Three-step promotion contract.** Adding an `authority: manifest`
entry is non-negotiable three-step:

1. Provision the secret in GCP Secret Manager with a fresh value.
   **Rotate at the consumer; never copy** the current Convex env value.
   Unknown-provenance values do not get laundered through this process.
2. Update the consumer (Cloud Run env binding, Worker `wrangler secret
   put`, etc.) to read the rotated value.
3. Append the `secrets:` entry **and remove the key from
   `audit_pending:`** in the same commit. Run
   `pnpm infra:dev:secrets:pull`; confirm the key moves
   `audit_pending` → `missing` → `managed` across two consecutive runs.

The manifest header documents the contract; this section ratifies it
as the binding promotion rule.

**Commutativity invariant.** `pull-secrets.sh`, `bootstrap-tunnel.sh`,
`bootstrap-projection.sh`, and `bootstrap-stripe.sh` write disjoint key
sets to Convex env.
Run in any order after `npx convex dev` has initialized
`apps/clawcraft/.env.local`. No global ordering is encoded. If a future
pull expansion (e.g., owning `CLOUDFLARE_API_TOKEN` in `infra/.env`)
introduces a real dependency, encode it per-secret as a `requires:`
field on the dependent generator — never as a global sequence.

**Spec 1 / Spec 2 / Spec 3 split.** The manifest is delivered across
three deliberately-separate specs. They are not optional ordering — the
discipline of provisioning ≠ sync ≠ migrate is load-bearing.

- **Spec 1 (`dev-secrets-manager`, shipped).** Schema + sync script +
  drift report + transitional `audit_pending:` block. Zero
  `authority: manifest` entries by design. The drift report's
  `audit_pending` count is the audit tool through which Spec 2 sees
  the territory.
- **Spec 2 (`secrets-audit`, deferred).** Walk every key in
  `audit_pending:` and every existing GCP secret; classify each as
  promote-to-manifest, promote-to-machine (some bootstrap script
  owns it), or delete (`convex env remove`). Output is a markdown
  table — no schema changes.
- **Spec 3 (`secrets-migrate`, deferred).** Per-secret provisioning,
  one commit per secret. Each commit follows the three-step contract
  above. By the final commit, `audit_pending:` is empty and the field
  is removed from the schema.

**Out of scope (still deferred after Spec 3).** Treasury target
(`apps/treasury/.env.local` writes), email-ingest target (waits on a
`wrangler dev` workflow), Terraform `secrets-dev.tf`, and any
reconciliation mode that deletes Convex env keys. Pull never deletes;
bootstrap scripts retain ownership of their machine-authority keys.

See also: §15.20 (per-dev tunnel), §15.21 (file-based secrets).

### 15.23 Tag-driven publish to GitHub Packages

Workspace packages that ship as private npm artifacts (e.g.,
`@soulbound-labs/praxis`) publish to GitHub Packages
(`https://npm.pkg.github.com`) on git-tag push, not on every merge to
`main`. The first such workflow is `.github/workflows/publish-praxis.yml`;
future packages mirror its structure. This section documents the
load-bearing constraints — each cost a debug cycle to discover.

**1. Scope must equal a GitHub identity.** GitHub Packages requires the
npm scope (`@soulbound-labs`) to match an org or user the publishing
token has write access to. The repo's auto-provided `GITHUB_TOKEN`
writes to `soulbound-labs/*` only. A scope owned by an unrelated
identity returns `403 — The requested installation does not exist.`
Brand-flavored scopes (`@clawcraft`, etc.) are NOT valid unless that
identity is owned by us and granted cross-org write access — neither of
which we maintain. New packages publish under `@soulbound-labs/<name>`.

**2. Tag-only trigger.** `on.push.tags: ["<package>-v*"]`. No
`branches`, no `pull_request`, no `workflow_dispatch`. The tag IS the
release intent; merges to `main` are not releases.

**3. Minimum-scope permissions.** `contents: read` + `packages: write`
on the publish job; `packages: read` on the downstream verify job.
Never `contents: write` (this workflow does not push commits or tags).

**4. No long-lived PATs.** Use `${{ secrets.GITHUB_TOKEN }}` exclusively
when publishing into the same org. PATs would re-introduce a rotation
surface this doctrine deliberately avoids — parallel to §15.4's WIF
rule for GCP. (Cross-org publishing requires PATs and is currently
out of scope.)

**5. Gate every publish.** In order:

1. Parse semver from tag suffix; reject malformed tags.
2. Tag↔`package.json#version` parity; reject mismatches.
3. Pre-flight `npm view` against the registry; refuse to re-publish an
   existing version (strict immutability).
4. Build.
5. `publint --strict` (errors fail; warnings advisory).
6. Tarball-size guard (5 MB ceiling).
7. Tarball-contents allowlist (`dist/`, `package.json`, `README.md`,
   `LICENSE`, optionally `NOTICE`; nothing else).
8. Publish.
9. Emit `GITHUB_STEP_SUMMARY` with version + tarball SHA-256 + URLs.

A failure at any gate means no artifact reaches the registry.

**6. Verify in a clean container.** A downstream `verify` job in
`node:20-alpine` (gated by `needs: publish`) installs the just-published
version and asserts the runtime version surface (`praxis --version` for
the praxis CLI). Catches build-cache poisoning, version-injection bugs,
and registry propagation issues.

**Consumer-side patterns** (e.g., the ZeroClaw fork's runtime image
installing `@soulbound-labs/praxis`) live in the package's own README,
not this doctrine. Two binding rules survive the move:

- **Token MUST be a BuildKit secret, never a build ARG.**
  `RUN --mount=type=secret,id=npm_token sh -c '… $(cat /run/secrets/npm_token) …; rm -f ~/.npmrc'`
  keeps the token out of every layer. `ARG GITHUB_TOKEN` would persist
  it in the layer history forever.
- **Scoped `.npmrc`, not `--registry` flag.** Configure
  `@<scope>:registry=https://npm.pkg.github.com` so transitive deps
  (e.g., `atomically`, `commander`) still resolve from `npmjs.org`.
  Passing `--registry=https://npm.pkg.github.com` on the install
  command forces every package through GitHub Packages and 404s on
  public deps.

See also: §6.9 (CI image build pattern for the ZeroClaw runtime —
the consumer of this published artifact), §15.4 (WIF for GCP-bound
workflows; this section is the GH-Packages parallel).

### 15.24 Local ZeroClaw dev loop

Sub-second feedback on Praxis source edits + ZeroClaw runtime changes,
against near-prod fidelity, without burning GKE budget. Replaces the legacy
`kubectl port-forward` + `redeploy-user-pod.ts` round-trip against GKE.

**Stack.** Three host processes — self-hosted Convex (`:3210`), Vite
(`:5173`), and the Praxis watcher (`pnpm dev:claw:watch-praxis` =
`tsdown --watch` natively on the host) — plus the `claw` Docker container
on `:42617`. Browser hits `ws://localhost:42617/ws/chat?token=…` directly;
no Nginx WS gateway, no Cloudflare tunnel, no Workload Identity, no GCS Fuse.

**Single-user invariant.** One fixed `CLAW_DEV_USER_ID` per machine, set in
`infra/.env`. The compose stack instantiates one `claw` container bound to
that single user. Multi-tenant local dev is out of scope.

**Bind mounts on `claw`.** Three:

| Host path | Container path | Mode |
|---|---|---|
| `packages/praxis` | `/opt/praxis` | ro |
| `infra/local/claw-workspace/<userId>/.zeroclaw` | `/zeroclaw-data/.zeroclaw` | rw |
| `infra/local/claw-workspace/<userId>/workspace` | `/zeroclaw-data/workspace` | rw |

The `/opt/praxis` mount intentionally targets the WHOLE npm package, not
just `dist/` — the prod symlink `/usr/local/bin/praxis →
/opt/praxis/dist/bin-bootstrap.cjs` only resolves if `dist/` is nested
under `/opt/praxis/`. The prod Dockerfile (sovereign-fork) MUST install
Praxis at `/opt/praxis/` for this parity to hold; the dev compose
declaration is the canonical statement of the path.

**Praxis hot-reload runs on the HOST, not in a container.** tsdown 0.20
uses rolldown's native Rust watcher (not chokidar), which uses Linux
inotify. Docker Desktop on macOS does not propagate inotify events from
host bind mounts into Linux containers, so a sibling `praxis-watch`
container would receive zero rebuild events. Workaround: run the watcher
via `pnpm dev:claw:watch-praxis` as a host process — FSEvents on macOS,
inotify on Linux hosts. The `claw` container reads new Praxis bytes via
the bind mount on the next agent invocation; no `claw` restart needed
(`tsdown --watch` writes atomically via rename).

**Token symmetry.** `CONTAINER_SERVICE_TOKEN` is mirrored between
`infra/.env` and the local Convex deployment env. `pnpm dev:claw:render`
is the symmetry guard:

- both empty → generate `dev-${randomBytes(32).toString("hex")}`, write to
  both atomically (umask 077 + atomic rename for `infra/.env`; `convex env
  set` for Convex)
- both set + equal → continue
- set on exactly one side OR set on both with different values → exit 5,
  print redacted prefixes, refuse to render until the operator aligns

This mirrors the §15.21 "fails fast and asks the operator to resolve"
pattern.

**UID parity (with macOS fallback).** `claw.user: "65534:65534"` matches
prod `securityContext.fsGroup: 65534` (§6.6). The renderer best-effort
`chgrp 65534` on the workspace tree, then falls back to `chmod -R 0777`
with a stderr warning when chgrp fails (macOS lacks gid 65534 in default
groups). The fallback is the only option short of running as root in dev,
and is acceptable because the workspace dir is gitignored single-user
scratch space.

**`claw` healthcheck override.** Compose-level healthcheck uses `["CMD",
"zeroclaw", "status", "--format=exit-code"]` — same as the image's own
`HEALTHCHECK`, just with tighter dev intervals (5s instead of 60s). The
spec-mentioned `wget`-based check fails: the Wolfi-base image
(`cgr.dev/chainguard/wolfi-base`) lacks both wget and curl.

**Renderer.** `infra/scripts/dev/render-claw-config.ts` ports the
production ConfigMap renderer to plain disk writes by composing
`buildConfigToml` + `buildWorkspaceFiles` + `renderIdentityTemplate` from
`apps/clawcraft/domain/`. Does NOT call
`internal.integrationActions.updateConfigMap` (K8s-coupled). Reads three
internal Convex queries via `convex run`: `users:getInternal`,
`personas:getByUser`, `integrations:listForUserInternal`. Inlines the two
small `buildActive*` helpers from `integrationActions.ts:595-636` rather
than carving out a new domain seam.

**Pod-state reconciliation (step 9c).** The renderer probes
`http://localhost:42617/health` at the end of its run; on `status: "ok"`
it flips `users.podState` and `users.containerStatus` to `"running"` via
`internal.users.setPodState` + `internal.users.updateContainerStatus`.
Mirrors `apps/clawcraft/convex/podActions.ts:tryRecoverFromK8s` for the
dev loop, where the production `container-health-check` cron is a
no-op (`getDeploymentStatus` calls GKE). Without this step, any prior
`relayToPod()` connection failure (claw-doctrine §4.2.1) leaves the user
row stuck in `podState: "error"` and the chat HUD shows "Error" even
when the WS gateway is healthy. The probe is best-effort: if /health is
unreachable (first render before `dev:claw:up`), the step skips with a
warning. `start-claw-all.sh` invokes `pnpm dev:claw:render` a second
time after the `/health` wait so fresh installs reconcile end-to-end on
the same script run.

**Persona / integration changes require manual cycle.** Editing a persona
in the local Convex dashboard does NOT auto-propagate. After the edit, run
`pnpm dev:claw:render && docker compose --profile claw restart claw` —
matches prod's "ConfigMap update → rolling restart" rule (§6.5: ZeroClaw
reads `config.toml` once at startup).

**Compose profile gates dev-only services.** `claw` lives under the `claw`
Compose profile (`profiles: ["claw"]`) so plain `pnpm infra:up`
(treasury-only devs) is unaffected. Compose profiles are the standard
isolation primitive when adding optional dev services to the existing
infra stack — see also the `tunnel` profile (§15.20).

**Resource table.**

| Resource | Path / identity | Ownership |
|---|---|---|
| Dev workspace dir | `infra/local/claw-workspace/<uid>/` | gitignored; host UID owns; chmod 0777 on macOS |
| Dev secrets | `infra/.env` (mode 0600) | §15.21 |
| Compose file | `infra/local/docker-compose.yml`, `claw` profile | this section |
| Vite dev-upload plugin | `infra/local/vite-dev-upload.ts` | this section |
| Renderer | `infra/scripts/dev/render-claw-config.ts` | this section |
| Reset script | `infra/scripts/dev/reset-claw-workspace.sh` | this section |
| Root scripts | `dev:claw:render`, `dev:claw:up`, `dev:claw:down`, `dev:claw:reset`, `dev:claw:watch-praxis` | this section |

**Out of scope for prod.** Per-dev imperative tooling.
`clawcraft-claw-runtime:dev` is local-only (sovereign-fork CI builds
`:<sha>` and pushes to GHCR). The dev/prod boundary at the
`claw-config.ts` + `claw-workspace.ts` domain layer is the single source
of truth — both renderers compose the same pure functions.

See also: §6.5 (ConfigMap restart-on-update rule), §6.6 (fsGroup parity),
§15.20 (Cloudflare tunnel — comparable opt-in dev profile pattern),
§15.21 (`infra/.env` mode 0600 + atomic-write requirements consumed
here), §15.22 (dev secrets manifest — `OPENROUTER_API_KEY` and
`CONTAINER_SERVICE_TOKEN` are tracked there).

**Praxis-link projection parity (dev/prod symmetry).** The projection
pipeline (pod → Convex bead-mirror tables, see `praxis-doctrine §4.2`)
has two pod-side wiring concerns, both symmetrised across dev and prod:

1. **`CLAW_CONVEX_SITE_URL` env var.** Convex splits its surface across two
   hostnames/ports — `.convex.cloud` / `:3210` for queries+mutations and
   `.convex.site` / `:3211` for HTTP actions. The praxis-link projection
   route (`POST /api/praxis/projection`) lives on the `.site` half. Both
   renderers set `CLAW_CONVEX_SITE_URL` alongside `CLAW_CONVEX_URL`:
   `gke.ts:buildDeploymentSpec` derives it from Convex's auto-injected
   `CONVEX_SITE_URL` (with a `.cloud → .site` fallback); the local compose
   sets it explicitly to `http://host.docker.internal:3211`. The producer
   prefers `CLAW_CONVEX_SITE_URL` and falls back to rewriting
   `CLAW_CONVEX_URL` for back-compat — the explicit env var is the
   forward path.

2. **Post-commit hook install.** In prod, the main container's
   `lifecycle.postStart.exec.command` runs `praxis project install-hooks`
   on first start (PR `7bcaf34`, gke.ts). In dev, Docker Compose has no
   equivalent of `lifecycle.postStart`, so `pnpm dev:claw:render` performs
   the same install at render time: `git init` the workspace bind-mount
   if absent, then run the praxis bin from `packages/praxis/dist/bin.mjs`
   against the workspace. Both paths land at the same end state: a
   `.git/hooks/post-commit` byte-equal to `dist/hooks/post-commit.sh`.
   Drift between the install action and the template file is caught by
   the tryscript at `packages/praxis/tests/cli-project.tryscript.md`.

The queue dir for failed projection payloads lives under the
workspace mount (`/zeroclaw-data/workspace/.projection-queue/`) rather
than at the PVC root — the root is owned by `root:root` in compose
(unwritable by uid 65534), and the workspace mount carries the agent's
write surface in both dev and prod.

**Browser → workspace file uploads in dev (Vite middleware).** Prod's
`generateUploadUrl` Convex action mints a signed GCS URL; the browser
PUTs straight to GCS; the pod reads via GCS Fuse at
`/zeroclaw-data/workspace/<gcsPath-minus-userId>`. Dev cannot honor that
contract — no service-account key, no real bucket. Mirror of the
`CLAW_LOCAL_POD_WEBHOOK_URL` env-gated branch pattern (claw-doctrine
v2.31.1):

1. `CLAW_LOCAL_UPLOAD_URL_BASE` env var is set in the local Convex
   deployment env by `pnpm dev:claw:render` (idempotent; defaults to
   `http://localhost:8080/dev-upload`). NEVER set in prod Convex env.
2. `apps/clawcraft/convex/gcsActions.ts:generateUploadUrl` checks the
   env var FIRST. When present, returns `{ uploadUrl: \`${base}/${gcsPath}\`, gcsPath }`
   and skips the GCS SDK entirely — no service-account key needed in
   dev.
3. The browser PUTs the file bytes to the returned URL. The receiver is
   a Vite dev-server middleware registered by
   `infra/local/vite-dev-upload.ts` (host process, `apply: "serve"`,
   same-origin so no CORS). The middleware extracts `<gcsPath>` from
   the URL suffix, splits off the userId segment, and writes to
   `infra/local/claw-workspace/<userId>/workspace/<rest-of-gcsPath>` —
   the same disk location the `claw` container's `workspace` bind mount
   exposes to the pod as `/zeroclaw-data/workspace/<rest-of-gcsPath>`.
4. The middleware writes atomically (temp file + rename) so a partial
   upload never leaves a half-written file the agent might read.
5. Path safety: reject any URL segment that is empty, `.`, `..`, or
   contains `\0`; defensively confirm the resolved target stays under
   `WORKSPACE_ROOT`. The path-resolution helper (`resolveWorkspaceTarget`)
   is exported pure and shared between the PUT and GET branches.
6. **Symmetric GET** (preview / download side): the same middleware also
   serves `GET /dev-upload/<gcsPath>`, streaming the file back from disk
   with `Content-Type` inferred from extension (small inline map covering
   the SOP intake set + chat-attachment set) and
   `Content-Disposition: inline` by default, `attachment` when `?download=1`
   is set. The Convex `/api/file-url` httpRouter route is env-gated on the
   same `CLAW_LOCAL_UPLOAD_URL_BASE`: when set, it returns a 302 to
   `${base}/${path}${download ? "?download=1" : ""}` instead of calling
   `internal.gcsActions.getDownloadUrl` (which would throw on missing
   `GKE_SERVICE_ACCOUNT_KEY` in dev). `Cache-Control: no-store` because
   dev files may be overwritten in-place.

**Why a Vite plugin and not a sidecar service.** Vite is already a host
process on `:8080` with full fs access; same-origin eliminates CORS;
disappears entirely in `vite build` (so prod is unaffected without
explicit env-gating on the receiver side); and `infra/local/` becomes
the single grep target for "all dev-only runtime surface" (compose
file, plugin module, workspace data). The alternative (standalone Hono
sidecar, new port, new container, new doctrine paragraph for its
lifecycle) is rejected as more moving parts for the same end state.
The `apply: "serve"` config is load-bearing — without it the plugin
body would compile into the prod bundle.

**Module-resolution direction.** `apps/clawcraft/vite.config.ts`
imports `../../infra/local/vite-dev-upload.ts`. This is the mirror of
the established pattern where `infra/scripts/dev/render-claw-config.ts`
imports from `apps/clawcraft/domain/`. Cross-directory imports between
`apps/` and `infra/` are not a layering violation — both live under
one git root, they are co-developed source, and the workspace-protocol
trust posture documented in §15.21 / §15.22 governs.

### 15.25 Dev Laminar observability backend

The first layer of the agentic-observability stack (logging agent intent —
`observability-doctrine.md`) needs an OTel-native sink. In dev that sink is a
**self-hosted Laminar** stack, run as a long-lived `docker-compose` profile
alongside the existing `claw`/`tunnel` profiles. Provisioned by SPEC 2
(`docs/tasks/ongoing/laminar/laminar-spec-2-hosting.md`); the contract it feeds
is `observability-doctrine.md` §5.

**Six-service `laminar` profile.** `infra/local/docker-compose.yml` carries six
services under `profiles: ["laminar"]`, each `restart: unless-stopped`:

| Service | Image (exact pin) | Host port(s) | Role |
|---|---|---|---|
| `laminar-postgres` | `postgres:16@sha256:…` | none (internal) | app metadata |
| `laminar-clickhouse` | `clickhouse/clickhouse-server:25.8.24.21@sha256:…` | none (internal) | analytics store |
| `laminar-query-engine` | `ghcr.io/lmnr-ai/query-engine:v0.1.46@sha256:…` | none (internal) | query layer |
| `laminar-quickwit` | `quickwit/quickwit:v0.8.2@sha256:…` | none (internal) | trace search/index |
| `laminar-frontend` | `ghcr.io/lmnr-ai/frontend:v0.1.46@sha256:…` | `5667` | **UI** |
| `laminar-app-server` | `ghcr.io/lmnr-ai/app-server:v0.1.46@sha256:…` | `8000`/`8001`/`8002` | API + **OTLP ingest** (gRPC `:8001`) |

**Image pins are binding (cross-ref §15.2).** Every Laminar image is pinned to
an exact `tag@sha256:<digest>`; `:latest` is forbidden. The vendored ClickHouse
profile (`infra/local/laminar/clickhouse-profiles-config.xml`, bind-mounted to
`/etc/clickhouse-server/users.d/lmnr.xml`) is a hard upstream dependency — the
Laminar CH DDL relies on its `date_time_input_format=best_effort`.

**Host-port discipline.** Only the UI (`5667`) and app-server
(`8000`/`8001`/`8002`) publish to the host. Postgres/ClickHouse/query-engine/
Quickwit are internal-only — minimal surface (§15.2) and it sidesteps the dev
`db` host `5432` (Laminar's bundled Postgres publishes no host port).

**Four `laminar-`-prefixed volumes.** `laminar-postgres-data`,
`laminar-clickhouse-data`, `laminar-clickhouse-logs`, `laminar-quickwit-data` —
the prefix guarantees no collision with the dev `db` `pgdata` volume. The stack
is long-lived; `pnpm dev:laminar:reset` (= `reset-laminar.sh`, `--profile
laminar down -v`) is the deliberate wipe path.

**Decoupled from `claw` (brief §4.3 — binding).** No Laminar service
`depends_on` `claw` and `claw` does not `depends_on` any Laminar service. The
profile keeps the 6-service ClickHouse stack out of plain `pnpm infra:up`
(treasury-only devs) and `--profile claw`. "Separate stack, pointed at" =
profile-isolation + network-pointing + `restart: unless-stopped` — the same
opt-in-profile primitive as the `claw` (§15.24) and `tunnel` (§15.20) profiles,
not a separate compose file.

**OTLP carrier wiring on `claw` (cross-ref observability §5/§5.1).** The zeroclaw
emitter reads its OTLP config from **`config.toml [observability]`**, NOT pod env
(the SPEC-1 `OTEL_*` env block on the `claw` service is **removed** — the emitter
ignored it). `pnpm dev:claw:render` (`render-claw-config.ts` → `buildConfigToml`)
renders `[observability] backend = "otel"`, `otel_endpoint =
http://host.docker.internal:8000`, `otel_service_name = zeroclaw`, and
`otel_headers` (the project-key Bearer) from `infra/.env.dev`
(`LAMINAR_OTLP_ENDPOINT` + `LAMINAR_OTLP_HEADERS`). **Transport is OTLP/HTTP** to
the app-server **`:8000/v1/traces`** receiver (`:8001` is gRPC-only and unused);
the existing `extra_hosts: host.docker.internal:host-gateway` resolves it.
Active-by-default in dev (the endpoint defaults to `:8000`); set
`LAMINAR_OTLP_ENDPOINT=` (empty) in `.env.dev` to opt a box out (`backend =
"log"`). After editing the key, re-render + recreate `claw` (`config.toml` is
read once at startup — §15.24). `claw` reaches Laminar over the host network
only.

**Compose-only secrets — NOT manifest entries (§15.22).** Laminar's internal
service credentials and the OTLP project key reach containers via `docker
compose --env-file infra/.env.dev` (`${VAR}` interpolation), never the Convex
deployment env. Per §15.22 the `secrets-manifest.yaml` scope is "every dev
secret reaching the **Convex** deployment env"; Laminar secrets have **no Convex
consumer**, so adding them to the manifest would be the schema-discipline
violation §15.22 forbids. They are `infra/.env.dev` keys only (mode 0600,
gitignored, no value committed anywhere):

| `.env.dev` key | Generation |
|---|---|
| `LAMINAR_POSTGRES_USER` / `_PASSWORD` / `_DB` | operator-set |
| `LAMINAR_CLICKHOUSE_USER` / `_PASSWORD` / `_RO_USER` / `_RO_PASSWORD` | operator-set; the official ClickHouse image creates only the single `CLICKHOUSE_USER`, so the read-only user **is** the main user (upstream convention) — the compose RO env defaults to it via `${LAMINAR_CLICKHOUSE_RO_USER:-${LAMINAR_CLICKHOUSE_USER}}`, so the frontend read path never 401s |
| `SHARED_SECRET_TOKEN`, `AEAD_SECRET_KEY` | `bootstrap-laminar.sh` (umask 077, atomic temp+rename; `AEAD_SECRET_KEY` is base64(32 bytes) per Laminar's required format — the deliberate exception to the §15.21 `dev-`-hex convention) |
| `LAMINAR_OTLP_ENDPOINT` | operator-set; defaults to `http://host.docker.internal:8000` (the app-server OTLP/HTTP `/v1/traces` ingest). Rendered into `config.toml [observability] otel_endpoint` by `render-claw-config.ts` — **not** pod env. |
| `LAMINAR_OTLP_HEADERS` | the UI-generated project API key, operator-pasted as `Authorization=Bearer <key>`. Rendered into `config.toml [observability] otel_headers` as a literal credential (claw §6.1/§6.2). Laminar drops unauthenticated spans. |

The project API key is created **in the Laminar UI after first bring-up** (it
cannot exist before the stack runs), so `LAMINAR_OTLP_HEADERS` is empty until
that step and the exporter is inert until then — legitimate
provision-then-configure ordering, not aspirational config.

**Script family.** `dev:laminar:bootstrap` (secret generation),
`dev:laminar:up` (`--profile laminar up -d`), `:down`, `:reset`
(`reset-laminar.sh`), `:logs` (follow `laminar-app-server`) — mirroring the
`dev:claw:*` / `infra:*` families.

**Opt-in UI exposure via the existing tunnel (§15.20).** The UI is reachable
remotely by adding an ingress rule on the per-dev Cloudflare tunnel pointing
`dev-laminar-<name>.clawcraft.ca` → `http://host.docker.internal:5667`. Because
the UI is a **non-Convex** service, run the tunnel bootstrap with
`SKIP_CONVEX_MIRROR=1` (§15.20) — there is no `TREASURY_BASE_URL`-style Convex
mirror to fold in. This is document-only in SPEC 2; no first-class
`bootstrap-tunnel.sh` Laminar support is added.

**Out of scope for prod.** Prod GKE Laminar hosting is a **deferred future
spec**. Per methodology §2 (no aspirational config) no prod Terraform,
manifests, `.env.prod` keys, or `gke.ts` changes are authored now — the
SPEC-1 inert prod OTLP trio stays exactly as-is (observability §5). When the
prod spec lands it provisions **one single self-hosted instance** (the brief
§4.3 "separate GCP project" path was dropped by user decision — re-confirm the
failure-domain guarantee there), threads the prod project key as a
`buildDeploymentSpec` parameter (backend §9), and tracks it in
`secrets-manifest.yaml` via the `EMAIL_WEBHOOK_SECRET` `audit_pending:` →
`secrets-migrate` precedent (rotate at the consumer; never copy the dev
`.env.dev` value — §15.21 / §15.22).

See also: §15.2 (image pinning), §15.20 (per-dev tunnel — comparable opt-in
profile + the UI exposure path), §15.21 (`infra/.env.dev` mode 0600 + atomic
write consumed by `bootstrap-laminar.sh`), §15.22 (why these are NOT manifest
entries), §15.24 (`claw` profile — the sibling opt-in dev profile);
`observability-doctrine.md` §5 (the OTLP carrier this feeds) / §9 (the
two-instance prod wall).

---

## 16. Renaming a control-plane service

When renaming a deployed service's user-facing identity (workspace
package, doctrine, dev scripts), preserve the GCP-operator-facing
identifiers:

| Renames (author-facing) | Stays (operator-facing) |
|---|---|
| `apps/<name>/` | Cloud Run service `name` |
| `@clawcraft/<name>` package | Artifact Registry repo |
| `@clawcraft/<name>-client` SDK | Cloud SQL instance, DB, user, schema |
| tsconfig path alias | Secret Manager secret names |
| `.github/workflows/deploy-<name>.yml` | HTTP auth headers, `*_TOKEN` / `*_URL` env keys |
| Doctrine file + id + cross-refs | Terraform local resource identifiers (`google_*.<name>*`) |
| Dockerfile COPY paths | `CLOUDSQL_INSTANCE`, `AR_REPO`, `SERVICE` workflow env |
| Root `package.json` scripts | Domain-noun class names (e.g., `LedgerClient`) |

Do the filesystem + workflow rename in a single pure-`git mv` commit to
preserve `git log --follow` blame. Update content in a second commit.
Treasury rename (April 2026) is the reference implementation: see
`docs/tasks/completed/treasury-restructure/`.

---

## 17. Linq SMS Operator Runbook

Linq numbers are provisioned **manually** (operator emails a Linq partner rep — there is
no self-serve provisioning API). The connect flow creates a `linq_numbers` row at
`status: pending`; an internal mutation advances `pending → registering → active`. The
ConfigMap renders `[channels_config.linq]` (claw-doctrine §17.3) ONLY at `active`.

1. **Initial subscription** (one-time, platform-wide): `infra/scripts/ops/linq-create-subscription.sh <convex-site>`. v1 uses a single platform subscription with the `phone_numbers` filter omitted, so one `LINQ_WEBHOOK_SIGNING_SECRET` covers every number.
2. **Number provisioning** (out-of-band): operator emails the Linq rep with 10DLC details; the rep returns the number.
3. **Status advancement**: `pnpx convex run internal.linqNumbers.advanceStatus {...}` to walk `pending → registering → active` (pass `phoneNumber` on the `→ active` step). The `→ active` transition regenerates the pod ConfigMap and restarts the pod (`restartPodForIntegration`).
4. **Health monitoring**: `phone_number.status_updated` webhook events auto-patch `linq_numbers.linqStatus`/`linqHealth` and fire `OPS_ALERT_WEBHOOK_URL` on `FLAGGED` / `AT_RISK` / `CRITICAL`. A `alert-linq-pending-stale` cron alerts when a row sits in `pending` > 24h.
5. **Subscription verification** (periodic): `infra/scripts/ops/linq-list-subscriptions.sh` confirms the platform subscription still exists and is bound to the right numbers (FMEA mitigation against a silently-dropped subscription).

**Secrets** (`§15.22` three-step contract): `LINQ_PLATFORM_API_TOKEN`, `LINQ_WEBHOOK_SIGNING_SECRET`, and `OPS_ALERT_WEBHOOK_URL` live in `infra/secrets-manifest.yaml` `audit_pending:`. Promotion to `secrets:` belongs to the `secrets-migrate` workflow, not here. `LINQ_PLATFORM_API_TOKEN` is also rendered into the pod ConfigMap (`[channels_config.linq] api_token`) for the pod-native outbound path (claw-doctrine §3.5).

End-to-end smoke: `infra/scripts/ops/linq-phase5-e2e.sh`.

---

## Document History

| Version | Date | Changes |
| 2.51.0 | 2026-07-06 | **`pollHealth` prod wake fix — K8s reconcile fallback + guarded escalation.** Root cause: the pod's ClusterIP DNS is unreachable from Convex Cloud, so `pollHealth`'s `/health` fetch ALWAYS fails in prod; its unconditional 90-attempt timeout then stomped every already-running pod back to `error` on every login-after-idle (the reaper made this fire constantly). §9 "Scale 0→1 timeout" row rewritten: `pollHealth` now reconciles from `getDeploymentStatus` (K8s Deployment `readyReplicas`) every 3rd attempt when the fetch fails — resolving prod wakes in seconds instead of on the 60s `healthCheckAll` cron — and, on timeout, re-reads the user and marks `error` **only if still `starting`** (never overwrites `running`). §10 invariant 13 amended: the `/health` fetch is the dev-only fast path; prod health resolves via K8s status. Code: `convex/podActions.ts` `pollHealth`. Secondary findings filed as beads (double-`scaleUp` 409 dedupe, frontend WS reconnect kick on `podState → running`, latent zonal-PD risk re-opened post-reaper) — not in this change. |
|---|---|---|
| 2.50.0 | 2026-07-05 | **Stripe dev loop + live-traffic notes (treasury-stripe-integration spec §4.5).** §15.13: `treasury-stripe-ingest` carries live Stripe traffic once the operator creates the webhook endpoints. §15.14: Stripe endpoints stay on the direct `*.run.app` URL until `treasury-cloud-armor` fronts the service (interim risk signed off 2026-07-05, spec F10). §15.20: stale `dev:*` alias references corrected to `infra:*`. §15.21: `bootstrap-stripe.sh` added to the secret-file-managing script enumeration + NEW divergence-policy variant paragraph (external-CLI authority: the `stripe listen --print-secret` value outranks both mirrors; fail-fast exit 3 only when both mirrors are set, mutually divergent, AND both differ from the CLI — vs. the generate-authority scripts' fail-on-any-divergence rule). §15.22: `bootstrap-stripe.sh` added to the machine-authority populated-by column + commutativity invariant; accepted-orphan note for dev `STRIPE_SECRET_KEY`/`STRIPE_PRICE_*` (no manifest entry — would front-run secrets Spec 3). New scripts `infra/scripts/dev/bootstrap-stripe.sh` (mirrors the ONE `stripe listen` device whsec to treasury `.env.local` `STRIPE_WEBHOOK_SECRET` + Convex dev `STRIPE_LIFECYCLE_WEBHOOK_SECRET` — dev's one-whsec asymmetry vs prod's two per-endpoint secrets) and `stripe-listen.sh` (two foreground forwarders: money → `localhost:8787/webhooks/stripe`, lifecycle → Convex dev site `/stripe-webhook`); root aliases `infra:stripe:bootstrap` + `dev:stripe:listen`. Comment-only fix: `cloud-run-ledger.tf` ingress comment said "no allUsers binding exists" — contradicted the `ledger_invoker_public` binding in the same file; corrected (no resource diff). |
| 2.48.0 | 2026-06-02 | **§15.25 — OTLP carrier moved env → `config.toml` (live-integration correction).** Verified against the zeroclaw emitter source: it reads `config.toml [observability]` (`otel_endpoint`/`otel_service_name`/`otel_headers`/`backend`) and **ignores the `OTEL_*` env**, and is **OTLP/HTTP-protobuf** to the app-server **`:8000/v1/traces`** receiver (not gRPC `:8001`). The `claw` service's `OTEL_*` env block is **removed** from `infra/local/docker-compose.yml`; `render-claw-config.ts` → `buildConfigToml` now renders the `[observability]` carrier (endpoint defaults to `:8000`; `otel_headers` = the project-key Bearer literal). `.env.dev` `LAMINAR_OTLP_ENDPOINT` re-pointed to `:8000`; the `LAMINAR_OTLP_HEADERS` / `LAMINAR_OTLP_ENDPOINT` rows note the config.toml rendering. Mirrors observability-doctrine v1.3.0 (auth mandatory; carrier is config.toml) + backend-doctrine v2.44.0 + claw-doctrine §6.1/§6.2 (OTLP key as a rendered config.toml credential). |
| 2.47.0 | 2026-06-01 | **§15.25 added — Dev Laminar observability backend** (Laminar SPEC 2). The six-service self-hosted Laminar stack lands under a new `profiles: ["laminar"]` gate in `infra/local/docker-compose.yml` — `laminar-{postgres,clickhouse,query-engine,quickwit,frontend,app-server}`, each `restart: unless-stopped` and pinned to an exact `tag@sha256:` digest (the lmnr-ai triplet at `v0.1.46`; ClickHouse at the `25.8` LTS line since upstream's `:latest` would hit the 26.3 `spans_v0` regression). Four `laminar-`-prefixed named volumes (no `pgdata` collision). Decoupled from `claw` (no `depends_on` either way; network-pointed; brief §4.3). Host ports limited to UI `5667` + app-server `8000`/`8001`/`8002` (avoids the dev `db` `5432`). The `claw` OTEL block becomes the active-by-default quintet — concrete dev endpoint default `http://host.docker.internal:8001` + `OTEL_EXPORTER_OTLP_PROTOCOL: grpc` + `OTEL_EXPORTER_OTLP_HEADERS` (project-key Bearer credential from uncommitted `.env.dev`; observability §5). Laminar secrets are **compose-only** in `infra/.env.dev` — explicitly **NOT** `secrets-manifest.yaml` entries (no Convex consumer — §15.22). New scripts `infra/scripts/dev/{bootstrap,reset}-laminar.sh` + vendored `infra/local/laminar/clickhouse-profiles-config.xml` added to §5.1; §15.2 gains the Laminar image-pin cite; `dev:laminar:*` script family added. Opt-in UI exposure documented via the existing per-dev tunnel + `SKIP_CONVEX_MIRROR=1` (§15.20). **Prod GKE Laminar is a deferred future spec** — no prod Terraform/manifest/`gke.ts` here (methodology §2); user-authorized brief §4.3 deviation (prod = one single instance, not a separate GCP project). Delivered by `laminar-spec-2-hosting.md`; mirrors observability-doctrine v1.1.0. |
| 2.46.0 | 2026-05-22 | **§15.24 — Pod-state reconciliation in the renderer** (`/substrate:quick-spec` agent-connection-error fix). Closes the gap where local-dev users stayed pinned to `podState: "error"` after any prior `relayToPod()` connection failure (claw-doctrine §4.2.1) — the production `container-health-check` cron (`apps/clawcraft/convex/podActions.ts:healthCheckAll`) reconciles via `getDeploymentStatus` (GKE-coupled), so it's a no-op in dev. Symptom: chat HUD on `localhost:8080/chat/<threadId>` shows "Error" / coral status dot via the `podState === "error"` short-circuit in `apps/clawcraft/src/routes/_authenticated/chat/$threadId.tsx:53` even when the WS gateway on `:42617` is fully healthy and accepting WS handshakes. Fix: new step 9c in `infra/scripts/dev/render-claw-config.ts` probes `http://localhost:42617/health`; on `status: "ok"`, it flips `users.podState` and `users.containerStatus` to `"running"` via `internal.users.setPodState` + `internal.users.updateContainerStatus`. Mirror of `apps/clawcraft/convex/podActions.ts:tryRecoverFromK8s` (564-572): probe actual liveness, patch both fields, swallow mutation errors. Idempotent (skips when both fields already `"running"`). `start-claw-all.sh` invokes `pnpm dev:claw:render` a second time after the `/health` wait so first-time installs reconcile end-to-end (the first render pass runs before `dev:claw:up` and the probe correctly skips; the second pass after `/health` confirms catches the patch). New `runConvexMutation` helper added next to `runConvexQuery` for clarity — same `convex run` invocation, void return. |
| 2.45.0 | 2026-05-19 | **§15.24 — GET symmetry for `infra/local/vite-dev-upload.ts`** (`/substrate:quick-spec` dev-preview-symmetry). Closes the gap that surfaced after v2.44.0 shipped: uploads worked in dev, but click-to-preview (via `/api/file-url`) still hit GCS and threw on missing `GKE_SERVICE_ACCOUNT_KEY`. The Vite plugin now serves a symmetric `GET /dev-upload/<gcsPath>` that streams the file with `Content-Type` from a tiny extension→MIME map (PDF + text/plain + image set + audio set; `application/octet-stream` fallback). `Content-Disposition: inline` by default; `attachment` when `?download=1`. `/api/file-url` in `convex/http.ts` gains the mirror env-gate: when `CLAW_LOCAL_UPLOAD_URL_BASE` is set, returns 302 to `${base}/${path}` instead of calling `internal.gcsActions.getDownloadUrl`. Path-resolution helper (`resolveWorkspaceTarget`) extracted as a pure exported function shared between PUT and GET branches; the path-safety guards (`..`/`.`/`\0`/empty rejection + `WORKSPACE_ROOT` confinement) now have unit coverage. `Cache-Control: no-store` on dev preview because files may be overwritten in-place between requests. No prod surface change — `apply: "serve"` keeps the plugin out of `vite build` output, and the Convex env var is local-only. |
| 2.44.0 | 2026-05-19 | **§15.24 — Browser → workspace file uploads in dev (Vite middleware).** Closes the gap where the SOP-ingestion dropzone (claw-doctrine §5.6.2 v2.32.3) couldn't function locally because `apps/clawcraft/convex/gcsActions.ts:generateUploadUrl` throws on missing `GKE_SERVICE_ACCOUNT_KEY`. Mirror of the v2.31.1 `CLAW_LOCAL_POD_WEBHOOK_URL` env-gated branch pattern: new `CLAW_LOCAL_UPLOAD_URL_BASE` Convex env var (default `http://localhost:8080/dev-upload`) makes the action return a Vite URL instead of calling GCS; `infra/local/vite-dev-upload.ts` (NEW, host-process Vite plugin under `apply: "serve"`) accepts `PUT /dev-upload/<gcsPath>` and writes atomically (temp + rename) to `infra/local/claw-workspace/<userId>/workspace/<rest-of-gcsPath>` — the same bind-mount path the pod reads from at `/zeroclaw-data/workspace/`. Path-traversal guard (`..` / `.` / `\0` / empty segments rejected; resolved target confirmed under `WORKSPACE_ROOT`). `pnpm dev:claw:render` gains a §3c block mirroring §3b's token-symmetry pattern for idempotent Convex-env mirroring. Resource table gains the new plugin row. Section explicitly rejects the "standalone Hono sidecar" alternative as more moving parts (new process, new port, new container, new CORS allowlist) for the same end state — Vite is already running, same-origin, fs-capable, and disappears in `vite build`. Module-resolution direction (`apps/clawcraft/vite.config.ts` importing `infra/local/...`) documented as the mirror of the established `infra/scripts/dev/render-claw-config.ts` importing from `apps/clawcraft/domain/` — both layering directions are workspace-protocol consumption, not external imports. Mirror entry in `backend-doctrine.md` v2.39.0. No claw-doctrine change (upload contract from the agent's perspective is unchanged). |
| 2.43.0 | 2026-05-11 | **Runtime header refreshed.** Header `Runtime:` line was anchored at `v0.1.8-alpha-p1` / `~76MB` / "distroless single binary" — three claims that have all reversed since the sovereign-fork landed `@soulbound-labs/praxis` install (commit `1aac3308`). Current image is `v0.6.9-alpha-p10`, Wolfi-base release stage with bash/coreutils/nodejs (NOT distroless, has a shell), ~398MB. Cross-refs to §6.10 (Wolfi healthcheck rule) and §15.24 (local dev loop) added. Surfaced by `zeroclaw-dev` session synthesis. |
| 2.42.0 | 2026-05-10 | **§6.10 added: healthcheck overrides on Wolfi-base images.** Compose `healthcheck:` overrides on the ZeroClaw runtime (Wolfi base) MUST use a binary already in the image — `wget`/`curl` are not present and a misconfigured override silently holds the container in `unhealthy` while the underlying `/health` endpoint serves 200s. Reference: `["CMD", "zeroclaw", "status", "--format=exit-code"]`. Surfaced during `zeroclaw-dev-spec.md` execution; documented now to prevent recurrence on every future Wolfi-image consumer. |
| 2.41.0 | 2026-05-10 | **Local ZeroClaw dev loop landed.** §15.24 added: full local stack for sub-second Praxis edit feedback — self-hosted Convex (:3210, host process), `clawcraft-claw-runtime:dev` Docker container under a `claw` Compose profile, bind mounts for `/opt/praxis` (whole package, prod-fidelity path) + `.zeroclaw` + `workspace`, single-user invariant via `CLAW_DEV_USER_ID`. Renderer at `infra/scripts/dev/render-claw-config.ts` composes existing pure functions (`buildConfigToml`, `buildWorkspaceFiles`, `renderIdentityTemplate`). Token symmetry (`CONTAINER_SERVICE_TOKEN`) between `infra/.env` and Convex env enforced at render time. Bypasses Nginx WS gateway, GCS Fuse, channels (web only), Cloudflare tunnel, and ConfigMap renderer. Three deviations from spec text recorded in the section: bind mount targets the WHOLE `packages/praxis/` (not `dist/` only) so prod symlink resolves; Praxis watcher runs HOST-side (not as a sibling container — rolldown's native Rust watcher does not see Docker Desktop on macOS bind-mount inotify events); `claw` healthcheck uses `zeroclaw status --format=exit-code` (Wolfi-base lacks wget). Delivered by `zeroclaw-dev-spec.md`. |
| 2.40.0 | 2026-05-10 | **§15.22 amendments from `dev-secrets-manager` Spec 1 execution.** Six-bucket drift model: `audit_pending` added between `delegated` and `orphan`, suppressing what would otherwise be ~40 keys of orphan-bucket noise during the transition. New "Transitional schema discipline" paragraph naming the pattern (a field with a real consumer + a named end state + a pinned removal commit). New "Three-step promotion contract" — provision (rotate at consumer, never copy), update consumer, append `secrets:` entry AND remove from `audit_pending:` in the same commit. "Out of scope" paragraph rewritten as the explicit Spec 1 / Spec 2 (`secrets-audit`, deferred) / Spec 3 (`secrets-migrate`, deferred) split, with the convergence-loop/inventory-as-audit-tool framing made first-class. The `orphan` bucket's "non-zero is a real signal" semantics now codified — under the empty-manifest steady state of Spec 1, orphan must be 0 by definition (every Convex env key is either delegated or audit_pending). |
| 2.39.0 | 2026-05-09 | **First tag-driven publish to GitHub Packages.** §15.23 added — six binding rules for publishing private workspace packages (`@soulbound-labs/*`) to GitHub Packages on git tag push. Captures the constraint "scope must equal a GitHub identity" (which we hit at first publish: `@clawcraft/praxis` returned `403 — installation does not exist`, fix was renaming to `@soulbound-labs/praxis`), tag-only trigger, minimum-scope `permissions:`, no-PAT rule, the seven-step gate sequence (parse → parity → re-publish refusal → build → publint → size → contents allowlist → publish), the clean-container verify job. Two consumer-side rules also encoded: token MUST be a BuildKit secret never an ARG; scoped `.npmrc` not `--registry` flag (the latter forces transitive deps through GH Packages and 404s on public ones). First workflow: `.github/workflows/publish-praxis.yml`; first published artifact: `@soulbound-labs/praxis@0.1.0`. |
| 2.38.0 | 2026-05-04 | **Per-dev Cloudflare Tunnel landed.** §5.1: `infra/local/`, `infra/scripts/` (with `dev/bootstrap-tunnel.sh`), `infra/.env`, `infra/.env.prod`, `infra/.envrc` added to directory layout — these had been silently undocumented siblings of `terraform/` for a long time; the rename of `infra/dev-tools/` → `infra/local/` made the gap freshly visible. §15.2: cloudflared parity rule appended — image pinned to `cloudflare/cloudflared:2026.3.0`, never `:latest`, mirroring the postgres parity discipline. §15.20 added: per-dev Cloudflare Tunnel pattern — hostname convention (`dev-<service>-<name>.clawcraft.ca`), idempotent bootstrap via `pnpm dev:tunnel:bootstrap`, resource table, scope requirements, "one tunnel per developer" rule, prod-divergence note. §15.21 added: local secrets storage convention — `infra/.env` / `infra/.env.prod` symmetric schema auto-loaded by direnv, file-mode and atomic-write requirements, per-app vs per-infra split. Delivered by `dev-tunnel-bootstrap-spec.md`. |
| 2.37.0 | 2026-05-02 | **Credit-ledger outbox + projection landed.** §15.13–15.14 extended with the third Cloud Tasks queue (`treasury-projections-out`) and the bearer-token sister-service auth path (treasury → Convex projection). §16 added (renaming control-plane service playbook from treasury-restructure). |
| 2.36.0 | 2026-04-29 | §15.18 added: public webhook ingress carve-out from §15.7 — services with per-route auth MAY allow `allUsers` for unauthenticated provider webhooks under five named conditions. §15.19 added: edge rate limiting and bot defense via HTTPS LB + Cloud Armor as the replacement outer defense when §15.18 is invoked. |
| 2.35.0 | 2026-04-24 | §15.13–15.17 added: Cloud Tasks as durable async substrate, route namespace split (/webhooks/* public sig-verified + /internal/tasks/* OIDC-gated), queue naming convention, per-capability invoker SA + Terraform co-location, OIDC verification conventions. Delivered by infra-google-cloud-task-spec.md. |
| 2.34.0 | 2026-04-22 | **Ledger (Part 1) landed.** §15 added covering Cloud Run control plane, Postgres version pinning + tier-migration downtime warning, per-service AR repo, WIF-only GHA auth, migrations-in-CI, Terraform-is-manual exceptions, IAM-gated ingress with no allUsers, unresolved Convex→Ledger auth blocker, Cloud Run↔Secret Manager IAM propagation workaround, production-env escalation trigger, and Node/pnpm version-drift traps. First non-GKE service live in the platform. |
| 2.33.1 | 2026-04-22 | Monorepo migration: paths updated. |
| 2.33.0 | 2026-04-15 | **GCS Fuse file storage.** §2.2: Workload Identity re-enabled for GCS Fuse. §2.3: GCS Fuse CSI driver binding constraint. §5.1: added `gcs.tf`. §5.2: added bucket, GSA, KSA naming. §6.3: `clawcraft-gcs-reader` GSA + bucket IAM bindings + WI binding. §6.4: `ensureServiceAccount` added to provisioning order. §6.6: deployment gains GCS Fuse pod annotation, `serviceAccountName: gcs-reader`, two CSI volumes, two volume mounts. §8: Pod → GCS trust boundary. §10: invariant 16 — GCS Fuse readOnly. |
| 2.32.0 | 2026-04-10 | PVC mount topology v2.0.0. §6.5: system files now ConfigMap subPath mounts at `/zeroclaw-data/workspace/system/*.md` (was directory mount at `/system`). emptyDir moved to `/zeroclaw-data/.zeroclaw/` (was `/zeroclaw-config/`). Config resolution uses Dockerfile defaults — no `ZEROCLAW_CONFIG_DIR` env var. §6.6: deployment template updated — `workingDir: /zeroclaw-data/workspace`, init container copies to `/zeroclaw-data/.zeroclaw/config.toml`, PVC at `/zeroclaw-data/workspace`, emptyDir at `/zeroclaw-data/.zeroclaw`, system files as subPath overlays, `ZEROCLAW_CONFIG_DIR` and `ZEROCLAW_WORKSPACE` env vars removed, `ZEROCLAW_SYSTEM_DIR` changed to `/zeroclaw-data/workspace/system`. ConfigMap volume no longer uses `items` projection. Notes rewritten for 2-directory topology. |
| 2.30.1 | 2026-04-09 | Full resource reconciliation on all lifecycle paths. §6.4: `scaleUp` and `restartPodForIntegration` now reconcile the full K8s resource set (PVC, NetworkPolicy, Service, ConfigMap, Deployment) — previously only `provisionUser` did this, so port changes and NetworkPolicy updates never propagated on pod restarts. `ensureService` now uses `patchNamespacedService` to update ports on existing services (fixes early-return bug where existing ClusterIP services were never updated). §6.5: integration restart flow updated. §6.6: deployment notes updated. §10: invariants 9 and 12 broadened to cover all three lifecycle paths. |
| 2.30.0 | 2026-04-09 | Webhook channel port 42618. §2: pod diagram updated. NetworkPolicy allows port 42618. Service exposes webhook port. Container spec includes webhook port. |
| 2.29.0 | 2026-04-09 | **Email relay via Nginx gateway.** §7.1: description expanded — gateway now routes relay traffic in addition to browser WS. §7.1.1: architecture diagram gains HTTP relay (`/relay/{userId}` → pod `/webhook`) and WS relay fallback (`/ws/relay/{userId}` → pod `/ws/chat`) flows. §7.1.6: three new rules — `/relay/{userId}` and `/ws/relay/{userId}` must validate `X-Relay-Token`, relay must proxy to pod endpoints with correct headers. §8: trust boundaries updated — `clawcraft-system`→Pod split into WS chat and relay rows; new Convex→Nginx gateway relay boundary added. Terraform: `gateway_relay_token` variable for Nginx ConfigMap. |
| 2.28.0 | 2026-04-07 | Email v2 — Cloudflare Email Routing migration. §5.1: `apps/email-ingest/` updated — `hmac.ts` removed (no Mailgun HMAC). Worker description updated to CF Email Routing proxy. §5.2: Email DNS naming updated — MX auto-managed by CF Email Routing (no Terraform MX/SPF records). `infra/terraform/cloudflare.tf`: Mailgun MX and SPF records removed. |
| 2.27.0 | 2026-04-06 | Email ingest infrastructure. §5.1: `infra/terraform/cloudflare.tf` added to directory layout (MX + SPF records for `agent.clawcraft.ca` → Mailgun). `apps/email-ingest/` directory added — Cloudflare Worker for HMAC verification, attachment upload to Convex storage, JSON forwarding to `/email-webhook`. §5.2: `agent.clawcraft.ca` DNS and `email-ingest` worker naming conventions added. |
| 2.26.0 | 2026-04-02 | Provisioning performance. §6.4: `provisionUser` K8s calls parallelized — Namespace → [PVC, ConfigMap, NetworkPolicy, Service] (parallel) → Deployment. 3 sequential rounds instead of 6. `pollHealth` initial delay reduced from 2000ms to 500ms, retry interval from 2000ms to 1000ms, `MAX_ATTEMPTS` raised from 45 to 90 (maintains ~90s total timeout). §9: scale timeout row updated for new poll cadence. |
| 2.25.0 | 2026-04-01 | Remove per-user LoadBalancers. §2: architecture diagram updated — per-user services are ClusterIP (no external IP). §5.1: NetworkPolicy template description corrected. §8: "Internet → Pod" trust boundary changed to blocked. §9: LB IP error handling row replaced with deterministic endpoint resolution. §10: invariant 3 updated (no external ingress), invariant 13 rewritten (deterministic pod endpoint via `ClawPodIdentity.endpoint`, no LB IP resolution). Code: `ensureService` creates ClusterIP, migrates existing LB→ClusterIP. `ensureNetworkPolicy` removes allow-all external ingress. `provisionUser` no longer waits for LB IP. `pollHealth` uses deterministic DNS. `healthCheckAll` LB IP recovery branch removed. `scaleUp` LB drift check removed. `tryRecoverFromK8s` uses deterministic DNS. `waitForLoadBalancerIP` deleted (dead code). |
| 2.24.0 | 2026-03-30 | Async LB IP resolution. §6.4: `provisionUser` returns `{ endpoint: string \| null }` — null when LB IP not assigned during cold-start. §9: new error recovery row for LB IP timeout. §10: invariant 13 — `healthCheckAll` is sole recovery path for missing endpoints (non-blocking `getLoadBalancerIP` per cron tick). `pollHealth` remains pure health checker. |
| 2.23.0 | 2026-03-27 | Gateway TLS. §7.1.1: architecture flow updated with Cloudflare + Origin CA two-layer TLS. §7.1.3: TLS section rewritten — Cloudflare Origin CA (15yr RSA), static IP 34.47.6.138, Full (Strict) mode, health on plain HTTP 8080. §7.1.6: TLS rule updated for two-layer termination. |
| 2.22.0 | 2026-03-26 | Gateway Terraform migration. §5.1: `k8s/ws-gateway/` replaced by `infra/terraform/ws-gateway.tf` — gateway resources now Terraform-managed (kubernetes provider). §5.1: per-user service template corrected to ClusterIP. §7.1: updated to reference `infra/terraform/ws-gateway.tf` instead of `k8s/ws-gateway/`. |
| 2.21.0 | 2026-03-26 | WebSocket gateway + always-on pods. §2: architecture diagram updated — Nginx WS gateway in `clawcraft-system` namespace. §2.1: gateway added to component table. §2.3: web chat relay rule updated for WS path. §3: runtime section updated — pods stay running, no idle scale-down. §4: pod lifecycle updated — always-on policy replaces idle timeout. §4.1: pod state rules updated. §5.1: `k8s/ws-gateway/` manifests added to directory layout. §5.2: gateway naming conventions added. §6.7: predictive wake-up simplified — `onPageLoad` scale-up removed. §6.8: rewritten — idle scale-down cron removed, always-on policy documented. §7.1: new section — Nginx WS gateway architecture, resources (nginx:1.27-alpine, 100m/64Mi, 1 replica), DNS (`gw.clawcraft.ca`), TLS (Cloudflare pending), BackendConfig (3600s idle timeout), NetworkPolicy integration, rules. §8: trust boundaries updated — `clawcraft-system` → Pod allowed. §9: idle error/starting recovery row marked removed. §10: invariants 3 and 10 updated for gateway. |
| 2.20.0 | 2026-03-25 | Native Telegram channel config. §6.5: added `[channels_config]`/`[channels_config.telegram]` conditional section documentation — present when Telegram integration connected, contains decrypted bot token (plaintext in ConfigMap, deliberate trade-off). |
| 2.19.0 | 2026-03-24 | Composio integration layer. §2.1: ZeroClaw pod responsibility updated to include Composio integrations. §2.2: removed "Composio" from "What Was Removed" — Composio is now the active integration layer via ZeroClaw's native tool. |
| 2.18.0 | 2026-03-22 | x86_64/AMD64 binding constraint. §2.3: new rule — ALL workloads MUST use x86_64 (AMD64) architecture. ARM/T2A not available in `northamerica-northeast1`. Container images MUST target `linux/amd64`. Deployments MUST include `nodeSelector: kubernetes.io/arch: amd64`. §6.6: deployment template updated with `nodeSelector`. §14: ARM future improvement updated with cross-reference to §2.3 and migration checklist. |
| 2.17.0 | 2026-03-22 | Config override via init container. §6.5: config.toml now mounted via init container + emptyDir at /zeroclaw-config/ instead of subPath at /zeroclaw-data/.zeroclaw/. ZEROCLAW_CONFIG_DIR env var redirects config resolution. Added timeout_secs = 120 to [http_request], [web_search], and new [web_fetch] section. §6.6: deployment template updated with initContainers, emptyDir volume, and new env vars. Fixes subPath-cannot-override-image-layer bug causing 408 timeouts. |
| 2.16.0 | 2026-03-19 | Workspace file mount path + http_request enabled. §6.5: mount rule corrected from `/zeroclaw-data/<filename>` to `/zeroclaw-data/workspace/<filename>` — ZeroClaw's `build_system_prompt` searches `$ZEROCLAW_WORKSPACE` for identity files. `[http_request]` section now requires `enabled = true` and includes both `.convex.cloud` and `.convex.site` domains. §6.6: deployment template mount paths updated to match. |
| 2.15.0 | 2026-03-19 | Standard resource labels for dev/prod visibility. §5.3: new section documenting `resourceLabels()` helper — all K8s resources (Namespace, PVC, ConfigMap, NetworkPolicy, Deployment, Service) now carry `app`, `env`, and `user` labels. `env` derived from `CLAWCRAFT_ENV` env var (defaults to `"dev"`). Deployment `app` label is `claw-{userId}` (selector requirement); other resources use `app: clawcraft`. Labels roll out incrementally on next lifecycle event. §10: invariant 15 added (standard resource labels). |
| 2.14.0 | 2026-03-19 | Pod recovery hardening. §6.4: two new rules — `scaleUp` MUST call `ensurePVC()` before `reconcileDeployment()` (users provisioned before PVC support have no PVC, causing `Pending` pods); `scaleUp` MUST pass `restartAnnotation` to force rolling updates (without it, CrashLoopBackOff pods are never restarted). §6.8: `getIdlePods` now queries running, error, AND starting pods — CrashLoopBackOff/stuck pods are scaled down instead of burning resources. §9: five new error recovery rows (missing PVC, CrashLoopBackOff, stuck error >10min, fundamentally broken after 3 restarts, idle error/starting >30min). §10: invariants 12-14 added (prerequisite ensuring, forced restarts, bounded pod state TTL with `podRestartCount` tracking). Schema: `podRestartCount: v.optional(v.number())` added to users table. |
| 2.13.0 | 2026-03-19 | Slack OAuth & environment-aware redirects. §8: added Slack API trust boundary (OAuth client credentials, signing secret, per-user bot token) and Slack OAuth redirect boundary (return URL encoded in state param, validated against `ALLOWED_ORIGINS` allowlist). §10: invariant 11 — OAuth redirect allowlist pattern; new domains must be added to `ALLOWED_ORIGINS` in `http.ts`. |
| 2.12.0 | 2026-03-19 | Self-healing scaleUp & unified recovery. §6.4: `scaleUp` now checks `getDeploymentStatus()` before patching — if namespace/deployment is `not_found`, falls back to `provisionUser()` (full reprovision). New rule added. §6.7: rewritten to match actual `chat.ts` implementation — `onPageLoad` is a public mutation (not internalAction) that schedules `scaleUp` for `scaled_down`, `error`, and `not_found` states. Section renamed to "Predictive Wake-Up & Recovery". §9: added recovery rows for namespace deletion and error/not_found states. §10: invariant 10 added (self-healing scaleUp, no terminal pod states). |
| 2.11.0 | 2026-03-18 | Declarative pod lifecycle. §6.4: `scaleUp`/`restartDeployment` replaced by `buildDeploymentSpec` + `reconcileDeployment` — every operation that starts or restarts a pod sends the full desired deployment spec. K8s diffs and applies. Prevents config drift (e.g., stale PVC mount path on existing deployments). `scaleDown` remains an imperative replica patch (no drift risk). §6.6: deployment notes updated. §10: invariant 9 added (declarative deployments). |
| 2.10.0 | 2026-03-18 | Enable memory tools on non-CLI channels. §6.5: `non_cli_excluded_tools` safe policy now enables `memory_store` and `memory_forget` (21 excluded, was 23). |
| 2.9.0 | 2026-03-18 | Fix PVC mount path. §6.6: PVC `volumeMount.mountPath` changed from `/zeroclaw-data/memory/` to `/zeroclaw-data/workspace/memory/` — ZeroClaw writes `brain.db` to `{ZEROCLAW_WORKSPACE}/memory/` which resolves to `/zeroclaw-data/workspace/memory/`. Previous mount was empty (`lost+found` only). §10: invariant 4 updated. |
| 2.8.0 | 2026-03-17 | Telegram webhook mode. §6.5: removed `[channels_config.telegram]` conditional from ConfigMap template — Telegram credentials no longer in pod ConfigMap. §8: replaced "ZeroClaw → Telegram API" trust boundary with "Convex → Telegram API" — pod never contacts Telegram; Convex handles `setWebhook`, `deleteWebhook`, `sendMessage` via encrypted bot token in DB. |
| 2.7.0 | 2026-03-17 | Durable memory via PVC. §6.4: provisioning order changed to Namespace → PVC → ConfigMap → NetworkPolicy → Deployment → Service. §6.6: deployment now includes `data` volume (PVC) mounted at `/zeroclaw-data/memory/` for `brain.db` persistence. `ClawPodIdentity` gains `pvcName`. §6.5: gateway gains `request_timeout_secs = 120` for `/api/chat` agent loop. Pods are no longer stateless — PVC survives scale-down/up. |
| 2.6.0 | 2026-03-17 | Tool exclusion & convexUrl hardening. §6.5: `non_cli_excluded_tools` rule strengthened — MUST match ZeroClaw schema.rs defaults (25 tools). Safe policy yields 23 excluded (enables `http_request`, `image_info`). `[http_request]` section is now a MUST (security invariant), `convexUrl` is required. |
| 2.5.0 | 2026-03-17 | Persona & workspace files. §6.5: ConfigMap now includes workspace markdown files (IDENTITY.md, SOUL.md, USER.md, AGENTS.md, TOOLS.md, MEMORY.md, conditional BOOTSTRAP.md). Added `non_cli_excluded_tools`, `[http_request]`, `[web_search]`, `[identity]` to config.toml template. §6.6: Deployment template updated with workspace file volume mounts (`subPath`, `readOnly: true`) and `items` field on configMap volume for explicit key projection. BOOTSTRAP.md conditionally included. Deployment changed to create-or-replace pattern. |
| 2.4.0 | 2026-03-15 | Billing migration to `clawcraft-489901`. §3: infra setup flow now includes `setup-convex-env.sh` step. §5.1: added `setup-convex-env.sh` and `clawcraft-convex-key.json` to directory layout, moved `placeholder-pod.yaml` into `k8s-templates/`. §5.2: project naming updated. §6.3: added compute SA `artifactregistry.reader` rule (GKE nodes need image pull access). §6.4/§6.9: project ID references updated. Terraform state bucket renamed to `clawcraft-489901-terraform-state`. |
| 2.3.0 | 2026-03-07 | Telegram integration alignment. §6.5: added ConfigMap update + restart rule for integration changes. §8: added ZeroClaw→Telegram API trust boundary (long polling, bot token in ConfigMap). |
| 2.2.0 | 2026-03-07 | Integrate-provisioned-claw alignment. Service type ClusterIP→LoadBalancer (Convex Cloud cannot reach ClusterIP). NetworkPolicy: deny-all→allow TCP 42617 from any (pod auth via pre-shared token). Trust boundary "Internet→Pod" now allowed on gateway port. Updated runtime to v0.1.8-alpha-p1. Model ID to `anthropic/claude-sonnet-4`. Replaced `CLAW_IMAGE_TAG` with full URI in `CLAW_DOCKER_IMAGE`. |
| 2.1.0 | 2026-03-04 | E2E smoke test alignment. Fixed ConfigMap defaultMode 0600→0644 (non-root container). Fixed autonomy field: max_cost_per_day→max_cost_per_day_cents, added allowed_commands/forbidden_paths. Removed networking.tf (not implemented). Noted ARM unavailable in Montreal. |
| 2.0.0 | 2026-02-26 | Complete rewrite for ZeroClaw runtime. Removed Vertex AI, Composio, PVCs, ghost chat. Added scale-to-zero, predictive wake-up, ConfigMap injection, warm node strategy. |
| 1.1.0 | 2026-02-26 | Restructured as `infra/terraform/` package with Terraform configs and k8s templates |
| 1.0.0 | 2026-02-26 | Initial GCP infrastructure doctrine (OpenClaw) |
