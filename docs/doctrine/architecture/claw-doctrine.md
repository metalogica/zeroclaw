# Claw Doctrine (Control Plane ↔ Compute Plane)

**Version**: 2.35.0
**Status**: Binding
**Author**: Architect Agent
**Date**: 2026-04-10
**App**: Clawcraft (Managed ZeroClaw Hosting Platform)

---

## 1. Authority

This document is **Binding**. Violations are architectural bugs.

Keywords MUST, MUST NOT, SHOULD, MAY follow RFC 2119.

This doctrine governs the boundary between Convex (control plane) and ZeroClaw (compute plane). It defines what each side owns, how they communicate, and where credentials, state, and logic live.

**Reference Implementation**: `apps/clawcraft/convex/relay.ts`, `apps/clawcraft/convex/telegramRelay.ts`, `apps/clawcraft/convex/clients/gke.ts`, `apps/clawcraft/convex/podActions.ts`, `apps/clawcraft/convex/integrationActions.ts`, `apps/clawcraft/domain/claw-config.ts`, `apps/clawcraft/domain/claw-workspace.ts`

---

## 2. Architecture Overview

```
┌──────────────────────────────────────────────────────────┐
│                   Convex (Control Plane)                  │
│                                                          │
│  ┌─────────────┐ ┌─────────────┐ ┌────────┐            │
│  │  Telegram    │ │  WhatsApp   │ │  Web   │            │
│  │  (Native)    │ │  Adapter    │ │  Chat  │            │
│  │             │ │             │ │        │            │
│  │ Pod handles │ │ register    │ │ WS via │            │
│  │ long poll   │ │ parse msg   │ │ Nginx  │            │
│  │ directly    │ │ send reply  │ │gateway │            │
│  └──────┬──────┘ └──────┬──────┘ └───┬────┘            │
│         │               │            │                  │
│         └───────┬───────┘            │                  │
│                 ▼                    │                  │
│         ┌──────────────────────────┐ │                  │
│         │    Unified Relay         │ │                  │
│         │  (channel adapters)     │ │                  │
│         │                          │             │       │
│         │  1. Extract text         │             │       │
│         │  2. POST /api/chat       │             │       │
│         │  3. Insert to messages   │             │       │
│         │  4. Route reply back     │             │       │
│         └────────────┬─────────────┘             │       │
│                      │                           │       │
└──────────────────────┼───────────────────────────┼───────┘
                       │ POST /api/chat            │ WS auth
                       │ { message }               │ (session JWT)
                       ▼                           ▼
              ┌─────────────────┐     ┌────────────────────┐
              │  ZeroClaw Pod   │     │  Nginx WS Gateway  │
              │                 │     │  (clawcraft-system) │
              │  /api/chat      │     │                    │
              │  /ws/chat       │◄────│  /ws/chat          │
              │  /webhook       │◄────│   auth_request →   │
              │  brain.db (PVC) │     │   Convex ws-auth   │
              │  Agent loop     │     │  /relay/{userId}   │
              │  Memory tools   │     │   POST → pod:42618 │
              │                 │     │   (webhook channel)│
              │  Zero channel   │     │   X-Relay-Token    │
              │  awareness      │     │  /ws/relay/{userId}│
              └─────────────────┘     │   WS → /ws/chat   │
                                      └────────────────────┘
                                               ▲
                                               │ wss://gw.clawcraft.ca
                                               │
                                       ┌──────┴──────┐
                                       │   Browser   │
                                       └─────────────┘
```

**Core principle**: ZeroClaw is compute, Convex is control plane. The pod is a pure text-in/text-out agent compute node for most channels. Telegram is the sole exception — the pod runs its native Telegram channel via long polling (§3.4). Web chat uses a direct WebSocket connection via the Nginx gateway (§2.2), bypassing the Convex relay for streaming. Email relay routes through the Nginx gateway's `/relay/{userId}` endpoint, which proxies HTTP POSTs to the pod's webhook channel on port 42618 with `X-Relay-Token` header (HMAC disabled; auth is nginx auth_request + NetworkPolicy). All other channel-specific concerns — credentials, webhooks, message formatting, reply routing — live in Convex.

### 2.2 Web Chat WebSocket Flow

Web chat uses a direct WebSocket connection from the browser to the pod's `/ws/chat` endpoint, routed through the Nginx WS gateway deployed in `clawcraft-system` namespace.

```
Browser → wss://gw.clawcraft.ca/ws/chat → Nginx (auth_request) → Convex GET /api/ws-auth → 200 + X-Pod-Upstream → Nginx proxies to pod /ws/chat
```

**Flow:**
1. Browser opens WebSocket to `wss://gw.clawcraft.ca/ws/chat?token={convexSessionJWT}`
2. Nginx `auth_request` subrequest sends the token to Convex `GET /api/ws-auth`
3. Convex validates the session JWT, looks up the user's pod endpoint, returns `X-Pod-Upstream` header
4. Nginx proxies the WebSocket to the pod's `/ws/chat` endpoint (ClusterIP, no auth — `require_pairing=false`)
5. Pod streams responses token-by-token over the WebSocket
6. Browser writes back persisted messages to Convex via `persistUserMessage` / `persistAssistantMessage` mutations

**Why this path exists:** The Convex relay path (`POST /api/chat`) is request-response — it cannot stream tokens. The WS gateway gives web chat real-time streaming without modifying the pod or adding channel awareness. Scheduled tasks continue to use the Convex relay.

### 2.1 Ownership Table

| Concern | Owner | Never In |
|---|---|---|
| Channel credentials (bot tokens, API keys) | Convex (DB, encrypted). **Exception**: Telegram bot token is injected into ConfigMap `[channels_config.telegram]` for native mode (§3.4) | — |
| Webhook registration/lifecycle | Convex (adapter actions). Telegram: no webhooks (native long polling) | Pod |
| Inbound message parsing | Convex (adapter) | Pod |
| Outbound reply formatting + send | Convex (adapter) | Pod |
| Message relay to agent | Convex (unified relay). Email relay sends `{ sender, content }` (channel webhook format) via gateway to pod:42618, not `{ message }` (gateway format). | — |
| LLM inference | Pod (via OpenRouter) | Convex |
| Agent memory (brain.db) | Pod (SQLite on PVC at `/zeroclaw-data/workspace/memory/brain.db`) — sole memory backend, runtime-managed. Full POSIX locking, no corruption risk. Convex never owned agent memory. | Convex |
| Agent scheduling / reminders | Pod (agent runtime's own scheduler; `/api/cron`, `cron_add`/`cron_remove` tools) — runtime-managed. | Convex |
| User auth, billing, pod lifecycle | Convex | Pod |
| Workspace files — system (identity, personality) | Convex → ConfigMap → `/zeroclaw-data/workspace/system` (subPath overlays, readOnly) | — |
| Workspace files — agent-owned (memory, user notes) | Agent creates on PVC at `/zeroclaw-data/workspace` (read-write) | — |
| User files (uploads) | GCS bucket (`clawcraft-489901-user-data`) via GCS Fuse mount in pod (`readOnly: true`). Convex index table for metadata. | Convex `_storage` (**DEPRECATED**, migration-targeted; see infra-doctrine §2.3 GCS canonical statement and `docs/tasks/ongoing/storage-migrate-to-gcs/`) |

---

## 3. Channel Adapter Model

Each channel adapter handles exactly three responsibilities:

```typescript
interface ChannelAdapter {
  // Webhook/connection setup (called on connect)
  register(credentials: ChannelCredentials): Promise<void>;

  // Inbound: platform webhook JSON → plain text + routing metadata
  parseInbound(webhookPayload: unknown): {
    text: string;
    senderId: string;
    replyTarget: string;  // chat_id, channel_id, etc.
  };

  // Outbound: agent response text → platform API call
  sendReply(replyTarget: string, text: string): Promise<void>;
}
```

### 3.1 Adapter Rules

- Adding a new Convex-relayed channel = one new file in Convex. Zero ZeroClaw changes, zero config changes, zero pod restarts.
- Each adapter MUST live in a dedicated Convex file (e.g., `telegramRelay.ts`).
- Each adapter MUST have a corresponding client in `apps/clawcraft/convex/clients/` for external API calls.
- Adapters MUST NOT import from each other. They are independent.
- Adapters MUST converge on the unified relay (§4) for pod communication.
- Adapters MUST store channel credentials in the Convex DB (encrypted). **Exception**: Telegram bot token is also injected into ConfigMap for native mode (§3.4).

### 3.2 Current Adapters

| Channel | Adapter File | Client File | Status |
|---|---|---|---|
| Web | WS gateway → pod `/ws/chat` (streaming); `relay.ts` (dormant for web) | — | Live (WS streaming, §2.2) |
| Telegram (relay) | `telegramRelay.ts` | `clients/telegram.ts` | Dormant (replaced by native) |
| Telegram (native) | — (pod-native) | — (pod-native) | Live (long polling, §3.4) |
| Email | `emailRelay.ts` | Cloudflare Worker (`apps/email-ingest/`) | Live (CF Email Routing → Worker → Convex → gateway relay POST to pod:42618 webhook channel, `{ sender, content }` format) |
| Linq (SMS/iMessage/RCS) | `linq.ts` + `linqRelay.ts`/`linqActions.ts` (`"use node"`); inbound `httpAction` `/linq-webhook` | `clients/linq.ts` (Partner V3, Web Crypto HMAC) | Live (Linq → Convex `/linq-webhook` HMAC → durable `messages` row → gateway `/relay/{userId}` → pod:42618, `[LINQ_RECEIVED]` envelope, `{ sender, content }` format). **Outbound is a Convex-side stopgap — see §3.5.** (add-linq) |
| Slack | — | — | Removed (will be re-added in a future release) |
| WhatsApp | — | — | Planned |
| Discord | — | — | Planned |

### 3.3 What This Eliminates From Pod Config

```toml
# BEFORE: pod needs credentials for every channel
[channels.telegram]
bot_token = "..."
allowed_users = ["..."]

[channels.discord]
bot_token = "..."

# AFTER: pod knows nothing about channels (except Telegram — see §3.4)
[gateway]
port = 42617
host = "0.0.0.0"
pre_shared_token = "..."
allow_public_bind = true

# Always present (webhook channel + optional Telegram)
[channels_config]
cli = true
message_timeout_secs = 300

[channels_config.webhook]
enabled = true
port = 42618
send_url = "<CONVEX_SITE_URL>/container-webhook"
# HMAC disabled (secret omitted); auth is handled by nginx auth_request + NetworkPolicy

# Conditional: present when Telegram integration is connected
[channels_config.telegram]
bot_token = "<decrypted-bot-token>"
allowed_users = ["*"]
```

### 3.4 Doctrine Exception: Telegram Native Channel

Telegram is the sole exception to the "zero channel awareness" rule. ZeroClaw's built-in Telegram channel runs long polling directly from the pod. This exception exists to leverage the native channel runtime's richer execution context (streaming, cancellation, approval, hooks, per-message timeouts) which the gateway relay path lacks.

**What changes:**
- Pod contacts `api.telegram.org` directly (outbound HTTPS only, no ingress needed)
- Bot token injected via `config.toml` `[channels_config.telegram]` section (decrypted plaintext in ConfigMap — see credential isolation note below)
- `buildConfigToml` renders `[channels_config]` with `cli = true` (required by ZeroClaw's `ChannelsConfig` deserializer) and `[channels_config.telegram]` with the bot token and `allowed_users = ["*"]`
- Pod uses long polling (`getUpdates` loop) — zero infrastructure changes, no webhooks needed
- `validateAndSetup` calls `deleteWebhook` (idempotent) to ensure no stale webhook intercepts `getUpdates` messages

**Credential isolation note:** The Telegram bot token appears in plaintext in the ConfigMap. This is a **deliberate trade-off** — ZeroClaw reads config from `config.toml`, and there is no env var interpolation in TOML. The token is still encrypted at rest in Convex DB; it is decrypted only at ConfigMap generation time by `provision`, `scaleUp`, and `restartPodForIntegration` actions.

**What stays the same:**
- Web chat uses WS gateway for streaming (§2.2), not Convex relay
- Convex remains source of truth for integration state (encrypted token in DB)
- Convex-side `telegramRelay.ts` remains for webhook mode (dormant, superseded by native)

**Message persistence gap:** Native Telegram messages exist only in brain.db (pod-local) and Telegram's client-side history. Convex's `messages` table does not see them. Dashboard chat history and billing metering are not available for native Telegram messages. This is a known gap — a message persistence callback is planned as follow-up work.

### 3.5 Channel Outbound Delivery (Linq — as-built, 2026-05-28)

For a channel with **Convex-mediated inbound** (Linq, and the planned slack/whatsapp/
discord), outbound (agent reply → external recipient) can take one of two paths. The
deciding factor is whether the sovereign-fork modification that lets the pod deliver
natively has shipped:

- **Pod-native (preferred target — NOT yet shipped):** the pod's `LinqChannel::send`
  calls Linq Partner V3 directly and POSTs a mirror to Convex `/container-webhook` for
  assistant-row persistence. The sovereign-fork PR enabling `LinqChannel::send` was
  **never merged** — do not expect to find it invoked in pod logs.
- **Convex-side stopgap (current v1, as-built):** the pod's generic webhook channel
  POSTs the reply to `/container-webhook`; `linq.tryRouteAssistantReply` (Convex)
  heuristically detects a Linq-context reply via the user's most-recent thread and calls
  `linqRelay.sendOutbound` (`"use node"`) → Linq Partner V3 `/chats/{chatId}/messages`,
  **before** the generic `source: "webhook"` branch runs.

```
Pod webhook channel (generic) → /container-webhook (Convex)
  → linq.tryRouteAssistantReply (heuristic: user's most-recent thread)
    → linqRelay.sendOutbound → Linq Partner V3 /chats/{chatId}/messages
```

**Known limitation (race):** the most-recent-thread heuristic misroutes when two
external senders text the same Linq number within ~1s — plausible in any production
tenancy. The pod-native path has no such race; landing the sovereign-fork PR supersedes
the stopgap and resolves it. **When that PR lands, a feature-flag guard MUST prevent
double-sends** (stopgap + native both firing). Shipped in `b565661`.

**Precedent:** future Convex-mediated channels face the same choice — pod-native
preferred, Convex-side stopgap acceptable as a time-bound v1, never silently retrofit
the asymmetry without a doctrine note.

---

## 4. Unified Relay Contract

All channels converge on a single relay path to the pod.

### 4.1 Request Format

```
POST /api/chat
Authorization: Bearer {pre_shared_token}
Content-Type: application/json

{ "message": "<plain text>" }
```

- MUST NOT send `session_id` in the HTTP relay body. ZeroClaw's `build_memory_context` uses strict session filtering; omitting `session_id` enables global memory recall across all channels (FMEA B1/B2).
- Web chat (WS path) prepends a `[conversationId: {threadId}]` tag to every message. This provides conversation context to the agent for organizing generated content (e.g., media output to `/media/{conversationId}/`). This is a text tag in the message body, not a protocol field — it does not affect ZeroClaw's session handling.
- MUST use `AbortSignal.timeout(180_000)` — the agent loop may invoke multi-step tool chains (composio discovery + execution, web search, HTTP request, memory) which take time.
- MUST use pre-shared bearer token for auth (per-user, stored in Convex).

### 4.2 Response Parsing

```typescript
const data = await response.json();
const responseText = data.content ?? data.reply ?? data.response;
```

The response field name varies by ZeroClaw version. Parse in priority order: `content`, `reply`, `response`.

### 4.2.1 Reference Implementation

`relayToPod()` in `apps/clawcraft/convex/relayHelpers.ts` is the canonical implementation of the unified relay fetch. All code that relays messages to the pod via `POST /api/chat` — channel adapters (web, Telegram) AND scheduled task execution (`scheduledTaskActions.ts`) — MUST call `relayToPod()` instead of inline fetch. The helper owns: fetch, timing, error classification, `podState` escalation on connection failures, and structured logging to the `relay_logs` table. `RelayChannel` type: `"email" | "web" | "telegram"`. Each caller retains ownership of response routing (assistant message insertion, channel-specific reply sending).

**Exception**: `emailRelay.ts` does NOT use `relayToPod()`. It has its own `relayViaGateway()` function that routes through the Nginx gateway (`gw.clawcraft.ca/relay/{userId}`) which proxies to the pod's webhook channel on port 42618. The payload uses channel webhook format (`{ sender, content }`) instead of gateway format (`{ message }`). This avoids per-user LoadBalancer IPs and uses the `X-Relay-Token` header for auth (HMAC disabled on the webhook channel; auth is at the network layer via nginx + NetworkPolicy).

### 4.3 Relay Flow (All Channels)

```
Channel inbound
      │
      ▼
  Adapter.parseInbound()
      │
      ├── Insert user message to messages table
      │   (source: "container" | "telegram" | ...)
      │
      ├── Pod running?
      │   ├── YES → POST /api/chat → parse response
      │   │         → Insert assistant message
      │   │         → Adapter.sendReply() (if external channel)
      │   │
      │   └── NO  → Schedule podActions.scaleUp
      │             → Message stays queued (status: "pending")
      │             → deliverQueued delivers after pollHealth succeeds
      │
      └── Return to caller
```

### 4.4 Queued Message Delivery Gap

`deliverQueued` (relay.ts) delivers pending messages after pod wake-up, but does NOT preserve channel-specific reply routing metadata (chatId, botToken). After delivery, the assistant response is inserted to DB but NOT sent back to the external channel.

**Impact**: Telegram users who message during pod wake-up get no reply. Web users are unaffected (Convex subscription delivers the response reactively).

**Tracked**: This is a known gap. Future fix: add reply routing metadata to the `messages` table so `deliverQueued` can route responses back to the originating channel.

---

## 5. Pod API Surface

ZeroClaw exposes a rich HTTP API on port 42617 (gateway) and port 42618 (webhook channel). This section documents the complete surface area, categorized by communication pattern.

### 5.0 Trap: `/webhook` is overloaded across two ports with different contracts

The path name `/webhook` exists on **both** the gateway (port 42617) and the webhook channel (port 42618), with **different request bodies, different agent-loop semantics, and different downstream effects**. Conflating them is a silent self-inflicted-regression class — the doctrine-violation log in this section's 2026-05-18 audit captured a working agent (re-anchoring this codebase) mistakenly diagnosing `emailRelay.ts`'s `{ sender, content }` payload as "wrong" after `curl`ing `:42617/webhook` and getting a 400 expecting `{ message }`. The doctrine-correct shape was the one already in the code — wrong port was tested.

| Port | Path | Expected body | Agent loop | Response | Use case |
|---|---|---|---|---|---|
| **42617** (gateway) | `/webhook` | `{ "message": "<text>" }` | **None** — `run_gateway_chat_simple`: bare LLM call, no tools / memory / personality | 200 with `{ model, response }` LLM reply inline | Legacy gateway chat; manual quick-test (`curl`) only |
| **42618** (webhook channel) | `/webhook` | `{ "sender": "<users._id>", "content": "<text>" }` | **Full** (async — returns 200 immediately; agent loop runs later with tools, memory, personality, session) | 200 with empty body; reply posted back to `send_url` (`/container-webhook`) | Email relay, channel webhook flows, **Convex-issued `[CLAWCRAFT_DIRECTIVE]`-tagged tasks** (Spec 4b — see §5.6.1) |

**MUST** before debugging a `/webhook` failure: confirm which port (and therefore which contract) you're hitting. The `curl` results for the two are not interchangeable. The 42617 echo path is **not** a debugging stand-in for the 42618 agent loop — if a directive on 42617 "works", you're confirming the LLM can spell a tool call, not that the agent ran it. Local dev exposes both ports as of `infra/local/docker-compose.yml` (2026-05-18); see §605.

**Spec 4b polysemy note (corrected v2.31.1).** v2.31.0 of this doctrine claimed Convex `internal.webhookDispatch.{sendValidationWebhook,sendExecutionWebhook}` POSTed `[system]`-prefixed bodies to port 42617 and called that path "Full agent loop" — both claims were factually wrong. Port 42617's `/webhook` is `run_gateway_chat_simple` (tool-less echo), so the directive class never reached an agent loop in production; the Validate / Run buttons were silently dead-end. The code in `webhookDispatch.ts` matched the wrong-doctrine claim (port 42617 + `{message}`) until v2.31.1, when both code and doctrine were corrected to route through the channel webhook (port 42618 + `{sender, content}`) — the same surface `emailRelay.ts` already uses. See `docs/tasks/completed/fix-webhook/` for the original split of these two `/webhook` paths.

### 5.1 Communication Surfaces

| | Single-turn (fire & forget) | Multi-turn (conversational) | Streaming | Introspection (read-only) | Convex-initiated control |
|---|---|---|---|---|---|
| **HTTP** | `/webhook`, `/api/chat` | `/api/chat` (with context field) | `/webhook?stream=true` (SSE) | `/api/*` dashboard endpoints | `/webhook` (channel webhook, port 42618) — Convex-issued `[CLAWCRAFT_DIRECTIVE]`-tagged tasks in the `content` field of `{sender, content}` (Spec 4b — replaces the retired `/api/validate/spec` + `/api/execute/start` dedicated handlers) |
| **WebSocket** | — | `/ws/chat` | `/ws/chat` (native) | `/api/events` (SSE) | — |
| **Channel webhooks** | — | `/whatsapp`, `/qq`, etc. | — | — | — |

**Convex-initiated control (Spec 4b, corrected v2.31.1).** Spec 4b collapsed the two dedicated control endpoints (`/api/validate/spec`, `/api/execute/start`) onto the pod's existing channel webhook — the same `/webhook` surface email-relay already uses (port 42618, `{sender, content}` body, full agent loop). The pre-4b "Convex-initiated control endpoint class" — described in this section as v2.30.0 — is RETIRED. Neither dedicated Rust handler ever shipped on the pod; the `/api/execute/start` handler that Spec 3b v2.35.0 expected was never built. Today the Convex→pod direction has exactly one channel: nginx `<WS_GATEWAY_URL>/relay/{userId}` → pod:42618/webhook, with relay-token auth at nginx (`X-Relay-Token`) plus pod-side bearer auth via `users.preSharedToken` (passed as `X-Pod-Token`, rewritten to `Authorization: Bearer …` on the upstream hop). New Convex-issued directives MUST compose a `[system]`-prefixed natural-language instruction (`[system] Please run: <praxis-cli-command>`) and POST via `apps/clawcraft/convex/webhookDispatch.ts`. Adding a new dedicated control handler on the pod gateway is a forbidden pattern (praxis-doctrine §8). The v2.31.0 attempt to land this on port 42617 was a doctrine-violating mis-route into the tool-less echo path — see §5.0 polysemy note for the audit trail.

### 5.2 Message Endpoints (agent response)

| Endpoint | Transport | Agent Loop | Tools | Memory | Session | Context/History | Streaming | Use Case |
|---|---|---|---|---|---|---|---|---|
| `/api/chat` | REST POST | Full | Yes | Yes | `session_id` param | `context` array (last 10) | No | Web chat + Telegram relay |
| `/ws/chat` | WebSocket | Full | Yes | Yes | Query param or auto | Server-side (128 turns) | Yes (chunks) | Future streaming web chat |
| `/v1/chat/completions` | REST POST | Full | Yes | Yes | Via messages array | OpenAI messages format | Optional (SSE) | OpenAI-compatible clients |
| `/webhook` | REST POST (port 42618) | None | No | No | No | No | Optional (SSE) | Email relay (via gateway on webhook channel port), channel webhook responses |
| `/whatsapp` | REST POST | Full | Yes | Yes | Per-sender | Auto | No | Direct WhatsApp integration |
| `/nextcloud-talk` | REST POST | Full | Yes | Yes | Per-sender | Auto | No | Nextcloud Talk |
| `/qq` | REST POST | Full | Yes | Yes | Per-sender | Auto | No | QQ Bot |
| `/github` | REST POST | Full | Yes | Yes | No | No | No | GitHub issue/PR comments |

### 5.3 Introspection Endpoints (no agent loop)

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/memory` | GET | Search/list brain.db entries |
| `/api/memory` | POST | Store a memory entry directly |
| `/api/memory/{key}` | DELETE | Delete a memory entry |
| `/api/config` | GET | Read current config (secrets masked) |
| `/api/config` | PUT | Update config at runtime |
| `/api/tools` | GET | List available tools |
| `/api/status` | GET | Agent/runtime status |
| `/api/doctor` | GET/POST | Diagnostic health check |
| `/api/health` | GET | Detailed health metrics |
| `/api/cron` | GET/POST | List/add scheduled jobs |
| `/api/cron/{id}` | DELETE | Remove a scheduled job |
| `/api/integrations` | GET | List configured integrations |
| `/api/cost` | GET | Usage/cost tracking |
| `/api/events` | GET (SSE) | Real-time event stream |
| `/api/pairing/devices` | GET | List paired devices |
| `/health` | GET | Liveness probe (no auth) |
| `/metrics` | GET | Prometheus metrics (no auth) |

### 5.4 Clawcraft Usage (What We Use Today)

| Flow | Path | Endpoint |
|---|---|---|
| Web chat (streaming) | Browser → Nginx WS gateway → pod | WebSocket `/ws/chat` (§2.2) |
| Email relay | Convex → Nginx gateway `/relay/{userId}` → pod:42618 | POST `/webhook` (webhook channel port 42618, `{ sender, content }` format) |
| Telegram (native) | Telegram → pod (long polling) | Native `[channels.telegram]` (§3.4) |
| Health polling | Convex → pod | GET `/health` |
| Media file serving | Pod → Convex | GET `/api/media-url?storageId={id}` (pod auth, ownership-checked). **Legacy path** — `_storage` is deprecated per infra-doctrine §2.3; future media access reads directly from the GCS Fuse mount at `/zeroclaw-data/workspace/{conversation-attachments,media}/`. Migration tracked at `docs/tasks/ongoing/storage-migrate-to-gcs/`. |
| Pod introspection | kubectl port-forward | `/api/memory`, `/api/config`, etc. |

### 5.5 Unused Opportunities

| Opportunity | Endpoint | What It Gives |
|---|---|---|
| Dashboard memory viewer | `/api/memory` | Show brain.db contents without sync job |
| Runtime config updates | PUT `/api/config` | Change model/provider without pod restart |
| Real-time events | `/api/events` (SSE) | Live activity feed in dashboard |
| OpenAI-compatible proxy | `/v1/chat/completions` | Let users point any OpenAI client at their claw |

### 5.6 Rules

- In Clawcraft's architecture, the pod MUST NOT expose any channel-specific endpoints to external services. Most channel traffic routes through Convex adapters. (The pod has built-in channel webhook endpoints like `/whatsapp`, `/qq`, `/github`, but Clawcraft does not use them — Convex handles channel I/O for non-native channels.)
- Pod MUST NOT contact external channel APIs directly — **except Telegram** in native mode (§3.4), where the pod runs long polling against `api.telegram.org`.
- Pod MUST authenticate all `/api/*` requests via pre-shared bearer token. `/health` and `/metrics` are unauthenticated.
- Pod MAY call Convex HTTP route `/api/integration-status` using the pre-shared token for integration status reporting. (Memory and scheduling are runtime-managed inside the pod — Convex exposes no memory/scheduling routes.)
- `/health` returns component status for gateway, daemon, channels, scheduler. Used by `pollHealth` and `healthCheckAll` cron.

### 5.6.1 `[CLAWCRAFT_TRIGGER]` signal envelope on the channel webhook (Spec 4b, format pivoted v2.34.0 — Convex-issued multi-phase directives retired in favor of intent-map routing)

The Convex control plane occasionally needs to signal that a user clicked a dashboard button — Validate or Run — on the per-user pod's agent. v2.34.0 (rnk-22q2 of the rnk-eelu epic) collapsed the multi-phase Convex-issued `[CLAWCRAFT_DIRECTIVE]` blocks (which inlined the entire grounded-semantic validate flow / three-phase execution walk loop as English-language coaching per click) onto a **tiny signal envelope** the agent routes through AGENTS.md's intent map — the same handler used when the user types "run" or "validate" in chat. Buttons and chat converge on one surface.

The signal that distinguishes a Convex-issued trigger from a user-channel chat message is a tagged block on the `content` field: **`[CLAWCRAFT_TRIGGER]` … `[/CLAWCRAFT_TRIGGER]`** — modeled on the proven `[EMAIL_RECEIVED]` … `[/EMAIL_RECEIVED]` pattern in `apps/clawcraft/domain/email/notification.ts:formatNotification`.

**Envelope-family inventory** (one page, per methodology §4 — every `[…_RECEIVED]` / `[CLAWCRAFT_…]` envelope composed by Convex and POSTed to the pod on the shared 42618 webhook channel; the `Source:`/tag line is what the channel-blind pod uses to label the trigger):

| Envelope | Channel | Composed by | Receiver routing |
|---|---|---|---|
| `[EMAIL_RECEIVED]` | Email | `domain/email/notification.ts:formatNotification` | Agent's standing instructions for inbound email |
| `[LINQ_RECEIVED]` | Linq SMS/iMessage/RCS | `domain/linq/notification.ts` (sibling of email) | Agent's standing instructions for inbound SMS (add-linq) |
| `[CLAWCRAFT_TRIGGER]` (`Source: run-button` / `validate-button`) | Dashboard buttons / Convex-issued (run, validate) | `convex/webhookDispatch.ts` | AGENTS.md intent map (`Source` + `Intent` → praxis verb) |
| `[CLAWCRAFT_TRIGGER]` (`Source: pod-cron`, `Intent: resume-execution`) | **Self-addressed pod cron** (a one-shot the agent arms at park time) | The agent itself — the cron job's message text IS this envelope | AGENTS.md intent map (`Source: pod-cron` + `Intent: resume-execution` → `praxis execute --execution <id>`) |
| `[CLAWCRAFT_TRIGGER]` (`Source: bead-resolve`, `Intent: resume-execution`) | A bead's recorded resolution (deferred composer) | praxis-intent-relay (ships LATER — NO dormant code in `webhookDispatch.ts` today) | AGENTS.md intent map (same `Intent: resume-execution` entry; §5.6.2 one-intent-many-sources precedent) |

The pod is channel-blind: it parses the in-band tag it was handed, it does not learn channels as a network concept. Adding a channel = a new `[…_RECEIVED]` envelope + a `notification.ts` composer + (if it needs intent routing) an AGENTS.md intent-map entry — never a new pod HTTP path, never a typed body discriminator on `/webhook` (§5.6.1 trailer rules below; praxis-doctrine §8).

**Envelope shape.** Exactly five (run-button) or four (validate-button) lines between the tags, each `Key: Value`:

```
[CLAWCRAFT_TRIGGER]
Source: run-button
Intent: run
ThreadId: <thread id>
TriggerPayload: <JSON, shell-escaped for single-quote interpolation>
[/CLAWCRAFT_TRIGGER]

[CLAWCRAFT_TRIGGER]
Source: validate-button
Intent: validate
ThreadId: <thread id>
[/CLAWCRAFT_TRIGGER]
```

Composer output is < 300 bytes (down from ~2000+ in the multi-phase directive era); a regression assertion in `apps/clawcraft/test/integration/webhookDispatch.test.ts` pins the budget.

**Receiver-side contract.** The agent reads the envelope's `Source` + `Intent` lines and looks up the matching entry in AGENTS.md's intent map (rendered by `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate`):

- `Source: run-button` + `Intent: run` → `praxis spec activate-execution <ThreadId> --trigger-payload '<TriggerPayload>'` — same handler as user typing "run".
- `Source: validate-button` + `Intent: validate` → `praxis spec verify --thread <ThreadId>` — same handler as user typing "validate".

The full multi-phase walk loop for execution (Phase 1 activate → Phase 2 ready-set/update loop → Phase 3 verify-close) lives in praxis-doctrine §10.5 + the agent's own discovery via `praxis manifest show "spec activate-execution"`. The grounded-semantic validate flow (mechanical pre-pass → semantic inject → persist) lives in praxis-doctrine §9.10 + `praxis manifest show "spec verify"`. None of that prose is inlined per click anymore.

The agent treats the envelope itself as the user's confirmation — execute the verb directly via the shell tool, then reply with a one-line summary (exit code + first line of stdout on success, stderr on failure). The "always reply" rule is load-bearing: it forces the tool call before the turn closes (an earlier "no reply on success" variant let the agent skip both — see retirement audit trail below).

**Routing contract (prod path).** Convex Cloud cannot reach pod ClusterIP DNS directly, so the dispatcher routes through the in-cluster Nginx gateway — same as email relay:

| Property | Value |
|---|---|
| URL | `<process.env.WS_GATEWAY_URL>/relay/<userId>` |
| HTTP method | POST |
| Auth (nginx → relay) | `X-Relay-Token: <process.env.GATEWAY_RELAY_TOKEN>` |
| Auth (pod-side bearer) | `X-Pod-Token: <users.preSharedToken>` (nginx rewrites to `Authorization: Bearer …` on the upstream hop) |
| Body | `{ "sender": "<users._id>", "content": "[CLAWCRAFT_TRIGGER]\\nSource: …\\nIntent: …\\nThreadId: …\\n[TriggerPayload: …\\n][/CLAWCRAFT_TRIGGER]" }` |
| Upstream | `pod:42618/webhook` (channel webhook — full agent loop) |
| Composer | `apps/clawcraft/convex/webhookDispatch.ts` (`sendMessage` private helper + the two public actions; `composeExecutionDirective` / `composeValidateDirective` retain their names to minimize churn but emit envelopes, not directives) |

**Local-dev override (direct pod fetch).** No nginx runs in local dev — `infra/local/docker-compose.yml` exposes pod ports 42617 + 42618 directly on the host (§5.0 polysemy table). To keep the same composer working without a sidecar reverse proxy, `webhookDispatch.ts` falls back to direct-pod fetch when `WS_GATEWAY_URL` is unset and `CLAW_LOCAL_POD_WEBHOOK_URL` is set:

| Property | Value (local-dev mode) |
|---|---|
| URL | `<process.env.CLAW_LOCAL_POD_WEBHOOK_URL>/webhook` (e.g. `http://localhost:42618/webhook`) |
| HTTP method | POST |
| Auth | `Authorization: Bearer <users.preSharedToken>` (mirrors what nginx rewrites X-Pod-Token to in prod) |
| Body | `{ "sender": "<users._id>", "content": "[CLAWCRAFT_TRIGGER]\\nSource: …\\n…\\n[/CLAWCRAFT_TRIGGER]" }` (unchanged across transports) |
| Set by | `infra/scripts/dev/render-claw-config.ts` (mirrors `CLAW_LOCAL_POD_WEBHOOK_URL=http://localhost:42618` into local Convex env on each run) |

The transport selector in `webhookDispatch.ts:resolveTransport` checks WS_GATEWAY_URL first, so any prod env will use nginx regardless of whether `CLAW_LOCAL_POD_WEBHOOK_URL` is also set; the local-dev branch is only reachable when WS_GATEWAY_URL is genuinely absent. The "don't bypass nginx in prod" rule still binds — `CLAW_LOCAL_POD_WEBHOOK_URL` is local-dev only and MUST NOT be set in prod Convex env.

The idle-pod reaper's local-control sidecar (`CLAW_LOCAL_POD_CONTROL_URL` → `infra/local/pod-control/`) mirrors this same precedent: no GKE locally, so Convex drives `docker stop/start` via a localhost shim instead of the cluster scale API, gated on `!isGkeConfigured()`. See infra-doctrine §6.8 and `docs/tasks/ongoing/idle-pod-reaper/`.

**Why the envelope, not the multi-phase directive.** Three problems made per-click directive coaching brittle:

1. **Drift surface.** Every protocol change (a new factory tool hint, a new validate step, a new verifier rule) required editing every directive composer; iterating on a spec verb was hours, not seconds.
2. **Authority split.** The agent had two sources of truth for the same verb — AGENTS.md's intent map for chat-triggered runs vs. the directive's inlined prose for button-triggered runs. Drift between the two meant button-clicks behaved differently than chat utterances on the same thread.
3. **Cost.** A directive embedded the full spec body + factory body + tool catalog + multi-step prose per click. The envelope is < 300 bytes; the agent re-uses the same intent-map prose it already loads at boot.

Collapsing to the envelope makes praxis-doctrine §9.10 (validate lifecycle) + §10.5 (execute walk loop) the single sources of truth for "how to walk this verb"; the agent discovers them via `praxis manifest show` (standing orchestration rule, AGENTS.md §"Standing orchestration rule"). Buttons become signals, not scripts.

**Trust posture (v1, intentionally permissive).** Any caller with the user's `preSharedToken` can POST a `[CLAWCRAFT_TRIGGER]` envelope and the agent will route through the intent map. A malicious user with chat access could type such a block themselves and the agent might honor it. Blast radius is bounded — the user's own pod runs the verb, no cross-user impact, and the intent-map verbs are all on the agent's normal tool surface anyway (so the privilege escalation is from "ask the agent to do something" to "tell the agent to do something specific"). Hardening options for v2: HMAC signature on Convex-issued envelopes, pod-side allowlist of sender IPs for trigger weight, or parse-time rejection of user chat messages containing the trigger tags. None are in scope for v1.

**Adding a new Convex-issued trigger.** Add a new `Source: ...` + `Intent: ...` pair to the envelope and extend the AGENTS.md intent map (in `renderAgentsTemplate`) to route the new intent to its praxis verb. Add a thin compose helper in `webhookDispatch.ts` and route through `sendMessage`. Do NOT add a new HTTP path on the pod, do NOT introduce a typed body discriminator on `/webhook`, do NOT add a `body.type` switch (praxis-doctrine §8 forbids re-introducing dedicated control handlers). Do NOT add a second Convex→pod fetch path that bypasses the nginx gateway in prod — Convex Cloud can't reach ClusterIP DNS, so any direct-pod-fetch attempt will silently degrade in prod (this was the v2.31.0 failure mode). Do NOT revive the `[system]` prefix or the multi-paragraph `[CLAWCRAFT_DIRECTIVE]` shape — the retirement audit trail below catalogues why both short-circuit the agent. Cross-ref: backend-doctrine §6.5.0 (Convex-side composer); praxis-doctrine §9.10 (validate lifecycle); praxis-doctrine §10.5 (execute walk loop).

**Resume-execution envelope (`Intent: resume-execution`, praxis-hardening 0.11.0, D13).** A fourth `[CLAWCRAFT_TRIGGER]` member, added when async (cross-turn) praxis walks shipped (§5.7). When a walk parks at a timer boundary, the agent arms a one-shot pod cron whose message text IS the envelope below; when the cron fires into the full agent loop (§14 cron facts), the agent's own intent map routes the fire back into the parked walk. The envelope is therefore **agent-composed and self-addressed** — a new inventory row of a class the prior three lacked (those were all composed by Convex / the client and addressed *to* the agent; this one the agent composes *for its own future self*).

```
[CLAWCRAFT_TRIGGER]
Source: pod-cron
Intent: resume-execution
ExecutionId: <ulid>
ThreadId: <thread id>     # optional — included only when cheap to carry
[/CLAWCRAFT_TRIGGER]
```

- `Source: pod-cron` + `Intent: resume-execution` → `praxis execute --execution <ExecutionId>` (re-enter the parked walk named by `ExecutionId`).
- `Source: bead-resolve` is **REGISTERED** in doctrine as a *second source* for the same `Intent: resume-execution`, but its composer ships LATER with praxis-intent-relay — there is **NO dormant code in `webhookDispatch.ts`** today (verified: only `composeExecutionDirective` / `composeValidateDirective` exist). Registering two sources for one intent follows the §5.6.2 one-intent-many-sources dispatch precedent; the §5.6.1 **authority-split** failure mode (problem #2 above) forbids inventing a *second convention* for resume — both sources MUST converge on the single `Intent: resume-execution` intent-map entry.

The pod-cron envelope is the *optimization* path; the correctness path is the AGENTS.md due-scan (§5.7, D14) — even if the cron fire is lost, `praxis execute --list` on the next activation re-discovers the parked walk. Cron persistence across restarts is VERIFIED (§14).

**`--resolved-by user` trust posture (FMEA-3, praxis-hardening 0.11.0).** When a parked walk hits a **human** gate, the agent resolves it with `praxis execute --update <bead> --resolved-by user`. This assertion is **agent-asserted — praxis cannot verify it**, so the doctrine binds *when* the agent may make it:

- It may ONLY follow an approval the agent observed on an **authenticated owner channel** — the web chat WS or native Telegram (§3.4). Those are the only channels whose sender is provably the pod's 1:1 owner (§11.1).
- It MUST NEVER be asserted from **envelope-relayed external content** — an email body, an SMS, a forwarded message. Those arrive via `[…_RECEIVED]` relays from third parties and carry no owner-authentication; treating their text as approval would let any external sender forge a human-gate clearance.
- **Forgeability honesty.** Even on an authenticated owner channel, `--resolved-by user` remains a trust assertion the agent makes about its own turn — there is no cryptographic owner-signature on the resolution in v1. The blast radius mirrors §5.6.1's permissive trust posture (the user's own pod, no cross-user impact); hardening (a signed approval token threaded from the WS/Telegram session into the resolve call) is a v2 item, not in scope.
- The agent NEVER self-approves to drain a parked human gate — it surfaces the bead's `resolve_hint` and ends the turn (see the AGENTS.md "Parked walks" + "Human-gate provenance" sections, §17.4).

**Retirement audit trail (chronological).**

- **v2.31.0** specified a bare `[system] Please run: <cmd>` prefix. The LLM read `[system]` as informational context and skipped tool execution ("🤖 No reply: system directive, not a user message"). No AGENTS.md prose overrode the reflex.
- **v2.31.1** corrected the transport (port 42617 → 42618, payload `{message}` → `{sender, content}`) but kept the `[system]` literal; the short-circuit persisted.
- **v2.31.2** dropped `[system]` and adopted the `[CLAWCRAFT_DIRECTIVE]` … `[/CLAWCRAFT_DIRECTIVE]` shape (mirror of the empirically-working `[EMAIL_RECEIVED]` pattern). A "no chat reply on success" guidance variant produced a different short-circuit (the agent skipped both the tool call and the reply); the always-reply rule landed as the load-bearing fix.
- **v2.32.0–2.32.3** introduced sibling client-WS-issued `[CLAWCRAFT_DIRECTIVE]` Sources (`bead-forward`, `validation-forward`, `resolver-choice`, `sop-ingestion`) — see §5.6.2; those are agent-prose tasks (no embedded shell command) and survive this collapse unchanged.
- **v2.33.0** rewrote AGENTS.md as a 4-section file (standing rule + intent map + manifest snapshot + the legacy directive section preserved verbatim during transition) — the foundation for routing button intents through the same surface as chat. Parity was verified by rnk-eelu/#4 (closed; projection-state parity test + manual rare-book smoke).
- **v2.34.0 (this entry, rnk-22q2).** Multi-phase Convex-issued `[CLAWCRAFT_DIRECTIVE]` blocks retired in favor of `[CLAWCRAFT_TRIGGER]` envelopes routed through the intent map. The transitional `## Convex-issued [CLAWCRAFT_DIRECTIVE] tasks` section in `renderAgentsTemplate` is deleted; the intent map gains a paragraph documenting envelope routing. Lesson: prompts that inline orchestration logic per call drift faster than the manifest they should be deferring to.

### 5.6.2 Client-WS-issued `[CLAWCRAFT_DIRECTIVE]` directives + assistant→client render tags

§5.6.1 governs **Convex-issued** directives that arrive at the pod via the channel webhook (port 42618). The same `[CLAWCRAFT_DIRECTIVE]` … `[/CLAWCRAFT_DIRECTIVE]` tagged-block convention is **also** used on the **client-issued WS path** (`/ws/chat`) for directives composed in the browser and sent over the live WebSocket. The pod sees identical tag shape regardless of transport — only the `Source:` line distinguishes the use case.

**Current client-WS-issued directive sources** (composed in `apps/clawcraft/src/hooks/useChat.ts`):

| `Source:` | Trigger | Body shape |
|---|---|---|
| `bead-forward` | ✈ button on any task in TasksTab | Bead metadata between `--- BEAD START ---` / `--- BEAD END ---` markers; agent reads, takes the next step or asks a clarifying question, replies in chat. |
| `validation-forward` | ✈ button on a validation prior-work bead (`writer === "validation" && role === "prior_work"`) | Carries a `ResolverContext:` line + finding body. Agent responds with EITHER a direct-fix chat reply OR an inline `[RESOLVER_OPTIONS]` render tag (see below). |
| `resolver-choice` | User clicks an option in the in-bubble `ResolverChoice` widget | Carries `Context:` (echoed from `ResolverContext`) + `Choice: A \| B \| C`, plus `UserText:` when `Choice: C`. Agent treats this as the user's selection and proceeds; on C, obeys `UserText:` verbatim. |
| `sop-ingestion` | User drops 1..10 PDF/text files in `SpecTab`'s empty-state `SopDropZone` (`apps/clawcraft/src/components/tasks/SopDropZone.tsx`) | Carries N `[MEDIA_UPLOAD: type=… \| name=… \| path=… \| size=…]` blocks (one per file) + ingestion instructions naming `praxis create "<filename>" -t reference --file <tmp>` per file + `praxis spec create --thread <id> --from-file <prose-tmp> --factory-from-file <factory-tmp>` afterward. Persistence: ONE synthetic user bubble in the `messages` table (`role: "user"`, `kind: "synthetic"`, `source: "container"`); `content` carries the joined MEDIA_UPLOAD blocks (NOT the directive prose — live ingestion instructions are WS-only and ephemeral); `attachments` carries N entries. See `docs/tasks/completed/spec-from-sop/spec-from-sop-spec.md` and the schema delta in `apps/clawcraft/convex/schema.ts` (`messages.kind` optional column). |

**Assistant→client render tags (current set).** Distinct concept class from `[CLAWCRAFT_DIRECTIVE]`: these are emitted by the agent inline in its assistant message and parsed by `apps/clawcraft/src/components/chat/MessageBubble.tsx` for in-bubble rendering. They flow agent → client, not client → agent.

| Tag | Renders | Parser site |
|---|---|---|
| `[INTEGRATION_LINK]{json}[/INTEGRATION_LINK]` | "Connect <provider>" link button below the message body | `extractIntegrationLinks` in `MessageBubble.tsx` |
| `[RESOLVER_OPTIONS]{json}[/RESOLVER_OPTIONS]` | A/B/C in-bubble widget (`ResolverChoice.tsx`) — button A, button B, button C with free-text override input | `extractResolverOptions` in `MessageBubble.tsx` |

`[INTEGRATION_LINK]` JSON has two forms (parser accepts both — see `MessageBubble.tsx:extractIntegrationLinks` + the href fallback at the render site). **Provider shortcut** (preferred for validation-resolver responses and any in-chat fix): `{ provider: <provider>, label: <"Connect …"> }`. The button deep-links to `/integrations?connect=<provider>`, which the route at `src/routes/_authenticated/integrations.tsx` auto-triggers OAuth for. **Two-step OAuth form** (used when you want the user to OAuth in a separate tab via a direct Composio link): `{ url: <redirectUrl>, label: <"Connect …"> }`, where `redirectUrl` is obtained by POSTing to `/api/integration-connect`. The agent-side guidance for both forms lives in `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate` §"Integration Links". The §"Validation Resolver Protocol" section MUST keep the provider-shortcut form pinned as the canonical "user needs to connect <X>" response — bare `/integrations?connect=<provider>` URLs in prose render as inert text, not a button (2026-05-19 audit; see v2.32.1 changelog).

`[RESOLVER_OPTIONS]` JSON shape: `{ context: string, optionA: { label: string }, optionB: { label: string }, optionC: { placeholder: string } }`. The `context` field MUST echo the originating directive's `ResolverContext:` value so the user's choice can be correlated. The widget calls `useChatBridgeStore`'s `resolverSend` (wired by `useChat`) to emit the follow-up `resolver-choice` directive. Per `methodology-doctrine §1` (polysemous labels are bugs): `[CLAWCRAFT_DIRECTIVE]` and `[RESOLVER_OPTIONS]` are deliberately distinct names — one is client/Convex → agent, the other is agent → client; never merge them.

**Adding a new client-WS-issued directive source.** Add a new branch in `useChat.ts:sendBeadToAgent` (or a new `useCallback` if the trigger isn't bead-shaped); compose a new `Source: <name>` body following the §5.6.1 tagged-block shape. If the agent needs to round-trip a structured response back into the UI, add a sibling render tag (parser in `MessageBubble.tsx`, renderer in `components/chat/`). Do NOT add a typed message-type discriminator to `WsClientMessage` — let `Source:` carry the dispatch, same as §5.6.1.

**Adding a new render tag.** Add the regex + parser in `MessageBubble.tsx` alongside `INTEGRATION_LINK_RE` / `RESOLVER_OPTIONS_RE`. Always strip the tag from `cleanContent` whether or not parsing succeeds — the user must never see raw tag text. Update the agent-side guidance in `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate` so the agent knows when and how to emit the tag.

**Trust posture.** Client-WS-issued directives flow over the user's own WS connection; the user already controls the tab. The §5.6.1 v2 hardening notes (HMAC, pod-side allowlist) target Convex-issued directives — not these. A malicious user typing a `[CLAWCRAFT_DIRECTIVE]` block into the chat input gets the same effect as forwarding a bead, which is by design.

### 5.6.3 Spec-first attention protocol — agent default behavior on human-issued conversational asks

§5.6.1 + §5.6.2 govern *agent-facing tagged-block directives* (Convex- and client-issued, respectively). §5.6.3 governs the **default behavior when the message is plain conversational chat** — no tagged block, just the human describing something they want.

**Binding rule.** Every chat thread owns at most one spec (per `praxis-doctrine.md` §5.2.2 — one prose file + one factory file at `<threadId>.md` / `<threadId>.factory.md` under praxis's owned `.praxis/data-sync/specs/`). When the human asks to BUILD, CREATE, CONFIGURE, AUTOMATE, or SET UP anything multi-step, the agent's default first move MUST be to author or update the thread's spec via `praxis spec create` / `praxis spec update` (the verbs already declared in `praxis-doctrine.md` §6.1 and `apps/clawcraft/domain/praxis-commands.ts:PRAXIS_AGENT_COMMANDS`), NOT to free-hand the answer in chat prose.

**Shape detector (declared canon).** The agent treats a user turn as spec-shaped if the message contains TWO OR MORE of: a trigger condition, an action verb, an integration tool name, a schedule/cadence, or a multi-step sequence. Two-or-more → reach for spec verbs. The two-or-more threshold is load-bearing: it protects against false positives on single-glossary-word matches like "what's the process for resetting my password?"

**Vocabulary mapping (canonical).** The following user words are doctrinally equivalent to "spec" for routing purposes: `workflow`, `automation`, `flow`, `process`, `rule`, `routine`, `setup`, `pipeline`, `job`. The list is closed for v1 — additions should be justified against an observed failure mode, not added speculatively.

**Failure mode this codifies.** Pre-2026-05-19, `renderAgentsTemplate()` had no spec-first guidance — when the user said "create me a workflow for managing email triage", the agent free-handed a multi-step plan in chat prose instead of running `praxis spec create`, losing the work to scrollback the moment the thread compacted. The pre-existing TOOLS.md "Per-thread specs" H3 (rendered by `renderToolsTemplate`) documented HOW to author a spec but never tied that machinery to a conversational trigger; §5.6.3 closes that gap by hoisting the rule into the higher-attention AGENTS.md surface.

**Agent-side wiring (v1, pre-rnk-eelu — retained as audit trail).** The agent originally learned this protocol from a dedicated `## Spec-Driven Workflow Protocol` section at the TOP of `renderAgentsTemplate()` placed BEFORE the `## Convex-issued [CLAWCRAFT_DIRECTIVE] tasks` section. Per the §5.6.1 retirement audit lesson — *"a convention that mirrors a pattern the LLM already handles correctly" beats "abstract rules"* — that section included a worked example contrasting the failure-mode chat-handling against the spec-write handling, using the exact failure phrase from 2026-05-19 ("create me a workflow for managing email triage").

**Agent-side wiring (v2, rnk-p7a0 — current).** The dedicated section was collapsed in `rnk-p7a0` (part of the `rnk-eelu` epic that migrates the spec-driven workflow agent surface from packed directives to praxis manifest + `next_action`). `renderAgentsTemplate()` no longer ships the worked example or the vocabulary/shape/carve-out prose; instead, AGENTS.md ships a minimal `## Intent map` that routes "run / execute / kick off the spec" → `praxis spec activate-execution <threadId>` and "validate / check the spec" → `praxis spec verify --thread <threadId>`, plus a frozen verb+summary **index** (projected from `praxis manifest list --mode all --json` by `manifest-snapshot.ts:projectManifestToIndex`, rnk-ih70) so the agent discovers `spec create` / `spec update` (and the rest of the spec surface) via the manifest's own `summary` fields. The index carries `{verb, summary}` only — per-verb `flags` / `positional_args` are **fetched on demand** via the standing rule's `praxis manifest show "<verb>" --json` (the embedded block is ~1.6 KB, not ~28.8 KB; it MUST NOT re-bake the full per-verb schema, which the agent does not read from the snapshot — dev trace `6baecbab` — and which otherwise costs ~7,400 tok on every agent turn). The vocabulary mapping, shape detector, and carve-out lessons above remain doctrinally binding — they apply when the agent reads the manifest's `summary` for `spec create` / `spec update` and routes a conversational ask through it — but they are no longer pre-baked into AGENTS.md prose. Whether the v2 surface produces equivalent behavior on the 2026-05-19 failure phrase is the parity goal of sibling task `rnk-eelu/#4`. If parity fails, the doctrine update is to re-bake the worked example into AGENTS.md or to extend praxis's `manifest list --json` output with richer per-verb routing hints; do not silently restore the v1 section without updating §5.6.3 accordingly.

**No escape hatch (still binding).** Don't tell the agent to ASK the user whether to write a spec — §5.6.1's "permission to skip work is asymmetric" lesson applies: agents accept the skip eagerly and ignore the surrounding "you still have to do the work" qualifiers. The agent writes; the user edits afterward via natural conversation (which becomes a `spec update`).

**Carve-out (still binding).** Spec-write fires on BUILD intent (shape detector ≥2), not on any tool-using turn. One-shot factual questions, read-only lookups, conversational chat, and meta-questions about the agent itself stay in chat — they are not spec-shaped.

### 5.7 Agent-Side Capabilities

The pod ships first-class capabilities that the agent reaches for directly,
distinct from delegated specialist agents:

| Capability | Purpose | Surface |
|---|---|---|
| `memory_store` / `memory_recall` / `memory_forget` | Single-shot facts, preferences, decisions | brain.db (SQLite, sole memory backend) |
| Web search | Current information lookup | ZeroClaw `[web_search]` tool |
| **Praxis** | Durable task tracker — multi-step work, follow-ups, anything with status. **Whether a candidate is bead-shaped at all is governed by `praxis-doctrine.md` §2 (Bead Semantics) — the agent applies the three-question filter (compaction-loss / reference / state) at `praxis create` time before promoting state to a bead.** | CLI installed at `/opt/praxis/`, writes to `workspace/.praxis/`. Agent discovers the verb *catalog* from the AGENTS.md manifest snapshot (`praxis manifest list`/`show`); `TOOLS.md` (rendered by `apps/clawcraft/domain/claw-workspace.ts:renderToolsTemplate`) carries the *usage doctrine* — the §2.2 filter block + per-thread spec walkthrough, with the few verb literals it names inline sourced from `apps/clawcraft/domain/praxis-commands.ts:PRAXIS_AGENT_COMMANDS` per `praxis-doctrine.md` §6.1/§6.5. The standalone TOOLS.md command-surface list was retired by `rnk-rwfn`. |

Praxis is **not** a delegate target — it has no LLM call of its own. The
agent invokes `praxis <subcommand>` directly via the shell tool. This
distinguishes it from the Composio-backed specialist agents (sheets, mail,
calendar, etc.) which live behind `delegate(agent="...")`.

**`/opt/praxis` path-shape contract (load-bearing for dev parity).** Praxis
is installed as the full npm package at `/opt/praxis/` (the contents of
`/usr/local/lib/node_modules/@soulbound-labs/praxis/` copied across in the
sovereign-fork Stage 3 build — see commit `1aac3308`). The runtime entry
point is at `/opt/praxis/dist/bin-bootstrap.cjs`, with `/usr/local/bin/praxis`
symlinked there. Any dev-time overlay of `/opt/praxis` MUST mount the
whole package directory (e.g. `packages/praxis:/opt/praxis:ro`), NOT
just the `dist/` subfolder — overlaying with `packages/praxis/dist`
would break the symlink because `/opt/praxis/dist/bin-bootstrap.cjs`
would resolve to a non-existent path. The local dev loop relies on this
contract; see infra-doctrine §15.24.

#### 5.7.1 Async (cross-turn) praxis walks — park & resume (praxis-hardening 0.11.0)

A praxis execution is **no longer single-turn**. A walk now **parks** at a human, timer, or event boundary and **resumes across turns** — the gap can be minutes, hours, or weeks. Park is signalled by `praxis execute` returning `next_action: null` + a `data.parked` snapshot (the keystone is praxis-only via `null` + `data.parked`; the bare `null` without `data.parked` still means the walk is *finished*). Two mechanisms keep a parked walk alive, and they are **not** redundant — one is the optimization, the other is the correctness guarantee:

- **Cron arming at park time (optimization, D13).** On a `timer` park, the agent arms a one-shot pod cron at `data.parked.dueAt` whose message text is the `[CLAWCRAFT_TRIGGER]` / `Source: pod-cron` / `Intent: resume-execution` envelope (§5.6.1). When it fires into the full agent loop (§14), the agent's intent map routes it straight back into `praxis execute --execution <id>`. This makes the *common* case low-latency.
- **AGENTS.md due-scan (correctness, D14).** On *any* activation, webhook, or resume trigger, the agent runs `praxis execute --list` FIRST and resumes every in-flight execution whose park wait is now satisfied. This is the catch-up net: it resumes a parked walk even when a cron fire was missed or the pod restarted between park and due. The cron is best-effort; the due-scan is the contract.

**No timer-driven `scaleUp`.** A parked run does NOT cause the control plane to wake a sleeping pod when its timer comes due. A parked walk on a sleeping pod resumes on the pod's *next natural wake* (a new message, an activation), at which point the due-scan picks it up — mirroring the §9 Linq queued-delivery posture (the pod acts when it next runs, not on an external timer push). **The SLA for a parked-walk resume is therefore EVENTUAL, not deadline-bound.** Under the §15.1 always-on pod policy the pod is generally awake and the cron fires promptly; the eventual-resume guarantee is what holds when it is not.

---

## 6. Credential Isolation

### 6.1 Rules

- Channel credentials (bot tokens, API keys, webhook secrets) MUST be stored in the Convex DB, encrypted via AES-256 (`lib/encryption.ts`).
- Channel credentials MUST NOT appear in pod ConfigMap, pod env vars, or workspace files — **except** the Telegram bot token, which is injected into `config.toml` `[channels_config.telegram]` for native mode (§3.4). This is a deliberate trade-off: ZeroClaw's TOML parser has no env var interpolation, so the token must be inline.
- **OTLP project key** (Laminar ingest auth) is the **third documented plaintext-in-ConfigMap exception**, peer to the Telegram bot token and the `CONTAINER_SERVICE_TOKEN` webhook `auth_header` (§9.x): the zeroclaw emitter reads its OTLP config from `config.toml` `[observability]` only (it ignores OTLP env vars), and the Laminar ingest drops unauthenticated spans — so the project key is rendered as a literal into `[observability] otel_headers` (`Authorization=Bearer <key>`) by `buildConfigToml`, same TOML-no-interpolation rationale. **Dev:** value lives uncommitted in `infra/.env.dev` (`LAMINAR_OTLP_HEADERS`). **Prod (managed, activated):** the **managed-project key**, read at the `gke.ts` client boundary from **prod Convex env** (`LAMINAR_OTLP_HEADERS`, Secret-Manager / `secrets-migrate` backed) and rendered per-pod via the `buildConfigToml` `otlpHeaders` param (backend §9) — never committed, **never copied from the dev project's key** (different projects; rotate at the consumer). The store of record is prod Convex env; the plaintext only materializes into the per-user ConfigMap at render time (control-plane-side). One shared managed-project key across all pods (single managed project), consistent with `PLATFORM_OPENROUTER_KEY`. Governed by `observability-doctrine.md` §5/§5.1.
- The pod ConfigMap contains: LLM API key (OpenRouter), gateway config (port, pre-shared token), autonomy settings, tool timeout overrides (`[http_request]`, `[web_search]`, `[web_fetch]` at 120s), native Telegram channel config (conditional), workspace files.
- The webhook channel secret is intentionally omitted from `[channels_config.webhook]` (HMAC disabled). Auth for the webhook channel is at the network layer: nginx `auth_request` validates the `X-Relay-Token` header, and NetworkPolicy restricts ingress to the gateway namespace. This is a deliberate trade-off documented in the webhook channel config fix spec.
- Decryption happens in Convex actions at the moment of use. For native Telegram, decryption happens at ConfigMap generation time (`provision`, `scaleUp`, `restartPodForIntegration`).

### 6.2 Credential Locations

| Credential | Stored In | Decrypted By |
|---|---|---|
| OpenRouter API key (platform) | Convex env var `PLATFORM_OPENROUTER_KEY` | `provisionUser` / `updateConfigMap` |
| OpenRouter API key (BYOK) | Convex DB `users.llmApiKeyEncrypted` | `provisionUser` / `updateConfigMap` |
| Telegram bot token | Convex DB `integrations.config.botTokenEncrypted` + ConfigMap `[channels_config.telegram]` (plaintext, native mode) | `podActions.provision`, `podActions.scaleUp`, `integrationActions.validateAndSetup`, `integrationActions.restartPodForIntegration` |
| App URL (frontend) | OAuth state parameter (primary), Convex env var `APP_URL` (fallback) | `http.ts` `resolveOAuthRedirect()` — validates against `ALLOWED_ORIGINS` allowlist |
| Pre-shared token | Convex DB `users.preSharedToken` | Injected into ConfigMap at provision time |
| Container service token | Convex env var `CONTAINER_SERVICE_TOKEN` | `http.ts` webhook handler |
| GKE SA key | Convex env var `GKE_SERVICE_ACCOUNT_KEY` | `clients/gcpAuth.ts` |
| Composio API key (platform) | Convex env var `COMPOSIO_API_KEY` | `buildConfigToml` → config.toml `[composio]` section |
| Laminar OTLP project key | dev: `infra/.env.dev` `LAMINAR_OTLP_HEADERS` (uncommitted); prod: **managed-project key in prod Convex env `LAMINAR_OTLP_HEADERS`** (Secret-Manager / `secrets-migrate` backed; `secrets-manifest.yaml audit_pending:`; never copied from dev) | `buildConfigToml` (`otlpHeaders` param, read at the `gke.ts` client boundary) → config.toml `[observability] otel_headers` (plaintext literal — observability §5/§5.1, §6.1 third plaintext-ConfigMap exception) |
| Composio auth config (Google Sheets) | Convex env var `COMPOSIO_AUTH_CONFIG_GOOGLE_SHEETS` | `integrationActions.initiateComposioOAuth` |
| Composio auth config (Gmail) | Convex env var `COMPOSIO_AUTH_CONFIG_GMAIL` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (Google Docs) | Convex env var `COMPOSIO_AUTH_CONFIG_GOOGLE_DOCS` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (Google Slides) | Convex env var `COMPOSIO_AUTH_CONFIG_GOOGLE_SLIDES` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (Google Calendar) | Convex env var `COMPOSIO_AUTH_CONFIG_GOOGLE_CALENDAR` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (Notion) | Convex env var `COMPOSIO_AUTH_CONFIG_NOTION` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (LinkedIn) | Convex env var `COMPOSIO_AUTH_CONFIG_LINKEDIN` | `integrationActions.initiateGenericComposioOAuth` |
| Composio auth config (Calendly) | Convex env var `COMPOSIO_AUTH_CONFIG_CALENDLY` | `integrationActions.initiateGenericComposioOAuth` |
| Email webhook secret | Cloudflare Worker secret `CONVEX_WEBHOOK_SECRET` + Convex env var `EMAIL_WEBHOOK_SECRET` | Worker → Convex Bearer auth on `/email-webhook` and `/email-upload-url` |
| Gateway relay token | Convex env var `GATEWAY_RELAY_TOKEN` + Terraform var `gateway_relay_token` (Nginx ConfigMap) | `emailRelay.relayViaGateway` → Nginx `X-Relay-Token` header |
| WS gateway URL | Convex env var `WS_GATEWAY_URL` | `emailRelay.relayViaGateway` — base URL for gateway relay (e.g., `https://gw.clawcraft.ca`) |

---

## 7. Tool Policy

The pod's `config.toml` controls which ZeroClaw tools are available on non-CLI channels (web chat, Telegram).

### 7.1 Configuration

```toml
[autonomy]
non_cli_excluded_tools = [<tools excluded from non-CLI channels>]
auto_approve = [<tools that don't require user confirmation>]
```

### 7.2 Current Policy (fully permissive)

All tools are enabled on all channels. `non_cli_excluded_tools` is empty. All tools are in `auto_approve`.

```toml
# Actual config rendered by buildConfigToml() in domain/claw-config.ts:
non_cli_excluded_tools = []
auto_approve = ["composio", "memory_observe", "memory_recall", "memory_store",
  "memory_forget", "http_request", "web_search", "web_search_tool", "web_fetch",
  "delegate", "delegate_coordination_status", "subagent_spawn", "subagent_list",
  "subagent_manage", "shell", "process", "file_write", "file_edit", "file_read",
  "git_operations", "browser", "browser_open", "screenshot", "schedule",
  "cron_add", "cron_remove", "cron_update", "cron_run", "pushover",
  "proxy_config", "web_access_config", "web_search_config",
  "model_routing_config", "channel_ack_config", "image_info"]
```

**Note**: `file_write` and `file_edit` are enabled and auto-approved. This is required for the agent to create USER.md and MEMORY.md on the workspace PVC during onboarding. Security is enforced at the mount level: `/zeroclaw-data/workspace/system/` files are kernel-level read-only (ConfigMap subPath overlays), so the agent cannot modify its own identity files even with file_write enabled.

### 7.3 Tool Policy Rendering

Tool policy is rendered by `apps/clawcraft/domain/claw-config.ts` → `buildConfigToml()`. The `ToolPolicy` type (`"safe" | "full"`) exists but is currently unused — the rendered config is always fully permissive regardless of the policy value.

---

## 8. Workspace Files Contract

ZeroClaw uses the OpenClaw identity format. Workspace files are split across two mounts:

- **System files** (`/zeroclaw-data/workspace/system/`, ConfigMap subPath overlays, readOnly): Operator-controlled identity, personality, tools, and bootstrap instructions. Rendered by Convex, injected via ConfigMap.
- **Agent-owned files** (`/zeroclaw-data/workspace/`, PVC, read-write): Files the agent creates and maintains. Persist across restarts on the PVC.

The `ZEROCLAW_SYSTEM_DIR=/zeroclaw-data/workspace/system` env var activates personality.rs split-mount lookup: system dir first, workspace fallback. If the agent writes a fake SOUL.md to the workspace PVC, the one at `/zeroclaw-data/workspace/system/SOUL.md` takes priority and a warning is logged.

### 8.1 Files

| File | Mount | Purpose | Source |
|---|---|---|---|
| `IDENTITY.md` | `system/` (RO overlay) | Agent identity (name, role) | Persona onboarding or seed defaults; Convex renders into ConfigMap |
| `SOUL.md` | `system/` (RO overlay) | Communication style, personality | Persona onboarding or seed defaults; Convex renders into ConfigMap |
| `AGENTS.md` | `system/` (RO overlay) | Agent capabilities description | Static template; Convex renders into ConfigMap |
| `TOOLS.md` | `system/` (RO overlay) | Available tools, scheduling API, Composio integrations | Dynamic template; Convex renders into ConfigMap |
| `BOOTSTRAP.md` | `system/` (RO overlay) | Onboarding instructions (always present, idempotency guard) | Static template; always in ConfigMap |
| `user-storage/` | workspace (GCS Fuse, RO) | User-uploaded files from file manager | GCS bucket via GCS Fuse CSI mount |
| `conversation-attachments/` | workspace (GCS Fuse, RO) | Chat attachment files (per-thread subdirs) | GCS bucket via GCS Fuse CSI mount |
| `USER.md` | workspace root (RW) | User info (name, preferences) | Agent-created on first boot via `file_write` (BOOTSTRAP.md instructions) |
| `MEMORY.md` | workspace root (RW) | Agent working memory | Agent-created on first boot via `file_write` (BOOTSTRAP.md instructions) |
| `HEARTBEAT.md` | workspace root (RW) | Agent runtime heartbeat | ZeroClaw-created at runtime |
| `MEMORY_SNAPSHOT.md` | workspace root (RW) | Memory hygiene snapshot | ZeroClaw-created at runtime |

### 8.2 Rendering

**System files** (ConfigMap) are rendered by `apps/clawcraft/domain/claw-workspace.ts` → `buildWorkspaceFiles()`. Inputs:

- `PersonaSeed` (agentName, userName, communicationStyle, timezone)
- Resolved persona markdown (if onboarding complete) — drives IDENTITY.md and SOUL.md
- `preSharedToken` (for AGENTS.md HTTP callback instructions)
- `convexUrl` (for schedule API endpoints)

**Agent-owned files** (PVC) are created by the agent during onboarding, driven by BOOTSTRAP.md instructions. The agent uses `file_write` to create USER.md and MEMORY.md. These files persist on the PVC and can be updated anytime via `file_edit`.

### 8.3 Rules

- MUST regenerate system files (ConfigMap) on every `scaleUp` — persona may have changed since last pod run.
- BOOTSTRAP.md is always included in the ConfigMap. It contains an idempotency guard that skips onboarding instructions when USER.md already exists.
- System files are ConfigMap `subPath` overlays at `$ZEROCLAW_WORKSPACE/system/` with `readOnly: true`. They appear as files inside the PVC workspace but are immutable at the kernel level.
- Agent-owned files (USER.md, MEMORY.md) are NOT in the ConfigMap. They live on the PVC at the workspace root.
- The agent CANNOT modify system files — `subPath` mounts with `readOnly: true` are kernel-level immutable. If the agent tries to write to `system/SOUL.md`, the syscall fails with EROFS.

---

## 9. Pod Filesystem Layout

### 9.1 Directory Structure

The pod filesystem uses two mount points. No env var overrides for `ZEROCLAW_WORKSPACE` or `ZEROCLAW_CONFIG_DIR` — the image's Dockerfile defaults apply. The only injected env var is `ZEROCLAW_SYSTEM_DIR` to activate the split-mount lookup.

```
/zeroclaw-data/.zeroclaw/          # ZEROCLAW_CONFIG_DIR default (emptyDir, writable)
├── config.toml                    # Init container copies from ConfigMap
├── .secret_key                    # AEAD key (created at runtime)
├── daemon_state.json              # Daemon state (written every 5s for health probe)
└── otp-secret                     # TOTP secret (created at runtime)

/zeroclaw-data/workspace/          # ZEROCLAW_WORKSPACE default (PVC, read-write)
├── system/                        # ZEROCLAW_SYSTEM_DIR — ConfigMap subPath overlays (readOnly)
│   ├── IDENTITY.md                # Operator-controlled identity
│   ├── SOUL.md                    # Operator-controlled personality
│   ├── AGENTS.md                  # Operator-controlled agent protocol
│   ├── TOOLS.md                   # Operator-controlled tool definitions
│   └── BOOTSTRAP.md               # Always present (idempotency guard)
├── user-storage/                  # GCS Fuse mount (readOnly) — user-uploaded files
│   └── {filename}                 # Files from file manager
├── conversation-attachments/      # GCS Fuse mount (readOnly) — chat attachments
│   └── {threadId}/
│       └── {filename}             # Per-thread attachment files
├── MEMORY.md                      # Agent-created on first boot (BOOTSTRAP.md instructions)
├── USER.md                        # Agent-created on first boot (BOOTSTRAP.md instructions)
├── HEARTBEAT.md                   # Agent-created at runtime
├── MEMORY_SNAPSHOT.md             # ZeroClaw-generated (memory hygiene)
├── memory/                        # brain.db — full POSIX locking, no corruption risk
│   ├── brain.db                   # Agent memory (SQLite, sole memory backend)
│   ├── brain.db-shm               # SQLite shared memory
│   └── brain.db-wal               # SQLite write-ahead log
├── sessions/                      # Session state
│   └── sessions.db
├── cron/                          # ZeroClaw native cron state
│   └── jobs.db
├── state/                         # Agent runtime state
│   └── memory_hygiene_state.json
├── .praxis/                       # praxis durable task tracker — opaque to claw (§5.1)
└── devices.db                     # Pairing state
```

**Mount topology**: The PVC is mounted at `/zeroclaw-data/workspace/`. System files are ConfigMap `subPath` overlays at `/zeroclaw-data/workspace/system/<filename>`, each with `readOnly: true`. The `system/` directory appears as a subdirectory of the workspace but its files are immutable — the PVC provides the directory, the ConfigMap provides the file contents via subPath. The emptyDir for config is mounted at `/zeroclaw-data/.zeroclaw/`.

#### 9.1.1 Config Directory Access Model

The daemon reads `/zeroclaw-data/.zeroclaw/config.toml` once at startup to configure the agent loop (provider, model, gateway port, autonomy settings, etc.). After that, the config is in memory. The daemon also writes `daemon_state.json` there every 5 seconds for the health probe.

The agent (LLM loop) never touches that directory. It operates entirely within `$ZEROCLAW_WORKSPACE` (`/zeroclaw-data/workspace/`):

| Component | Reads `/zeroclaw-data/.zeroclaw/` | Writes `/zeroclaw-data/.zeroclaw/` |
|---|---|---|
| Init container | No | Yes (copies `config.toml`) |
| Daemon (Rust process) | Yes (startup config load) | Yes (`daemon_state.json`, `.secret_key`) |
| Agent loop (LLM) | No | No |
| Health probe | Yes (reads `daemon_state.json`) | No |

The agent's tools (`file_read`, `file_write`, `shell`, etc.) are scoped by the security policy. With `workspace_only = true`, the agent literally cannot access `/zeroclaw-data/.zeroclaw/` even if it tried — the security policy blocks it before the syscall. The config directory is infrastructure plumbing for the Rust daemon, invisible to the agent.

**Security**: System files at `system/` are kernel-level read-only (ConfigMap `subPath` mounts with `readOnly: true`). If the agent writes a fake SOUL.md to the workspace root, personality.rs ignores it — `system/SOUL.md` takes priority (system-dir-first lookup via `ZEROCLAW_SYSTEM_DIR`). A warning is logged for observability.

### 9.2 Environment Variables

The pod's filesystem layout and gateway port are governed by environment variables. The deployment uses Dockerfile defaults for `HOME`, `ZEROCLAW_CONFIG_DIR`, and `ZEROCLAW_WORKSPACE` — no overrides. The only path env var injected by the deployment is `ZEROCLAW_SYSTEM_DIR`.

**Image defaults (NOT overridden by deployment):**

| Variable | Value (Dockerfile) | Purpose |
|---|---|---|
| `HOME` | `/zeroclaw-data` | Root data directory. |
| `ZEROCLAW_CONFIG_DIR` | `/zeroclaw-data/.zeroclaw` | Config directory. Daemon reads `config.toml` here at startup, writes `daemon_state.json` for health probe. |
| `ZEROCLAW_WORKSPACE` | `/zeroclaw-data/workspace` | Working directory for ALL workspace content: agent-created .md files, brain.db, cron, state. PVC mounted here. |
| `ZEROCLAW_GATEWAY_PORT` | `42617` | HTTP gateway listen port. MUST match `[gateway] port` in `config.toml`, `containerPort` in deployment, and `GATEWAY_PORT` constant in `apps/clawcraft/domain/claw-pod-identity.ts`. |
| *(webhook channel port)* | `42618` | Webhook channel listen port (separate from gateway). MUST match `[channels_config.webhook] port` in `config.toml` and `WEBHOOK_PORT` constant in `apps/clawcraft/domain/claw-pod-identity.ts`. |

**Deployment-injected env vars:**

| Variable | Value | Purpose |
|---|---|---|
| `ZEROCLAW_SYSTEM_DIR` | `/zeroclaw-data/workspace/system` | Activates split-mount lookup. personality.rs searches here first for .md files, then falls back to `$ZEROCLAW_WORKSPACE`. ConfigMap subPath overlays mounted here (readOnly). |
| `CLAW_USER_ID` | `{userId}` | Convex user ID. Used by the pod for callback identification. |
| `CLAW_CONVEX_URL` | `{convexUrl}` | Convex deployment URL. Used by the pod for schedule HTTP callbacks. |
| `CLAW_CONVEX_TOKEN` | `{convexServiceToken}` | Container service token. Authenticates pod → Convex HTTP routes. |
| *(no `OTEL_*` env — retired)* | — | **OTLP trace export carries NO pod env.** The SPEC-1 `OTEL_EXPORTER_OTLP_*` trio was vestigial — the zeroclaw emitter ignores OTLP env entirely and reads its config from `config.toml` `[observability]` (verified: `gke.ts` injects no `OTEL_*` env). The single carrier is the `[observability]` block rendered by `buildConfigToml` (`backend`/`otel_endpoint`/`otel_service_name`/`otel_headers`), gated on `LAMINAR_OTLP_ENDPOINT`. See §16.7 and `observability-doctrine.md` §5/§5.1. Re-adding `OTEL_*` env is dead config (observability §10 #3). |

**Net effect vs v1.3.0 topology**: Two env var overrides eliminated (`ZEROCLAW_CONFIG_DIR`, `ZEROCLAW_WORKSPACE`). Three top-level mount dirs (`/system`, `/workspace`, `/zeroclaw-config`) consolidated to two (`/zeroclaw-data/.zeroclaw/`, `/zeroclaw-data/workspace/`). System files moved from a standalone directory mount to subPath overlays inside the PVC workspace. Only env override is `ZEROCLAW_SYSTEM_DIR=/zeroclaw-data/workspace/system`.

#### 9.2.1 Alignment Rules

- **config.toml**: Init container copies from ConfigMap into emptyDir at `$ZEROCLAW_CONFIG_DIR/config.toml` = `/zeroclaw-data/.zeroclaw/config.toml`. Uses the image's default config path — no env var override needed.
- **System files**: ConfigMap `subPath` overlays at `$ZEROCLAW_SYSTEM_DIR/<filename>` = `/zeroclaw-data/workspace/system/<filename>`, each with `readOnly: true`. Contains IDENTITY.md, SOUL.md, AGENTS.md, TOOLS.md, and BOOTSTRAP.md.
- **Workspace (PVC)**: PVC mounted at `$ZEROCLAW_WORKSPACE` = `/zeroclaw-data/workspace/`. Agent creates MEMORY.md, USER.md at the workspace root. brain.db lands at `/zeroclaw-data/workspace/memory/brain.db` — full POSIX locking and mmap, no corruption risk.
- **Gateway port**: `42617` appears in four places that MUST stay in sync: `ZEROCLAW_GATEWAY_PORT` (image), `[gateway] port` in `config.toml` (ConfigMap), `containerPort` (deployment), and `GATEWAY_PORT` in `apps/clawcraft/domain/claw-pod-identity.ts`.
- **Webhook port**: `42618` appears in four places that MUST stay in sync: `[channels_config.webhook] port` in `config.toml` (ConfigMap), `containerPort` (deployment, second port), `WEBHOOK_PORT` in `apps/clawcraft/domain/claw-pod-identity.ts`, and the `claw` service's published ports in `infra/local/docker-compose.yml` (added 2026-05-18 to enable end-to-end webhook testing in local dev). If the standalone `zeroclaw-test` container is running, its `42617/tcp -> 0.0.0.0:42618` mapping conflicts with the new `claw` mapping — stop it first.
- **Local-dev convex site URL override**: `buildConfigToml`'s prod-only `.cloud → .site` derivation is a no-op for `http://host.docker.internal:3210` (no `.convex.cloud` substring), so `infra/scripts/dev/render-claw-config.ts` MUST pass `convexSiteUrl: "http://host.docker.internal:3211"` explicitly. Without it, the pod's outbound `/container-webhook` callback POSTs to :3210 (cloud port, 404s) instead of :3211 (site port — where the httpRouter that owns `/container-webhook` actually lives). Prod callers (`clients/gke.ts`) omit the param and the derivation works fine on `https://*.convex.cloud`.

---

## 10. Message Routing Flows

### 10.1 Web Chat (WS Streaming — Live)

```
User sends message (React UI)
  → useChat hook sends message over WebSocket (via wsManager)
    → WS connection: browser → wss://gw.clawcraft.ca → Nginx (auth_request) → pod /ws/chat
    → Pod streams response tokens over WebSocket
    → Browser renders streaming text in real-time
    → On turn complete:
      → persistUserMessage mutation (write-back to Convex messages table)
      → persistAssistantMessage mutation (write-back to Convex messages table)
      → Convex subscription delivers persisted messages for history
```

### 10.1.1 Web Chat (Convex Relay — Dormant)

The previous web chat path (`messages.sendMessage` → `relay.sendToContainer` → `POST /api/chat`) is no longer used for web chat. It remains active for scheduled task execution via `relayToPod()`.

```
(Dormant for web) messages.sendMessage mutation
  → Schedule relay.sendToContainer
    → POST /api/chat to pod
    → Parse response → insertAssistantMessage (with threadId)
    → Still used by: scheduled task execution
```

### 10.2 Telegram (Native Mode — Live)

```
Telegram user sends message
  → Telegram API delivers via getUpdates (long polling by pod)
  → ZeroClaw's TelegramChannel::listen() processes message
  → Full agent loop (tools, memory, streaming) runs in-pod
  → Pod sends reply directly via Telegram Bot API
  → Message stored in brain.db only (not Convex messages table — see §3.4 persistence gap)
```

### 10.2.1 Telegram (Webhook Mode — Dormant)

Superseded by native mode (§3.4). The webhook relay path (`telegramRelay.ts`) remains in code but is not active — `validateAndSetup` no longer calls `setWebhook`.

```
Telegram user sends message
  → POST /telegram-webhook?userId={userId} (Convex HTTP route)
    → Schedule telegramRelay.handleInbound
      → Validate user exists + integration connected
      → Validate sender authorized (telegram_identities)
      → Insert user message (source: "telegram")
      → Pod running?
        → YES: POST /api/chat → insertAssistantMessage → sendTelegramMessage reply
        → NO:  Schedule podActions.scaleUp (reply lost — see §4.4)
```

### 10.2.2 Email (Gateway Relay — Live)

```
Inbound email arrives
  → CF Email Routing → Cloudflare Worker (parse, attachment upload)
    → POST /email-webhook (Convex HTTP route, Bearer auth)
      → insertInboundEmail (trust decision, idempotent)
      → Schedule emailRelay.handleInbound
        → POST https://gw.clawcraft.ca/relay/{userId} { sender, content } (X-Relay-Token auth)
          → Nginx proxies to pod:42618 /webhook (webhook channel port)
          → Pod processes via webhook channel (no agent loop, fire-and-forget)
          → Pod POSTs response to /container-webhook (send_url) if agent replies
        → Insert relay_log (with responseBody for observability)
        → On failure: retry backoff (5s/30s/2min, max 3 attempts)
```

### 10.2.3 `/container-webhook` Dispatch (Convex HTTP Route)

The pod's webhook channel sends responses back to Convex via the `send_url` configured in `[channels_config.webhook]`. The Convex `/container-webhook` HTTP route dispatches based on the request body shape:

| Condition | Action | Source |
|---|---|---|
| `body.type === "health"` | Health status update | Pod health ping |
| `body.type === "brain_memory_sync"` | Brain memory sync | Pod memory push |
| `body.userId && body.content` | Insert assistant message (gateway response format) | Legacy gateway relay |
| `!body.type && !body.userId && body.content` | Insert assistant message with `source: "webhook"` | Channel webhook response (email relay, webhook channel) |

The channel webhook response case (`!body.type && !body.userId && body.content`) is the path taken when the pod's webhook channel processes an inbound message (e.g. from the email-relay gateway path) and the agent produces a reply. The pod POSTs the response to `/container-webhook` with body `{ content, recipient, thread_id? }` — no `type`, no `userId`. The handler verifies the static `Authorization: Bearer ${CONTAINER_SERVICE_TOKEN}` (deployment-wide secret; same for every pod), reads the user from `body.recipient` (cast to `Id<"users">`), and inserts the assistant message with `source: "webhook"`. The `thread_id` field is currently informational only — the assistant message is inserted against the user, not threaded.

**Pod-side auth header (2026-05-18 follow-up).** The pod's outbound `Authorization` header was previously missing, producing 401 at `/container-webhook` for every async reply. `buildConfigToml` now emits `auth_header = "Bearer ${containerServiceToken}"` inside `[channels_config.webhook]` so the pod reads the literal token from `config.toml` (sibling of the existing `[gateway].pre_shared_token`). The token is a deployment-wide secret — same trade as the Telegram bot token plaintext-in-ConfigMap call (§credential isolation note in §2.3) — and is sourced from `process.env.CONTAINER_SERVICE_TOKEN` at pod-provision time via `requireContainerServiceToken()` in `clients/gke.ts`, which fails loud at provision if the env var is missing. See investigation 2026-05-18 (Convex log line `container-webhook 401 {hasToken:false,...}`) for the original detection.

**Rejection-site observability rule (binding).** Convex HTTP routes that the pod POSTs to MUST emit a structured `console.warn` line at every 4XX rejection site. The line MUST: (a) tag the route + status code (e.g. `"container-webhook 401"`); (b) include enough body/header surface area to root-cause the rejection (key presence flags, header lengths, token suffixes — NEVER full token values, NEVER full body content because of PII); and (c) NOT consume body content beyond what's needed for the flag set. Reference implementation: `apps/clawcraft/convex/http.ts` `/container-webhook` (shipped 2026-05-18 commit `dfb3d42`). Rationale: before this rule, `/container-webhook` rejected silently and the prod 401 took multi-turn instrumentation to isolate; with the rule, a single grep against Convex dashboard logs identifies the failure mode in one trip. **MUST** apply to any new pod→Convex HTTP handler added under §3.6's "Convex-initiated control endpoint class" inversion (pod→Convex direction).

### 10.3 Queued Delivery (After Wake-Up)

```
pollHealth succeeds
  → Schedule relay.deliverQueued
    → Query messages with status: "pending"
    → For each: schedule relay.sendToContainer
      → POST /api/chat → insertAssistantMessage
      → NOTE: No channel reply routing (web OK via subscription, Telegram/etc. lost)
```

---

## 11. Pairing & Identity Model

### 11.1 1:1 Bot-to-Owner

Each Telegram integration has one owner. The owner is paired automatically during the connect flow.

### 11.2 Auto-Pair Flow

```
User pastes bot token in dashboard
  → connectTelegram mutation (status: "connecting")
    → Schedule validateAndSetup action
      → validateBotToken with Telegram API
      → deleteWebhook (clears any stale webhook so pod can use long polling)
      → Auto-generate owner pairing code + deep link
      → Store deep link in integration config
      → Update ConfigMap (with [channels_config.telegram] containing bot token), restart pod
  → Dashboard shows "Open in Telegram to pair" CTA
  → User clicks deep link → Telegram opens → /start CLAW-XXXX
    → consumePairingCode → telegram_identities row created (role: "owner")
    → Dashboard reactively shows owner info
```

### 11.3 Sender Authorization

Every inbound Telegram message is checked against `telegram_identities`:

```typescript
const isAuthorized = telegramUsers.some(
  (u) => u.telegramChatId === chatId ||
    (senderUsername && u.telegramUsername === senderUsername),
);
if (!isAuthorized) return; // Silent drop
```

### 11.4 Identity Cleanup on Disconnect

`telegram_identities` are **deleted** on disconnect (`disconnectTelegram`). This ensures a clean "state zero" — no stale identities with wrong `chatId` format or wrong `role` can block re-pairing. Fresh identities are created via `consumePairingCode` on the next connect + pair flow. Full account deletion also cleans them up via `USER_OWNED_TABLES` registry.

### 11.5 App Integration Lifecycle (Composio)

All Composio OAuth integrations share a generic connect/disconnect lifecycle:

Two flows coexist:

**Google Sheets** (dedicated flow, unchanged from parent spec):
- `startGoogleSheetsOAuth` → `initiateComposioOAuth` → `completeGoogleSheetsOAuth`
- Uses hardcoded `COMPOSIO_AUTH_CONFIG_GOOGLE_SHEETS` env var
- `disconnectGoogleSheets` deletes Composio connection + integration row + restarts pod

**All other Composio integrations** (generic flow):
- `startComposioOAuth` → `initiateGenericComposioOAuth` → `completeGenericComposioOAuth`
- Uses `COMPOSIO_AUTH_CONFIGS[provider]` registry lookup
- `disconnectComposioIntegration` accepts any supported provider
- OAuth callback routes by `provider` query param: present → generic, absent → Sheets

**Generic connect flow**:
1. Mutation creates/updates `integrations` row with `status: "connecting"` and `oauthState`.
2. `initiateGenericComposioOAuth` looks up auth config env var from `COMPOSIO_AUTH_CONFIGS[provider]`, calls Composio API, updates integration with `redirectUrl`.
3. Frontend redirects user to Composio OAuth consent screen.
4. `/composio-oauth-callback` extracts `provider` from query params, calls `completeGenericComposioOAuth`.
5. `completeGenericComposioOAuth` verifies ACTIVE status, marks connected, schedules pod restart.

**Supported generic providers**: Gmail (`gmail`), Google Docs (`google_docs`), Google Slides (`google_slides`), Google Calendar (`google_calendar`), LinkedIn (`linkedin`), Notion (`notion`), Calendly (`calendly`).

The UI transitions reactively via `getByProvider` Convex subscription.

### 11.6 Composio Integration Registry

**Auth config mapping** (`COMPOSIO_AUTH_CONFIGS` in `apps/clawcraft/convex/integrationActions.ts`):

| Provider | Env Var |
|---|---|
| `gmail` | `COMPOSIO_AUTH_CONFIG_GMAIL` |
| `google_calendar` | `COMPOSIO_AUTH_CONFIG_GOOGLE_CALENDAR` |
| `google_docs` | `COMPOSIO_AUTH_CONFIG_GOOGLE_DOCS` |
| `google_slides` | `COMPOSIO_AUTH_CONFIG_GOOGLE_SLIDES` |
| `linkedin` | `COMPOSIO_AUTH_CONFIG_LINKEDIN` |
| `notion` | `COMPOSIO_AUTH_CONFIG_NOTION` |
| `calendly` | `COMPOSIO_AUTH_CONFIG_CALENDLY` |

Google Sheets uses its own dedicated env var `COMPOSIO_AUTH_CONFIG_GOOGLE_SHEETS` (not in the registry).

**App name mapping** (`COMPOSIO_APP_NAMES` in `apps/clawcraft/domain/claw-workspace.ts`):

| Provider | Composio App Name |
|---|---|
| `gmail` | gmail |
| `google_calendar` | googlecalendar |
| `google_docs` | googledocs |
| `google_sheets` | googlesheets |
| `google_slides` | googleslides |
| `linkedin` | linkedin |
| `notion` | notion |
| `calendly` | calendly |

**Per-app capability hints** (`COMPOSIO_APP_HINTS` in `apps/clawcraft/domain/claw-workspace.ts`):

Each connected app gets a one-liner in TOOLS.md describing what it's FOR and critical gotchas. This prevents the agent from misusing apps (e.g., using Calendly to create calendar events). Key hints:

| App | Hint |
|---|---|
| calendly | Scheduling links and availability only. CANNOT create events — use googlecalendar. |
| gmail | Confirm with user before sending. |
| googlecalendar | The tool for scheduling events at specific dates/times. |
| googlesheets | Limit reads to 200 rows. |
| linkedin | Post content, comment, read profile, view analytics. Always confirm before publishing. |
| notion | Has pages AND databases with typed properties. |

TOOLS.md also includes a "Sending caution" rule: never send emails (Gmail) or create public scheduling links (Calendly) without explicit user confirmation.

All integrations are per-user OAuth (one `integrations` row per user per provider).

---

## 12. ConfigMap Lifecycle

### 12.1 Contents

The ConfigMap (`claw-{userId}-config`) contains:

- `config.toml` — Gateway, autonomy, LLM, tool policy, HTTP request domain restriction, tool timeout overrides (`[http_request]`, `[web_search]`, `[web_fetch]` all at 120s), always-present `[channels_config]`/`[channels_config.webhook]` (port 42618, secret omitted), conditional `[channels_config.telegram]` (present when Telegram integration is connected), always-present `[observability]` block (`backend="log"` LogObserver by default; `backend="otel"` + `otel_endpoint`/`otel_service_name`/`otel_headers` when `LAMINAR_OTLP_ENDPOINT` is set — the `otel_headers` Bearer key is a **plaintext credential literal**, §6.1 third plaintext-ConfigMap exception)
- Workspace files — IDENTITY.md, SOUL.md, USER.md, AGENTS.md, TOOLS.md, MEMORY.md, conditional BOOTSTRAP.md

### 12.2 Regeneration Triggers

| Event | What Changes | Action |
|---|---|---|
| `scaleUp` (pod wake-up) | Workspace files (persona + memories may have changed) | `updateConfigMap` → `ensureService` + `ensureNetworkPolicy` → `restartDeployment` |
| Integration connect/disconnect | Config changes (Telegram: `[channels_config.telegram]` added/removed) | `updateConfigMap` → `ensureService` + `ensureNetworkPolicy` → `reconcileDeployment` (rolling update) |
| Persona onboarding complete | BOOTSTRAP.md removed, identity/soul/user files updated | `updateConfigMap` → `restartDeployment` |
| Plan change | Autonomy limits change | `updateConfigMap` → `restartDeployment` |

### 12.3 Rules

- MUST regenerate ConfigMap before starting the pod — ZeroClaw reads `config.toml` once at startup.
- MUST use create-or-replace pattern (not patch) to reconcile volume items when BOOTSTRAP.md presence changes.
- MUST reconcile the full resource set (ConfigMap, Service, NetworkPolicy, Deployment) on every pod lifecycle path — `provisionUser`, `scaleUp`, and `restartPodForIntegration`. `ensureService` and `ensureNetworkPolicy` (exported from `apps/clawcraft/convex/clients/gke.ts`) MUST be called alongside `updateConfigMap` so that port changes and network ingress rules propagate on pod restarts, not only on initial provision.
- MUST restart deployment (annotation-based rollout or scale down/up) after ConfigMap update. The init container copies `config.toml` from the ConfigMap volume to the writable emptyDir on every pod start — restart is required for config changes to take effect.
- `defaultMode: 0644` on volume mount (container runs as non-root; 0600 causes permission denied).

---

## 13. Trust Boundaries

| Boundary | Mechanism | Credential Location |
|---|---|---|
| Browser → Nginx WS gateway | Convex session JWT via `?token=` query param | Convex Auth (session token from `useAuthToken()`) |
| Nginx → Convex (`/api/ws-auth`) | Session JWT forwarded for validation | — (stateless validation) |
| Nginx → ZeroClaw pod | No auth (`require_pairing=false`, ClusterIP) | — (cluster-internal only) |
| Convex → ZeroClaw pod | Pre-shared bearer token (per-user) | Convex DB `users.preSharedToken` |
| ZeroClaw pod → Convex | Container service token | Pod env var `CLAW_CONVEX_TOKEN`, Convex env var `CONTAINER_SERVICE_TOKEN` |
| Convex → Telegram API | Bot token (encrypted at rest) | Convex DB `integrations.config.botTokenEncrypted` |
| ZeroClaw pod → Telegram API | Bot token (plaintext in config.toml, native mode §3.4) | ConfigMap `[channels_config.telegram]` |
| Convex → GKE API | Service account key (JWT → OAuth2) | Convex env var `GKE_SERVICE_ACCOUNT_KEY` |
| ZeroClaw → OpenRouter | API key in config.toml | ConfigMap (per-user or platform key) |
| CF Email Routing → Cloudflare Worker | Cloudflare internal routing (same account, no external auth needed) | CF Email Routing configuration |
| Cloudflare Worker → Convex | Bearer token on `/email-webhook`, `/email-upload-url` | Worker secret `CONVEX_WEBHOOK_SECRET` / Convex env var `EMAIL_WEBHOOK_SECRET` |
| Convex → Nginx gateway (relay) | `X-Relay-Token` header on `/relay/{userId}` | Convex env var `GATEWAY_RELAY_TOKEN` / Terraform var `gateway_relay_token` (Nginx ConfigMap) |
| Nginx gateway → Pod (`/webhook` on port 42618) | No HMAC (secret omitted); auth is nginx `auth_request` (X-Relay-Token) + NetworkPolicy (cluster-internal only) | — |
| Internet → Pod | LoadBalancer on port 42617, pre-shared bearer token | — |
| `clawcraft-system` → Pod | NetworkPolicy allows ingress from gateway namespace | — (cluster-internal) |
| Pod → Pod | Blocked by NetworkPolicy | — |

---

## 14. Known Gaps & Future Work

| ID | Gap | Severity | Description |
|---|---|---|---|
| ~~rnk-7iol~~ | ~~PVC mount path mismatch~~ | ~~P0~~ | Fixed: PVC now mounts at `/zeroclaw-data/workspace/memory/`. |
| ~~rnk-veaw~~ | ~~Memory tools excluded from non-CLI~~ | ~~P1~~ | Fixed: `memory_store`/`memory_forget` enabled and auto-approved on all channels. |
| — | deliverQueued reply routing | P2 | Queued messages delivered after wake-up don't route replies back to external channels (Telegram). Web unaffected. |
| ~~—~~ | ~~Webhook URL race on disconnect/reconnect~~ | ~~P3~~ | Moot: native Telegram mode uses long polling, not webhooks. `validateAndSetup` calls `deleteWebhook` to clear stale webhooks, but never sets a new one. |
| — | session_id for per-thread memory | P3 | HTTP relay omits session_id for global memory recall. Web chat includes `[conversationId: {threadId}]` tag in message body for media organization. Future: evaluate per-thread memory scoping when multi-thread Telegram support is added. |

**Pod cron durability — VERIFIED LIVE (Phase 0, praxis-hardening 0.11.0).** Earlier worry that the ZeroClaw scheduler was in-memory (which would have made the §5.7.1 cron-arming optimization unreliable across restarts) is **DISPROVEN**. Verified facts — record as load-bearing guarantees, not gaps:

- **Agent-cron fires the FULL agent loop.** `zeroclaw cron once <delay> '<prompt>' --agent` (and `cron add-at <RFC3339> --agent`) fires into the full agent loop — a Laminar `agent.activation` root span + live tools, not a bare prompt eval. (Observed: the fired agent ran `praxis manifest show execute` to orient toward resuming.) One-shot and at-datetime scheduling are both supported.
- **Disk-backed, on the PVC workspace volume.** Cron jobs PERSIST in SQLite at `/zeroclaw-data/workspace/cron/jobs.db` — on the **PVC-mounted workspace volume**, NOT the container's writable layer. They therefore survive a process restart, a `dev:claw:up` recreate, AND prod pod restarts, as long as the workspace volume is mounted.
- **Startup catch-up replay.** The scheduler runs a startup catch-up pass (log line `Scheduler startup: catching up overdue jobs`) that replays a fire missed during downtime — a second, runtime-level safety net beneath the AGENTS.md due-scan (§5.7.1).

---

## 15. Tradeoffs

### 15.1 Always-On Pod Policy

Pods are no longer auto-scaled to zero on idle. The idle scale-down cron has been removed. Pods remain running (`replicas: 1`) indefinitely once provisioned. User-controlled sleep (manual scale-down from dashboard) is planned but deferred.

**Rationale:** WebSocket connections require a running pod. Auto scale-down would break active WS sessions and require reconnection logic. With the WS gateway, pods are always reachable at their ClusterIP — keeping them running simplifies the connection model.

**Impact:** Pod compute cost is no longer proportional to active usage. Cost model (§11 in infra-doctrine) should be re-evaluated for always-on billing.

### 15.2 Convex as Single Point of Failure

Convex is the single point of failure for all channels. If Convex is down, no channel works — including web chat (WS auth fails), Telegram, and any future channels.

This is acceptable because:
- Convex has strong uptime guarantees (it is the app platform)
- The alternative is duplicating routing logic in both Convex and ZeroClaw
- Clawcraft already depends on Convex for web chat auth, user auth, billing — it is already the SPOF

### 15.3 Channel-Agnostic Pod (With Telegram Exception)

The pod has zero channel awareness for most channels. Telegram is the exception (§3.4). This means:
- **Pro**: Adding a non-Telegram channel never requires pod changes, image rebuilds, or config updates
- **Pro**: All observability (logging, rate limiting, billing) is unified in Convex for relay-mode channels
- **Pro**: Telegram native mode gives richer agent experience (streaming, cancellation, per-message timeouts)
- **Con**: Pod cannot initiate outbound messages to relay-mode channels (only respond)
- **Con**: Native Telegram messages are not visible in Convex `messages` table (persistence gap)
- **Con**: Channel-specific rich features (reactions, typing indicators, file attachments) require Convex-side handling for relay channels

---

## 16. Pod Operations Runbook

Quick reference for Claude Code agents and operators managing live pods.

### 16.1 What Requires a Pod Restart

| Change | Hot (no restart) | Restart Required |
|---|---|---|
| Convex function code (relay, actions, mutations) | Yes — auto-deployed by Convex | — |
| Workspace file templates (`apps/clawcraft/domain/claw-workspace.ts`) | — | Yes — pod reads at startup |
| config.toml settings (tool policy, plan limits) | — | Yes — ZeroClaw reads once at startup |
| Deployment spec (volume mounts, env vars, probes, image) | — | Yes — but auto-reconciled on next `scaleUp` |
| Telegram bot token (native mode) | — | Yes — token is in config.toml, pod reads once at startup |

### 16.2 Redeploying a Single Pod

```bash
# Scale down then up — scaleUp rebuilds ConfigMap + reconciles deployment spec
pnpx convex run podActions:scaleDown '{"userId": "<userId>"}'
pnpx convex run podActions:scaleUp '{"userId": "<userId>"}'
```

Add `--prod` for production. The `scaleUp` action:
1. Reads current persona + memories from Convex DB
2. Rebuilds workspace files from current `claw-workspace.ts` templates
3. Replaces ConfigMap (atomic, full replace)
4. Reconciles Service and NetworkPolicy via `ensureService` + `ensureNetworkPolicy`
5. Sends full deployment spec via `reconcileDeployment` (fixes any drift)
6. Scales replicas 0 → 1
7. Polls health until pod is ready, then delivers queued messages

### 16.3 Redeploying All Running Pods

```bash
# Find all running pods
for NS in $(kubectl get ns | grep ^claw- | awk '{print $1}'); do
  USERID=$(echo $NS | sed 's/^claw-//')
  REPLICAS=$(kubectl get deployment -n $NS -o jsonpath='{.items[0].spec.replicas}' 2>/dev/null)
  [ "$REPLICAS" = "1" ] && echo "$USERID"
done

# Then for each userId:
pnpx convex run podActions:scaleDown '{"userId": "<userId>"}'
pnpx convex run podActions:scaleUp '{"userId": "<userId>"}'
```

### 16.4 Debugging a Pod

Since sovereign-fork commit `1aac3308` (`@soulbound-labs/praxis` install
landed in the release image), the runtime is **Wolfi-base with bash +
coreutils + vim + git + nodejs** — no longer distroless. The previous
`kubectl debug` + busybox-sidecar dance is obsolete; just exec in:

```bash
NAMESPACE="claw-<userId>"
POD=$(kubectl get pods -n $NAMESPACE -o jsonpath='{.items[0].metadata.name}')
kubectl exec -it $POD -n $NAMESPACE -- bash
```

`vim` and `git` are in-image for live inspection of brain.db / workspace
/ config without leaving the pod. The legacy busybox-sidecar pattern is
only needed if a future Stage-3 release stage strips bash again — at
that point, restore this section's previous wording from git history.

From inside the pod, verify config override is working:

```sh
# Config at default path (init container + emptyDir)
cat /proc/1/root/zeroclaw-data/.zeroclaw/config.toml  # should contain timeout_secs = 120
ls /proc/1/root/zeroclaw-data/.zeroclaw/               # should show .secret_key after startup

# Env vars
cat /proc/1/environ | tr '\0' '\n' | grep ZEROCLAW    # ZEROCLAW_SYSTEM_DIR (only path override)

# Workspace files
ls /proc/1/root/zeroclaw-data/workspace/*.md           # MEMORY.md, USER.md, etc.
ls /proc/1/root/zeroclaw-data/workspace/system/*.md    # IDENTITY.md, SOUL.md, AGENTS.md, TOOLS.md, BOOTSTRAP.md

# Brain DB (copy all three files for WAL consistency)
cp /proc/1/root/zeroclaw-data/workspace/memory/brain.db /tmp/brain.db
cp /proc/1/root/zeroclaw-data/workspace/memory/brain.db-wal /tmp/brain.db-wal 2>/dev/null
cp /proc/1/root/zeroclaw-data/workspace/memory/brain.db-shm /tmp/brain.db-shm 2>/dev/null
```

Key paths via `/proc/1/root/`:
- Config (active): `/zeroclaw-data/.zeroclaw/config.toml` (emptyDir, copied by init container)
- System files: `/zeroclaw-data/workspace/system/IDENTITY.md`, `SOUL.md`, etc. (ConfigMap subPath, readOnly)
- Workspace files: `/zeroclaw-data/workspace/MEMORY.md`, `USER.md`, `HEARTBEAT.md`, etc.
- Brain DB: `/zeroclaw-data/workspace/memory/brain.db` (+ WAL files — always copy all three)
- Pod env vars: `cat /proc/1/environ | tr '\0' '\n'`

**Important**: By default ZeroClaw logs nothing for `/api/chat` requests. Empty pod logs does NOT mean no traffic. Enable `[observability]` (see §16.7) for agent loop logging, or verify via brain.db timestamps.

### 16.5 Checking Pod Health

```bash
# From outside the cluster (via LoadBalancer)
curl http://<containerEndpoint>/health

# From inside via debug container
kubectl exec $POD -n $NAMESPACE -c debugger-xxxxx -- \
  curl -s http://localhost:42617/health
```

### 16.6 Full Brain Debugging Protocol

For comprehensive memory diagnostics, run `tbd shortcut claw-brain-debugging` **from the operator's host shell at the clawcraft repo root** — NOT inside the pod. `tbd` is not installed in the runtime image (the only npm CLI that lands is `@soulbound-labs/praxis` at `/opt/praxis/`; see §15.24 in infra-doctrine). The shortcut is a host-side diagnostic harness; the protocol it prints covers: baseline snapshot, per-channel write/recall tests, cross-channel consistency, PVC mount verification, and a MECE diagnostic matrix.

### 16.7 Observability & Agent Loop Logging

> **Two distinct "observability" surfaces — both live in `config.toml`, distinguished by
> which keys are present (NOT env-vs-toml).** Both surfaces are rendered into the same
> `[observability]` block by `buildConfigToml`:
> - **`backend = "log"` — the in-pod LogObserver** (local agent-loop logging: JSONL events,
>   Live Logs, `kubectl logs`). Active when no OTLP endpoint is configured.
> - **`backend = "otel"` + `otel_endpoint` / `otel_service_name` / `otel_headers` — OTLP
>   trace export to Laminar**, governed by `observability-doctrine.md`. Active when
>   `LAMINAR_OTLP_ENDPOINT` is set (dev: local self-host; prod: managed cloud, TLS).
>
> The selector is the `otel_endpoint` gate — set ⇒ `backend="otel"` exporting, unset ⇒
> `backend="log"` local-only. There are **no `OTEL_*` pod env vars** — that SPEC-1 env trio
> was vestigial and is retired (the emitter ignores OTLP env; §9.2, observability §5/§5.1).
> The OTLP auth key rides `otel_headers` as a plaintext literal (§6.1 third
> plaintext-ConfigMap exception), never pod env.

ZeroClaw does **not** emit request-path logs by default (verified through v0.1.8-alpha-p2; behaviour assumed to hold for current v0.6.9-alpha-p10 but not re-verified — re-check if you bump and the logging behaviour changes). `RUST_LOG=zeroclaw=trace` enables DEBUG-level output (plugin init, config load) but does NOT produce per-request or agent loop traces — the request handling code path has no `tracing` instrumentation at this level.

To get agent loop visibility, configure the `[observability]` section in `config.toml`:

```toml
[observability]
backend = "log"                    # Activates LogObserver
runtime_trace_mode = "rolling"     # Writes JSONL events to state/runtime-trace.jsonl
runtime_trace_max_entries = 500    # Max entries before rolling
```

**What `backend = "log"` emits** (via `tracing::info!`):
- `Agent_start` — model, provider
- `Llm_request` — model, provider (marks outbound LLM call)
- `Agent_end` — model, provider, duration_ms, cost_usd, tokens_used
- Tool events: `tool.start`, `tool.call`

These events are visible in the ZeroClaw web dashboard (Live Logs) and in `kubectl logs`.

**What `runtime_trace_mode = "rolling"` emits** (structured JSONL):
- All of the above plus tool call parameters and LLM timing
- Written to `state/runtime-trace.jsonl` inside the container
- Requires a volume mount or `kubectl debug` to read

**Key debugging pattern**: If a request returns 408 (gateway timeout), check the dashboard Live Logs for the sequence:
1. `Agent_start` + `Llm_request` — first LLM call (tool selection)
2. `Agent_end` — first call completes (note `duration_ms`)
3. `Agent_start` + `Llm_request` — second LLM call (e.g., summarizing tool results)
4. Missing `Agent_end` = **the LLM call hung** until the gateway timeout fired

**RUST_LOG**: Set `RUST_LOG=zeroclaw=trace` as a pod env var for maximum ZeroClaw library debug output. This is orthogonal to `[observability]` — both should be enabled during debugging. Note: ZeroClaw may compile with a max log level that caps TRACE output; DEBUG lines will still appear.

**Current Clawcraft config**: Both `[observability]` and `RUST_LOG=zeroclaw=trace` are enabled by default in `buildDeploymentSpec` and `buildConfigToml` respectively. The `[observability]` section is unconditional (not gated on plan or feature flag).

### 16.8 Spec Validation Flow Debugging

The praxis validation pipeline (praxis-doctrine §9 v1.9.1, backend-doctrine §3.5 v2.33.0) fans across three triggers and lands rows in four Convex tables. When something looks wrong, this is the order to check:

**Step 1 — Verify the trigger fired**:
**Rewritten for Spec 4b (v2.31.0).** The pre-4b flow assumed the validation pipeline ran Convex-side via `internal.specValidationActions.runValidation` and that `postValidate` made an HTTPS call to a dedicated `/api/validate/spec` Rust handler. Spec 4b collapsed both: validation now flows through `/webhook` `{message}` with a `[system]`-prefixed agent directive (see §5.6.1). The runbook below reflects the post-4b topology.

**Step 1 — Was the trigger received?**

```bash
# Did the V8 mutation fire?
npx convex logs --component scheduler | grep "specValidation:runValidation" | tail -20

# Or query for recent rows:
npx convex run specValidation:latestForSpec '{"specId":"<id>"}'
```

If no `spec_validations` row exists AND no scheduled function fired, the trigger never reached Convex. Re-check:
- **Validate button**: `api.specValidation.requestValidation` requires a logged-in owner of the spec. If the button is greyed out, see Step 3.
- **Projection trigger**: was the projection `kind: "applied"` (vs. `duplicate` / `stale`)? Look at `commits` table.
- **No `integrations_change` trigger anymore** — the listener wrapper was retired in Spec 4b. If you connect/disconnect an integration, the spec is NOT auto-revalidated; the user must click Validate.

**Step 2 — Cache hit vs cache miss?**

Cache hit (fast path): `spec_validations` row appears within ~50ms with `stagesRun: {structure: false, tools: false, semantic: true}` + `semanticSkipReason: "cache_hit"`. No webhook fired. Confirm via:

```bash
# Was the semantic-cache lookup a hit?
npx convex run specValidation:lookupSemanticCache \
  '{"userId":"...","specContentHash":"<sha256>","modelVersion":"anthropic/claude-haiku-4.5"}'
```

Cache miss (slow path): `runValidation` schedules `internal.webhookDispatch.sendValidationWebhook`. Confirm via:

```bash
npx convex logs --component actions | grep "webhookDispatch" | tail -20
```

**Step 3 — Webhook dispatch outcome**:

Inspect the action's `WebhookDispatchOutcome`:
- `{ outcome: "ok" }` → pod accepted the `[system]`-prefixed message. Move to Step 4.
- `{ outcome: "skipped", reason: "pod_unavailable" }` → `users.containerEndpoint` was absent OR the network call failed. Under the §15.1 always-on policy there is no idle scale-to-zero — likely causes are provisioning-not-yet-complete, a crashed pod, the brief window of a manual redeploy (`scaleDown`→`scaleUp`), or a DNS/LB issue.
- `{ outcome: "skipped", reason: "spec_unthreaded" }` → spec has no `threadId` (pre-thread fixture row). Cannot run `praxis spec verify --thread <id>`.
- `{ outcome: "skipped", reason: "spec_missing" }` → spec row deleted between schedule and action fire.
- `{ outcome: "skipped", reason: "http_<status>" }` → pod responded non-2xx (e.g., `http_503` = pod briefly unavailable, typically mid-provision or during a manual redeploy — NOT idle scale-to-zero, which the §15.1 always-on policy removed; `http_404` = `/webhook` not found, which would indicate a ZeroClaw deploy regression).
- `{ outcome: "skipped", reason: "timeout" }` → `> ZEROCLAW_WEBHOOK_TIMEOUT_MS` (default 30s).

**Step 4 — Did the agent run the embedded command?**

```bash
# Inspect agent runtime trace for the [system] directive + the praxis invocation:
kubectl exec <pod> -- cat /zeroclaw-data/workspace/runtime-trace.jsonl \
  | grep -E "\[system\]|praxis spec verify"
```

If the agent received the directive but didn't run praxis:
- **Agent ignored the prefix**: the agent replied conversationally instead of running the command. No retry logic in v1 — user re-clicks Validate. Consider reinforcing `[system]` semantics in the pod's system prompt (out of scope for Spec 4b; tracked as v2 hardening per §5.6.1).
- **Praxis CLI error**: agent ran the command but `praxis` exited non-zero (e.g., unknown flag `--persist` → ZeroClaw image didn't rebuild with `PRAXIS_VERSION=0.6.1`). Verify: `kubectl exec <pod> -- praxis --version`.

**Step 5 — Did the projection land the findings?**

`praxis spec verify --persist` writes `<dataSync>/validations/<id>.json` and fires the auto-sync commit. The next git push triggers the projection emitter, which carries the row as `ProjectionValidationChange` in the payload's `validations[]` array.

```bash
# Was a projection POST received?
npx convex logs --component http | grep "/praxis-link/projection" | tail -10

# Was reconcileValidations called?
npx convex logs --component mutations | grep "reconcileValidations" | tail -10
```

Common failure: `praxis-link spec not found, skipping` warn line in `reconcileValidations` → the `specPath` in the validation row didn't match any spec the projection had just landed. Self-corrects on the next projection.

**Step 6 — Prior_work bead reconciliation**:

```bash
npx convex run specValidation:priorWorkBeadsForSpec '{"specId":"..."}'
```

Each open bead carries the fingerprint that produced it. When the user fixes the underlying gap (e.g., connects the missing tool) and re-clicks Validate, the next `reconcileValidations` archives stale beads via the inlined `closeStalePriorWorkBeadsInline` helper. If beads don't archive, check that the `validationFingerprint` substring (`|<specId>|`) is present.

### 16.9 Spec Execution Flow Debugging

**Rewritten for Spec 4b (v2.31.0).** The pre-4b flow assumed `requestStart` called a dedicated `/api/execute/start` Rust handler via `postExecuteStart` and eagerly inserted the `executions` row on the daemon's `ok` response. Spec 4b collapsed both: `requestStart` now dispatches via `internal.webhookDispatch.sendExecutionWebhook` (a `[system]`-prefixed `/webhook` directive), the pod's agent runs `praxis spec activate-execution`, and the row lands lazily via the projection emitter (`reconcileExecutions`).

**Step 1 — Verify the action accepted**:

```bash
# Most-recent execution row for the spec (running-first; falls back to most-recent completed):
npx convex run specExecution:latestForSpec '{"specId":"<specId>"}'
```

- Returns `null` → no row ever inserted. Either the action returned `{ outcome: queued }` and the projection hasn't landed the row yet (common — wait 10–30s) OR the action returned `{ outcome: skipped }` (check Step 2) OR the spec hasn't been run.
- `status: "in_flight"` → projection consumer landed the row from `praxis spec activate-execution`'s auto-commit. Agent runtime is walking the factory.
- `status: "completed"` → terminal. `terminalReferenceBeadId` should resolve to the output bead.

**Step 2 — Webhook dispatch outcome**:

```bash
# Live-tail action logs to see the sendExecutionWebhook outcome:
npx convex logs --component actions | grep -E "specExecution|sendExecutionWebhook" | tail -10
```

`requestStart` returns three shapes (see backend-doctrine §3.6):
- `{ outcome: { status: "queued" } }` → webhook dispatched successfully; awaiting projection. Normal happy path.
- `{ outcome: { status: "skipped", reason } }` → dispatch failed; UI surfaces `executionPodUnavailable()`. Reasons mirror §16.8 Step 3 (`pod_unavailable`, `http_<status>`, `timeout`, `spec_missing`).
- `{ alreadyInFlight: true, praxisExecutionId }` → rate-limit hit (see Step 4).

**Step 3 — Auth + ownership + active gate**:

The action throws three distinct errors before reaching dispatch — if the user sees `executionSpecInactive()` copy, the spec needs Activate (§3.5.1 + claw-system-state-machine §8.1):

- `UNAUTHORIZED` → user is unauthenticated or tombstoned.
- `NOT_FOUND` → spec doesn't exist OR caller is not the owner.
- `INVALID_STATE: spec_inactive` → `specs.active !== true`. User clicks Validate → Activate first.

**Step 4 — Per-(user, spec) rate-limit hit**:

The action returns `{ alreadyInFlight: true }` + the existing `praxisExecutionId` when an in-flight row exists for the (user, spec) pair. UI surfaces `executionAlreadyInFlight()` copy. The daemon-side mutex (praxis-doctrine §10.6) is the dedup source of truth; this Convex-side check is advisory.

**Step 5 — Did the agent run `praxis spec activate-execution`?**

```bash
kubectl exec <pod> -- cat /zeroclaw-data/workspace/runtime-trace.jsonl \
  | grep -E "\[system\]|praxis spec activate-execution"
```

Same agent-recognition + praxis-version concerns as §16.8 Step 4. The `--trigger-payload '<json>'` arg is single-quoted with `'\''` POSIX escapes; if the payload contains characters that break the agent's shell tool's quote parsing, the praxis CLI errors and the agent reports to chat. v2: HMAC-signed directives + strict payload escaping audit.

**Step 6 — Projection consumer (lazy row insert)**:

`praxis spec activate-execution` auto-commits the `executions/<id>.json` row to the praxis-sync worktree. The next git push triggers the projection emitter, which carries the row in `payload.executions[]`. `praxisLink.reconcileExecutions` upserts it on `by_praxis_execution_id` AFTER `reconcileSpecChanges`. Common warns:

- `praxis-link execution: spec not found, skipping` → the projection emitted an execution before its spec landed. Self-corrects on the next projection.
- `praxis-link execution: terminal bead not found, leaving unset` → daemon emitted `completed` before the projection's `beads[]` array materialized the terminal bead. Row patches with `terminalReferenceBeadId: undefined`; surfaces in the UI as "completed without terminal output reference".

**Step 7 — Beads tagged with the execution**:

```bash
npx convex run specExecution:beadsForExecution '{"praxisExecutionId":"01HXY..."}'
```

v1 implementation is collect-then-filter on `beads.metadata.executionId`. If the bead count grows past the React-render budget, `praxis-spec-execution-ui` brief adds a `by_user_execution_id` index per backend OQ2.

### 16.10 Parked Execution Debugging (async walks, praxis-hardening 0.11.0)

A walk that "stopped without finishing" is usually **parked** (§5.7.1), not failed — it returned `next_action: null` + `data.parked` and is waiting at a human / timer / event boundary. Inspection surface, pod-side:

```bash
# 1. Which executions are in-flight (parked walks show here until resumed/closed):
kubectl exec <pod> -- praxis execute --list

# 2. The parked snapshot on the execution row — kind, dueAt, the waiting beads:
#    (look for data.parked.kind ∈ {timer, human, event}; for timer, data.parked.dueAt)
kubectl exec <pod> -- praxis execute --execution <id> --json   # re-entry is also the resume

# 3. Is the resume cron armed? (timer parks only)
kubectl exec <pod> -- zeroclaw cron list

# 4. The disk-backed cron store — survives restarts (§14); confirm the one-shot row exists:
kubectl exec <pod> -- sqlite3 /zeroclaw-data/workspace/cron/jobs.db 'SELECT * FROM jobs;'
```

Triage by `data.parked.kind`:

- **`timer`** — compare `data.parked.dueAt` against now. If `dueAt` has passed but the walk is still in-flight, the cron fire was missed (or the pod was asleep across it). The fix is NOT manual intervention — the AGENTS.md due-scan (§5.7.1) resumes it on the next activation; send any message to the pod, or wait for the §14 startup catch-up pass to replay on the next pod start. **Overdue-timer hint:** an in-flight `timer` park whose `dueAt` is in the past + no message since = waiting on the next wake; this is the EVENTUAL SLA working as designed (§5.7.1), not a hang.
- **`human`** — the walk is correctly waiting on owner approval. It will not resume until the agent sees approval on an authenticated owner channel (§5.6.1 `--resolved-by user` rule). Check the surfaced `resolve_hint` reached the user.
- **`event`** — waiting on `data.parked.beads[].eventName`. It resumes when the inbound message/webhook that satisfies the event bead arrives and the agent completes the bead with `--state done --output`.

Do NOT "unstick" a parked human or event walk by fabricating an output — that violates the §5.6.1 / §17.4 "never fabricate an output to finish a parked walk" rule and corrupts the execution's provenance.

---

## 17. Domain Layer Reference

The control plane imports pure domain functions from `apps/clawcraft/domain/` for configuration rendering, workspace file generation, resource naming, and user-facing error messages. These are the single source of truth for business logic — infrastructure (`apps/clawcraft/convex/`) imports domain, never the reverse.

See `domain-doctrine.md` for general domain layer rules (purity, immutability, Result pattern, Brand types). This section documents the claw-specific API surface.

### 17.1 Files & Responsibilities

| File | Purpose | Consumers |
|---|---|---|
| `claw-pod-identity.ts` | Value object: `userId` → K8s resource names | `clients/gke.ts`, `podActions.ts` |
| `claw-config.ts` | `config.toml` rendering, plan limits, tool policy, agent registry (`audio_read`, `vision_read`) | `clients/gke.ts` (ConfigMap) |
| `claw-workspace.ts` | Workspace file rendering (persona, memory, Praxis, tools, bootstrap, multimedia routing) | `clients/gke.ts` (ConfigMap) |
| `praxis-commands.ts` | `PRAXIS_AGENT_COMMANDS` — single source for the verb literals TOOLS.md/AGENTS.md usage-prose names inline (5 verbs; NOT the agent's verb catalog — that is the AGENTS.md manifest snapshot, `rnk-rwfn`). Also `PRAXIS_PROJECT_BOOTSTRAP_COMMAND` (pod postStart). Consumed by `claw-workspace.ts` via the `verb(...)` extractor | `claw-workspace.ts` |
| `user-errors.ts` | User-facing error messages for relay + integration error paths | `relayHelpers.ts`, `telegramRelay.ts`, `scheduledTaskActions.ts` |
| `schedule-limits.ts` | Plan-specific scheduling limits | `scheduledTasks.ts` |
| `telegram-validation.ts` | Bot token format validation, pairing code, deep link builder | `integrationActions.ts`, `telegramRelay.ts` |
| `email-address-generator.ts` | Agent email address generation (320×280 word lists) | `emailActions.ts` |
| `email-threading.ts` | RFC 2822 thread ID resolution | `emails.ts` (via inline logic, domain fn available for reuse) |
| `media-constraints.ts` | MIME validation, agent routing, size limits, MIME normalization | `ChatInput.tsx`, `useChat.ts` (frontend) |

### 17.2 ClawPodIdentity

Value object that deterministically derives all K8s resource names from a `userId`. Single source of truth for the naming invariant.

```typescript
export class ClawPodIdentity {
  readonly configMapName: string;      // `claw-${userId}-config`
  readonly deploymentName: string;     // `claw-${userId}`
  readonly endpoint: string;           // `http://${serviceName}.${namespace}.svc.cluster.local:42617` (gateway)
  readonly webhookEndpoint: string;    // `http://${serviceName}.${namespace}.svc.cluster.local:42618` (webhook channel)
  readonly namespace: string;          // `claw-${userId}`
  readonly networkPolicyName: string;  // `claw-${userId}-netpol`
  readonly pvcName: string;            // `claw-${userId}-data`
  readonly serviceName: string;        // `claw-${userId}-svc`
  readonly userId: string;

  constructor({ userId }: { userId: string });
}

export const GATEWAY_PORT = 42617;
export const WEBHOOK_PORT = 42618;
```

**Note:** `endpoint` is cluster-internal (`svc.cluster.local`). `provisionUser` in `clients/gke.ts` overrides with the external LoadBalancer IP for Convex → pod communication.

### 17.3 Config Rendering (`claw-config.ts`)

#### Types

```typescript
export type Plan = "trial" | "byok" | "pro";
export type ToolPolicy = "safe" | "full";
```

#### Plan Limits

```typescript
const PLAN_LIMITS: Record<Plan, PlanLimits> = {
  byok:  { maxActionsPerHour: 50,  maxCostPerDay: 10 },
  pro:   { maxActionsPerHour: 100, maxCostPerDay: 25 },
  trial: { maxActionsPerHour: 10,  maxCostPerDay: 1  },
};

export function getPlanLimits({ plan }: { plan: Plan }): PlanLimits;
```

#### Tool Policy

`getExcludedTools()` derives the `non_cli_excluded_tools` list for `config.toml`. Starting from ZeroClaw's 25 default excluded tools (ZEROCLAW_DEFAULT_EXCLUDED), it removes deliberately enabled tools (SAFE_ENABLES).

```typescript
// SAFE_ENABLES = { composio, http_request, image_info, memory_forget, memory_store }
// "safe" → 25 defaults − 5 enabled = 20 excluded
// "full" → 0 excluded (all tools available)
export function getExcludedTools({ toolPolicy }: { toolPolicy: ToolPolicy }): string[];
```

See §7.2 for the full excluded/enabled/auto-approved breakdown.

#### Config TOML Builder

```typescript
export function buildConfigToml({
  braveApiKey?,
  composioApiKey?,
  composioEntityId?,
  convexSiteUrl,       // Required — used for [channels_config.webhook] send_url (from process.env.CONVEX_SITE_URL)
  convexUrl,           // Required — domain-restricts [http_request] to Convex URL
  firecrawlApiKey?,
  openRouterApiKey,
  preSharedToken,
  telegramBotToken?,   // Decrypted bot token — renders [channels_config.telegram] when present
  toolPolicy? = "safe",
}: { ... }): string;
```

Renders complete `config.toml` with sections:

| TOML Section | Content |
|---|---|
| *(root)* | `api_key`, `default_provider` ("openrouter"), `default_model` ("qwen/qwen3-max") |
| `[gateway]` | port, host, pre_shared_token, allow_public_bind, request_timeout_secs = 300 |
| `[memory]` | backend = "sqlite", auto_save = true |
| `[autonomy]` | Plan limits, `non_cli_excluded_tools`, `auto_approve` |
| `[http_request]` | enabled, `timeout_secs = 120`, domain-restricted to Convex URL (`.convex.cloud` + `.convex.site`) |
| `[web_search]` | enabled, `timeout_secs = 120`, Brave (primary) / Firecrawl / DuckDuckGo (fallback) |
| `[web_fetch]` | `timeout_secs = 120` |
| `[composio]` | *(conditional)* Google Workspace integration (requires API key) |
| `[security.otp]` | enabled = false (OTP unusable on emptyDir — secret regenerates every restart) |
| `[logging]` | level = "trace" |
| `[observability]` | backend = "log", runtime_trace_mode = "rolling", runtime_trace_max_entries = 500 (see §16.7) |
| `[identity]` | format = "openclaw" |
| `[channels_config]` | *(always present)* cli = true, message_timeout_secs = 300 |
| `[channels_config.webhook]` | *(always present)* enabled = true, port = 42618, send_url = `{convexSiteUrl}/container-webhook` (secret omitted — HMAC disabled, auth via nginx + NetworkPolicy) |
| `[channels_config.telegram]` | *(conditional — present when `telegramBotToken` is provided)* bot_token, allowed_users = ["*"] |
| `[channels_config.linq]` | *(conditional — present when `linqConfig` is provided)* `api_token`, `from_phone`, `allowed_senders = ["*"]`. **`signing_secret` DELIBERATELY OMITTED** — inbound HMAC verification lives Convex-side (`/linq-webhook`); the field name is `from_phone` (NOT `sender_phone`), pinned to Linq's wire format (add-linq) |

**`[channels_config.linq]` rendering contract (add-linq).** The block is rendered by `buildConfigToml` (`claw-config.ts`) from `linqConfig: { apiToken, fromPhone }`. Two pins, both earned by prod bugs (see methodology §3):
- **No `signing_secret`.** HMAC verification of inbound Linq webhooks is Convex-side (`/linq-webhook`), so the pod never needs the secret — a cross-fork-boundary field removal (the sovereign-fork `LinqChannel` accepts the block without it).
- **`from_phone`, not `sender_phone`.** The field name is pinned to Linq's actual wire format; a `sender_phone` paraphrase cost a prod-deploy cycle.

Today this block feeds only the pod-side inbound `LinqChannel` (registered at boot). Pod-native *outbound* (`LinqChannel::send`) is not yet merged, so outbound currently routes Convex-side (§3.5); when the sovereign-fork lands, `api_token`/`from_phone` here are what it delivers with.

### 17.4 Workspace Files (`claw-workspace.ts`)

#### Types

```typescript
export interface PersonaSeed {
  agentName: string;
  communicationStyle: string;
  timezone: string;
  userName: string;
}

export interface PersonaResolved {
  identityMarkdown: string;
  soulMarkdown: string;
  userMarkdown: string;
}

export interface PersonaReadyData {
  agentName: string;
  communicationStyle: string;
  identitySummary: string;
  soulSummary: string;
  userName: string;
  userSummary: string;
}

export interface MemoryEntry {
  category: string;   // "key_fact" | "decision" | "preference" | "context"
  content: string;
}

export type WorkspaceFiles = Record<string, string>;
```

#### Main Builder

```typescript
export function buildWorkspaceFiles({
  activeChannels,
  activeIntegrations? = [],
  agentEmailAddress?,        // "{adj}-{animal}@agent.clawcraft.ca"
  convexUrl,
  memories,
  onboardingComplete,
  preSharedToken,
  resolved?,
  seed,
}: { ... }): WorkspaceFiles;
```

Returns `Record<string, string>` mapping filename → markdown content. See §8.1 for the file list. Conditional logic:

- `onboardingComplete = false` → uses seed templates + includes `BOOTSTRAP.md`
- `onboardingComplete = true` + `resolved` → uses resolved persona markdown, omits `BOOTSTRAP.md`

#### Template Functions

| Function | Output | Notes |
|---|---|---|
| `renderIdentityTemplate({ seed })` | `IDENTITY.md` | Pre-onboarding default |
| `renderSoulTemplate({ seed })` | `SOUL.md` | Pre-onboarding default |
| `renderUserTemplate({ seed })` | `USER.md` | Pre-onboarding default |
| `renderAgentsTemplate({ manifestSnapshot })` | `AGENTS.md` | Standing orchestration rule + intent map + manifest snapshot + async-walk sections (see note below) |
| `renderToolsTemplate({ activeChannels, activeIntegrations, agentEmailAddress, convexUrl, preSharedToken })` | `TOOLS.md` | Memory, **Praxis** (durable task tracker, between Memory and Web Search; usage doctrine + §2.2 bead filter + per-thread spec walkthrough — NO verb catalog as of `rnk-rwfn`; inline verb literals sourced from `praxis-commands.ts:PRAXIS_AGENT_COMMANDS`, full catalog lives in AGENTS.md's manifest snapshot), web search, HTTP, scheduling API, email inbox (conditional on `agentEmailAddress`), generic Composio integrations section (conditional on connected apps) |
| `renderMemoryTemplate({ memories })` | `MEMORY.md` | Categorized: key_fact, decision/preference, context |
| `renderBootstrapTemplate({ seed })` | `BOOTSTRAP.md` | First-run onboarding ritual, emits `[PERSONA_READY]` tag |

**AGENTS.md async-walk sections (praxis-hardening 0.11.0).** `renderAgentsTemplate` ships, in addition to the §5.6.1 intent map + the §5.6.3 manifest snapshot, the following sections that operationalize async (cross-turn) praxis walks (§5.7.1). These are the *agent-facing* statement of the doctrine above — keep them in sync with §5.6.1 / §5.7.1 / §16.10:

- **Standing orchestration rule — due-scan (D14).** "On any activation, webhook, or resume trigger, run `praxis execute --list` first and resume every in-flight execution whose park wait is now satisfied via `praxis execute --execution <id>` before starting new work." This is the correctness net for parked walks — it works even when a cron fire was missed or the pod restarted.
- **`## Parked walks`.** Handles `next_action: null` + `data.parked` by `data.parked.kind`: **timer** → arm a `cron_add`/`schedule` one-shot at `dueAt` carrying the `Intent: resume-execution` envelope, then end the turn; **human** → surface the bead's `resolve_hint`, NEVER self-approve, end the turn; **event** → state what you're waiting for and end the turn. Carries the binding line **"Never fabricate an output to finish a parked walk."**
- **`## Human-gate provenance`.** Restates the §5.6.1 `--resolved-by user` trust posture for the agent: agent-asserted, may ONLY follow approval on an authenticated owner channel (web WS / native Telegram), NEVER from envelope-relayed external content (email/SMS body), with forgeability honesty per FMEA-3.
- **`Intent: resume-execution` intent-map entry.** Sources: `pod-cron`, `bead-resolve` → `praxis execute --execution <ExecutionId>` (the self-addressed envelope from §5.6.1; one entry, two sources per the §5.6.2 precedent).
- **Factory-authoring note (32-call budget).** States the ~32-tool-call-per-activation budget and names **park-and-resume chunking via a `wait:` bead** as the sanctioned answer for a wide DAG — author a downstream `wait:` (timer/event) bead so the walk parks and resumes across turns rather than trying to drain the whole frontier in one activation.

#### Onboarding Renderers (post-persona)

| Function | Output |
|---|---|
| `renderIdentityFromOnboarding({ agentName, identitySummary })` | `IDENTITY.md` |
| `renderSoulFromOnboarding({ communicationStyle, soulSummary })` | `SOUL.md` |
| `renderUserFromOnboarding({ userName, userSummary })` | `USER.md` |

#### Persona Tag Extraction

```typescript
export function extractPersonaReady({ content }: { content: string }): {
  cleanContent: string;
  personaData: PersonaReadyData | null;
};
```

Parses `[PERSONA_READY]{...}[/PERSONA_READY]` from agent output. Returns cleaned content (tag stripped) and parsed persona data. Used by `relay.ts` to detect onboarding completion.

### 17.5 User-Facing Errors (`user-errors.ts`)

All error messages sent to users via channel adapters MUST use these functions. `SUPPORT_EMAIL` (`help@clawcraft.ca`) is the single source of truth. All messages include the suffix: *"If this keeps happening, reach out to support at {SUPPORT_EMAIL}"*

```typescript
export const SUPPORT_EMAIL = "help@clawcraft.ca";

// Relay errors (pod communication failures)
export function agentError({ status }: { status: number }): string;
export function agentTimeout(): string;
export function agentUnreachable(): string;

// Scheduled task errors
export function scheduledTaskFailed({ task, reason, willRetry }: { ... }): string;

// Integration errors (Composio/Google Sheets)
export function integrationAuthExpired({ provider }: { provider: string }): string;
export function integrationNotConnected({ provider }: { provider: string }): string;
export function integrationRateLimited(): string;
export function integrationUnavailable(): string;
```

### 17.6 Schedule Limits (`schedule-limits.ts`)

```typescript
export const SCHEDULE_LIMITS = {
  maxActiveSchedules: { byok: 10, pro: 10, trial: 3 },
  maxDailyExecutions: 96,
  minIntervalMinutes: 5,
} as const;
```

### 17.7 Telegram Validation (`telegram-validation.ts`)

```typescript
// Returns Result<string, string> — ok(token) or err(message)
export function validateBotTokenFormat({ token }: { token: string }): Result<string, string>;

// Generates "CLAW-XXXX" pairing code (4 alphanumeric chars)
export function generatePairingCode({ randomBytes? }: { ... }): string;

// Returns "https://t.me/{botUsername}?start={code}"
export function buildDeepLink({ botUsername, code }: { ... }): string;
```

Used by `integrationActions.ts` (connect flow, §11.2) and `telegramRelay.ts` (pairing, §11.3).

---

## Document History

| Version | Date | Changes |
|---|---|---|
| 2.38.0 | 2026-06-11 | **Async (cross-turn) praxis walks — park & resume (praxis-hardening 0.11.0).** Reconciles the doctrine to the claw/pod-side machinery shipped in praxis-hardening: AGENTS.md gained the due-scan standing rule + `## Parked walks` / `## Human-gate provenance` sections + the `Intent: resume-execution` intent-map entry + the 32-call authoring note (all already live in `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate`), and `praxis execute` now parks at human/timer/event boundaries and resumes across turns. **§5.6.1** — added two `[CLAWCRAFT_TRIGGER]` inventory rows: `Source: pod-cron` (a **self-addressed** one-shot the agent arms at park time, `Intent: resume-execution` → `praxis execute --execution <id>`; a new class — agent-composed *for its own future self*, unlike the prior Convex/client-composed rows) and `Source: bead-resolve` (REGISTERED as a second source for the same intent per the §5.6.2 one-intent-many-sources precedent, but its composer ships LATER with praxis-intent-relay — **verified NO dormant code in `webhookDispatch.ts`** today). Added the resume-execution envelope shape and a `--resolved-by user` trust-posture appendix (FMEA-3: agent-asserted; authenticated-owner-channel-only — web WS / native Telegram; NEVER from envelope-relayed external content; forgeability honesty). **§5.7.1 (new)** — park-&-resume contract: cron-arming-at-park-time is the *optimization*, the AGENTS.md due-scan is the *correctness* mechanism (D13/D14); NO timer-driven `scaleUp` — a parked run resumes on next natural pod wake (mirrors §9 Linq posture); resume SLA is EVENTUAL. **§14** — recorded pod-cron durability as VERIFIED LIVE (the "in-memory scheduler" worry is DISPROVEN): agent-cron fires the full agent loop, jobs persist disk-backed in `/zeroclaw-data/workspace/cron/jobs.db` on the **PVC workspace volume** (survive process restart, `dev:claw:up` recreate, prod pod restart), + a startup catch-up replay pass. **§16.10 (new)** — parked-execution debugging runbook (inspection surface: `praxis execute --list`, `praxis execute --execution <id>`, `zeroclaw cron list`, `cron/jobs.db`, the row's `data.parked` snapshot; per-kind triage + the overdue-timer hint that an in-flight past-`dueAt` timer is the EVENTUAL SLA working, not a hang). **§17.4** — `renderAgentsTemplate` table row + a new note cataloguing the five async-walk AGENTS.md sections. **No code change in this PR** — pure doctrine reconciliation to already-shipped claw/pod-side code per `feedback_doctrine_drift_scope`. |
| 2.37.0 | 2026-06-09 | **§5.7 — TOOLS.md praxis command-surface catalog retired; manifest snapshot is the sole verb catalog (`rnk-rwfn`).** Completes the `rnk-ih70` (v2.36.0) → `rnk-rwfn` arc: with AGENTS.md's manifest snapshot now the lean single catalog, the duplicate `### Command surface` verb list in TOOLS.md (`claw-workspace.ts:renderPraxisSection`, ~2,250 B rendered from `PRAXIS_AGENT_COMMANDS`) is removed — the agent no longer sees the verb list twice in two formats. `PRAXIS_AGENT_COMMANDS` trimmed 20 → 5 entries (only the verbs whose literal shape TOOLS.md/AGENTS.md usage-prose names inline); it is no longer a catalog. All other Praxis-section usage doctrine (the §2.2 bead filter, Praxis-vs-memory decision, per-thread spec walkthrough, hygiene loop) is unchanged. **§5.7 surface description** + the `praxis-commands.ts` and `renderToolsTemplate` rows in the file/render-output tables reconciled to "manifest snapshot = catalog; `PRAXIS_AGENT_COMMANDS` = prose-literal source only." The binding doctrine for the retirement lives in `praxis-doctrine.md` §6.1/§6.5/§3.1 (v1.13.11), reconciled in the same PR. **Tests:** `claw-workspace.test.ts` (catalog-coverage assertion → catalog-absent guard; 20→5-entry single-source; section snapshot regenerated) + `praxis-commands.test.ts` (verb-roster trim). No code change beyond `claw-workspace.ts` + `praxis-commands.ts`; no praxis package change. |
| 2.36.0 | 2026-06-09 | **§5.6.3 — manifest snapshot trimmed to a verb+summary index (rnk-ih70).** AGENTS.md's `## Manifest snapshot` block previously embedded the full `praxis manifest list --mode all --json` envelope (~28.8 KB, 20 verbs, each with its `flags` / `positional_args` / `modes` / `agent_safe` schema). Measured cost: ~7,900 tok = ~32% of the 24,620-token standing context that is **re-sent on every agent turn** (dev trace `845d8209` turn-1 `input_tokens`=24,620). New `apps/clawcraft/domain/manifest-snapshot.ts:projectManifestToIndex(raw)` projects the envelope to `{ data: [{verb, summary}] }` (~1.6 KB, ~94% smaller, ~7,400 tok/turn saved); `snapshotManifest()` pipes its `execFileSync` output through it. **Behavior-preserving:** the `summary` fields §5.6.3 relies on for discovery are kept verbatim; only the per-verb flag schema is dropped, and the on-demand path for it already existed and is documented in the standing rule (`praxis manifest show "<verb>" --json`, claw-workspace.ts:215). **Evidence the dropped schema was dead weight:** dev trace `6baecbab` — the agent authored `spec create --from-file/--factory-from-file` from its own knowledge and never read the snapshot flags (nor called `manifest show`). Does NOT depend on `rnk-h6g3`: this removes a redundant eager copy of an already-documented on-demand path, it does not deepen recursion. **Fail-soft:** non-`{data:[…]}` input (parse error, error placeholder, schema drift) returns verbatim — never emit empty. **Tests.** New `apps/clawcraft/test/unit/domain/manifest-snapshot.test.ts` pins the projection (→ `{verb,summary}` only, schema dropped, verbs+summaries preserved, fail-soft). `claw-workspace.test.ts` feeds a stub snapshot to the renderer → unaffected; `agents-intent-coverage.test.ts` shells the raw CLI itself → unaffected. AGENTS.md `## Manifest snapshot` header prose updated to label the block an index and point at `manifest show` for flags. Per `feedback_doctrine_drift_scope`: §5.6.3's "Agent-side wiring (v2)" paragraph reconciled in the same PR (the snapshot description my code change contradicted). Sibling follow-on `rnk-rwfn` (retire the duplicate TOOLS.md praxis command surface) sequenced after this. No new verbs (per praxis-doctrine §6.1). |
| 2.35.0 | 2026-06-02 | **OTLP carrier drift cleanup + managed-prod activation (laminar prod-activation spec).** §9.2: the stale "Deployment-injected env vars" row claiming `gke.ts:buildDeploymentSpec` injects an `OTEL_EXPORTER_OTLP_ENDPOINT`/`OTEL_SERVICE_NAME`/`OTEL_RESOURCE_ATTRIBUTES` trio is **deleted** (verified: `gke.ts` injects NO `OTEL_*` env — the emitter ignores OTLP env, reads `config.toml [observability]`); replaced with a "no `OTEL_*` env — retired" row pointing at the config.toml carrier. §16.7: the "two observability surfaces" callout rewritten — both surfaces (`backend="log"` LogObserver vs `backend="otel"` + `otel_endpoint`/`otel_headers` OTLP export) now live in `config.toml`, distinguished by which keys are present, NOT env-vs-toml. §6.1: the OTLP project key promoted to the **third documented plaintext-in-ConfigMap exception** (peer to the Telegram bot token + `CONTAINER_SERVICE_TOKEN` `auth_header`); prod path documented as the **managed-project key read at the `gke.ts` client boundary from prod Convex env** (`secrets-manifest.yaml audit_pending:`), rendered per-pod via `buildConfigToml otlpHeaders`, never committed, never copied from dev (store of record = prod Convex env, materialized into the ConfigMap at render time; one shared managed key, like `PLATFORM_OPENROUTER_KEY`). §6.2 credential-location row + §12.1 ConfigMap contents updated to match. Cross-ref observability-doctrine §5/§9 v1.5.0, backend-doctrine §9 v2.45.0. No code change in this doctrine's scope beyond the already-shipped `gke.ts` `otlpHeaders` threading. |
| 2.34.0 | 2026-05-20 | **§5.6.1 — `[CLAWCRAFT_TRIGGER]` envelope replaces multi-phase `[CLAWCRAFT_DIRECTIVE]` for Convex-issued button clicks (rnk-22q2 of rnk-eelu epic).** `apps/clawcraft/convex/webhookDispatch.ts`: `composeExecutionDirective` and `composeValidateDirective` collapsed from ~2000-byte multi-phase walk-loop / grounded-semantic prose to < 300-byte signal envelopes (`Source`/`Intent`/`ThreadId`/optional `TriggerPayload` lines between `[CLAWCRAFT_TRIGGER]` … `[/CLAWCRAFT_TRIGGER]` tags). `sendValidationWebhook` drops the integrations query, `crossReferenceTools`/`INTEGRATION_CATALOG` imports, `--connected-tools` cross-pass parity machinery, `--inject-semantic` path generation, and the request-id correlation — all of which are now praxis-doctrine §9.10's concern; the agent discovers the flag set via `praxis manifest show "spec verify"`. `sendExecutionWebhook` keeps only `triggerPayload` JSON-stringify + POSIX `'\''` shell-escape so the agent can interpolate it onto `--trigger-payload '<json>'` verbatim. `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate`: the transitional `## Convex-issued [CLAWCRAFT_DIRECTIVE] tasks` section (preserved during v2.33.0's collapse) is removed; the intent map gains a paragraph routing `[CLAWCRAFT_TRIGGER]` envelopes through the same handlers as chat ("run" / "validate"), so button clicks and chat utterances converge on one surface. `## Validation Resolver Protocol` section unchanged — still in use for client-WS-issued `[CLAWCRAFT_DIRECTIVE]` Sources (`validation-forward`, `resolver-choice` per §5.6.2). **Tests.** `apps/clawcraft/test/integration/webhookDispatch.test.ts` rewritten: directive-content assertions (Step 1/2/3 prose, TOOL CATALOG CONTEXT, --connected-tools/--persist/--inject-semantic regex, Phase 1/2/3 prose, SPEC BODY/FACTORY blocks, tool-hint mapping) replaced by envelope-shape assertions + regression assertion `body.content.length < 300`; degraded-mode tests (404, 503, network error, timeout, spec_unthreaded, spec_missing) unchanged. `chat-triggered-execution.test.ts:377` updated for `[CLAWCRAFT_TRIGGER]` prefix + `Intent: run` line. `apps/clawcraft/test/unit/domain/claw-workspace.test.ts` `renderAgentsTemplate` block: legacy-section-survives assertion replaced by legacy-section-deleted + envelope-routing-coverage assertion; Validation Resolver Protocol survival pinned. **Doctrine drift reconciled in the same PR** per `feedback_doctrine_drift_scope`: §5.6.1 rewritten end-to-end with retirement audit trail spanning v2.31.0 → v2.34.0. **Out of scope.** MCP server (forward-compatible with this design but separate epic). Validation-path `next_action` emission (deferred from rnk-eelu/#2; can land before or after this task). No new verbs introduced (per praxis-doctrine §6.1). |
| 2.33.0 | 2026-05-20 | **§5.6.3 — AGENTS.md collapse (rnk-p7a0 of rnk-eelu epic)**. `renderAgentsTemplate()` rewritten in `apps/clawcraft/domain/claw-workspace.ts` from a directive-coaching document into a minimal 4-section file: (1) standing orchestration rule ("run `praxis manifest list --mode execute --json`, then `praxis manifest show <verb> --json`, then follow `next_action` until null"), (2) intent map routing chat phrases ("run / execute / kick off the spec" → `praxis spec activate-execution <threadId>`; "validate / check the spec" → `praxis spec verify --thread <threadId>`; "attach files for the run" → `/zeroclaw-data/workspace/conversation-attachments/<threadId>/` + `--trigger-payload`), (3) frozen `praxis manifest list --mode all --json` snapshot as a fenced JSON block so the agent has first-turn verb discoverability without a round-trip, (4) the v1 `## Convex-issued [CLAWCRAFT_DIRECTIVE] tasks` + `## Validation Resolver Protocol` sections preserved verbatim until sibling task `rnk-eelu/#5` retires the convention after parity is verified by `rnk-eelu/#4`. The v1 `## Spec-Driven Workflow Protocol` section (vocabulary glossary, shape detector, worked example, hard short-circuit, carve-out — added in v2.32.2) was COLLAPSED — the binding lessons (no escape hatch, carve-out) remain in §5.6.3 but are no longer pre-baked into AGENTS.md prose; the agent now discovers `spec create` / `spec update` via the manifest snapshot's `summary` fields and routes through the intent map. **Snapshot mechanics.** New `apps/clawcraft/domain/manifest-snapshot.ts` module exports `snapshotManifest()` which shells out via `node:child_process` to `praxis manifest list --mode all --json`; lives in its own module to keep `claw-workspace.ts` free of `node:` imports so Convex's non-"use node" bundle (relay.ts → `extractPersonaReady` etc.) doesn't fail to resolve `child_process`. `renderAgentsTemplate({ manifestSnapshot })` and `buildWorkspaceFiles({ ..., manifestSnapshot })` now take the snapshot as a REQUIRED parameter — `apps/clawcraft/convex/integrationActions.ts`, `apps/clawcraft/convex/podActions.ts` (both "use node"), `infra/scripts/dev/render-claw-config.ts`, and the two `apps/clawcraft/scripts/print*` helpers call `snapshotManifest()` and pass it in. `render-claw-config.ts` gains a pre-flight that runs `praxis --version` and `process.exit(7)` with an actionable error if praxis isn't on PATH (snapshot missing = silent verb-discoverability loss = worst failure mode). **Tests.** `apps/clawcraft/test/unit/domain/claw-workspace.test.ts` `renderAgentsTemplate — spec-first attention protocol` block REPLACED by `renderAgentsTemplate — manifest + intent map collapse (rnk-p7a0)`: five assertions pinning the four-section order, the standing rule's manifest list/show + next_action references, the intent map's three routings, sanity that the embedded snapshot contains `spec activate-execution` + `spec verify`, and that the legacy [CLAWCRAFT_DIRECTIVE] + Validation Resolver Protocol sections survive verbatim. New file `apps/clawcraft/test/unit/domain/agents-intent-coverage.test.ts` shells out to real praxis CLI and asserts every intent-map verb is present in `manifest list` output AND is `agent_safe: true` — cross-package liveness check that catches verb renames before agents see a broken intent map. **threadId injection (deferred parity item for `rnk-eelu/#4`).** Intent map references `<threadId>` as a literal placeholder; the relay path (`apps/clawcraft/convex/relay.ts`) currently sends thread history but does not inject the threadId itself into per-thread system context. Whether the agent can substitute the placeholder correctly at chat-runtime is the parity-verification scope of `rnk-eelu/#4`; this task ships the intent-map shape with the understanding that #4 will validate (and either land the threadId-injection wiring or amend the intent-map shape). Per `feedback_doctrine_drift_scope`: §5.6.3 (the agent-side wiring section my code change directly contradicted) is reconciled in this PR with an audit-trail v1/v2 split — the lessons stay binding; the wiring story moves from "baked into AGENTS.md prose" to "discovered via manifest + routed via intent map." No new verbs introduced (per praxis-doctrine §6.1). |
| 2.32.3 | 2026-05-19 | **§5.6.2 — new `sop-ingestion` Source row** (`/substrate:execute` spec-from-sop). User-initiated drop of 1..10 PDF/text files in `SpecTab`'s empty-state `SopDropZone` emits a `[CLAWCRAFT_DIRECTIVE] Source: sop-ingestion` over the client WS. The directive instructs the agent to materialize ONE `reference` bead per file (workspace-durable ground truth) before disambiguating the workflow into a praxis spec. Persistence delta: NEW optional `messages.kind: "user" \| "synthetic"` column (additive, no migration; legacy/absent ≡ `"user"`) + NEW public mutation `api.messages.persistSyntheticUserMessage` writing ONE synthetic bubble per drop with the joined `[MEDIA_UPLOAD]` blocks in `content` and N attachments. The full directive prose is WS-only — only the joined MEDIA_UPLOAD blocks land in `messages.content` (history-durable). `MessageBubble` synthetic branch strips MEDIA_UPLOAD via `parseAllMediaUploadBlocks` from the new `apps/clawcraft/domain/media-upload-block.ts` pure helper (also adopted by the existing `sendMessage` path so the wire format has one source of truth). v1 MIME admission: PDF + `text/plain` only; `application/pdf` and `text/plain` route to the existing `vision_read` agent role (no new role). Pre-existing `useChat.sendMessage` inline `[MEDIA_UPLOAD]` template migrated to `composeMediaUploadBlock`; pre-existing per-file upload work extracted to a shared `uploadFileToGcs` helper. Per `feedback_doctrine_drift_scope`: claw + frontend + domain + backend doctrines reconciled in the same PR. |
| 2.32.2 | 2026-05-19 | **New §5.6.3 — Spec-first attention protocol** (`/substrate:quick-spec` spec-first-attention-routing). Closes the gap surfaced 2026-05-19 when the user said "create me a workflow for managing email triage" and the agent free-handed a multi-step plan in chat prose instead of running `praxis spec create` — losing the work to scrollback. Pre-fix, `renderAgentsTemplate()` had no spec-first guidance; TOOLS.md documented HOW to author a spec but never tied that machinery to a conversational trigger. §5.6.3 adds: (a) a binding rule that BUILD/CREATE/CONFIGURE intent maps to `praxis spec create | update` by default; (b) a shape detector (≥2 of: trigger, action, integration, schedule, multi-step); (c) a closed-for-v1 vocabulary mapping making `workflow / automation / flow / process / rule / routine / setup / pipeline / job` doctrinally equivalent to "spec" for routing; (d) a carve-out for one-shot factual / read-only / conversational / meta turns. **Code:** `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate` gets a top-of-file `## Spec-Driven Workflow Protocol` section placed BEFORE the existing `## Convex-issued [CLAWCRAFT_DIRECTIVE] tasks` section so it lands in the highest-attention slot (the directive-handler is for Convex→agent button traffic, which is dwarfed in volume by human→agent traffic — spec-first belongs first). Worked example uses the exact failure phrase ("create me a workflow for managing email triage") per the §5.6.1 retirement audit lesson — empirical mimicry > normative rules. **No escape hatch** for "ask the user if they want a spec" — §5.6.1's "permission to skip work is asymmetric" lesson applies. **Tests:** new `describe("renderAgentsTemplate — spec-first attention protocol")` block in `claw-workspace.test.ts` with five assertions pinning section presence, section-ordering invariant (spec-first appears before the directive-handler section), glossary keywords, worked-example phrase, shape detector keywords, and carve-out keywords. Tests pin structure not prose so wording can evolve without breakage. No new verbs introduced — the change reframes the conversational trigger to existing `praxis spec create / update` verbs (per praxis-doctrine §6.1). |
| 2.32.1 | 2026-05-19 | **§5.6.2 — `[INTEGRATION_LINK]` provider-shortcut form pinned as canonical validation-resolver response** (`/substrate:quick-spec` integration-button-fix). The v2.32.0 audit named `[INTEGRATION_LINK]` as a render tag but the agent-side guidance had two collisions that produced inert-text "broken link" output in live use (screenshot trail on thread `p176z49wnmymv38p6y1zvweq6h8711vs`, 2026-05-19): (1) the §"Validation Resolver Protocol" Direct-fix example in `apps/clawcraft/domain/claw-workspace.ts:renderAgentsTemplate` literally suggested writing "user needs to connect Google Sheets at /integrations?connect=google_sheets" as chat prose — the agent obeyed and the URL rendered as plain text, not a button; (2) the `webhookDispatch.ts` semantic-validation prompt told the agent to seed `semantic_mismatch` findings with "direct them to /integrations?connect=<provider>" — that string echoed through to the bead title and back into the validation-forward directive body, doubling down on the prose path. **Fix landed in the same PR.** §5.6.2 INTEGRATION_LINK row now documents both forms explicitly: **provider shortcut** `{provider, label}` deep-links to `/integrations?connect=<provider>` (route at `src/routes/_authenticated/integrations.tsx` auto-triggers OAuth in-app — already shipped, parser already supports it via the `href` fallback at `MessageBubble.tsx:168`); **two-step OAuth** `{url, label}` remains for cases where you want OAuth in a separate tab. Provider shortcut is now pinned as the canonical validation-resolver response. `renderAgentsTemplate` §"Validation Resolver Protocol" rewritten so Direct-fix's canonical example emits a provider-shortcut button; §"Direct fix is the default" clarifies "direct fix" means skipping A/B/C, not skipping the button tag; §"Integration Links" gains a "Provider shortcut (fast-path)" subsection. `webhookDispatch.ts` semantic-prompt extension drops the raw URL from the prescribed message (`Spec relies on <provider> but the user has not connected it yet.`) and explicitly tells the agent the resolver protocol will surface a button — the integration test `webhookDispatch.test.ts:279` was re-pinned to the new shape. **Why this matters beyond polish.** Per the v2.31.2 lesson, prompts that point the agent at the wrong example shape cost more than prompts that omit guidance — the LLM mimics the example more eagerly than it reads the surrounding warnings. The fix mirrors that lesson: replace the bad example with a good one, don't add a "but actually" qualifier. Per `feedback_doctrine_drift_scope`: the polysemy-doctrine contract that `[INTEGRATION_LINK]` is a button (named in v2.32.0's row) was violated by the canonical example pointing at prose — reconciled in the same PR. |
| 2.32.0 | 2026-05-19 | **New §5.6.2 — Client-WS-issued `[CLAWCRAFT_DIRECTIVE]` directives + assistant→client render tags** (`/substrate:quick-spec` validation-resolver-widget). §5.6.1 has always governed Convex→pod channel-webhook directives, but the client-WS path (`/ws/chat`) was carrying its own `[CLAWCRAFT_DIRECTIVE]` traffic — `bead-forward` shipped in `useChat.ts:sendBeadToAgent` with no doctrine home, predating this entry. §5.6.2 documents that drift plus adds two new sources for the validation-resolver widget: `validation-forward` (✈ on a validation prior-work bead → directive asking for direct-fix OR an inline `[RESOLVER_OPTIONS]` render tag) and `resolver-choice` (in-bubble A/B/C widget click → follow-up directive carrying `Context:`, `Choice:`, optional `UserText:`). New concept class introduced: **assistant→client render tags**, distinct from `[CLAWCRAFT_DIRECTIVE]` per `methodology-doctrine §1` (polysemous labels are bugs) — currently `[INTEGRATION_LINK]` (existing, undocumented until now) and `[RESOLVER_OPTIONS]` (new). Parser sites in `apps/clawcraft/src/components/chat/MessageBubble.tsx`; widget at `components/chat/ResolverChoice.tsx`; round-trip via `chatBridgeStore.resolverSend`. Agent-side guidance lives in `renderAgentsTemplate` (Validation Resolver Protocol section). Per `feedback_doctrine_drift_scope`: pre-existing `bead-forward` drift reconciled in the same PR. |
| 2.31.2 | 2026-05-18 | **§5.0 + §5.1 + §5.6.1 — `[system]` prefix retired in favor of `[CLAWCRAFT_DIRECTIVE]` tagged block + always-reply guidance** (`/substrate:quick-spec` iterations 5+6; mirror entry in `backend-doctrine.md` v2.36.2). Two compounding agent short-circuits resolved in the same row. **Short-circuit 1 (the `[system]` reflex).** v2.31.1's `[system] Please run: <cmd>` convention reached the agent loop correctly but the agent emitted "🤖 No reply (Xms): Latest message is a system directive to run a praxis command, not a user message requiring a visible reply" — the `[system]` literal triggered an LLM-trained "system role = informational context, not action" reflex that no AGENTS.md prose could override (two iterations of AGENTS.md tightening had no effect on the short-circuit). The fix: drop `[system]`, adopt the proven `[EMAIL_RECEIVED]` … `[/EMAIL_RECEIVED]` shape from `domain/email/notification.ts:formatNotification` (which has been eliciting tool execution from the same agent without issue). New convention: `[CLAWCRAFT_DIRECTIVE]\\nSource: <button>\\nRun the following command via your shell tool, exactly as written:\\n\\n  <command>\\n\\nOn success (exit code 0): no chat reply is needed; the result reaches Convex via the projection round-trip.\\nOn failure: reply with the exit code and the first line of stderr.\\n[/CLAWCRAFT_DIRECTIVE]`. Code: `apps/clawcraft/convex/webhookDispatch.ts` gains a `composeDirective({source, command})` helper; both `sendValidationWebhook` + `sendExecutionWebhook` use it. Tests: assertion shape changed from regex match on `[system] Please run: …` to assertions on tagged-block prefix/suffix + `Source: <button>` line + embedded command regex. AGENTS.md: replaced the "Convex-issued [system] directives" section with "Convex-issued [CLAWCRAFT_DIRECTIVE] tasks" — leaner because the new format is self-explanatory (the agent already knows what to do with tagged-block + imperative). Doctrine: §5.6.1 documents the retirement audit trail; the "Adding a new directive" section gains a "do NOT revive [system]" rule. **Short-circuit 2 (the "no-reply-on-success" reflex).** After the `[CLAWCRAFT_DIRECTIVE]` format landed, the live agent emitted "🤖 No reply (4639ms): CLAWCRAFT_DIRECTIVE instructs no chat reply on success; command must be executed first to determine outcome." The agent parsed the directive correctly and even knew it needed to execute, but interpreted the "no reply on success" guidance as license to skip both the tool call AND the reply — closing the turn without doing anything. Fix: removed the success-silent path; reply guidance now ALWAYS requires a one-line summary (exit code + first line of stdout/stderr). The always-reply rule is load-bearing — it forces the shell tool call because the agent can't compose a summary without real output. Yes, this means a small chat-noise echo per Validate/Run click; filtering `[CLAWCRAFT_DIRECTIVE]`-response chat lines from the rendered UI is out-of-scope v2 polish. **Lesson** (`feedback_doctrine_drift_scope` adjacent): a convention that requires the LLM to read a custom doc to behave correctly is more brittle than one that mirrors a pattern the LLM already handles correctly — when designing agent-facing message shapes, prefer empirical mimicry over normative documentation. Permission to skip work is asymmetric: agents accept it eagerly and ignore the surrounding "you still have to do the work" qualifiers, so don't offer it. |
| 2.31.1 | 2026-05-18 | **§5.0 + §5.1 + §5.6.1 — Convex→pod routing correction** (`/substrate:quick-spec` validate-flow fix; mirror entry in `backend-doctrine.md` v2.36.1). v2.31.0 of §5.0 claimed port-42617 `/webhook` `{message}` carried Convex-issued `[system]`-prefixed directives through "Full" agent loop — both claims were wrong. Port 42617's `/webhook` is `run_gateway_chat_simple` (tool-less echo, see `docs/tasks/completed/fix-webhook/`); the agent loop with tools/memory/personality/session lives on the channel webhook at port 42618 (the same path `emailRelay.ts` uses). v2.31.0 of §5.6.1 also documented `{message}` on `/webhook` as the directive contract — the matching `webhookDispatch.ts` code therefore POSTed `{message}` to port 42617, which (a) silently degraded in prod because Convex Cloud cannot resolve ClusterIP DNS for `users.containerEndpoint`, and (b) even if reachable, would hit the echo path not the agent loop. **Corrected contract.** §5.0 table: 42617 row marked "None" agent-loop and use case narrowed to "Legacy gateway chat; manual quick-test only"; 42618 row picks up the "Convex-issued `[system]`-prefixed directives" use case. §5.1 communication-surfaces table: Convex-initiated control column repointed at the channel webhook (`{sender, content}`, port 42618, nginx `/relay/{userId}` upstream). §5.6.1 rewritten end-to-end as the channel-webhook contract with explicit routing table (`<WS_GATEWAY_URL>/relay/{userId}`, `X-Relay-Token` + `X-Pod-Token`, body `{sender, content}`). **Code landed in the same PR**: `apps/clawcraft/convex/webhookDispatch.ts` sendMessage rewrite (`containerStatus !== "running"` replaces obsolete `containerEndpoint` gate; new `skipped(env_missing)`); `apps/clawcraft/test/integration/webhookDispatch.test.ts` URL/body/header re-assertion. **Local-dev fixes bundled**: (a) `infra/scripts/dev/render-claw-config.ts` was writing AGENTS.md/TOOLS.md/SOUL.md/BOOTSTRAP.md/IDENTITY.md to `workspace/system/` but prod's `gke.ts:794-813` ConfigMap subPath mounts those at `workspace/` root — local renderer now matches prod; (b) no nginx runs in local dev, so `sendMessage` gains a second transport branch gated on `CLAW_LOCAL_POD_WEBHOOK_URL` (auto-mirrored by the renderer to `http://localhost:42618`) — direct-pod fetch with `Authorization: Bearer <preSharedToken>`, body unchanged. Prod is unaffected (selector prefers WS_GATEWAY_URL when set). **Agent-side wiring**: `renderAgentsTemplate` (which produces the ConfigMap-mounted `workspace/AGENTS.md`) gained a top-of-file "Convex-issued [system] directives (HIGHEST PRIORITY)" section. v2.31.0 documented the receiver-side rule in doctrine but never wired it into the agent's actual guidance — the pod's agent recognized `[system]` as "informational" and emitted "No reply (4591ms): system instruction, not a user message" instead of executing the embedded `praxis ...` command. The new AGENTS.md section tells the agent to extract the literal command after `Please run: ` and run it via the shell tool, with no conversational reply. Per `feedback_doctrine_drift_scope`: this is a contract violation the doctrine itself names — reconciliation lands in the same PR as the code fix. |
| 2.31.0 | 2026-05-18 | **§5.0 + §5.1 + new §5.6.1 + §16.8 + §16.9 rewrites — Spec 4b control-plane collapse** (`docs/tasks/completed/praxis-control-plane-collapse/praxis-control-plane-collapse-wiring-spec.md`). §5.0 polysemy guard extended: port-42617 `/webhook` `{message}` now carries an additional caller class — Convex `webhookDispatch.{sendValidationWebhook,sendExecutionWebhook}` with `[system]`-prefixed bodies. §5.1 communication-surfaces table updated: the "Convex-initiated control" column is REWRITTEN — the v2.30.0 dedicated-endpoint paragraphs (`/api/validate/spec`, `/api/execute/start`) are RETIRED (neither Rust handler ever shipped on the pod; the entire dedicated-handler track is collapsed onto `/webhook` `{message}` with a `[system]` prefix). NEW §5.6.1 — `[system]` prefix convention contract (receiver-side semantics, current directives, v1 trust posture, v2 hardening options, "do not add new dedicated handlers" rule cross-referencing praxis-doctrine §8). §16.8 (Spec Validation Flow Debugging) REWRITTEN top-to-bottom for the post-4b topology: trigger checks no longer mention `integrations_change` (listener retired), cache-hit vs cache-miss bisect added, webhook-dispatch outcome bullets replace the pre-4b `postValidate` skip-reason taxonomy, agent-recognition troubleshooting step added (`[system]` prefix may be ignored), projection-side reconciliation steps documented. §16.9 (Spec Execution Flow Debugging) REWRITTEN: `requestStart` returns the new `{ outcome: { status: "queued" \| "skipped" } }` envelope instead of the eager-row pattern; webhook-dispatch outcome bullets replace the `postExecuteStart` shape; "did the agent run the embedded command?" step added; projection-consumer step documents the lazy `executions` row insert. **No code changes** in this entry — purely doctrine reconciliation to the v2.36.0 backend + v1.11.0 praxis closure landed in the same PR. The doctrine-asserted contract (`/api/validate/spec` + `/api/execute/start` as live endpoints) was violated by code (the endpoints never existed and were just removed from the Convex client); this entry brings doctrine text into alignment per `feedback_doctrine_drift_scope`. |
| 2.30.1 | 2026-05-18 | **§5.0 new — `/webhook` polysemy guard; §10.2.3 — rejection-site observability rule** (high-leverage post-incident hardening from the 2026-05-18 email-flow 4XX session). §5.0 added as the first subsection of §5 with a side-by-side table contrasting the two `/webhook` paths: gateway `:42617` expects `{message}` and returns the LLM reply synchronously, vs webhook channel `:42618` expects `{sender, content}` and returns 200 immediately with reply posted back via `send_url`. Existed implicitly across §5.1 / §5.2 / line 104 / §9 routing flows, but the failure mode (agent mistakenly "fixes" `emailRelay.ts`'s correct `{sender, content}` payload after curling the wrong port and getting 400 expecting `{message}`) was real enough this session to warrant a top-of-section trap callout. §10.2.3 gains a binding rule: Convex HTTP routes the pod POSTs to MUST emit structured `console.warn` lines at every 4XX rejection site (route + status code + presence flags + token suffix only — no full token, no full body content). Reference implementation `/container-webhook` shipped in commit `dfb3d42` — pre-rule the prod 401 took multi-turn instrumentation to isolate; post-rule a single grep against Convex dashboard logs identifies the failure mode in one trip. **No code changes** in this entry — purely doctrine-hardening from the Pareto-selected drift items the session surfaced (W8 polysemy + A7 observability per session synthesis). |
| 2.30.0 | 2026-05-17 | **§5.1 + new §16.9 — `/api/execute/start` joins the Convex-initiated control endpoint class** (cross-ref praxis-doctrine §10 v1.10.1 + backend-doctrine §3.6 v2.35.0 + `docs/tasks/ongoing/praxis-spec-execution/praxis-spec-execution-wiring-spec.md`, Spec 3b of 2). §5.1 communication-surfaces table extended with the new endpoint; a paragraph after the table walks through the wake-up posture (same as `/api/validate/spec` — convex MUST NOT auto-wake; degraded-mode handling collapses pod absence to `skipped`; NO `executions` row persisted on the skip path so the user can click Run again once the upstream Rust handler ships). New §16.9 "Spec Execution Flow Debugging" runbook mirrors §16.8's six-step shape — Step 1 (latest row), Step 2 (daemon outcome via action logs), Step 3 (auth/ownership/active gate), Step 4 (rate-limit), Step 5 (projection consumer resolution warns), Step 6 (beadsForExecution lookup). **§9.1 filesystem tree** intentionally unchanged — `.praxis/` opacity rule preserved per v2.29.0 precedent (the executions/<id>.json on-disk detail stays in praxis-doctrine §5.3). **§5.7 / §17.4 unchanged** — no new agent-side capability; `praxis spec activate-execution` is daemon-internal per praxis-doctrine §10.10 (NOT on the agent surface). **In-PR drift reconciliation** per `feedback_doctrine_drift_scope`: `apps/clawcraft/test/unit/domain/claw-workspace.test.ts` byte-budget raised 4400 → 5200 and `PRAXIS_AGENT_COMMANDS` expected list extended (17 → 20) — Spec 3a added 3 verbs to the agent surface (`praxis execute --execution`, `praxis verify --execution`, `praxis update --state=done --output`) but did not update the snapshot/length assertions; rolled into this PR. |
| 2.29.0 | 2026-05-15 | **§9.1 filesystem ASCII tree now lists `.praxis/`** (one-line entry, opaque) so doctrine readers walking the pod layout can see where praxis lives. The §5.1 opacity rule is preserved — claw treats `.praxis/` as a black box; the detailed topology stays in `praxis-doctrine.md` §5.3. Cross-doctrine amendment landed alongside `docs/tasks/ongoing/praxis-specs-first-class/` per `feedback_doctrine_drift_scope` (reconcile in same PR, no deferred drift). |
| 2.28.0 | 2026-05-11 | **§5 Praxis path-shape contract added (load-bearing for dev parity).** New paragraph after the Praxis row spelling out the prod symlink chain: `/opt/praxis/` is the full npm-package directory, entry is `/opt/praxis/dist/bin-bootstrap.cjs`, `/usr/local/bin/praxis → /opt/praxis/dist/bin-bootstrap.cjs`. Any dev overlay of `/opt/praxis` MUST mount the whole package, NOT just `dist/` — otherwise the symlink resolves to a non-existent path. Cross-refs infra-doctrine §15.24. Caught by `zeroclaw-dev` Phase 1 audit; codifying so the next dev-loop iteration doesn't redo the debug cycle. |
| 2.27.2 | 2026-05-11 | **§16.7 logging anchor refreshed.** Was hard-anchored at "ZeroClaw v0.1.8-alpha-p2 does not emit request-path logs by default". Rewritten to be version-agnostic with an explicit re-verification note ("behaviour assumed to hold for current v0.6.9-alpha-p10 but not re-verified"). Cheaper than verifying the claim against the current Rust source right now, and the rephrasing prevents the version anchor from rotting further. |
| 2.27.1 | 2026-05-11 | **§16.6 clarified: `tbd shortcut` runs HOST-side, not in-pod.** Previous wording was location-ambiguous and could be read as "run this inside the pod" — but `tbd` is not in the runtime image (only `@soulbound-labs/praxis` is, at `/opt/praxis/`). Pin location to operator's host shell at the clawcraft repo root + cross-ref infra-doctrine §15.24. |
| 2.27.0 | 2026-05-11 | **§16.4 rewritten: pod is no longer distroless.** Heading dropped the "(Distroless)" qualifier. Body replaces the `kubectl debug` + busybox-sidecar dance with a direct `kubectl exec -it … -- bash`. Wolfi-base release stage (since sovereign-fork commit `1aac3308`) ships bash + coreutils + vim + git + nodejs, so debug-in-place works. Operators following the old wording were doing unnecessary ceremony. Surfaced by `/substrate:synthesize-session` during zeroclaw-dev synthesis. |
| 2.26.1 | 2026-05-09 | Praxis follow-through. §17.1: added `praxis-commands.ts` row to Files & Responsibilities. §17.4: `renderToolsTemplate` row updated to list "Praxis" between Memory and Web Search and point to `praxis-commands.ts:PRAXIS_AGENT_COMMANDS` as source. |
| 2.26.0 | 2026-05-09 | §5.7 added: Praxis as first-class agent-side capability alongside memory and web search. Pointer to `renderToolsTemplate` and `PRAXIS_AGENT_COMMANDS`. |
| 2.25.1 | 2026-04-22 | Monorepo migration: paths updated. |
| 2.25.0 | 2026-04-15 | **GCS Fuse file storage.** §2.1: added user files ownership row (GCS bucket + Fuse mount). §8.1: added `user-storage/` and `conversation-attachments/` GCS Fuse mount rows. §9.1: added GCS Fuse mount directories to pod filesystem layout. |
| 2.24.0 | 2026-04-15 | **Multimedia read support.** §5.4: added media file serving row (pod calls `GET /api/media-url` with preSharedToken auth + ownership check). §17.1: updated `claw-config.ts` description (gains `audio_read`, `vision_read` agents in `AGENT_REGISTRY`), `claw-workspace.ts` description (gains multimedia routing in TOOLS.md), added `media-constraints.ts` to domain files table. TOOLS.md gains "Multimedia Files" section — agent parses `[MEDIA_UPLOAD]` context blocks, resolves signed URLs via `/api/media-url`, downloads to PVC, delegates to `vision_read`/`audio_read` sub-agents. AGENTS.md gains `vision_read` and `audio_read` rows. |
| 2.23.0 | 2026-04-10 | **v2.0.0 PVC mount topology alignment.** Pod filesystem consolidated from 3 dirs to 2. §2.1: ownership table paths updated — brain.db at `/zeroclaw-data/workspace/memory/brain.db`, system files at `/zeroclaw-data/workspace/system`, agent-owned at `/zeroclaw-data/workspace`. §7.2: security note updated — `/zeroclaw-data/workspace/system/` (was `/system`). §8: workspace files contract paths updated — system files at `/zeroclaw-data/workspace/system/` (was `/system`), agent-owned at `/zeroclaw-data/workspace/` (was `/workspace`). `ZEROCLAW_SYSTEM_DIR` path updated. §9.2: "Net effect" paragraph updated for v2.0.0 topology. §16.4: debugging runbook paths updated — config path corrected to `/zeroclaw-data/.zeroclaw/` (was `/zeroclaw-config/`), workspace listing split into system files and agent files. |
| 2.21.0 | 2026-04-10 | **Split-mount topology & agent-owned workspace files.** §2.1: ownership table — agent memory is solely brain.db on PVC; `memories` and `brain_memories` tables marked STALE (pending removal). Workspace files split into system (ConfigMap at `/system`) and agent-owned (PVC at `/workspace`). §7.2: tool policy updated to reflect actual state — fully permissive (`non_cli_excluded_tools = []`, all tools auto-approved). §7.3: `ToolPolicy` type noted as unused. §8: workspace files contract rewritten — system files (IDENTITY, SOUL, AGENTS, TOOLS, BOOTSTRAP) at `/system` readOnly; agent-owned files (USER.md, MEMORY.md) created by agent on PVC at `/workspace`. `buildWorkspaceFiles` no longer renders MEMORY.md or USER.md. §8.2: rendering inputs updated (no `MemoryEntry[]`). §9: pod filesystem rewritten for three-mount topology (`/system`, `/workspace`, `/user`, `/zeroclaw-config`). `ZEROCLAW_WORKSPACE` changed from `/zeroclaw-data/workspace` to `/workspace`. `ZEROCLAW_SYSTEM_DIR=/system` added. Alignment rules updated for new paths. |
| 2.20.1 | 2026-04-09 | **Full resource reconciliation on all pod lifecycle paths.** Previously only `provisionUser` called `ensureService` and `ensureNetworkPolicy`; `scaleUp` and `restartPodForIntegration` only updated ConfigMap + Deployment, so port changes and NetworkPolicy updates did not propagate on pod restarts. Now all three paths reconcile the full resource set (ConfigMap, Service, NetworkPolicy, Deployment). `ensureService` and `ensureNetworkPolicy` are now exported from `apps/clawcraft/convex/clients/gke.ts` and called by `podActions.ts` (`scaleUp`) and `integrationActions.ts` (`restartPodForIntegration`). §1: added `apps/clawcraft/convex/podActions.ts` and `apps/clawcraft/convex/integrationActions.ts` to reference implementation list. §12.2: regeneration triggers updated — `scaleUp` and integration connect/disconnect rows now show `ensureService` + `ensureNetworkPolicy` in the action column. §12.3: new rule — MUST reconcile full resource set on every lifecycle path. §16.2: `scaleUp` action steps updated — step 4 added (Service + NetworkPolicy reconciliation), subsequent steps renumbered. |
| 2.20.0 | 2026-04-09 | **Webhook channel port separation & config fix.** §2: architecture diagram updated — Nginx relay path now shows `pod:42618` (webhook channel) instead of `/webhook`, `X-Webhook-Secret` removed. §2.1: ownership table — email relay sends `{ sender, content }` (channel webhook format). §3.2: Email adapter description updated for port 42618 and payload format. §3.3: `[channels_config.webhook]` port changed from 42617 to 42618, `secret` line removed (HMAC disabled; auth via nginx + NetworkPolicy). §4.2.1: emailRelay exception updated — port 42618, `{ sender, content }` format, HMAC disabled. §5: pod API surface notes both ports (42617 gateway, 42618 webhook). §5.2: `/webhook` endpoint annotated with port 42618. §5.4: email relay row updated for port 42618. §6.1: webhook channel secret intentionally omitted — network-layer auth. §9.2: webhook channel port row added. §9.2.1: webhook port alignment rule added (`WEBHOOK_PORT`). §10.2.2: email relay flow updated — `{ sender, content }` format, port 42618, response path via `/container-webhook`. §10.2.3: new section — `/container-webhook` dispatch documentation (4 body shape cases including channel webhook response `!body.type && !body.userId && body.content`). §13: Nginx→Pod trust boundary updated — no HMAC, auth is nginx auth_request + NetworkPolicy. §17.2: `WEBHOOK_PORT = 42618` constant added, `webhookEndpoint` field added to `ClawPodIdentity`. §17.3: `[channels_config.webhook]` table row updated — port 42618, secret omitted. |
| 2.19.0 | 2026-04-09 | **Email relay via Nginx gateway.** §2: architecture diagram updated — Nginx gateway now shows `/relay/{userId}` (HTTP POST → pod `/webhook`) and `/ws/relay/{userId}` (WS → pod `/ws/chat`) location blocks. §3.2: Email adapter description updated — gateway relay to `/webhook` replaces `relayToPod()`. §3.3: `[channels_config]` and `[channels_config.webhook]` are now always emitted (was conditional on Telegram). §4.2.1: `emailRelay.ts` no longer uses `relayToPod()` — has its own `relayViaGateway()` that routes through Nginx gateway with `X-Relay-Token` + `X-Webhook-Secret` auth. §5.2: `/webhook` use case updated from "Legacy/testing only" to "Email relay (via gateway), webhook channel". §5.4: Email relay row added to Clawcraft usage table. §6.2: `GATEWAY_RELAY_TOKEN` and `WS_GATEWAY_URL` credentials added. §10.2.2: new section — Email gateway relay message routing flow. §12.1: ConfigMap contents updated — `[channels_config]`/`[channels_config.webhook]` always present. §13: Convex→Nginx gateway relay and Nginx→Pod `/webhook` trust boundaries added. §17.3: `buildConfigToml` gains `convexSiteUrl` param (from `process.env.CONVEX_SITE_URL`). TOML table: `[channels_config]` now always present, `[channels_config.webhook]` added (always present). Terraform: `gateway_relay_token` variable added for Nginx ConfigMap. |
| 2.18.0 | 2026-04-07 | **Email v2 — Cloudflare Email Routing migration.** §3.2: Email adapter flow updated (CF Email Routing → Worker → Convex → opaque notify → pod pull). §6.2: Removed `MAILGUN_SIGNING_KEY` credential (Mailgun no longer used). §13: Mailgun→Worker trust boundary replaced with CF Email Routing→Worker (internal routing, no HMAC). |
| 2.17.0 | 2026-04-06 | **Email integration.** §3.2: Email adapter added (Live). Flow: Mailgun → Cloudflare Worker (HMAC verify, attachment upload) → Convex `/email-webhook` (Bearer auth) → `emailRelay.handleInbound` (relay with retry). §4.2.1: `RelayChannel` gains `"email"`. §6.2: `MAILGUN_SIGNING_KEY` and `EMAIL_WEBHOOK_SECRET` credentials added. §8.1: `renderToolsTemplate` gains `agentEmailAddress` param — email inbox section in TOOLS.md (conditional). §13: Mailgun→Worker and Worker→Convex trust boundaries added. §17.1: `email-address-generator.ts` and `email-threading.ts` added to domain reference. `buildWorkspaceFiles` gains `agentEmailAddress` param. |
| 2.16.0 | 2026-03-31 | **Slack integration removed.** §2: removed Slack from architecture diagram. §3.2: Slack adapter status changed from "Live" to "Removed (will be re-added in a future release)". §6.2: removed Slack credential rows. §10.1.1: removed Slack references from dormant relay path. §13: removed Slack trust boundaries. Slack will be re-added in a future release. |
| 2.15.0 | 2026-03-26 | WebSocket chat gateway. §2: architecture diagram updated — Nginx WS gateway between browser and pod for streaming web chat. §2.2: new subsection — WS flow (browser → wss://gw.clawcraft.ca → Nginx auth_request → Convex session validation → pod /ws/chat). §3.2: Web adapter updated to WS gateway. §5.4: web chat now uses WS `/ws/chat`, `/api/chat` still used by Slack/scheduled tasks. §5.5: streaming web chat removed from unused opportunities. §10.1: rewritten for WS streaming; old relay path moved to §10.1.1 (dormant for web). §13: trust boundaries updated — Browser→Nginx (session JWT), Nginx→Pod (no auth, ClusterIP), `clawcraft-system`→Pod (NetworkPolicy). §15.1: new — always-on pod policy (idle scale-down cron removed). §15.2/§15.3: renumbered. |
| 2.14.0 | 2026-03-25 | Native Telegram channel config delivery via ConfigMap. §2: architecture diagram updated — Telegram adapter now shows "Native/long poll" instead of webhook operations. §2.1: ownership table updated — Telegram bot token exception for ConfigMap injection. §3.1: adapter rules updated with Telegram credential exception. §3.3: TOML after block shows `[channels_config]`/`[channels_config.telegram]` sections. §3.4: rewritten — token delivery changed from K8s Secret + env var to inline `config.toml` via `buildConfigToml`; section name corrected to `[channels_config.telegram]`; `validateAndSetup` now only calls `deleteWebhook` (no `setWebhook`). §5.6: rules updated with Telegram native exception. §6.1: credential isolation rules updated — Telegram bot token is now in ConfigMap (deliberate trade-off, no TOML env var interpolation). §6.2: Telegram bot token decrypted by `provision`/`scaleUp`/`restartPodForIntegration`. §10.2: split into native mode (live) and webhook mode (dormant). §11.2: auto-pair flow removes `setWebhook`, adds `deleteWebhook` + ConfigMap with `[channels_config.telegram]`. §12.1: ConfigMap contents updated. §12.2: integration trigger updated. §13: added pod → Telegram API trust boundary. §14: webhook race gap marked moot. §15.2: updated for Telegram exception. §16.1: Telegram webhook URL row replaced with native mode restart requirement. §17.3: `buildConfigToml` gains `telegramBotToken` param; TOML table adds `[channels_config]` and `[channels_config.telegram]`. |
| 2.12.0 | 2026-03-24 | Per-app capability hints. §11.6: added `COMPOSIO_APP_HINTS` registry documentation — per-app one-liners in TOOLS.md preventing misuse (e.g., Calendly ≠ calendar). §8.1: TOOLS.md description updated. New "Sending caution" rule: agent must confirm before sending emails or creating scheduling links. |
| 2.11.0 | 2026-03-24 | Composio horizontal expansion (spec v2.0.0). §11.5: revised — generic functions (`initiateGenericComposioOAuth`, `completeGenericComposioOAuth`) coexist alongside Sheets-specific functions. Google Sheets keeps its dedicated flow. §11.6: updated registry — per-service Google providers (gmail, google_docs, google_slides, google_calendar) replace unified google_suite. Dropped google_maps. §6.2: all 7 Composio auth config env vars documented. §17.4: removed `platformComposioApps` from `renderToolsTemplate` signature. |
| 2.10.0 | 2026-03-24 | Composio horizontal expansion (spec v1.0.0, superseded by v2.0.0). |
| 2.9.0 | 2026-03-23 | Observability & logging. §16.7: new section — agent loop logging via `[observability]` config (`backend = "log"`, `runtime_trace_mode = "rolling"`). Documents LogObserver events (Agent_start, Llm_request, Agent_end, tool.start, tool.call), JSONL runtime trace, RUST_LOG behavior, and 408 debugging pattern. §16.4: updated stale note — ZeroClaw no longer "logs nothing" when observability is enabled. §17.3: TOML table updated — added `[security.otp]`, `[logging]`, `[observability]` sections; added `request_timeout_secs = 300` and `timeout_secs = 300` to `[gateway]`. |
| 2.8.0 | 2026-03-23 | Native composio tool hardening. §4.1: relay timeout bumped 120s→180s (multi-step composio tool chains exceed 120s). §7.2: `composio` now unconditionally auto-approved (was conditional on API key). §8.1: TOOLS.md Google Sheets section gains `text` parameter prohibition and concrete example flow (discovery→search→read) — fixes model bypassing structured params via NLP text field. §5.6: `/api/composio/execute` proxy removed, replaced by `/api/integration-status` thin bridge. |
| 2.7.0 | 2026-03-23 | Google Sheets disconnect documentation + bug fixes. §11.5: new section — Composio/Google Sheets disconnect flow (delete connection, delete record, restart pod). §12.2: fixed stale disconnect action — was `scale down → scale up`, now `reconcileDeployment` (rolling update). Bug fix: `restartPodForIntegration` error handler was hardcoded to `provider: "telegram"` — now surfaces errors on any remaining integration for the user. |
| 2.6.0 | 2026-03-22 | Post-deploy doctrine sweep. §8: fixed workspace mount path (`/zeroclaw-data/workspace/<filename>`, was `/zeroclaw-data/`). §6.1: ConfigMap contents description updated to include tool timeout overrides. §16.4: debugging runbook rewritten — uses AR busybox image (not alpine), config path updated to `/zeroclaw-config/config.toml`, workspace paths corrected, added config override verification steps. |
| 2.5.0 | 2026-03-22 | Normalize-claw-config alignment. §17.3: TOML section table updated — removed phantom `request_timeout_secs` from `[gateway]`, added `timeout_secs = 120` to `[http_request]` and `[web_search]`, added new `[web_fetch]` section. §12.1: ConfigMap contents updated to mention tool timeout overrides. §12.3: restart rule updated to explain init container config copy mechanism. |
| 2.4.0 | 2026-03-22 | Config override via init container. §9.1: added /zeroclaw-config/ filesystem tree. §9.2: added ZEROCLAW_CONFIG_DIR and ZEROCLAW_WORKSPACE to env var tables. §9.2.1: config.toml alignment rule updated — now mounts via init container + emptyDir, not subPath at image-baked path. |
| 2.3.0 | 2026-03-22 | Domain layer reference. §17: new section — complete API surface for all claw-relevant domain functions (`claw-pod-identity.ts`, `claw-config.ts`, `claw-workspace.ts`, `user-errors.ts`, `schedule-limits.ts`, `telegram-validation.ts`). Includes types, function signatures, constants, and consumer mapping. §7.2: fixed stale tool counts — `composio` now in SAFE_ENABLES (20 excluded, 5 enabled, 6–7 auto-approved). |
| 2.2.0 | 2026-03-21 | Composio tool proxy. §5.6: pod MAY call `/api/composio/execute` using pre-shared token for Composio tool execution (Google Sheets). New HTTP routes: `/composio-oauth-callback` (GET, OAuth redirect), `/api/composio/execute` (POST, pod-auth). Google Sheets is a tool integration, not a channel adapter — pod calls Convex which proxies to Composio. |
| 2.1.0 | 2026-03-19 | Telegram disconnect clean state zero. §11.4: rewritten — `telegram_identities` now DELETED on disconnect (was preserved). Stale identities with wrong chatId format or role blocked re-pairing. §14: webhook URL race marked as fixed — `validateAndSetup` now calls `deleteWebhook` before `setWebhook`, making disconnect/reconnect idempotent. `consumePairingCode`: re-pairing now updates existing identity when userId or role differs (was silently skipped). |
| 2.0.0 | 2026-03-19 | Scheduled task execution now uses `relayToPod()`. §1: added `apps/clawcraft/convex/scheduledTaskActions.ts` to reference implementation list. §4.2.1: scope expanded from "channel adapters" to "all code that relays messages to the pod" — explicitly includes scheduled task execution. Three bugs fixed: (1) `executeOneTask` used raw `fetch()` bypassing `relayToPod()` — zero relay_logs, zero observability. (2) `handleTaskFailure` used stale `task.threadId` from DB query instead of the local variable — failure messages inserted with `threadId: undefined`, invisible in UI. (3) Channel delivery (Telegram, Slack) was gated on pod relay success — failures produced zero notifications on any channel. Now: failures are relayed to all configured channels via `fanOutToChannels()`. |
| 1.9.0 | 2026-03-19 | Unified relay helper. §4.2.1: new section — `relayToPod()` in `apps/clawcraft/convex/relayHelpers.ts` is the canonical implementation of the unified relay fetch. All channel adapters MUST call it instead of inline fetch. Owns: fetch, timing, error classification, podState escalation, structured logging to `relay_logs` table. Bug fix: Telegram and Slack now escalate `podState: "error"` on connection failures (was only web). |
| 1.8.0 | 2026-03-19 | Workspace file mount path correction. §9.1: completely rewritten — directory structure now shows all files under `$ZEROCLAW_WORKSPACE` (`/zeroclaw-data/workspace/`), not `$HOME`. Added `MEMORY_SNAPSHOT.md`, `cron/jobs.db`, `state/memory_hygiene_state.json`. Critical note: workspace `.md` files MUST mount at `workspace/*.md`, not at root — `build_system_prompt` only searches `workspace_dir`. §9.2: added default value column; documented `ZEROCLAW_WORKSPACE` default (`~/.zeroclaw/workspace`, schema.rs:6428-6434). §9.2.1: alignment rules rewritten as concrete path equations instead of conditional statements. |
| 1.7.0 | 2026-03-19 | Slack adapter live + OAuth state-encoded redirects + scheduling feature. §3.2: Slack adapter status changed from "Planned" to "Live" (`slackRelay.ts`). §6.2: added Slack credential locations. |
| 1.6.0 | 2026-03-18 | Pod Operations Runbook. §16: new section for Claude Code agents and operators. §16.1: what requires restart vs hot reload. §16.2: single pod redeploy via `podActions:scaleDown`/`scaleUp` CLI. §16.3: bulk redeploy. §16.4: distroless pod debugging (kubectl debug, brain.db inspection, key paths). §16.5: health checks. §16.6: pointer to `tbd shortcut claw-brain-debugging`. |
| 1.5.0 | 2026-03-18 | Complete Pod API Surface documentation. §5 rewritten from 2-row table to full endpoint catalog: §5.1 communication surfaces matrix (HTTP/WebSocket/channel webhooks × single-turn/multi-turn/streaming/introspection). §5.2: 8 message endpoints with agent loop, tools, memory, session, streaming columns. §5.3: 17 introspection endpoints (`/api/memory`, `/api/config`, `/api/tools`, `/api/cron`, `/api/events`, etc.). §5.4: current Clawcraft usage. §5.5: unused opportunities (`/ws/chat` streaming, `/api/memory` dashboard viewer, `/v1/chat/completions` OpenAI proxy, PUT `/api/config` runtime updates, `/api/events` SSE). §5.6: rules updated — clarify pod has built-in channel endpoints but Clawcraft routes all channels through Convex. |
| 1.4.0 | 2026-03-18 | Add brain memory sync (read-only). §2.1: new ownership row for brain memory visibility. §5.1: pod MAY call `/api/brain-memories/sync`. §8.1: TOOLS.md includes brain sync instructions. New `brain_memories` Convex table + HTTP endpoint for pod-pushed snapshots. |
| 1.3.0 | 2026-03-18 | Add §9.2 "Environment Variables" documenting ZeroClaw image env vars (`HOME`, `ZEROCLAW_WORKSPACE`, `ZEROCLAW_GATEWAY_PORT`) and Clawcraft-injected env vars (`CLAW_USER_ID`, `CLAW_CONVEX_URL`, `CLAW_CONVEX_TOKEN`). §9.2.1 alignment rules for mount paths and port sync. Existing §9 content moved under §9.1. |
| 1.2.0 | 2026-03-18 | Enable memory tools on non-CLI channels. §7.2: `memory_store`/`memory_forget` moved from excluded to enabled (21 excluded, 4 enabled, 6 auto-approved). §7.3: removed known gap section (was memory tools). §14: rnk-veaw marked as fixed. |
| 1.1.0 | 2026-03-18 | Fix PVC mount path. §9: removed §9.1 PVC mount gap — PVC now mounts at `/zeroclaw-data/workspace/memory/` (was `/zeroclaw-data/memory/`). Filesystem layout corrected: removed orphaned `/zeroclaw-data/memory/` entry. §14: rnk-7iol marked as fixed. |
| 1.0.0 | 2026-03-18 | Initial claw doctrine. Covers control plane / compute plane boundary, channel adapter model, unified relay, pod API surface, credential isolation, tool policy, workspace files, pod filesystem, message routing, pairing model, ConfigMap lifecycle, trust boundaries, known gaps. |
