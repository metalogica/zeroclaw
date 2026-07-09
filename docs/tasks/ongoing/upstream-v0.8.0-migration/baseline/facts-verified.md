# Residual v0.8.0 fact verification (Phase 1, Step 1.3 — zc-m67s)

Verdicts on 5 residual upstream facts, verified against the `v0.8.0` tag (commit `5fc9d3c38`) with
`file:line` evidence. These bind downstream beads **3.3** (config-compat), **5.2** (Laminar wiring),
and **6.2** (provider patches). A wrong verdict causes real rework, so each is cited.

## Fact 1: are `otel_headers` applied to BOTH exporters?

**Verdict:** YES — exactly two OTLP exporters exist and both receive the configured headers. No
silent-drop path; no separate logs exporter.

**Evidence:**
- `crates/zeroclaw-runtime/src/observability/otel.rs:52-60` — Span exporter (`SpanExporter::builder().with_http()`); headers applied `:55-57` via `span_builder.with_headers(h.clone())`.
- `crates/zeroclaw-runtime/src/observability/otel.rs:74-82` — Metric exporter (`MetricExporter::builder().with_http()`); headers applied `:77-79` via `metric_builder.with_headers(h.clone())`.
- `crates/zeroclaw-runtime/src/observability/mod.rs:184` — passes `config.otel_headers.clone()` into `OtelObserver::new(endpoint, service_name, headers)`.
- `crates/zeroclaw-config/src/schema.rs:9365` — `pub otel_headers: Option<HashMap<String,String>>`.
- Negative check: no `LogExporter` / `/v1/logs` / `LoggerProvider` anywhere → no OTLP logs signal.

**Downstream impact:** bead 5.2 (`zc-jfun`, FD-07) does **NOT** need to add `.with_headers()` — the
§10#12 silent 100%-ingest-drop risk is already mitigated upstream. One fewer patch.

## Fact 2: OpenRouter `reasoning` alias?

**Verdict:** NO — v0.8.0 uses a field literally named `reasoning_content`; there is no `reasoning`
field and no `#[serde(alias = "reasoning")]`.

**Evidence:**
- `crates/zeroclaw-providers/src/openrouter.rs:122-125` — request `NativeMessage.reasoning_content: Option<String>` (skip_serializing_if; no alias).
- `crates/zeroclaw-providers/src/openrouter.rs:192-194` — response `NativeResponseMessage.reasoning_content: Option<String>` (serde default; no alias).
- `crates/zeroclaw-providers/src/openrouter.rs:97-111` — request struct has NO `reasoning` field.

**Downstream impact:** bead 6.2 (`zc-qskr`) — the fork's OpenRouter `reasoning` alias is **net-new**
(absent upstream); KEEP it, do not drop.

## Fact 3: images-field output extraction?

**Verdict:** NO — no provider parses generated images out of a chat response. Image handling is
input-only, plus one unrelated DALL-E tool call.

**Evidence:**
- `crates/zeroclaw-providers/src/gemini.rs:234-241` — response `ResponsePart` deserializes only `text`/`thought`; no `inline_data` on the response part (returned image bytes dropped).
- `crates/zeroclaw-providers/src/gemini.rs:150,183` — `inline_data` only on the **request** `Part::Inline`.
- `crates/zeroclaw-providers/src/openrouter.rs:73,357`, `copilot.rs:147,307`, `openai_codex.rs:345` — `image_url` used only to send inputs; response structs carry only text/reasoning_content/tool_calls.
- Negative check: `b64_json` appears only in `crates/zeroclaw-tools/src/linkedin_client.rs:1082,1102` (a DALL-E tool), not chat-response extraction.

**Downstream impact:** bead 6.2 (`zc-qskr`) — image-gen output extraction is **net-new**; KEEP the
fork's version.

## Fact 4: Thinking TurnEvents?

**Verdict:** YES for `TurnEvent` (a `Thinking { delta }` variant exists); NO for `StreamEvent`
(no Thinking variant).

**Evidence:**
- `crates/zeroclaw-api/src/agent.rs:8,12` — `pub enum TurnEvent { ... Thinking { delta: String }, ... }`.
- `crates/zeroclaw-runtime/src/agent/agent.rs:2637` — emits `TurnEvent::Thinking { delta: reasoning }`.
- `crates/zeroclaw-runtime/src/rpc/dispatch.rs:3336` — maps `TurnEvent::Thinking` → `SessionUpdateEvent::AgentThoughtChunk`.
- `crates/zeroclaw-gateway/src/ws.rs:1099` — gateway WS handles `TurnEvent::Thinking { delta }`.
- Negative check: `crates/zeroclaw-api/src/model_provider.rs:229-244` — `StreamEvent` = TextDelta, ToolCall, PreExecutedToolCall, PreExecutedToolResult, Usage, Final — no Thinking.

**Downstream impact:** bead 6.2 (`zc-qskr`) — Thinking events are **partially now-upstream** (present
at the `TurnEvent` layer, absent at `StreamEvent`); DROP the fork's TurnEvent-layer duplication,
re-verify the StreamEvent-layer gap before dropping anything there.

## Fact 5: `process_message` location + own tool loop?

**Verdict:** NO own loop — `process_message` delegates to the shared `agent_turn` loop.

**Evidence:**
- `crates/zeroclaw-runtime/src/agent/loop_.rs:4764` — `pub async fn process_message(config, agent_alias, message, session_id)`.
- `crates/zeroclaw-runtime/src/agent/loop_.rs:5335-5361` — terminal action wraps `agent_turn(..., agent.resolved.max_tool_iterations, ...)`; no `loop{}`/iteration in `process_message` itself.
- `crates/zeroclaw-runtime/src/agent/loop_.rs:1373` — `pub async fn agent_turn(...)`, the shared loop.
- `crates/zeroclaw-runtime/src/agent/loop_.rs:~1595` — `for iteration in 0..max_iterations` (call chat/stream → parse tool_calls → execute → re-prompt → break when empty).

**Downstream impact:** beads 3.3-continuation / `zc-rfri` / `zc-n1mx` — wiring the praxis continuation
guard into `agent_turn` (loop_.rs:1373) covers **both** the interactive `run()` path and
`process_message` automatically; `process_message` needs no separate guard wiring, only a coverage
check (which `zc-n1mx` performs).
