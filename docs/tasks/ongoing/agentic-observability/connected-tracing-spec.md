---
title: Connected Agentic Tracing (SPEC 1 — Emission + Trace Lifetime)
description: Turn zeroclaw's orphan OTel spans into one connected, replayable trace per agent activation
author: Rei (with Claude)
---
# Feature: Connected Agentic Tracing — one trace per activation

**Date:** 2026-06-01 (last updated 2026-06-01)

**Author:** Rei

**Status:** Draft

> This is **SPEC 1** of the 3-spec Agentic Observability program (see the brief).
> SPEC 2 (self-hosting Laminar: docker-compose dev / GKE prod) and SPEC 3 (the Go
> redaction collector/sidecar) are **separate tracks** and add no Rust lines here.
> Scope here = **emission + trace lifetime inside the `zeroclaw` runtime only**.

---

## Overview

zeroclaw already ships a working OpenTelemetry layer: the `opentelemetry*` crates are
vendored behind the `observability-otel` feature, `ObservabilityConfig` exposes an
env-driven OTLP endpoint, and `OtelObserver` already exports spans and 16 metric
instruments. The metrics are good. **The traces are not.**

Every span today is built **detached** — `tracer.build(SpanBuilder::from_name(...)).end()`
(`src/observability/otel.rs:238, 269, 313, 348, 374`) — so each `llm.call`, `tool.call`,
`agent.invocation`, and `error` becomes its *own* single-span trace with its own
`trace_id` and no parent. In Laminar you see a firehose of orphan spans, never one
connected thread. The brief's whole optimization function — *replay any activation
end-to-end without hand-stitching* — is unreachable by configuration alone.

This feature introduces an **activation-scoped trace**: one root span per agent
activation, with every LLM / tool / hand span parented beneath it, carrying the
`thread_id` and trigger, closing when the terminal reply is dispatched.

## Goals

- One **connected trace per agent activation** (trigger → reasoning → actions → output),
  rooted at the agent runtime's channel ingress (per brief C1).
- Children (`llm.call`, `tool.call`, `hand.run`, `error`) share the activation's
  `trace_id` and are parented to the activation root span.
- **`service.name = zeroclaw`** remains the sole emitter (brief C2). No new emitters.
- A **new user turn in the same thread = a separate trace** sharing `thread_id`
  (grouping attribute, never trace identity).
- A `self_schedule` activation = a trace grouped under a synthetic key.
- Concurrent activations never cross-link (independent guards, no shared span state).
- The interface change is **additive and behavior-neutral at Phase 1** — the 7 non-OTel
  observers compile unchanged via a default no-op guard.

## Non-Goals

- **praxis spans.** `grep praxis src/` returns zero matches — praxis is not invoked in
  the codebase today. `praxis.<cmd>` + exit-code spans are deferred until that
  integration lands (it will route through `execute_one_tool`, so the seam is ready).
- **Redaction.** Deferred per brief §5 — a hard dev→prod gate, designed elsewhere
  (SPEC 3). No PII scrubbing in this spec; dev data only.
- **Laminar hosting / collector sidecar.** SPEC 2 / SPEC 3.
- **Browser/Vite, Convex internals, upstream relay hops.** Out per brief C1 (not agent
  intent / dark boundaries).
- **Large-payload by-reference store** for 38k-token prompts/reasoning — deferred to the
  enrichment phase; Tier A keeps spans lean.

## Background

**Locked design decisions** (resolved with the owner — do not re-litigate):

| Decision | Choice | Consequence |
|---|---|---|
| Context propagation | **Explicit activation guard** | the root span handle (`&dyn Span`) threads through `run` → `run_tool_call_loop` → `execute_one_tool`; no task-local magic |
| Root span ownership | **Owned by the guard** | No shared mutable state in the observer → concurrent activations are trivially isolated |
| Enrichment carrier | **Span attributes, no enum change** | `ObserverEvent`/`ObserverMetric` enums untouched → 7 observer impls untouched |
| Child span timing | **Real start→end spans** | Span opened before the call, handle held across the await, ended after — true wall-clock timing |

Why explicit-guard *simplifies* the hardest part: OTel's task-local `Context` does **not**
survive `tokio::spawn`, so the implicit approach would need re-attaching at the
`listener → queue → dispatch` boundaries in `channels/mod.rs`. An owned guard is instead
**minted on the far side of the spawn**, inside the dispatch handler
(`process_channel_message`), so it never crosses the queue. This is also consistent with
C1 (trace originates at agent ingress; the raw listener→queue wait is pre-ingress
plumbing and legitimately out of trace).

The cost accepted in exchange: **signature plumbing** down the `run_tool_call_loop` spine.

## Design

### Approach

1. Add a recursive **`Span`** abstraction to the `Observer` interface. The root span owns
   the trace and issues child spans parented to it. `Observer::start_activation` gets a
   **default no-op impl**, so only `OtelObserver` returns a real span.
2. **Strip span construction out of `OtelObserver::record_event`** (keep the metric
   counters/histograms). All span building moves into `OtelSpan`. This deletes
   the detached-span antipattern, including the backdated `agent.invocation`
   (`otel.rs:264-285`).
3. **Mint a root span at each activation owner** and thread it (`&dyn Span`) down to the
   LLM/tool call sites, which open real start→end child spans.
4. **Close the root** when the activation reaches quiescence + terminal reply (guard
   drop), not at inbound ACK.

### Components

| File | Change |
|---|---|
| `src/observability/traits.rs:156-188` | Add recursive `Span` trait, `Trigger` + `AttrValue` enums, `start_activation` default-no-op method on `Observer`, a `NoopSpan`. |
| `src/observability/otel.rs` | Implement `OtelSpan` (recursive); reduce `record_event` to metrics-only (remove span blocks at `:234-254, :269-285, :313-324, :348-357, :374-423`). |
| `src/observability/{log,verbose,prometheus,noop,multi,dora,runtime_trace}.rs` | **No change** — inherit default no-op guard. |
| `src/observability/mod.rs` | No factory signature change. |
| `src/agent/loop_.rs` | Mint root span in **both** activation owners: `run()` (`:3464`, CLI + cron) and `process_message()` (`:4469`, channel webhooks — reaches the loop via `agent_turn` at `:4790`). Add `parent: &dyn Span` to `run_tool_call_loop` (`:2275`) and the `agent_turn` chokepoint (`:2118`). Open `parent.child("llm.call")` around the provider call. Drop root near `AgentEnd` (`:4456`). |
| `src/agent/tool_execution.rs:37` | Add `parent: &dyn Span`; open `parent.child("tool.call")` around `tool.execute`. For the `delegate` tool, hand that `tool.call` span into the sub-loop as its parent (see delegate row). |
| `src/channels/mod.rs:2438` | Mint guard in `process_channel_message`; pass to `run_tool_call_loop` call at `:3006`. |
| `src/gateway/ws.rs:577` / `src/agent/agent.rs` | **Mint guard PER `message` frame** in the loop, just past the `effective` thread_id computation (`ws.rs:652`); `thread_id = effective`, `session_id` = grouping fallback. Agent (connection-scoped) owns the observer at `agent.rs:416`; the guard is per-message. Malformed / empty / non-`message` frames are rejected pre-agent and get **no trace** (per C1). |
| `src/gateway/mod.rs` | `/webhook` (`handle_webhook:1337` → `run_gateway_chat_simple:1289`) is **shallow** — root + one `llm.call`, no tool loop; mint there. The **channel webhooks** (WhatsApp `:1602`, Linq `:1709`, WATI `:1853`, Nextcloud Talk `:1944`) route via `run_gateway_chat_with_tools:1321` → `agent::process_message` and are **covered by the `process_message` mint** — these handlers need no separate mint. |
| `src/cron/scheduler.rs:321` | Covered transitively — `run_agent_job` calls `crate::agent::run()`, which mints the guard; pass a synthetic `thread_id` group key for `self_schedule`. |
| `src/tools/traits.rs:22` | Add **one defaulted** method `execute_with_span(&self, args, parent: &dyn Span)` whose default impl calls `self.execute(args)`. Non-breaking — every existing tool inherits it unchanged; **no ripple**. `execute_one_tool` calls this instead of `execute`. |
| `src/tools/delegate.rs:1156` | Sub-agent delegation — **nest** (decided). `DelegateTool` overrides `execute_with_span` to pass the received `parent` span into its sub-`run_tool_call_loop`, so the sub-agent's `llm.call`/`tool.call` spans nest beneath the parent's `delegate` span; recursion to any depth via the recursive `Span`. Replaces the inline `NoopObserver` for spans. Metrics behavior may stay as-is (span ≠ metrics). |

### API Changes

**New trait surface (`traits.rs`):**

A single **recursive `Span`** trait — any span can open children — so the root activation
span and a nested sub-agent span are the same type. This is what makes the delegate-nesting
decision (see Resolved Decisions) work to arbitrary depth.

```rust
/// What started the activation. Sets the trigger attribute + thread semantics.
pub enum Trigger { WebChat, Webhook, Sms, Telegram, SelfSchedule, Cli }

/// Attribute value carried onto spans (kept non-generic for object safety).
pub enum AttrValue { Str(String), Int(i64), Float(f64), Bool(bool) }

/// A span in the activation tree. The ROOT (returned by `start_activation`) owns the
/// trace; `.child()` opens descendants parented beneath. Real start→end timing: a span
/// opens on creation and ends on drop. Recursive — a `delegate` span can itself parent a
/// nested sub-agent's spans, to any depth.
pub trait Span: Send + Sync {
    fn child(&self, name: &str) -> Box<dyn Span>;
    fn set_attr(&self, key: &str, value: AttrValue);
    fn set_status(&self, ok: bool);
}

/// Added to the existing Observer trait — DEFAULT no-op so non-OTel impls are untouched.
pub trait Observer: Send + Sync + 'static {
    fn record_event(&self, event: &ObserverEvent);        // unchanged — metrics
    fn record_metric(&self, metric: &ObserverMetric);     // unchanged
    fn flush(&self) {}                                    // unchanged
    fn name(&self) -> &str;                               // unchanged
    fn as_any(&self) -> &dyn std::any::Any;               // unchanged
    /// Mint the root span for a new activation. Default: a no-op span.
    fn start_activation(&self, _trigger: Trigger, _thread_id: Option<&str>)
        -> Box<dyn Span> {
        Box::new(NoopSpan)                                // default
    }
}
```

The threaded handle is `&dyn Span` (the current parent): `run_tool_call_loop` opens
`parent.child("llm.call")`; `execute_one_tool` opens `parent.child("tool.call")`. For
delegation, the `delegate` `tool.call` span *is* the parent handed to the sub-loop.

**Object safety:** `start_activation`/`child` return `Box<dyn Span>`; attributes use a
concrete `AttrValue` enum (no generics). `dyn Observer` / `dyn Span` remain valid. ✅

**Span ≠ metrics:** span construction (this trait) is fully independent of metrics
(`record_event`). Consequence: nested sub-agent **spans** can be enabled without changing
sub-agent **metric** behavior — the delegate sub-loop can keep its current metrics
treatment while still nesting its spans.

**Span schema (Tier A):**

| Span | Parent | Attributes |
|---|---|---|
| `agent.activation` (root) | none | `trigger`, `thread_id?`, `provider`, `model`, status |
| `llm.call` | activation | `gen_ai.system`, `model`, `success`, real duration |
| `tool.call` | activation | `tool.name`, `tool.success`, real duration |
| `hand.run` | activation | `hand.name`, `hand.success`, `hand.findings` |
| `error` | activation | `component`, `error.message` |

`record_event` retains **all metric recording** (the 16 instruments) — only span
construction moves to the guard.

## Implementation Plan

### Phase 1: Interface + OTel guard (additive, behavior-neutral)

- [ ] Add `Trigger`, `AttrValue`, recursive `Span` trait, `NoopSpan` to `traits.rs`.
- [ ] Add `start_activation` default-no-op to `Observer`.
- [ ] Implement recursive `OtelSpan` in `otel.rs` (root span on construct, `.child()` parents via the span's `Context`, real start time on open / end on drop, root ends on drop).
- [ ] Reduce `OtelObserver::record_event` to metrics-only; delete the 5 detached-span blocks and the backdated `agent.invocation`.
- [ ] Unit tests: guard creates parented children; child shares root `trace_id`; drop ends spans without panic (unreachable-endpoint, mirroring `otel.rs:607-692`).
- [ ] **Gate:** `cargo build --features observability-otel` green; full test suite green; **no behavioral change** for the 7 non-OTel observers (no-op default). Merge-able alone.

### Phase 2: Plumbing + mint at ingress (traces connect)

- [ ] Add `parent: &dyn Span` to `run_tool_call_loop` (`loop_.rs:2275`), `agent_turn` (`:2118`), `execute_one_tool` (`tool_execution.rs:37`); update the production call sites (`loop_.rs:2137, 3968, 4278`; `channels/mod.rs:3006`).
- [ ] Open `parent.child("llm.call")` around the provider call; `parent.child("tool.call")` around `tool.execute` (real start→end; handle held across the await).
- [ ] **Delegate nesting:** special-case the delegate tool in `execute_one_tool` to hand its `tool.call` span into the sub-`run_tool_call_loop` (`delegate.rs:1156`) as parent; recursion handled by recursive `Span`. Do not change `Tool::execute`.
- [ ] Mint root spans at the 5 owners: `run()` (CLI + cron), `process_message()` (channel webhooks), `process_channel_message` (channels), `handle_socket` per message (WS), `handle_webhook` (shallow `/webhook`).
- [ ] Set `thread_id` + `trigger`; synthetic group key for `self_schedule`.
- [ ] Close root on terminal-reply quiescence (guard drop), **not** at ACK.
- [ ] **Gate:** local Laminar (or otel-tui) shows one connected trace per activation across all 4 ingress paths; concurrent activations isolated.

### Phase 3: Enrichment + Laminar verification

- [ ] OpenRouter reasoning → `llm.call` attribute/event (wire from the `reasoning_content` Thinking event, commit `02474cc03`).
- [ ] Composio metadata on `tool.call` (`log_…` id, toolkit, status) — set at `execute_one_tool` where the guard is now in scope.
- [ ] Large-payload by-reference handling decision for fat blobs.
- [ ] **Gate:** end-to-end replay in self-hosted Laminar; acceptance criteria (brief §7) demonstrably met.

## Testing Strategy

- **Unit (Phase 1):** parented-child trace_id equality; status propagation; drop-safety under unreachable endpoint.
- **Integration (Phase 2):** per-ingress trace shape (webhook / WS / channel / cron); a second turn in one `thread_id` = a *separate* trace; two concurrent activations produce two disjoint traces with no shared spans.
- **Lifetime:** assert the root span closes on terminal reply, not on inbound ACK (the async-gap invariant).
- **Regression:** existing `otel.rs` resilience tests (`:607-692`) and metric tests stay green.

## Rollout Plan

- **Phase 0 (no code, optional, de-risks SPEC 2):** flip `backend = "otel"` + `otel_endpoint` and build `--features observability-otel` to validate Laminar OTLP ingest and the dev/prod hosting wall *before* this PR. Yields metrics + orphan spans only — knowingly not the feature.
- **Phase 1** merges behind the feature flag with zero behavior change.
- **Phase 2** delivers connected traces in dev (local Laminar per SPEC 2).
- Prod enablement is gated on SPEC 3 redaction (hard dev→prod PII gate).

## Resolved Decisions

- **WS granularity → PER MESSAGE.** The user message is the primitive. Mint a guard per
  valid `type=="message"` frame in the loop (`ws.rs:577`), just past the `effective`
  thread_id computation (`:652`). One message = one activation = one trace; multiple
  messages on one socket = separate traces sharing the thread. Malformed / empty /
  non-`message` frames are rejected before reaching the agent → **no trace** (per C1).
- **Send across await → not a problem, by construction.** The `Span` trait is declared
  `Send + Sync`; OTel `BoxedSpan` is `Send + Sync`. The only `!Send` hazard in OTel Rust is
  `ContextGuard` from `cx.attach()` — the **explicit-span design never calls `attach()`**,
  so it never holds one. Confirmed at compile time in Phase 1; no design risk.

- **Delegate visibility → NEST (decided).** Sub-agent runs nest as child spans under the
  parent's `delegate` `tool.call` span, to arbitrary depth (recursive `Span`). Mechanism:
  special-case delegate in `execute_one_tool`; the `Tool::execute` trait is untouched.
  Sub-loop metrics behavior may stay as-is (span construction is independent of metrics).

### Activation owners (5) — definitive mint-point map

1. `agent::run()` (`loop_.rs:3464`) — CLI + cron/`self_schedule`.
2. `agent::process_message()` (`loop_.rs:4469`) — the 4 channel webhooks, via `agent_turn` (`:4790`).
3. `process_channel_message()` (`channels/mod.rs:2438`) — native channel listeners (Telegram/SMS/etc.).
4. `handle_socket` (`ws.rs:577`) — WS, **per message**.
5. `handle_webhook` simple path (`gateway/mod.rs:1337`) — shallow `/webhook`, LLM-only.

Production `run_tool_call_loop` call sites that receive `parent: &dyn Span`: `loop_.rs:2137`
(`agent_turn`), `:3968` + `:4278` (`run`), `channels/mod.rs:3006`, and `delegate.rs:1156`
(parent = the `delegate` `tool.call` span — nesting). All `:5825+` sites are `#[cfg(test)]`.

## Open Questions

- **OTLP protocol/port for Laminar:** exporter is HTTP/proto on `/v1/traces` (`otel.rs:42`) —
  confirm against Laminar self-host guide (gRPC vs HTTP). (SPEC 2 concern, noted here.)

## References

- Brief: Agentic Observability (this directory) — C1/C2, §4 locked decisions, §6 integration surface, §7 acceptance.
- `src/observability/otel.rs` — current detached spans (`:238,269,313,348,374`); resilience tests (`:607-692`).
- `src/observability/traits.rs:156-188` — `Observer` trait (object-safe).
- `src/agent/loop_.rs:2275` (`run_tool_call_loop`), `:3464` (`run`), `:3658`/`:4456` (AgentStart/End).
- `src/agent/tool_execution.rs:37` (`execute_one_tool`).
- `src/channels/mod.rs:2438` (`process_channel_message`), `:3006` (tool-loop call).
- `src/gateway/mod.rs:1337` (`handle_webhook`), `src/gateway/ws.rs:361` (`handle_socket`).
- `src/cron/scheduler.rs:256-334` (`run_agent_job` → `agent::run` at `:321`).
- `src/config/schema.rs:5362-5386` (`ObservabilityConfig`); `Cargo.toml` `observability-otel` feature.
