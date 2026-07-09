# Observability Doctrine (Agentic Trace Contract)

**Version**: 1.8.0
**Status**: Binding
**Author**: Architect Agent
**Date**: 2026-06-04
**App**: Clawcraft (Managed ZeroClaw Hosting Platform)
**Scope note**: This doctrine governs the **contract**. The emitter that implements it
is the `zeroclaw` Rust runtime, which lives in a **separate repository** — there is no
`Cargo.toml` in clawcraft. Clawcraft owns only the OTLP config-injection surface
(§5) and this binding contract; the instrumentation (root/child spans, async
context-stash, by-reference blob store) is `zeroclaw`-repo work (cross-repo handoff:
`docs/tasks/ongoing/laminar/zeroclaw-emission-handoff.md`).

---

## 1. Authority

This document is **Binding**. Violations are architectural bugs.

Keywords MUST, MUST NOT, SHOULD, MAY follow RFC 2119.

This doctrine defines the agentic-observability trace/span/labelling contract for the
first layer of the observability stack: **logging agent intent**. The backend is Laminar
(OTel-native): **dev = a local self-hosted stack** (SPEC 2, `docker-compose`); **prod = a
managed Laminar cloud service** (SPEC PROD, 2026-06-02 — no self-hosted prod infra). The
trace's subject is what the agent *reasoned, decided, and did* — never user-experience or
client telemetry.

**Reference (clawcraft side):** `apps/clawcraft/convex/clients/gke.ts`
(`readOtlpCarrier` → `buildConfigToml` `[observability]` — the prod OTLP carrier; there is
**no** `OTEL_*` pod env), `infra/scripts/dev/render-claw-config.ts` (the dev carrier).
**Reference (contract consumer):** the `zeroclaw`
Rust emitter (separate repo).

**Cross-references:** claw-doctrine §3.2 (Linq adapter), §3.4 (Telegram native), §3.5
(Linq outbound, as-built), §4.1 (`{ message }` + `[conversationId:]` tag), §4.4
(queued-delivery gap), §5.6.1 (envelope family — `[EMAIL_RECEIVED]` / `[LINQ_RECEIVED]`
/ `[CLAWCRAFT_TRIGGER]`), §9.2 (ports 42617/42618), §13 (trust boundaries);
methodology-doctrine §1 (polysemous labels), §2 (no aspirational config), §3
(transitional schema discipline), §4 (inventory-as-debugging); backend-doctrine §16
(env-var registry); brief `docs/tasks/ongoing/laminar/laminar-brief.md` §2 (C1/C2), §4.

---

## 2. First-class scope constraints (LOCKED — MUST NOT relax)

- **C1 — Scope = agent intent.** The trace's subject is what the agent reasoned,
  decided, and did. It originates at the agent runtime's channel ingress. Everything
  upstream of the pod (provider, relay, Convex hop, browser/WS) is **OUT of scope**. No
  `traceparent` propagation is owed from upstream; the agent starts fresh at ingress.
- **C2 — Emitter vs boundary.** `service.name` names a process we *run and instrument*.
  The only emitter is `zeroclaw`. praxis / OpenRouter / Composio / Convex / channels are
  **never** emitters and **never** assigned a `service.name` — they are child spans
  `zeroclaw` creates, or dark boundaries.

---

## 3. Labelling split rules (binding)

Per methodology §1, every overloaded label is a bug. Each rule below admits no
"and"/"except".

| Label | Binding rule |
|---|---|
| `service.name` | Names ONLY a process we run **and** instrument. Sole value: `zeroclaw`. praxis / OpenRouter / Composio / Convex / channels NEVER carry it. |
| `trace_id` | Identity = the **activation boundary**, always. New activation ⇒ new `trace_id`, **even when an optional `traceparent` is supplied**. The discriminator is never "was a `traceparent` present." |
| `thread_id` | A **nullable trace-level attribute** only. Groups traces into a conversation; never IS the trace, never merges traces. Autonomous (`self_schedule`) traces carry a **synthetic group key**. |
| `deployment.environment` | In-instance organization only (`dev` / `staging` / `prod`). Isolation is the two-instance wall (separate instance — prod spec, mechanism TBD), **never** this attribute. |
| span name vs attribute | External-call identity lives in the span **name** + attributes, never `service.name`. praxis ⇒ span `praxis.<cmd>` + exit code; OpenRouter ⇒ `gen_ai.system=openrouter` + reasoning; Composio ⇒ toolkit + `log_…` id + status + duration. |

**Resource attributes carry ZERO credentials and ZERO PII** (claw-doctrine §6.1). The
resource-attribute set is exactly `{ service.name=zeroclaw, deployment.environment }`.
No token, no `preSharedToken`, no sender handle, no message content in resource attrs.
The OTLP auth header (§5, `config.toml [observability] otel_headers` —
`Authorization=Bearer <project-api-key>`) is a **transport credential** on the export
channel, **distinct** from resource attributes; it does **NOT** relax this zero-credential
rule (the key never enters the resource attributes / `service.name` /
`deployment.environment` set).

---

## 4. Trace lifetime

One activation = one trace. The `zeroclaw` runtime is the **sole trace owner**; nothing
else originates or owns a trace.

### 4.1 Start (mint a fresh `trace_id`)

At the pod's channel ingress, the moment `zeroclaw` accepts the trigger. The pod has
exactly **three ingress surfaces**; the *channel* is derived from the in-band envelope
tag (§4.4), **not** from the port:

- **WS** (`/ws/chat`, port 42617 gateway, pod-terminated streaming — claw §2.2) →
  channel `web`: root span starts on the **first frame of the turn**. `thread_id` from
  the `[conversationId: {threadId}]` text tag (claw §4.1).
- **Channel webhook** (`/webhook`, port 42618 — claw §9.2; async: returns 200
  immediately, the agent loop runs later) — the **shared ingress** for:
  - `email` — CF Email Routing → Worker → Convex `emailRelay.ts` → gateway
    `/relay/{userId}` → pod:42618, `[EMAIL_RECEIVED]` envelope (claw §5.6.1).
  - `sms` — the **Linq** channel (SMS/iMessage/RCS, LIVE): Linq → Convex
    `/linq-webhook` (HMAC) → durable `messages` row → gateway `/relay/{userId}` →
    pod:42618, `[LINQ_RECEIVED]` envelope (claw §3.2 / §3.5 / §5.6.1).
  - Convex-issued directives (`[CLAWCRAFT_TRIGGER]`, claw §5.6.1) and `self_schedule`
    cron.

  The root span starts when the `/webhook` handler accepts `{ sender, content }` —
  **before** it returns 200. The 200-ACK closes only its own short ingress span (a
  child/sibling), **never** the activation span.
- **Telegram native** (`getUpdates` long-poll — the one channel the pod terminates
  directly, claw §3.4) → channel `telegram`.

`self_schedule` is **autonomous**: there is no user thread; `thread_id` = a **synthetic
group key**.

**Single exporting observer (binding).** All three ingress surfaces MUST emit their root
activation span through the **same** exporting observer / `TracerProvider`. A runtime that
instruments a surface but routes its span to a *different*, non-exporting observer instance
silently drops that trigger's **entire** trace while the other surfaces succeed — a §7.1
violation that passes every config check (endpoint, `otel_headers`, `backend` are all
correct). The emitter MUST share one observer across its runtime components
(gateway / channels / scheduler / heartbeat / …), never construct one per component.
*(Incident 2026-06-02: the WS first-frame path called `start_activation` on a per-component
non-OTel observer while the webhook path held the real `OtelObserver`; only `web_chat` traces
were dropped. Fixed by sharing one observer — see the zeroclaw emission handoff.)*

### 4.2 End (close at activation quiescence)

At activation quiescence: no remaining work for this activation **AND** the terminal
reply dispatched on the originating channel. **NOT** at inbound ACK. The activation span
stays open across the async gap while the agent loop runs and posts back
(`/container-webhook` for the webhook channel; final WS frame for web; Telegram Bot API
send for telegram).

**Lost-reply rule (binding):** an activation whose terminal external reply is lost
(claw-doctrine §4.4 queued-delivery gap) **still closes its span at quiescence**. The
doctrine MUST NOT assume every activation ends with a *successful external reply* — it
ends when the agent has no remaining work.

### 4.3 Carrier (across the async gap)

The agent restores its **own stashed context**. There is **no handoff target** — Convex
is a dark boundary and cannot propagate context. The carrier is the agent's in-pod
stash, never a bare id handed across a boundary. This is `zeroclaw`-repo work,
contracted here.

---

## 5. OTLP carrier: `config.toml [observability]` (NOT pod env)

The `zeroclaw` emitter reads its OTLP configuration from `config.toml`
`[observability]` — it does **NOT** read the standard OTLP env vars
(`OTEL_EXPORTER_OTLP_*`). The SPEC-1 env-injection surface (the `gke.ts` env trio + the dev
compose `OTEL_*` block) was **vestigial** and is **retired**: the emitter ignored it. The
single carrier is `config.toml`, rendered by `buildConfigToml`
(`apps/clawcraft/domain/claw-config.ts`).

The rendered block (mirrors the emitter's `ObservabilityConfig`):

```toml
[observability]
backend = "otel"                                        # "otel" ⇒ export; "log" ⇒ local rolling buffer, no export
otel_endpoint = "http://host.docker.internal:8000"      # OTLP/HTTP base; emitter POSTs {endpoint}/v1/traces
otel_service_name = "zeroclaw"
otel_headers = "Authorization=Bearer <project-api-key>" # rendered credential (claw §6.1/§6.2)
otel_deployment_environment = "dev"                     # dev/staging/prod — resource + every-span attr (§7.1 dual-emit); non-PII; omitted when export off
runtime_trace_mode = "rolling"
runtime_trace_max_entries = 500
```

`otel_deployment_environment` is rendered **only** when an endpoint is set (it is
meaningless without export). Its value is a **non-secret org enum**, sourced the same way
the endpoint is — dev: `LAMINAR_DEPLOYMENT_ENV` from `infra/.env.dev` (defaults to `"dev"`);
prod: `readOtlpCarrier` (`LAMINAR_DEPLOYMENT_ENVIRONMENT`, defaults to `"prod"` once the
endpoint is active) → param into the pure `buildConfigToml` (backend §9 — env read at the
client boundary, never inside the renderer). The emitter stamps it as a resource attr **and**
a span attr on **every** span (§7.1 dual-emit — Laminar drops resource attrs and does not
inherit env from the root).

**Transport is OTLP/HTTP-protobuf, not gRPC.** The emitter POSTs to
`{otel_endpoint}/v1/traces`. The Laminar ingest is the app-server **`:8000`** HTTP receiver
(`:8001` is gRPC-only and is **not** used). `otel_endpoint` is a **base URL** (no
`/v1/traces` suffix).

**One carrier, one renderer per environment:**

| Env | Renderer | Gate (`otlpEndpoint`) | Result |
|---|---|---|---|
| **Dev** | `infra/scripts/dev/render-claw-config.ts` → `buildConfigToml` | `LAMINAR_OTLP_ENDPOINT` from `infra/.env.dev` (defaults to `http://host.docker.internal:8000`; opt out by setting it empty) | **active-by-default** (local self-host) |
| **Prod** | `gke.ts:buildConfigToml` (both call sites) → ConfigMap | `process.env.LAMINAR_OTLP_ENDPOINT` (set by ops at activation to the **managed cloud OTLP URL**, `https`/TLS) | **`backend = "log"` until ops activates**; once set ⇒ `backend = "otel"` exporting to managed Laminar |

**Single gate.** `otel_endpoint` set (non-empty) ⇒ `backend = "otel"`; absent/empty ⇒
`backend = "log"` (no export, otel fields omitted). One signal (`LAMINAR_OTLP_ENDPOINT`)
drives both the selector and the endpoint, so they cannot disagree.

**Auth is mandatory.** The Laminar ingest **drops unauthenticated spans**, so `otel_headers`
carries the project API key (`Authorization=Bearer <key>`). It is rendered as a **literal**
into `config.toml`, exactly like the OpenRouter `api_key` and `pre_shared_token` already are
— ZeroClaw's TOML parser has no env interpolation, so a credential the pod needs is inlined
at render time (claw-doctrine §6.1/§6.2 — the OTLP key is a documented `config.toml`
credential, peer to the Telegram bot token). The key is **never** a resource attribute (§3).

**Prod = managed activation (SPEC PROD, 2026-06-02).** Prod is a **managed Laminar cloud
service** — there is no self-hosted prod infra. The `gke.ts` carrier threads BOTH the
endpoint and the managed-project key from prod Convex env, read at the **Convex client
boundary** (`readOtlpCarrier`) and passed as `buildConfigToml` parameters
(`otlpEndpoint`/`otlpHeaders`) — the env read at the client boundary, the pure builder
receiving params, is exactly backend §9 (peer to `CONTAINER_SERVICE_TOKEN`). The carrier is
**inert by default** (`backend = "log"`) and activates only when ops sets the two prod Convex
env vars at activation (`docs/tasks/completed/laminar/laminar-prod-activation-runbook.md`):

- `LAMINAR_OTLP_ENDPOINT` = the **managed cloud OTLP base URL**, scheme **`https://` (TLS)** —
  not the dev self-host's insecure h2c `:8000`. No `/v1/traces` suffix.
- `LAMINAR_OTLP_HEADERS` = `Authorization=Bearer <managed-project-key>` — the SECRET,
  sourced from prod Convex env (Secret-Manager / `secrets-migrate` backed, registered in
  `infra/secrets-manifest.yaml audit_pending:`), **never** committed and **never** copied
  from the dev project's key (dev and managed-prod are different projects — rotate at the
  consumer).

A **concrete** prod `otel_endpoint`/`otel_headers` value MUST NOT be committed to `gke.ts`
or any tracked file (methodology §2 — provision-then-configure ordering). When the endpoint
is set but the headers are empty, `readOtlpCarrier` **fails loud** (managed Laminar drops
unauthenticated spans — §10 #12). Dev/prod reach parity at activation; the only differences
are environment-set values (managed URL + managed key + TLS vs the local self-host).

### 5.1 Why config.toml, not env (cross-repo contract)

The emitter's `ObservabilityConfig` (zeroclaw repo, `src/config/schema.rs`) has **eight**
fields: `backend`, `otel_endpoint`, `otel_service_name`, `otel_headers`,
`otel_deployment_environment`, `runtime_trace_mode`, `runtime_trace_path`,
`runtime_trace_max_entries`. It builds the OTLP/HTTP exporter as
`.with_http().with_endpoint(otel_endpoint).with_headers(otel_headers)` — there is **no env
read** anywhere in the emitter's observability path. This was confirmed link-by-link during
the SPEC-2 live integration: the emitter initialized to its `http://localhost:4318` default
while every `OTEL_*` env was set, and an authenticated HTTP/protobuf span to `:8000`
landed while an unauthenticated one was dropped. Hence the carrier is `config.toml`; the env
injection is retired. The carriers are `render-claw-config.ts` (dev) and
`gke.ts:buildConfigToml` (prod); `updateConfigMap` renders the ConfigMap that carries it.

---

## 6. Channel + `thread_id` attribution

Convex/relay supplies `thread_id` **in-band** where a thread concept exists upstream:

- **web** — the `[conversationId: {threadId}]` text tag (claw §4.1).
- **channel webhook** — the `[X_RECEIVED]` / `[CLAWCRAFT_TRIGGER]` envelope
  `ThreadId:` / `Source:` lines (claw §5.6.1).

`zeroclaw` labels the **channel** from the in-band envelope tag on the shared 42618
webhook (`[EMAIL_RECEIVED]` → `email`, `[LINQ_RECEIVED]` → `sms`, `[CLAWCRAFT_TRIGGER]`
→ directive/cron), and from the ingress surface for WS (`web`) and Telegram native
(`telegram`).

This keeps the pod **channel-blind**: it parses the tags it was handed; it does not
learn channels as a network concept. Observability introduces **NO new channel
awareness** and **NO new protocol field** — the relay bodies (`{ message }`,
`{ sender, content }`) MUST NOT be widened (claw-doctrine §4.1 / §5.6.1 / §5.6.2).
Attribution rides in-band only.

**Open dependency — email `thread_id`.** Unlike Linq, the inbound email relay is not
confirmed to carry an in-band thread tag today. `thread_id` for `email` is **nullable**
until that carrier ships (backend dependency — out of scope here; do not implement).

---

## 7. Span set (closed positive set + explicit negative set)

### 7.1 Positive set — the field-level governance allowlist

§7.1 is the **closed allowlist** the emitter gates against: `zeroclaw` MUST NOT emit a
span, span-attribute, or resource-attribute not enumerated here. A field is illegal
until it lands in this set — this is the cross-repo gate (zeroclaw blocks emission of
un-allowlisted fields; commit `3fd09e639` shipped `deployment.environment` +
`gen_ai.reasoning` ahead of the doctrine — reconciled below).

**Spans `zeroclaw` emits:**

- **Root activation span** — `service.name=zeroclaw`, `deployment.environment`, channel,
  nullable `thread_id`, (dev-only) `user.id` (+ its `lmnr.association.properties.user_id`
  twin — see dual-emit below), and the Laminar replay `lmnr.span.input` / `lmnr.span.output`
  (the Root input/output columns — see §7.1 note). Also carries the **native OTel Span Status**
  (`Ok` on a clean turn / `Error`, empty description, on a failed turn) — set at every trigger
  site from the turn outcome; a native OTel field, **not** a §7.1 attribute (see Status note).
  Lifetime per §4.
- **`llm.call` (OpenRouter client) span** — `gen_ai.system=openrouter` + `gen_ai.reasoning`
  (inner thoughts), and this call's Laminar replay `lmnr.span.input` / `lmnr.span.output`
  (the scrubbed prompt/completion — Laminar's per-span `llm.call` **message-view + full-text**
  source; see §7.1 note).
- **`tool.call` (Composio + other instrumented tool) span** — toolkit, `log_…` id, status,
  duration.
- **`praxis.<cmd>` child span** — `cmd` + exit code ONLY (black box; praxis is **never**
  internally instrumented — C2). `tool.output` / `tool.error` MUST NOT be attached to a
  praxis span.
- **`delivery` child span** — wraps the **outbound response-delivery POST** in
  `WebhookChannel::send` (the **native channel** `[channels_config.webhook]` `send_url` path
  **only** — NOT the gateway sync `/webhook`, whose reply returns in the axum response body,
  inherently covered by the root). Child of the `agent.activation` root (the root span Arc is
  hoisted out of the LLM-loop closure in `process_channel_message` so the trace stays open
  through delivery). Closes the trace lifetime honestly — distinguishes *"agent generated a
  reply"* from *"reply reached the user"*. Attrs: `channel="webhook"`, `net.peer.name` (**host
  only**), `http.status_code` (on a completed send). Status set from the HTTP outcome (`Error`
  on transport failure / non-2xx, `Ok` on 2xx) — see the delivery + exception-event note. **Not**
  a content/PII span (no full URL / query / `Authorization` / body / recipient — host only), so
  **not** under the dev-data-only content gate.
- **Convex-POST wrapper span** — optional, only if `zeroclaw` wraps its outbound POST.

**Span-attribute allowlist (content + attribution fields).** Each field is permitted ONLY
on the named span/scope, ONLY under its emitter constraint (already enforced in zeroclaw):

| Field | Span / scope | Emitter constraint |
|---|---|---|
| `gen_ai.prompt` | `llm.call` | `scrub_credentials` + truncate 16k. OTel GenAI semantics; **not** Laminar's Root-input source (see note) |
| `gen_ai.completion` | `llm.call` | `scrub_credentials` + truncate 16k. OTel GenAI semantics; **not** Laminar's Root-output source (see note) |
| `lmnr.span.input` | **root span** + `llm.call` | `scrub_credentials` + truncate 16k. On the **root**: scrubbed final user message → Laminar's Root **input** replay column. On **`llm.call`**: this call's scrubbed prompt → that span's own **input** (message-view + full-text). See both notes. |
| `lmnr.span.output` | **root span** + `llm.call` | `scrub_credentials` + truncate 16k. On the **root**: scrubbed final assistant reply → Laminar's Root **output** replay column. On **`llm.call`**: this call's scrubbed completion → that span's own **output** (message-view + full-text). See both notes. |
| `gen_ai.reasoning` | `llm.call` (incl. streaming) | already in set — W1a stopped dropping it on the streaming path (no new field) |
| `tool.input` | `tool.call` | `scrub_credentials` (**mandatory even in dev**) + truncate 16k via `truncate_with_ellipsis(x, 16_000)`. **Secrets-bearing — strictest content tier**; see note |
| `tool.output` | `tool.call` | `scrub_credentials` + truncate |
| `tool.error` | `tool.call` (failure) | `scrub_credentials` |
| `channel` | `delivery` (also a root attr) | delivery-target channel enum (`"webhook"`). Non-content; no scrub |
| `net.peer.name` | `delivery` | outbound **host only** (`reqwest::Url::host_str`) — **never** the full URL / path / query / `Authorization` / body. Non-PII; no scrub |
| `http.status_code` | `delivery` (completed send) | int (e.g. `200`). Absent on a transport-level failure (the POST never got a status). Non-content; no scrub |
| `reliability.*` (retry/fallback/exception summary) | **root span** | UI-visibility **dual-emit** of the reliability span events (see span-event note). `scrub_credentials` + 16k on `*.last_error` / `*.exception.message`; enum/int/bool otherwise. **Summary only** — never unbounded indexed keys (`reliability.retry.0.*`) |
| `user.id` | **root span only** | span attribute, **never** a resource attribute (§3 / §10 #6). **Dual-emitted** with `lmnr.association.properties.user_id` — see below. |
| `lmnr.association.properties.user_id` | **root span only** | Laminar association-property twin of `user.id` (same validated pod-owner id); the copy Laminar projects into its indexed `user_id` column. Same emitter constraint + dev-only gate as `user.id`; **never** a resource attribute. |
| `deployment.environment` | **every span** (root + all children — mirror of the resource attr) | **dual-emitted, every span** — see below. Non-PII org name (§3); `scrub`/truncate N/A (short enum) |

**`deployment.environment` is dual-emitted (resource attr AND a span attr on EVERY span) —
binding.** Self-hosted **Laminar discards OTLP resource attributes** (it consumes
`service.name` for project routing and drops the rest; verified — `service.name` and
`deployment.environment` land in **0** queryable spans when sent resource-only). So the
resource attribute alone is **invisible and unfilterable** in Laminar. The emitter therefore
**also stamps `deployment.environment` as a span attribute on every span it emits** — the root
`agent.activation` span **and every child** (`llm.call`, `tool.call`, `praxis.<cmd>`, the
Convex-POST wrapper) — re-stamped in zeroclaw `OtelSpan` with the same `set_attr` as
`trigger`/`thread_id`/`channel`. **Root-only is insufficient — binding:** Laminar's span
filter reads each span's **own** attributes and does **NOT** inherit env from the parent/root,
so a root-only stamp leaves every child span **unfilterable by environment** even under an
env-stamped root (verified — **108 historical child spans** (`76 llm.call + 32 tool.call`)
carried a parent root with `deployment.environment=dev` yet were themselves env-blank, and an
env-slice matched **0** of them; this is *not* a renderer no-op). With the every-span stamp a
`deployment.environment` slice returns child spans alongside roots. This is the **one** field
allowed in both the resource set and the span set (now on *every* span, not just the root); it
is non-PII (an org-name enum), so the every-span placement does not relax §3. **Verified live
2026-06-04 (bead `zc-bsgi`, zeroclaw branch `feat/zc-bsgi-every-span-env` commit `245e7277c`,
merge recommended):** a tool-forcing activation produced root + `llm.call` ×2 + `tool.call` ×1
**all** carrying `deployment.environment=dev` in ClickHouse, env-filterable as a set, while
pre-branch child spans were env-blank. **Queryable, not necessarily UI-surfaced:** the
every-span env is genuinely **queryable** (ClickHouse attribute slice confirmed) — unlike the
native OTel Status note this is *not* emitted-but-unqueryable; whether the v0.1.46 UI exposes
an attribute-filter affordance for it is a generic Laminar-UI-version question, unverified
here. (Reconciles the zeroclaw `zc-bsgi` change that widened the W6 / v1.6.1 root-only span
stamp to every span.)

**`user.id` is dual-emitted (`user.id` AND `lmnr.association.properties.user_id`, both
root-span attributes) — binding.** Same Laminar-sink rationale as `deployment.environment`,
one layer deeper: the OTel-semconv `user.id` span attribute lands only in Laminar's **raw
`attributes` blob**, NOT in its **indexed `user_id` column** (verified — the typed `user_id`
column stays empty for every span when only `user.id` is sent), so per-user
filtering/grouping in the Laminar UI — the entire point of the field (multi-tenant
attribution) — does not work. The emitter therefore **also** stamps Laminar's
association-property key `lmnr.association.properties.user_id` with the **same** validated
pod-owner id; that is the copy Laminar projects into its queryable `user_id` column. Both
keys are gated by the same `pod_user_id()` `Some(...)` guard (a bare 32-char `^[a-z0-9]{32}$`
Convex id), so an absent/malformed `CLAW_USER_ID` (unset, `claw-` prefix, wrong length) yields
**NEITHER** key (absence, never an empty string — never synthesized). Unlike
`deployment.environment` (resource + span), this dual-emit is **span + span** — neither key is
**ever** a resource attribute (§3). Verified live 2026-06-03 across all four user-facing
activation roots (web WS first-frame + main-loop, simple webhook, native channel,
process_message): both keys present and the indexed `user_id` column populated; CLI/cron and
invalid-id leave both keys **and** the typed column empty. (Reconciles the zeroclaw W5-A fix
that post-dated the original W5 `user.id`-only emission.)

**Laminar Root input/output derive from `lmnr.span.input` / `lmnr.span.output` on the ROOT
span — binding.** Same Laminar-sink rationale as the two dual-emit notes above, applied to the
replay content columns: Laminar's **Root input** / **Root output** columns
(`traces_replacing.root_span_input` / `root_span_output`) are derived from the trace's
**root/top span**, and Laminar fills a span's `input`/`output` **only** from the Laminar-native
`lmnr.span.input` / `lmnr.span.output` attributes (its manual-override path — it lifts them out
of the raw `attributes` blob into the dedicated typed columns). It does **NOT** map the flat
OTel `gen_ai.prompt` / `gen_ai.completion` strings into `input`/`output` (verified — with
`gen_ai.*` on the `llm.call` child only, **0** spans carried any input/output and **both** Root
columns stayed empty across CLI / WS / webhook). The emitter therefore stamps `lmnr.span.input`
(scrubbed + 16k-truncated final user message) and `lmnr.span.output` (scrubbed + 16k-truncated
final assistant reply) on the **root `agent.activation` span**; `gen_ai.prompt` /
`gen_ai.completion` remain on the `llm.call` child for OTel-semantic completeness but are **not**
the Root-column source. **Coverage (partial — binding to record):** wired and verified live
2026-06-03 on the **WS** (`process_chat_message`) and **channel-webhook** (`handle_webhook`)
activation roots (Root input scrubbed — `token: ghp_*[REDACTED]` — and 16k-truncated with a
trailing ellipsis; Root output populated) and on the **channel-trigger** (`process_message`)
root; the **CLI / cron `agent -m` root** (`Trigger::Cli`, session-scoped multi-turn, bead
`zc-0e0i`) **and** the **native-channel root** (`process_channel_message` — the :42618 webhook →
`run_tool_call_loop` path, bead `zc-tro4`) are **deferred** — their Root input/output stay null
(CLI until the multi-turn "what is *the* activation input" question is resolved; native-channel
until that path gains a root `lmnr.span.*` setter — `process_channel_message` has none today).
Confirmed blank live 2026-06-03: a native-channel activation root carried `has_input=0` /
`has_output=0` while its child `llm.call` was populated (see the `llm.call` note below).
(Reconciles the zeroclaw W2 fix `7ce12af71` that post-dated the original `gen_ai.*`-on-child
emission.)

**Laminar `llm.call` message-view + full-text search derive from `lmnr.span.input` /
`lmnr.span.output` on the `llm.call` span itself — binding.** The same Laminar-reads-its-own-keys
mechanism as the Root note, applied per-span one level **below** the root: Laminar fills **every**
span's `input`/`output` typed columns — and renders that span's UI message-view + indexes it for
full-text search — **only** from that span's Laminar-native `lmnr.span.input` / `lmnr.span.output`;
it does **NOT** read the OTel `gen_ai.prompt` / `gen_ai.completion` the `llm.call` already carries.
So before this fix the `llm.call` message-view was **blank** ("W2 blank") and its prompt/completion
text was not full-text searchable, even though `gen_ai.*` was set correctly. The emitter therefore
**mirrors** the same scrubbed + 16k-truncated content onto `lmnr.span.input` / `lmnr.span.output` on
the `llm.call` span, scrubbed **once** per site so the OTel and Laminar keys cannot drift;
`gen_ai.prompt` / `gen_ai.completion` stay on `llm.call` for OTel-semantic completeness but are
**not** the message-view source. **Coverage:** all four `llm.call` emit sites — `run_tool_call_loop`
(channel + CLI), `Agent::turn` (non-streaming), `Agent::turn_streamed` (web WS), `run_gateway_chat`
(simple webhook) — carry the mirror; verified live 2026-06-03 across three engines (WS /
simple-webhook / native-channel): each drove a real turn whose `llm.call` carried populated `input`
+ `output` and whose probe string was full-text searchable, while pre-fix `llm.call` rows were empty
(the `Agent::turn` non-streaming site shares the identical two-line mirror and was not separately
driven). Unlike the Root note this is **not** deferred for any engine — the `llm.call` mirror is
uniform across paths. (Reconciles the zeroclaw W2 fix `c15c1f1dd` / bead `zc-11qb` that post-dated
the root-only `lmnr.span.*` emission.)

**Per-iteration coverage incl. tool-call-only intermediate calls — binding.** The mirror above
is per **emit site**; this closes the gap **within** a multi-round tool loop. **Every** `llm.call`
iteration MUST carry non-blank `input` **and** `output` — including a **tool-call-only intermediate
call**, i.e. a model turn that emits *only* a tool call with no assistant prose. Such a call's
completion holds no text, so before this fix its `lmnr.span.output` / `gen_ai.completion` rendered
**blank** (the per-iteration analogue of the W2 blank, occurring mid-loop rather than per-site). The
emitter now serializes a content-free tool-call turn as a **`name(args)` summary** into
`lmnr.span.output` / `gen_ai.completion` (e.g. `shell({"command":"echo $RANDOM"})`,
`calculator({"a":623,"b":50,"function":"modulo"})`), with the growing tool-result delta in
`lmnr.span.input` / `gen_ai.prompt` — so no iteration's message-view is empty. Verified live
2026-06-03 (bead `zc-k42f`, zeroclaw commit `f35ca9ed4`): a 7-step dependent-chain prompt drove a
**13-iteration** loop (13 `llm.call` + 12 `tool.call`) on the **WS** path; all **13 / 13** `llm.call`
carried non-blank input + output, the **7** tool-call-only intermediate calls each showing the
`name(args)` summary in `output` (pre-fix: blank). Coverage is exercised on the **WS** (and channel /
CLI) loop path; the gateway **simple webhook** (`run_gateway_chat`, POST :42617 `/webhook`) is
**single-shot** — it returns the model's first completion raw and does **not** iterate, so it yields
exactly one `llm.call` and cannot exercise mid-loop coverage by construction (not a gap — a property
of that endpoint). (Reconciles the zeroclaw `zc-k42f` fix `f35ca9ed4` that post-dated the per-site
`zc-11qb` mirror.)

**Native OTel root Status (`Ok` / `Error`) set at every trigger site — binding; emitted but
NOT queryable on self-hosted Laminar.** The root `agent.activation` span carries the **native
OpenTelemetry Span Status** — `Status::Ok` on a clean turn, `Status::error("")` on a failed turn
(empty description: no raw error text, so the field is content-free and **ungated** — not subject
to the §7.1 redaction tiers). It is set from the outcome each site already holds: `process_message`
(`result.is_ok()`), CLI/cron `run()` (false on fatal turn error / true on clean exit — rare `?`
early-returns, e.g. session-history load/save, leave it Unset), webhook `handle_webhook` (Ok⇒true /
Err⇒false), WS `process_chat_message` (true/false via the ambient `current_span()`), and
`process_channel_message` (true only on `Completed(Ok(Ok(_)))`; cancel/timeout/provider-error ⇒
Error); the streamed `llm.call` error path is also closed (mirrors `turn()`). This is a **native
OTel field, NOT a §7.1 attribute** — it is part of the root span's definition (enumerated in the
Root activation span bullet), so the closed allowlist is unchanged and **no** resource/span
attribute is added (the emitter correctly required "no 7.1 allowlist change"). **Self-host caveat
(binding):** unlike the three dual-emit notes above there is **no `lmnr.*` workaround**, and none is
owed — Status is the correct native field. But self-hosted **Laminar v0.1.46 does not ingest OTLP
Status** into its queryable `status` column. Verified 2026-06-03: the wire carries it correctly —
raw OTLP capture decoded `Span.status.code` = `STATUS_CODE_OK` on a clean webhook turn and
`STATUS_CODE_ERROR` (empty message) on a forced-fail turn, on **both** the root and the child
`llm.call` — yet ClickHouse `spans.status` / `traces_replacing.status` stay **empty** for every
`agent.activation` row, clean and failed alike (**0 of 572** spans in the dev Laminar DB have ever
carried a non-empty status, including the child `llm.call`/`tool.call` spans that have set Status
since before this change). So on self-host, trace-level Ok/Error is **emitted but not queryable**;
the fix's value (queryable error-rate, alerting, "show only failed traces") lands only on a backend
that reads OTLP Status — **managed Laminar Cloud may; unverified**. A Laminar-self-host ingestion
gap, **not** a zeroclaw defect — the emitter side is complete and correct. (Records zeroclaw bead
`zc-sdjr`, commit `948328a82`.)

**`tool.input` is the strictest (secrets-bearing) content field — binding.** Tool
invocation arguments (`message_id`, `query`, `recipient`, raw shell command, …),
serialized to a string, routinely carry live secrets and PII — API keys, tokens,
recipients — at higher density than any other content field. It is the sibling **input**
counterpart to `tool.output` / `tool.error` on the `tool.call` span. Because of that
higher risk: (a) `scrub_credentials(...)` is **mandatory even in dev** — for `tool.input`
the scrub is the *dev* gate too, not merely the prod gate; (b) the **SPEC 3 redaction
tiers** are **load-bearing** for prod enablement — `tool.input` MUST NOT emit in the
managed-prod instance until they land, and it is the **last** content field to be cleared
for prod (never part of a blanket flip). Same 16k truncation bound
(`truncate_with_ellipsis(x, 16_000)`) and `tool.call`-only placement as `tool.output`.
This is the strictest member of the content tier; any future "redaction-sensitive"
sub-tier MUST include `tool.input` ahead of `tool.output`.

**Verified live 2026-06-03 (dev — bead `zc-72i1`).** A `tool.call` span carried `tool.input`
alongside `tool.output` (success path) and alongside `tool.error` (failure path); a
credential-bearing call whose args held `api_key` / `token` was redacted to `sk-t*[REDACTED]`
— the fake secret did **not** survive in plaintext. Both engine sites apply the same
`truncate_with_ellipsis(scrub_credentials(args), 16_000)` (`tool_execution.rs`
`run_tool_call_loop`, `agent.rs` `Agent::turn`); for `composio`, `tool.input` carries the
**full** args string while `composio.action` / `composio.toolkit` are set independently.

**`scrub_credentials` is key-driven — a known blind spot (binding).** The scrubber
(`SENSITIVE_KV_REGEX`, zeroclaw `loop_.rs`) only redacts a `<sensitive-key><:|=><value≥8>`
shape (key ∈ {`token`, `api_key`, `password`, `secret`, `user_key`, `bearer`,
`credential`}). It does **NOT** catch a token in `Authorization: Bearer <token>` value form —
the most common HTTP auth header an agent puts in `http_request` args — because
`Authorization` is not a sensitive key and `bearer` is followed by a **space**, not `:`/`=`
(verified leak 2026-06-03: `api_key=…` / `"api_key":"…"` / `token: …` scrub, but
`Authorization: Bearer …` and `Authorization=Bearer …` pass through). The same blind spot
applies to `tool.output` / `gen_ai.*` / `lmnr.span.*`. This is **load-bearing**: the
"scrub is the dev gate" guarantee above is only as strong as the scrubber, so zeroclaw MUST
extend `scrub_credentials` (add a `Bearer\s+<token>` value rule + treat `authorization` as a
sensitive key) **before** `tool.input` (or any content field) is cleared for prod under the
SPEC 3 redaction tiers. Tracked in §10 #16. *(Minor: scrubbing a JSON-quoted key emits a
stray double-quote — `{""api_key": "…"}` — cosmetic, no leak.)*

**Resource-attribute allowlist.** The resource set is **exactly**
`{ service.name=zeroclaw, deployment.environment }` — nothing else, ever.
`deployment.environment` (in-instance org name, non-PII — §3) is allowlisted here **and**
mirrored onto **every** span per the note above. No credential, no PII, no `user.id` may be a
resource attribute (§3 zero-credential/zero-PII rule; §10 #6).

**Dev-data-only caveat — prod enablement stays blocked.** The content + attribution fields
above (`gen_ai.prompt`, `gen_ai.completion`, `lmnr.span.input`, `lmnr.span.output`,
`tool.input` (the highest-risk member — secrets-bearing, see note), `tool.output`,
`tool.error`, `user.id` +
its `lmnr.association.properties.user_id` twin) carry agent/user content and are emitted
against **dev data only**. Their **prod** enablement is
gated on the **SPEC 3 redaction tiers** — until those land, these fields MUST NOT be emitted
in the managed-prod instance. This is a **field-specific content gate**, NOT a reinstatement
of the blanket "redaction-before-any-prod-PII" gate that §8 retired (2026-06-02): §8 keeps
incidental PII TOS-covered in prod for the *existing* span set; this caveat blocks only the
new high-magnitude content fields until redaction tiers exist. The interim payload bound is
**16k truncation**; the by-reference large-payload store is deferred — no `blob_ref` /
external-store field is pre-reserved (methodology §3).

**Span-event allowlist (reliability layer) — `retry-attempt` / `fallback-fired` / `exception`.**
§7.1 also governs OTel span **events** (a span's timestamped `events[]`, a distinct mechanism
from span attributes). The reliability layer (`src/providers/reliable.rs`) emits exactly three,
all on the **root `agent.activation` span** (the ambient span — never `llm.call`), via
`span.add_event`:

| Event | Fires when | Attributes |
|---|---|---|
| `retry-attempt` | every failed attempt (`push_failure`) | `attempt` (int), `max_attempts` (int), `provider`, `model`, `retryable` (bool), `reason`, `error` (`scrub_credentials` + 16k) |
| `fallback-fired` | a fallback provider/model **succeeds** (`record_provider_fallback`) | `from` = `provider/model`, `to` = `provider/model` (config enums — no scrub) |
| `exception` | terminal exhaustion — all providers/models failed (`all_failed`) | `exception.type` = `"provider_exhausted"`, `exception.message` (`scrub_credentials` + 16k), `exception.escaped` (bool) |

Stable event **names** + typed attrs are the contract form (`retry-attempt` with an `attempt`
attribute, **not** a per-attempt event name). No single activation shows all three
(`fallback-fired` needs a successful fallback; `exception` needs total failure). The same
scrub/16k content discipline as the other free-text fields applies to `error` /
`exception.message` (and inherits the `scrub_credentials` `Bearer`-value blind spot — §10 #16).
**Dev-data-only** under the same SPEC 3 prod gate as the content fields.

**Laminar v0.1.46 renders no span-event *timeline*, but DOES derive a span's red error
status from an `exception` event — binding caveat (refined 2026-06-04, bead `zc-hf7r`).**
Verified live 2026-06-04 (bead `zc-jz9y`): all three events land in self-hosted Laminar's
ClickHouse `spans.events` column (`Array(Tuple(timestamp, name, attributes))`) with attrs
intact — so, **unlike** OTLP resource attributes (dropped at ingest), span events **survive
ingest**. The v0.1.46 frontend **never queries `spans.events`** (no such SELECT, no span-events
component in the bundle), so the per-attempt **event timeline is invisible** in the dev trace UI
— queryable only via direct ClickHouse. **BUT — and this corrects the earlier "events entirely
invisible" claim — Laminar's ingest *does* read the `exception`-named event and projects it into
the span's typed `status` column** (value `"error"`), which the UI renders as the span's **red
error icon**. Empirically (2026-06-04, all spans in the dev ClickHouse): `status != ''` **iff**
the span carries an `exception` event — an exact 1:1 correlation, every other span (incl.
`set_status`-only failures) `status=''`. So the *terminal* `exception` event surfaces as red
trace status; the non-`exception` events (`retry-attempt` / `fallback-fired`) and the bare OTel
**StatusCode** do not. This is the mechanism behind the **root-Status gap (#15 / `zc-sdjr`)**:
v0.1.46 derives error status **only** from an `exception` span event (the OTel `record_exception`
convention + `exception.escaped`), **never** from `set_status(Error)`. Rendering the full event
*timeline* is still a Laminar-side change; surfacing a failure as red **today** is an emitter
lever — record an `exception` event (see §10 #15/#21, §11 R5).

**UI-visibility workaround — `reliability.*` root-span attributes (dual-emit).** Because the UI
*does* render the span **attribute** panel, the emitter **also** stamps a compact summary as
attributes on the root span — the established Laminar dual-emit pattern (`deployment.environment`
/ `user.id` / `lmnr.span.*`): `reliability.retry.count`,
`reliability.retry.last_{provider,model,reason}`, `reliability.retry.last_retryable`,
`reliability.retry.last_error` (scrub+16k), `reliability.fallback.{fired,from,to}`,
`reliability.exception.{type,escaped,message}` (`message` scrub+16k). This is a **summary**, not
the per-attempt timeline (which stays in the span events, ClickHouse-queryable); the emitter
MUST NOT emit unbounded indexed keys (`reliability.retry.0.*`). The OTel span events remain the
canonical, vendor-neutral record.

**`delivery` span — verified live; its failure status is currently invisible in v0.1.46 —
binding (bead `zc-hf7r`, zeroclaw branch `zc-hf7r-delivery-span` @ `c034820c2`, merge
recommended).** Verified live 2026-06-04 against dev self-hosted Laminar, both paths, native
channel `[channels_config.webhook]`:
- **Structure (PASS):** the `delivery` span is a **child of the `agent.activation` root** (not
  a sibling/orphan) on both runs — the hoisted root-span Arc keeps the trace open through the
  send. Confirmed in ClickHouse: `delivery.parent_span_id` = the activation root in both traces.
- **Attributes (PASS):** failure run → `{channel="webhook", net.peer.name="127.0.0.1"}` (no
  `http.status_code` — the POST never got a status); success run (200 sink) →
  `{channel="webhook", net.peer.name="127.0.0.1", http.status_code=200}`. **No secrets** — no
  full URL, query, `Authorization`, or body on either; `net.peer.name` is the **host only**.
- **Status (emitted correctly, but NOT visible in v0.1.46):** the span sets status from the HTTP
  outcome via the **bare OTel `set_status`** (`set_status(false)` on transport error / non-2xx →
  `Status::error("")`; `set_status(status.is_success())` on a response → `Ok`; `otel.rs:573`).
  Because v0.1.46 derives red error status **only** from an `exception` span event (see the
  span-event caveat above; root-Status gap #15) and the delivery path records **no** exception
  event, a **failed delivery does not render red** in the dev trace UI — its `status` column is
  blank, identical to a successful one. The OK/ERROR distinction is verifiable **only** by the
  attribute heuristic (`http.status_code` present + 2xx ⇒ OK; absent ⇒ transport-fail ⇒ Error)
  or by raw OTLP wire capture — **not** from the dev UI. This is the same emitted-vs-surfaced
  gap as #15, now also at the `delivery` child level. **Emitter lever (§11 R5):** to make a
  failed delivery red on v0.1.46 today, record an `exception` event (host + status code, no
  secrets) on the failure paths alongside `set_status(false)` — then it lights up like the
  reliability `exception` / `bogus-model` trace.

### 7.2 Negative set (NO spans at all)

Browser / Vite, Convex internals, praxis internals, and upstream/inbound relays (CF
Worker, `/relay/{userId}`, `/linq-webhook`, githook). This negative set MUST be a
consistent subset of claw-doctrine §13's trust-boundary table.

**Dark boundaries (span ONLY if `zeroclaw` wraps its own outbound call):** Convex,
inbound relays, githook (claw §13).

---

## 8. Redaction (customer-trigger — NOT a prod gate)

**PII in prod is accepted by design, TOS-covered for the alpha** (brief §5 amended
2026-06-02). Redaction is **NOT a prod blocker** and is **NOT pre-built**. It is built
**only on a customer trigger** — an explicit data-control / redaction request (or another
concrete trigger such as a contractual data-residency mandate). Under managed prod there is
no self-hosted collector, so when that trigger lands redaction is designed **in-SDK /
pre-export in the `zeroclaw` emitter**.

Per methodology §3 (transitional schema discipline): **MUST NOT** pre-reserve a
redaction-tier field, flag, or attribute now (no `redaction_tier:` / `redact:` param). A
field ships with a real consumer, a named end state, and a pinned removal commit — none of
which exist until a customer trigger lands.

> **History.** Through v1.4.0 this section read "redaction REQUIRED before prod PII — a hard
> dev→prod gate." The 2026-06-02 brief amendment (managed prod; PII TOS-covered for alpha)
> retired the hard gate: redaction is now a customer-triggered build, not a prod
> prerequisite. The no-pre-reserved-field rule is preserved unchanged.

---

## 9. Two-instance isolation (the prod wall)

`deployment.environment` is organization, never isolation (§3). Isolation is the
**two-instance wall**: a non-prod Laminar (dev; loose access) and a prod Laminar (tight
access), never one instance spanning the prod boundary. Prod observability MUST NOT share a
failure domain with the prod app.

- **Dev = a local self-hosted Laminar stack** (SPEC 2 / infra §15.25): a single long-lived
  `docker-compose` stack (`laminar` profile), decoupled from the `claw` runtime, reached
  over the host network.
- **Prod = a managed Laminar cloud service** (SPEC PROD, 2026-06-02): the operator's managed
  account/project, reached over the public internet (TLS egress) — no self-hosted prod
  datastores. The two-instance wall is **trivially satisfied**: dev (local self-host) and
  prod (managed cloud) are separate infra and separate failure domains by construction —
  managed prod cannot share a failure domain with the prod app. Access to the managed
  project is limited to PII-cleared operators (brief §4.3 access rule).

> **History.** v1.1.0 named "one single self-hosted Laminar instance, failure-domain
> mechanism (namespace vs GCP project) TBD at the prod-hosting spec." The 2026-06-02 brief
> amendment **dropped self-hosting prod on GKE in favor of a managed service** — so the
> namespace/separate-project failure-domain question is **moot** (managed cloud is a
> distinct provider/account). The dev≠prod wall and the no-shared-failure-domain guarantee
> are preserved and now satisfied by construction. Self-hosting prod on GKE remains
> revisitable if a trigger appears (cost at scale, data residency, customer mandate) — the
> deleted `laminar-spec-prod-hosting.md` is the starting point (recover from git history).

---

## 10. Failure modes this doctrine guards (binding checklist)

| # | Failure mode | Rule that forbids it |
|---|---|---|
| 1 | A **concrete** prod `otel_endpoint`/`otel_headers` value committed to `gke.ts` or any tracked file | §5 (no aspirational/committed config — methodology §2; ops sets both as prod Convex env at activation, provision-then-configure). The **dev** endpoint is concrete (a real local Laminar answers it); the **managed-prod** value is set at activation, never committed. |
| 2 | OTLP config rendered in one render path only | §5 (`buildConfigToml` is the single renderer — both `gke.ts` call sites + `render-claw-config.ts` must stay in sync) |
| 3 | OTLP config put in **pod ENV** (`OTEL_EXPORTER_OTLP_*`) | §5/§5.1 — the emitter ignores OTLP env; the carrier is `config.toml [observability]`. Env injection is retired; re-adding it is dead config (methodology §2). |
| 4 | `service.name` assigned to a boundary | §3 / C2 |
| 5 | `trace_id` reused when `traceparent` supplied | §3 (activation boundary is the sole discriminator) |
| 6 | PII/credential in resource attributes | §3 (attrs = `{service.name, deployment.environment}` only — claw §6.1; the OTLP auth header is a transport credential in `otel_headers`, never a resource attr) |
| 7 | New channel/protocol field added for attribution | §6 (in-band only; relay bodies not widened) |
| 8 | `otel_endpoint` empty/absent but `backend = "otel"` rendered | §5 (single gate — empty/absent endpoint ⇒ `backend = "log"`, otel fields omitted) |
| 9 | Laminar project API key committed to git, **or the managed-prod key copied from the dev project's key** | §5 (rendered `config.toml` credential — sourced from uncommitted `infra/.env.dev` in dev; read at the `gke.ts` client boundary from prod Convex env per backend §9 in prod). Never a literal in `gke.ts`/compose/committed files; dev and managed-prod are different projects — rotate at the consumer, never copy. |
| 10 | Prod `otel_endpoint`/`otel_headers` hardcoded in `gke.ts` | §5 (ops sets both as prod Convex env at activation; methodology §2 — never a committed literal) |
| 11 | Prod `otel_endpoint` missing the `https://` (TLS) scheme, or carrying a `/v1/traces` suffix (or dev pointing at gRPC `:8001`) | §5 (the managed endpoint is public TLS — `https://`, base URL with no suffix; the emitter is HTTP/protobuf and appends `/v1/traces`; the dev `:8001` gRPC port silently drops spans) |
| 12 | Span exported without `otel_headers` ⇒ Laminar drops it | §5 (auth is mandatory — `otel_headers` carries `Authorization=Bearer <key>`; unauthenticated spans are dropped) |
| 13 | One ingress surface's root spans silently never export (the component holds a per-component, non-exporting observer instance) while other surfaces export normally | §4.1 single-exporting-observer note / §7.1 (every ingress surface MUST emit through the **single** exporting observer; a per-component observer split is forbidden — passes all config checks, so only a per-trigger span count catches it) |
| 14 | Laminar **Root input/output** left empty because the user message / reply was set as `gen_ai.prompt` / `gen_ai.completion` on the `llm.call` **child** instead of `lmnr.span.input` / `lmnr.span.output` on the **root** activation span | §7.1 Laminar-Root note (Laminar derives the Root columns from the root/top span's `input`/`output`, populated **only** from `lmnr.span.*`; flat `gen_ai.*` strings are never lifted into `input`/`output` — verified live: 0 spans carried input/output under the `gen_ai.*`-on-child emission) |
| 15 | Root `agent.activation` span left **Status Unset** — trace-level success/failure only inferable by string-matching child `tool.error` (failed traces unqueryable / unalertable) | §7.1 native-OTel-root-Status note (every trigger site MUST set the root Status from the turn outcome — `Ok`/`Error`, empty description; native OTel field, ungated). **NB** this guards the *emit* side; self-hosted Laminar v0.1.46 does not surface the bare OTLP **StatusCode** (`set_status`) — verified emitted-but-unqueryable. **Refined 2026-06-04 (`zc-hf7r`):** v0.1.46 *does* populate the `status` column + red UI icon, but **only** from an `exception` span event (`record_exception`), never from `set_status` — exact 1:1 across the dev ClickHouse. So a failure shows red **iff** it is recorded as an `exception` event; the emitter lever to surface red today is `record_exception`, not `set_status` (§11 R5) |
| 16 | A live secret survives `scrub_credentials` because it sits in a non-key shape (`Authorization: Bearer <token>`, or any value not preceded by `<sensitive-key><:|=>`) and reaches `tool.input` / `tool.output` / `gen_ai.*` / `lmnr.span.*` in plaintext | §7.1 `scrub_credentials`-key-driven-blind-spot note (the scrubber is `SENSITIVE_KV_REGEX` key→value only — verified leak 2026-06-03; the "scrub is the dev gate" guarantee is only as strong as the scrubber, so zeroclaw MUST add a `Bearer\s+<token>` value rule + treat `authorization` as a sensitive key before any content field is cleared for prod under the SPEC 3 redaction tiers) |
| 17 | Laminar **`llm.call` message-view** left blank / not full-text searchable because the call's prompt/completion was set only as `gen_ai.prompt` / `gen_ai.completion` (OTel keys) and not mirrored to `lmnr.span.input` / `lmnr.span.output` on the **`llm.call`** span | §7.1 Laminar-`llm.call` note (Laminar fills **each** span's `input`/`output` + full-text index **only** from that span's `lmnr.span.*`; `gen_ai.*` is never read for the message-view — verified live: pre-fix `llm.call` rows carried 0 input/output under `gen_ai.*`-only; the per-span analogue of #14, which is the root-column case) |
| 18 | A **tool-call-only intermediate `llm.call`** (mid-loop turn emitting only a tool call, no assistant prose) left with **blank `output`** because the completion held no text | §7.1 per-iteration-coverage note (every `llm.call` iteration MUST carry non-blank input + output; a content-free tool-call turn is serialized as a `name(args)` summary into `lmnr.span.output` / `gen_ai.completion` — verified live 13/13, bead `zc-k42f` commit `f35ca9ed4`; the mid-loop analogue of #17). NB exercised on the WS/channel/CLI **loop** path — the simple-webhook `/webhook` is single-shot and emits one `llm.call` by construction |
| 19 | Child spans (`llm.call` / `tool.call` / …) **unfilterable by `deployment.environment`** because env was stamped on the **root span only** — Laminar reads each span's own attributes and does **not** inherit env from the parent/root | §7.1 `deployment.environment` every-span dual-emit note (the emitter MUST re-stamp env on **every** span, not just the root — verified live 2026-06-04: 108 historical child spans under env-stamped roots were themselves env-blank and matched **0** of an env-slice; bead `zc-bsgi`, branch commit `245e7277c`). NB env is genuinely **queryable** once per-span-stamped (ClickHouse confirmed) — distinct from #15 Status, which is emitted-but-unqueryable on self-host |
| 20 | Reliability span-event **timeline** (`retry-attempt` / `fallback-fired` / `exception`) emitted + ingested into Laminar `spans.events` but the per-attempt **timeline is invisible in the v0.1.46 UI** (frontend never queries that column; no MV lifts them into the `events` table) | §7.1 span-event note (the *timeline* is queryable only via direct ClickHouse; the UI-visibility path is the `reliability.*` root-span attribute dual-emit). **Refined 2026-06-04 (`zc-hf7r`):** the **`exception`** event is **not** invisible — v0.1.46 projects it into the span `status` column (red icon). Only the non-`exception` events + the timeline detail are unsurfaced; same ingest-vs-surface family as #15 (`zc-sdjr`); verified live, bead `zc-jz9y` |
| 21 | A **failed `delivery`** (outbound response POST failed / non-2xx) does **not** render red in the v0.1.46 UI — its status is set via bare `set_status(false)`, which v0.1.46 drops, and the delivery path records **no** `exception` event, so a failed delivery is indistinguishable from a successful one in the trace UI | §7.1 `delivery`-span note (status correctly emitted on the wire — `Error`/`Ok` from the HTTP outcome — but invisible on self-host for the same reason as #15; the OK/ERROR distinction is recoverable only via the `http.status_code` attribute heuristic or wire capture. Emitter lever to surface red today: `record_exception` on the failure paths — §11 R5. Verified live 2026-06-04, bead `zc-hf7r`, branch `zc-hf7r-delivery-span` @ `c034820c2`) |

---

## 11. Open emitter-side requests (cross-repo)

Consolidated, actionable list of the outstanding **`zeroclaw`-repo** work this doctrine is
waiting on — the single place to read before a cross-repo paste-back. Full rationale lives in
the linked §7.1 / §10 notes; the standing handoff file is
`docs/tasks/ongoing/laminar/zeroclaw-emission-handoff.md`. Ordered by priority.

| # | Priority | Request | Anchor | Bead |
|---|---|---|---|---|
| R1 | **P0 — prod-blocking** | Extend `scrub_credentials` (`SENSITIVE_KV_REGEX`, `loop_.rs`): add a `Bearer\s+<token>` **value-form** rule **and** treat `authorization` as a sensitive key. The scrubber is key-driven and leaks `Authorization: Bearer <token>` (verified 2026-06-03). MUST land **before** any content field (`tool.input` / `tool.output` / `gen_ai.*` / `lmnr.span.*`) is cleared for managed-prod under the SPEC 3 redaction tiers — the "scrub is the dev gate" guarantee is only as strong as the scrubber. Minor sibling: scrubbing a JSON-quoted key emits a stray `"` (`{""api_key": …}`) — cosmetic, no leak. | §7.1 scrubber-blind-spot note / §10 #16 | `zc-72i1` follow-up |
| R2 | P1 | Add a **root `lmnr.span.input` / `lmnr.span.output` setter** to `process_channel_message` (`src/channels/mod.rs` — the native-channel :42618 → `run_tool_call_loop` path), which has none today. Confirmed live: the native-channel activation root is `has_input=0` / `has_output=0` while its child `llm.call` is populated. Mirror the setter the WS / `handle_webhook` / `process_message` roots already carry. | §7.1 Root-input/output note / §10 #14 | `zc-tro4` |
| R3 | P1 (design) | Resolve **"what *is* the activation input"** for the session-scoped multi-turn CLI/cron `agent -m` root (`Trigger::Cli`, `src/agent/loop_.rs`), then wire its root `lmnr.span.*`. A product/design call (which turn's message is the trace input across a multi-turn session), not a mechanical fix. | §7.1 Root-input/output note | `zc-0e0i` |
| R4 | P2 (minor) | Set root Status on the CLI/cron `run()` **rare `?`-early-return** paths (e.g. session-history load/save) that bypass `set_status` and leave the root **Unset**. Correct on the emit side even though it is moot on self-host until a backend reads OTLP Status. | §7.1 native-OTel-root-Status note / §10 #15 | `zc-sdjr` follow-up |
| R5 | P2 | **Record an `exception` event** (host + status code, **no secrets** — same discipline as the reliability `exception` event) on the `WebhookChannel::send` **delivery-failure** paths (transport error + non-2xx), alongside the existing `set_status(false)`. v0.1.46 derives the red error icon **only** from an `exception` event, not from `set_status` (verified 1:1) — so a failed delivery is currently **invisible** as a failure in the dev UI. This is an **emitter-side lever** that surfaces the failure red **today** (not a backend gap). Generalizable: the same lever would make any `set_status(Error)` failure (incl. the root, #15) red on v0.1.46. | §7.1 `delivery`-span + span-event notes / §10 #21, #15 | `zc-hf7r` follow-up |

**Out of scope here — backend-side, NOT emitter requests.** Items whose **only** remaining gap
is Laminar-self-host ingestion are excluded from this list because the emitter side is already
complete and correct; they land for free on a backend that reads the field (managed Laminar Cloud
— unverified): the bare OTLP **StatusCode** ingestion (#15, `zc-sdjr`) and the span-event
**timeline** rendering (#20, `zc-jz9y`) are genuine Laminar-side gaps. **Caveat (2026-06-04,
`zc-hf7r`):** surfacing a *failure as red* is **not** purely backend — v0.1.46 already projects an
`exception` span event into the red status column, so `record_exception` is an emitter lever that
works today (filed as **R5** for the delivery case, generalizable to the root). The
`deployment.environment` per-span case (#19, `zc-bsgi`) is **already an emitter fix** (every-span
stamping) and is **done** — not listed. Do not file the StatusCode/timeline *rendering* gaps
against `zeroclaw`.

---

## 12. Change Log

| Version | Date | Changes |
| 1.8.0 | 2026-06-04 | **New `delivery` span legalized + the v0.1.46 error-status mechanism corrected (bead `zc-hf7r`, zeroclaw branch `zc-hf7r-delivery-span` @ `c034820c2`, merge recommended; verified live dev).** §7.1 gains the **`delivery`** child span — wraps the outbound response-delivery POST in `WebhookChannel::send` (native channel `[channels_config.webhook]` `send_url` path **only**; the gateway sync `/webhook` returns its reply in the response body, no child) as a child of the hoisted `agent.activation` root, distinguishing "reply generated" from "reply reached the user". Span-attribute allowlist gains three **non-content** rows scoped to `delivery`: `channel` (enum), `net.peer.name` (**host only** — never URL/query/auth/body), `http.status_code` (int, absent on transport failure). **Not** under the dev-data-only content gate (no PII). **Verified live 2026-06-04** both paths: failure (unreachable `127.0.0.1:1`) → `delivery` child of an OK root with `{channel, net.peer.name}` and no status code; success (200 sink) → `+http.status_code=200`; secret check clean (host-only) on both. **Major correction — the v0.1.46 error-status mechanism:** the prior #20 / span-event note claimed span events are "entirely invisible" on v0.1.46 and #15 framed root Status as wholly emitted-but-unsurfaced. Empirically (all spans in dev ClickHouse), the typed `status="error"` column + the UI's **red icon** are populated **iff** the span carries an `exception` span event (`record_exception`) — exact 1:1 — and **never** from the bare OTel `set_status` StatusCode. So: (a) the `exception` event **is** surfaced (as red status, not as a timeline); (b) bare `set_status` is dropped (confirms #15's emit-vs-surface gap at the wire level); (c) the `delivery` span sets status via bare `set_status(false/true)` + records no exception event, so a **failed delivery is invisible as a failure** in the dev UI (recoverable only via the `http.status_code` attribute heuristic or wire capture — §10 #21). Span-event caveat rewritten, #15 + #20 refined, #21 added. **§11 R5 added** — `record_exception` (host + status code, no secrets) on the delivery-failure paths is an **emitter lever** that surfaces the failure red on v0.1.46 today (generalizable to the root, #15); the out-of-scope note re-scoped so only the bare-StatusCode ingestion + the event-*timeline* rendering remain pure backend gaps. **No lifetime / labelling / carrier / resource-set / isolation change** — one new non-content span + a corrected backend-surface mechanism. |
|---------|------|---------|
| 1.7.1 | 2026-06-04 | **New §11 "Open emitter-side requests (cross-repo)" — consolidation, no contract change.** The outstanding `zeroclaw`-repo asks were already documented but scattered across §7.1 notes + §10 failure rows; §11 collects them into one priority-ordered, actionable table for cross-repo paste-back (Change Log renumbered §11 → §12; no other section referenced §11). Lists **R1** (P0 prod-blocking — extend `scrub_credentials` for `Bearer`/`authorization`, §10 #16 / `zc-72i1` follow-up), **R2** (native-channel root `lmnr.span.*` setter, §10 #14 / `zc-tro4`), **R3** (CLI multi-turn "what is the activation input" design call + root wiring, `zc-0e0i`), **R4** (CLI `run()` `?`-early-return Status, §10 #15 / `zc-sdjr` follow-up). Crucially **scopes out backend-only gaps** so they are NOT filed against `zeroclaw`: root OTel Status (#15) and reliability span-event UI rendering (#20) are Laminar-self-host ingestion gaps with the emitter side complete; the `deployment.environment` per-span case (#19, `zc-bsgi`) is an already-shipped emitter fix. **No new field, no allowlist / lifetime / labelling / carrier / resource-set / isolation change** — a tracking section pointing at existing binding notes. |
| 1.7.0 | 2026-06-04 | **§7 events clause — reliability OTel span events legalized + `reliability.*` UI-visibility dual-emit (bead `zc-jz9y`, zeroclaw branch `zc-jz9y-reliability-events` @ `6c887fa5e`).** §7.1 gains a **span-event allowlist** — the closed allowlist now governs OTel span *events* (distinct from span attributes) for the first time: the reliability layer (`src/providers/reliable.rs`) emits exactly three on the **root `agent.activation` span** — `retry-attempt` (every failed attempt; `attempt`/`max_attempts`/`provider`/`model`/`retryable`/`reason`/`error` scrub+16k), `fallback-fired` (a fallback succeeds; `from`/`to` enums), `exception` (terminal exhaustion; `exception.type=provider_exhausted` / `exception.message` scrub+16k / `exception.escaped`). Stable event **names** + typed attrs are the contract form; no single activation shows all three. **Binding caveat:** verified live 2026-06-04 — all three land in self-hosted Laminar's ClickHouse `spans.events` (survive ingest, unlike resource attrs) **but the v0.1.46 UI never renders them** (no `spans.events` SELECT / no span-events component / no MV into the `events` table) — same ingest-vs-surface family as root-Status (`zc-sdjr`). **UI-visibility workaround:** the emitter dual-emits a compact `reliability.*` summary as **root-span attributes** (the panel the UI *does* render) — the established `deployment.environment` / `user.id` / `lmnr.span.*` pattern — summary only, never unbounded indexed keys; the span events stay the canonical vendor-neutral record. §7.1 span-attribute allowlist gains the `reliability.*` row; §10 #20 added. **Dev-data-only** under the same SPEC 3 prod gate; `error` / `exception.message` / `*.last_error` carry `scrub_credentials`+16k (and inherit the `Bearer`-value scrubber blind spot — `zc-72i1`). **No lifetime / labelling / carrier / resource-set / isolation change** — a new governed signal class (span events) + its UI dual-emit. |
| 1.6.10 | 2026-06-04 | **`deployment.environment` span-attr scope widened from "root span only" to "EVERY span" (zc-bsgi — drift reconciled from live verification; zeroclaw branch `feat/zc-bsgi-every-span-env` commit `245e7277c`, merge recommended).** v1.6.1 legalized env as a root-span attr (dual-emit) because Laminar discards OTLP resource attrs; this **completes** it. Laminar's span filter reads each span's **own** attributes and does **not** inherit env from the parent/root, so a root-only stamp left every child (`llm.call` / `tool.call` / `praxis.<cmd>` / Convex-POST wrapper) **unfilterable by environment** even under an env-stamped root. zeroclaw now re-stamps `deployment.environment` in `OtelSpan` on **every** span. §7.1: allowlist row rescoped `root span` → `every span`; the dual-emit note rewritten (every-span stamp + **root-only-insufficient** rule + queryable-not-no-op proof); §5 + the resource-attribute allowlist mirror-target updated root→every span. §10 #19 added. **Contingency resolved (this is NOT a renderer no-op):** verified live 2026-06-04 — a tool-forcing activation through the dev `clawcraft-claw` pod produced root + `llm.call` ×2 + `tool.call` ×1 **all** carrying `deployment.environment=dev` in ClickHouse, env-filterable as a set; **108 pre-branch child spans** (`76 llm.call + 32 tool.call`) under env-stamped roots were themselves env-blank and matched **0** of an env-slice, proving Laminar does not inherit env from the root, so the per-span stamp closes a real gap. Env is genuinely **queryable** once per-span-stamped (ClickHouse confirmed — distinct from #15 native OTel Status, which is emitted-but-unqueryable on self-host); the v0.1.46 UI attribute-filter affordance is a generic Laminar-UI question, unverified. **No contract / lifetime / labelling / carrier / resource-set / isolation change** — one already-legalized field's span scope widened root→every-span to match the only sink semantics Laminar supports. |
| 1.6.9 | 2026-06-03 | **Per-iteration `llm.call` input/output coverage incl. tool-call-only intermediate calls (the "Gate B" fix — drift reconciled from live verification; bead `zc-k42f`, zeroclaw commit `f35ca9ed4`).** v1.6.8 reconciled the `lmnr.span.*` mirror per **emit site**; this closes the gap **within** a multi-round tool loop. A **tool-call-only intermediate `llm.call`** (a mid-loop turn emitting only a tool call, no assistant prose) holds no completion text, so its `lmnr.span.output` / `gen_ai.completion` rendered **blank** pre-fix — the per-iteration analogue of the W2 blank, occurring mid-loop rather than per-site. zeroclaw now serializes a content-free tool-call turn as a **`name(args)` summary** into `lmnr.span.output` / `gen_ai.completion` (with the growing tool-result delta in `lmnr.span.input` / `gen_ai.prompt`), so **every** iteration's message-view is populated. §7.1 `llm.call` note gains a binding **per-iteration-coverage** sub-note; §10 #18 added (the mid-loop analogue of #17). **Coverage:** verified live 2026-06-03 — a 7-step dependent-chain prompt drove a **13-iteration** loop on the **WS** path (13 `llm.call` + 12 `tool.call`); all **13/13** carried non-blank input + output, the **7** tool-call-only intermediates each showing the `name(args)` summary in `output` (pre-fix blank). Also records that the gateway **simple webhook** (`run_gateway_chat`, :42617 `/webhook`) is **single-shot** — it returns the first completion without iterating, yielding one `llm.call` by construction, so mid-loop coverage is exercised via the WS/channel/CLI loop path, never the webhook. **No new field, no allowlist change** — a content-population guarantee on an already-legalized field (`lmnr.span.output` / `gen_ai.completion` on `llm.call`) reconciled to cover the content-free-completion case. |
| 1.6.8 | 2026-06-03 | **`lmnr.span.input` / `lmnr.span.output` scope widened from "root span only" to "root span + `llm.call`" (W2 `llm.call` message-view fix — drift reconciled from live verification; bead `zc-11qb`, zeroclaw commit `c15c1f1dd`).** v1.6.4 legalized these two keys on the **root** span (→ Laminar's Root replay columns); zeroclaw then also mirrored them onto the **`llm.call`** span so each call's prompt/completion renders in Laminar's per-span message-view + full-text search — but the §7.1 allowlist still scoped them "root span only", so the `llm.call` emission was an un-enumerated-scope §7.1 violation. Root cause is the same Laminar-reads-its-own-keys mechanism as the Root note, one level below the root: Laminar fills **every** span's `input`/`output` (and renders/indexes its message-view) **only** from that span's `lmnr.span.*`, never from the OTel `gen_ai.prompt` / `gen_ai.completion` the `llm.call` already carried — so the `llm.call` view was **blank** ("W2 blank") pre-fix. §7.1 now: (a) widens both allowlist rows to `root span + llm.call` with per-scope treatment; (b) adds `lmnr.span.input` / `lmnr.span.output` to the `llm.call` span bullet; (c) adds a binding **Laminar-`llm.call`-message-view** note (mirror scrubbed once per site so OTel/Laminar keys can't drift; `gen_ai.*` retained but not the view source). §10 #17 added (per-span analogue of #14). **Coverage:** all four `llm.call` sites (`run_tool_call_loop`, `Agent::turn`, `Agent::turn_streamed`, `run_gateway_chat`) carry the mirror; verified live 2026-06-03 across three engines (WS / simple-webhook / native-channel) — each `llm.call` carried populated `input`+`output`, probe full-text searchable, pre-fix rows empty (`Agent::turn` non-streaming shares the identical mirror, not separately driven). **Also corrected** the v1.6.4 Root-coverage note: the **native-channel root** (`process_channel_message`) is added to the **deferred** set alongside CLI (bead `zc-tro4`) — confirmed blank live (`has_input=0`/`has_output=0`) while its child `llm.call` was populated; `process_channel_message` has no root `lmnr.span.*` setter today. **No contract change** — a field's scope reconciled to the additional sink Laminar surfaces it on. |
| 1.6.7 | 2026-06-03 | **`tool.input` verified live in dev + `scrub_credentials` key-driven blind spot recorded (bead `zc-72i1`).** The v1.6.5 forward legalization is now **confirmed emitting**: a dev `tool.call` span carried `tool.input` alongside `tool.output` / `tool.error`, and a credential-bearing call (`api_key` / `token` args) was redacted to `sk-t*[REDACTED]` — the fake secret did not survive (both engine sites + composio full-args-with-separate-`composio.action`/`toolkit` confirmed). §7.1 `tool.input` note gains the live-verification record **and** a binding **`scrub_credentials`-blind-spot** caveat: the scrubber is key-driven (`SENSITIVE_KV_REGEX`, `<sensitive-key><:|=><value>` only) and **leaks `Authorization: Bearer <token>` value-form secrets** (verified leak — `Authorization` not a sensitive key, `bearer` followed by a space, not `:`/`=`); the same blind spot covers `tool.output` / `gen_ai.*` / `lmnr.span.*`, so the §7.1 "scrub is the dev gate" guarantee is only as strong as the scrubber — zeroclaw MUST extend it (`Bearer\s+<token>` value rule + `authorization` as a sensitive key) before any content field is cleared for prod. §10 #16 added. **§5.1 drift fixed:** `ObservabilityConfig` is **eight** fields (was stated as six — `otel_headers` + `otel_deployment_environment` were missing). **No contract change** — live verification + a recorded scrubber limitation + a stale field-count correction. |
| 1.6.6 | 2026-06-03 | **Native OTel root-span Status documented (zeroclaw bead `zc-sdjr`, commit `948328a82`) — root `agent.activation` now carries trace-level success/failure.** Before this, only child spans (`tool.call` / `llm.call`) carried OTel Status; the root was always **Unset**, so failed traces could only be inferred by string-matching `tool.error` (not queryable/alertable). zeroclaw now sets the **native OTel Span Status** on the root at every trigger site — `Ok` on a clean turn, `Error` (empty description, no raw error text — **ungated**) on a failure — from the outcome each site already holds (`process_message`, CLI/cron `run()`, webhook, WS, `process_channel_message`; the streamed `llm.call` error path is also closed). §7.1: the Root activation span bullet now lists the native Status, and a binding note records it as a **native OTel field, NOT a §7.1 attribute** (closed allowlist unchanged — no attribute added; the emitter's "no 7.1 allowlist change" is correct). §10 #15 added. **Binding caveat — emitted but not queryable on self-host:** unlike the 1.6.1/1.6.3/1.6.4 dual-emit reconciliations there is **no `lmnr.*` workaround** (none owed — Status is the right native field), but self-hosted **Laminar v0.1.46 does not ingest OTLP Status** — verified 2026-06-03 by raw OTLP **wire capture** (`Span.status.code` = `STATUS_CODE_OK` on a clean webhook turn / `STATUS_CODE_ERROR` on a forced-fail turn, on **both** root and child `llm.call`) against an **empty** ClickHouse `status` column on every `agent.activation` row (**0 of 572** spans ever non-empty, incl. historically-statused child spans). Trace-level Ok/Error lands on the wire correctly; queryability awaits a backend that reads OTLP Status (managed Laminar Cloud may — unverified). A Laminar-self-host ingestion gap, not a zeroclaw defect. **No contract / lifetime / labelling / carrier / resource-set / isolation change** — a native trace-level field documented + a backend caveat recorded. |
| 1.6.5 | 2026-06-03 | **`tool.input` legalized on the `tool.call` span — ahead-of-emitter cross-repo gate clear for zeroclaw bead `zc-72i1`.** Unlike the 1.6.1/1.6.3/1.6.4 reconciliations (which followed live emission), this is a **forward** allowlist addition: §7.1 is the governance gate zeroclaw waits on, so the doctrine legalizes `tool.input` first and zeroclaw then lands the 2-line `set_attr("tool.input", …)` in both engines behind this commit. §7.1 now: (a) adds `tool.input` to the span-attribute allowlist on `tool.call` — the sibling **input** counterpart to `tool.output` / `tool.error` — classified as a **content** field (free-text, same tier as `tool.output` / `gen_ai.prompt` / `gen_ai.completion`), treatment `scrub_credentials(...)` + 16k `truncate_with_ellipsis`; (b) gains a binding **secrets-bearing strictest-tier** note — tool arguments carry secrets/PII (API keys, tokens, recipients, raw shell) at higher density than any other content field, so `scrub_credentials` is **mandatory even in dev** (the scrub is the dev gate, not just the prod gate) and the SPEC 3 redaction tiers are **load-bearing** for prod (`tool.input` is the **last** content field cleared for prod, never a blanket flip; any future redaction-sensitive sub-tier MUST rank it ahead of `tool.output`); (c) extends the dev-data-only content gate to list `tool.input` as the highest-risk member. **No clawcraft-side test change** — the closed-allowlist conformance gate is enforced in the zeroclaw repo (clawcraft has no field-enumeration test); `doctrine-manifest.test.ts` is unaffected (no new file). **No lifetime / labelling / carrier / resource-set / isolation change** — one additional legalized content field on an existing span. |
| 1.6.4 | 2026-06-03 | **Laminar Root input/output legalized as `lmnr.span.input` / `lmnr.span.output` on the root activation span (W2 / `zc-0e0i` — drift reconciled from live verification).** Original W2 set `gen_ai.prompt` (first `llm.call`) + `gen_ai.completion` (each `llm.call`) intending to fill Laminar's replay **Root input/output** columns, but live ClickHouse verification proved Laminar **never** lifts the flat OTel `gen_ai.*` strings into a span's `input`/`output` (**0** spans carried input/output; both Root columns stayed empty across CLI/WS/webhook). Laminar derives the Root columns (`traces_replacing.root_span_input` / `root_span_output`) from the trace's **root/top span**, populated **only** from the Laminar-native `lmnr.span.input` / `lmnr.span.output` attrs. §7.1 now: (a) adds `lmnr.span.input` + `lmnr.span.output` to the span-attribute allowlist (**root span only**, `scrub_credentials` + 16k); (b) gains a binding note recording the Laminar-reads-root mechanism (same dual-emit rationale as `deployment.environment` / `user.id`); (c) corrects `gen_ai.prompt` scope from "`llm.call` / root" to "`llm.call`" (OTel-semantic, retained, but **not** the Root source); (d) extends the dev-data-only content gate + lists the two fields on the root activation span. §10 #14 added. **Coverage (partial):** verified live 2026-06-03 — WS (`process_chat_message`) + channel-webhook (`handle_webhook`) roots carry scrubbed, 16k-truncated input/output (Root columns populated; secret redacted to `ghp_*[REDACTED]`); channel-trigger (`process_message`) wired; **CLI/cron `agent -m` root (`Trigger::Cli`, session-scoped multi-turn) deferred — bead `zc-0e0i` stays open for that path.** **No contract change** — a field's placement reconciled to the only sink that surfaces it. |
| 1.6.3 | 2026-06-03 | **`user.id` legalized as a dual-emitted (span + span) attribute — drift reconciled from live verification.** Original W5 set only the OTel-semconv `user.id` span attribute, which lands in Laminar's raw `attributes` blob but does **NOT** populate Laminar's **indexed `user_id` column** (verified — the typed column stays empty for every span when only `user.id` is sent), so per-user filtering/grouping in the Laminar UI — the field's whole purpose (multi-tenant attribution) — did not work. The zeroclaw W5-A fix (commit `658fcf95e`) **also** stamps Laminar's association-property key `lmnr.association.properties.user_id` with the same validated pod-owner id (the copy Laminar projects into the `user_id` column), at all five root sites, gated by the same `pod_user_id()` `Some(...)` guard. §7.1 now enumerates `lmnr.association.properties.user_id` in the span-attribute allowlist (closed-allowlist gate — an un-enumerated key would be a §7.1 violation) and adds the `user.id` dual-emit note (parallel to `deployment.environment`'s, but **span + span**, never a resource attr). Verified live 2026-06-03 across all four user-facing roots (web WS first-frame + main-loop, simple webhook, native channel, process_message): both keys present + indexed column populated; CLI/cron + invalid-id (`claw-` prefix) leave both keys and the typed column empty. **No contract change** — a field's companion key reconciled to match the only sink Laminar indexes; both keys are the same non-credential attribution id, span-only, dev-data-only (SPEC 3 redaction gate unchanged). |
| 1.6.2 | 2026-06-03 | **`buildConfigToml` now renders `otel_deployment_environment` (closes the W6 dev/prod wiring gap).** The emitter read the key since `3fd09e639`, but the clawcraft renderer never wrote it, so every config booted `<unset>`. §5 updated: the rendered `[observability]` block gains `otel_deployment_environment`, emitted **only when an endpoint is set** (meaningless without export); value sourced like the endpoint — dev `LAMINAR_DEPLOYMENT_ENV` (`infra/.env.dev`, default `"dev"`) via `render-claw-config.ts`; prod `LAMINAR_DEPLOYMENT_ENVIRONMENT` (default `"prod"` once active) via `readOtlpCarrier` → param into the pure `buildConfigToml` (backend §9 boundary rule). Non-secret org enum — not a credential, never a K8s label/env (§3). **No contract change** — wires an already-allowlisted field (§7.1) end-to-end. |
| 1.6.1 | 2026-06-03 | **`deployment.environment` legalized as a root-span attribute (dual-emit) — drift reconciled from live verification.** The v1.6.0 allowlist sanctioned `deployment.environment` as a **resource** attribute only, but self-hosted Laminar **discards OTLP resource attributes** (confirmed end-to-end: `service.name` and `deployment.environment` land in **0** queryable spans when sent resource-only), so the resource copy is invisible/unfilterable. The zeroclaw fix (post-W6) additionally stamps it on the **root `agent.activation` span**; §7.1 now enumerates `deployment.environment` in BOTH the resource-attribute and span-attribute allowlists (the one field permitted in both — non-PII org enum, so §3 zero-PII is not relaxed) and records the Laminar-resource-drop rationale as binding. Verified live: a CLI activation with `otel_deployment_environment="dev"` produced an `agent.activation` span carrying `deployment.environment=dev` in ClickHouse. **No contract change** — a field's placement reconciled to match the only sink that retains it. |
| 1.6.0 | 2026-06-03 | **§7.1 reframed as the field-level governance allowlist + new zeroclaw content/attribution fields legalized (cross-repo gate for zeroclaw W2/W3/W5 + W6).** §7.1 now states the closed-allowlist gate explicitly (emitter MUST NOT emit an un-enumerated span/attribute/resource-attr) and gains three sub-allowlists: (a) the existing **span set**, with `tool.call` named as a first-class span and the praxis black-box carve-out restated (`tool.output`/`tool.error` MUST NOT attach to a `praxis.<cmd>` span — C2); (b) a **span-attribute allowlist** adding `gen_ai.prompt` (llm.call/root, `scrub_credentials`+16k), `gen_ai.completion` (llm.call, `scrub_credentials`+16k), `tool.output` (tool.call, `scrub_credentials`+truncate), `tool.error` (tool.call failure, `scrub_credentials`), `user.id` (root span ONLY, never a resource attr); (c) a **resource-attribute allowlist** affirming the set is exactly `{service.name=zeroclaw, deployment.environment}` — never credentials/PII. **Drift reconciled** (already emitted by zeroclaw `3fd09e639`, doctrine lagging): `deployment.environment` resource attr added to the allowlist; `gen_ai.reasoning` confirmed **already-in-set** on the streaming `llm.call` path (W1a stopped dropping it — no new field). **Dev-only hold:** the new content/attribution fields are **dev-data-only**; prod enablement is gated on the **SPEC 3 redaction tiers** — a **field-specific content gate**, explicitly NOT a reinstatement of the §8-retired blanket redaction-before-prod gate (§8 keeps incidental PII TOS-covered for the existing span set; this caveat blocks only the new high-magnitude content fields). 16k truncation is the interim bound; by-reference large-payload store deferred — no `blob_ref` field pre-reserved (methodology §3). **No lifetime / labelling / carrier / isolation change** — same trace contract, additional legalized fields. |
| 1.5.0 | 2026-06-02 | **Prod activation on managed Laminar (SPEC PROD).** Prod hosting moved from self-hosted-on-GKE to a **managed Laminar cloud service** (brief amended 2026-06-02) — no self-hosted prod infra. §1 backend note now splits dev (local self-host) vs prod (managed cloud). §5: the prod renderer-table row flips from "deferred — `backend = "log"`" to "**inert until ops activates**, then `backend = "otel"` to the managed endpoint"; the "prod still deferred" paragraph is rewritten as **managed activation** — `gke.ts` threads BOTH endpoint and the managed-project key from prod Convex env via `readOtlpCarrier` (env read at the Convex client boundary, params into the pure `buildConfigToml` — backend §9), inert by default, activated by ops setting two prod Convex env vars; endpoint is the **managed cloud OTLP base URL over `https`/TLS** (vs dev's insecure h2c `:8000`); the key is sourced from prod Convex env (`secrets-manifest.yaml audit_pending:`), never committed, never copied from dev; `readOtlpCarrier` **fails loud** when the endpoint is set but headers are empty. §8 **redaction reconciled** from "REQUIRED before prod PII — hard dev→prod gate" to **"PII in prod accepted by design, TOS-covered for alpha; redaction is NOT a prod gate, built only on a customer trigger, in-SDK/pre-export in `zeroclaw`"** — the no-pre-reserved-field rule preserved. §9 **isolation reconciled to managed**: prod = managed cloud, dev = local self-host; the two-instance wall is satisfied by construction; the namespace/separate-project failure-domain question is moot; self-hosting prod remains a revisitable trigger. §10 rows reworked: #1 (concrete prod value committed), #9 (key committed **or copied from dev**), #10 (ops sets env at activation), #11 (managed endpoint missing TLS / carrying `/v1/traces` suffix). **No span-set / lifetime / single-exporting-observer / labelling / in-band-attribution change** — managed prod is a different sink + credential, not a different contract. |
| 1.4.0 | 2026-06-02 | **Single-exporting-observer invariant (added after a live WS span-drop incident).** §4.1 gains a binding note: all three ingress surfaces MUST emit their root activation span through the **same** exporting observer / `TracerProvider`; a runtime that instruments a surface but routes its span to a per-component non-exporting observer silently drops that trigger's whole trace while others succeed — a §7.1 violation that passes every config check. §10 #13 added (only a per-trigger span count catches it). **No contract change** — the existing §4.1/§7.1 "root span per ingress surface" guarantee was reinforced, not altered; clawcraft's config-injection surface (§5) is unaffected. Incident + fix recorded in `zeroclaw-emission-handoff.md`: the WS first-frame path reached `start_activation` on a non-OTel observer while `channel`/`cli` held the real `OtelObserver`, so only `web_chat` traces were dropped (config, endpoint `:8000`, and `otel_headers` were all correct throughout). Resolved 2026-06-02 21:23Z by sharing one observer across components; a connected `web_chat` trace (root `agent.activation` + nested `llm.call`, same `trace_id`) then landed alongside the `channel` control. |
| 1.3.0 | 2026-06-02 | **OTLP carrier moved env → `config.toml` (resolved by link-by-link verification against the zeroclaw emitter source).** The emitter reads **`config.toml [observability]`** (`backend`, `otel_endpoint`, `otel_service_name`, `otel_headers`, `runtime_trace_*`) and **ignores the `OTEL_EXPORTER_OTLP_*` env entirely** — so the SPEC-1 env injection (the `gke.ts` trio + dev compose `OTEL_*` block) was vestigial and is **retired**. §5 rewritten: the single carrier is `buildConfigToml` rendering `[observability]` (dev `render-claw-config.ts`, prod `gke.ts`); `otel_endpoint`/`otel_service_name`/`otel_headers` are literals, gated on `LAMINAR_OTLP_ENDPOINT` (set ⇒ `backend = "otel"`, else `"log"`). **Transport corrected to OTLP/HTTP-protobuf** → Laminar app-server **`:8000/v1/traces`** (not gRPC `:8001`). **Auth is mandatory and is a rendered `config.toml` credential** (`otel_headers = "Authorization=Bearer <key>"`, peer to `api_key`/`pre_shared_token` — claw §6.1/§6.2); Laminar drops unauthenticated spans. §3 auth-header note repointed to `otel_headers`. §10 failure modes reworked (#2 single renderer, #3 OTLP-in-pod-env now forbidden, #8 endpoint-gate, #11 gRPC `:8001` vs HTTP `:8000`, #12 missing `otel_headers`). New §5.1 records the cross-repo contract (`ObservabilityConfig` 6 fields; `.with_http().with_endpoint().with_headers()`; no env read). Supersedes the v1.1.0/v1.2.0 env-carrier model. |
| 1.2.0 | 2026-06-01 | **Backend selector reconciliation (found during the live first-trace bring-up).** The `zeroclaw` runtime gates its OTLP exporter on `config.toml [observability] backend` — it will **not** export unless `backend = "otlp"`, regardless of the `OTEL_*` env. New §5.1 documents the selector: rendered by `buildConfigToml` from an `otlpEnabled` param, gated on the same `LAMINAR_OTLP_ENDPOINT` signal as the env injection (prod `gke.ts` both call sites: `Boolean(process.env.LAMINAR_OTLP_ENDPOINT)` ⇒ `log` until prod ships; dev `render-claw-config.ts`: `env.LAMINAR_OTLP_ENDPOINT !== ""` ⇒ `otlp` active-by-default). Corrects the prior over-broad claims: §5 now says the **endpoint+credentials** are env-only (the selector is the one config.toml exception — a static enum, not a secret), and `render-claw-config.ts`/`updateConfigMap` **are** carriers for the selector (not the endpoint). §10 #3 narrowed to "endpoint/credentials in config.toml"; new §10 #12 (env wired but `backend` left `"log"`). Endpoint stays the load-bearing inert-when-unset gate. |
| 1.1.0 | 2026-06-01 | **SPEC 2 — dev hosting (self-hosted Laminar).** §5: the dev carrier becomes the **active-by-default quintet** — the dev `OTEL_EXPORTER_OTLP_ENDPOINT` default is now **concrete** (`http://host.docker.internal:8001`, a real local Laminar app-server gRPC ingest — no longer aspirational), plus dev-carrier-only `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` and `OTEL_EXPORTER_OTLP_HEADERS` (the project-API-key Bearer credential from uncommitted `infra/.env.dev`, inert-when-empty). The **prod** carrier (`gke.ts`) is **unchanged** — still the inert trio (prod hosting deferred); the dev/prod asymmetry is deliberate. §3: the OTLP auth header named as a transport credential distinct from resource attributes (does not relax the zero-credential rule); `deployment.environment` parenthetical changed to "(separate instance — prod spec, mechanism TBD)". §9: dev provisioned (one long-lived `laminar`-profile compose stack, decoupled from `claw` — infra §15.25); prod restated as **one single self-hosted instance**, failure-domain mechanism deferred to the prod spec. **User-authorized brief §4.3 deviation:** the "separate GCP project (preferred)" prod isolation path is dropped in favor of one single instance; the two-instance wall + no-shared-failure-domain guarantee preserved and re-confirmed at the prod spec. §10: row 1 narrowed to the prod carrier (dev default now concrete); rows 9 (project key committed), 10 (dev-only protocol/headers/endpoint leaking into prod `gke.ts`), 11 (HTTP-default vs gRPC `:8001` protocol mismatch) added. Cross-refs infra §15.25, backend §16. |
| 1.0.0 | 2026-06-01 | Initial doctrine. Encodes the SPEC 1 §3.2 contract: five label split rules, trace lifetime (start per channel / end at quiescence not ACK / in-pod carrier), 2-place OTLP env-injection discipline with inert-when-unset gate, channel+`thread_id` in-band attribution (channel-blind pod, no new protocol field), closed span set + negative set as subset of claw §13, deferred redaction (hard dev→prod gate, no pre-reserved field), two-instance prod wall. Emitter is the out-of-repo `zeroclaw` Rust runtime. |
