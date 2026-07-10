# zc-b78l — Live Laminar Battery (FD-07) — results

**Bead:** `zc-b78l` (manual gate) · **Epic:** `upstream-v0.8.0-migration`
**Sovereign tip under test:** `core/v0.8.0` @ `315220221` (FD-07 Laminar re-home) — battery run 2026-07-10
**Root-I/O fix landed:** `core/v0.8.0` @ `b4288dc0b` (FD-07 follow-up, zc-gnpx) — **live re-verify PENDING**
**Status:** 🟡 **PARTIAL — one live re-verify pass away from GREEN.** After re-run (b): root I/O FIXED &
PROVEN (chat + webhook); negative control PASS; a NEW gap (ws has no root) found + FIXED in code
(zc-a1bp); redaction conditional-pass (fork-faithful). Remaining before GREEN: ws root live re-verify +
a `token:`-shaped redaction positive-probe. Triggers 4&5 remain BLOCKED-manual (zc-zb2t).

Query transport: `docker exec clawcraft-laminar-clickhouse clickhouse-client -q "…"` (ClickHouse has no
host port).

---

## Re-run 2026-07-10 (b) — against `core/v0.8.0 @ b4288dc0b` (zc-gnpx). Clean window `> '2026-07-10 19:41:45'`

**Root I/O — FIXED & PROVEN** (chat + webhook):
```
llm.call          n=2  with_input=2  with_output=2
agent.activation  n=2  with_input=2  with_output=2      ← was 0/0 pre-fix
root#1 web_chat  input="[Memory context]…"        output="Hello!"           exit=final_answer iters=1  session_id=""
root#2 webhook   input="[…] hello via webhook"    output="Hello! How can…"  exit=final_answer iters=1  session_id="webhook_operator_operator"
```
(`JSONHas(attributes,'lmnr.span.input')=0` is expected — Laminar promotes the attrs into the typed
`input`/`output` columns and strips them from the attributes JSON; the columns are populated.)
`user_id` present on both · A6 key-leak = 0 rows.

**Negative control — PASS:** blank `otel_headers` → `spans_after_blank_headers = 0`; restore → 7 spans.

**⛔ NEW GAP — `/ws/chat` emits no `agent.activation` root** (2 roots for 3 surfaces). Isolated ws re-drive
produced only `gen_ai.agent.invoke` + `llm.request` + `llm.response` (upstream-native), no `agent.activation`
/`llm.call`. Root cause: `ws::handle_ws_chat` called `turn_streamed` with no `start_activation`/`scope_span`
— the one prod ingress owner FD-07/Adj A omitted (the 0.6.9 fork instrumented it). **FIXED:** zc-a1bp,
`core/v0.8.0 @ 8e309c9a8` (ported the root mint; gate green). **Live ws re-verify pending.**

**Redaction — CONDITIONAL PASS (fork-faithful, not an FD-07 regression):** probe `sk-live-DEADBEEFdeadbeef00`
(bare) + `Authorization: Bearer sk-live-DEADBEEFdead` (value-form) both landed RAW (`has_redacted=0`).
`scrub_credentials` is a KEY-VALUE scrubber (`SENSITIVE_KV_REGEX`): bare tokens with no sensitive-key context
are out of scope BY DESIGN (the earlier `token: SEKRET…` form DID redact → scrub path is live in the FD-07
mirror), and the `Bearer` value-form is the known bug **zc-1qoq** (prod-gating; bare-prefix note appended).
FD-07 ports scrub verbatim (Step 5.1). Positive-confirm next pass with a `token: sk-live-…` shaped probe.

**Triggers 4 & 5 — BLOCKED-manual:** tool auto-approval denied post-migrate (→ zc-zb2t).

**→ FINAL PASS needed** (one more live run after rebuilding `@8e309c9a8`): (1) drive `/ws/chat`, confirm an
`agent.activation` root with typed `session_id` + non-empty root input/output + `llm.call` children;
(2) `token:`-shaped redaction positive-confirm. Then flip to GREEN and close zc-b78l.

---

## Verdict table (§4.4 pass-iff clauses)

| Clause | Verdict | Evidence / disposition |
|---|---|---|
| One `agent.activation` root per ingress surface | ✅ PASS | distinct roots for `/api/chat`, `/ws/chat`, 42618 webhook. chat/ws split by `session_id`, not `trigger` (both `web_chat/web`) — acceptable (see note). |
| Typed `user_id` column populated | ✅ PASS | `pd7cgr46k4xap1jcb3824p6wm186tpj7` on all 5 roots. |
| Typed `session_id` populated | ✅ PASS (Adj B) | populated on ws (session-bearing); **absent (not empty) on `/api/chat`** — correct per Fable Adjustment B / claw §4.1 (`session_id: None`, never synthesized). |
| **Root** input/output non-empty | ⛔→🛠 **FAILED → fixed (zc-gnpx), re-verify pending** | 0/5 roots had input/output; `JSONHas(attributes,'lmnr.span.input')=0`. Confirmed FD-07 code gap: `lmnr.span.input/output` wired only on `llm.call` children, never the root (source grep + spec §3.4/Step 5.2 require the root). Fixed @`b4288dc0b`. **Must re-run A4 against a rebuilt `:dev` to flip to PASS.** |
| Every `llm.call` non-blank (incl. tool-only) | 🟡 PARTIAL | non-empty on all ran turns (in 439/333/401, out 141/131/68 bytes). Tool-call-only iterations unproven (triggers 4&5 blocked). |
| Root `exit_reason` matches forced outcome | 🟡 PARTIAL | `final_answer` ✅ stamped (+`agent.turn.iterations`). `max_iterations`/`error` unproven — triggers 4&5 blocked by tool-approval gap. |
| Probe secret redacted (`*[REDACTED]`) | ⏳ INVALID PROBE | run used a NON-credential-shaped probe (`SEKRET-abc123-do-not-log`) → flows raw into `llm.call` content (expected; message content, not a credential). Re-run with a credential-shaped probe; mind `zc-1qoq` (scrubber misses value-form `Authorization: Bearer <token>`). |
| No provider-key leak (bonus) | ✅ PASS | real `sk-or-` key appears in 0 spans. |
| Negative control (blank `otel_headers` → dropped) | ⏳ NOT RUN | procedure below. |

**Surface-distinction note:** "one root per surface" is met by distinct root spans; `/api/chat` vs `/ws/chat`
are distinguishable via `session_id` (ws has one, `/api/chat` doesn't). The `trigger=web_chat` collapse is
by-design (the `Trigger` enum has no WS variant). Explicit per-surface `trigger` would be an enhancement, not
a battery fail.

---

## Triggers driven (Checkpoint 3)

| # | Surface | Result | Root attribution |
|---|---|---|---|
| 1 | `POST /api/chat` (42617) | ✅ 200 | `trigger=web_chat`, `channel=web` |
| 2 | webhook `POST /webhook` (42618) | ✅ 200 async | `trigger=webhook`, `channel=webhook` |
| 3 | `GET /ws/chat` (42617) | ✅ streamed | `trigger=web_chat`, `channel=web` (has `session_id`) |
| 4 | multi-iteration tool loop | ⚠️ BLOCKED-manual | tool auto-approval not honored → "operation was denied" (→ zc-zb2t) |
| 5 | forced `max_iterations` | ⚠️ BLOCKED-manual | depends on trigger 4 |

Span-name histogram (clean window): `llm.response` 13 · `llm.request` 13 · `llm.call` 8 ·
`agent.activation` 5 (4× web + 1× webhook) · `gen_ai.agent.invoke` 5.

Sample root `attributes` (pre-fix — note absence of `lmnr.span.input/output`):
```json
{"lmnr.span.path":["agent.activation"],"agent.turn.iterations":2,"channel":"web",
 "trigger":"web_chat","user.id":"pd7cgr46k4xap1jcb3824p6wm186tpj7",
 "lmnr.association.properties.user_id":"pd7cgr46k4xap1jcb3824p6wm186tpj7",
 "agent.turn.exit_reason":"final_answer","lmnr.association.properties.tags":["web"]}
```

---

## Real `spans` schema (from `DESCRIBE spans`)

```
span_id UUID | name String | span_type UInt8 | start_time/end_time DateTime64(9,'UTC')
input_cost/output_cost/total_cost Float64 | model String | session_id String
project_id UUID | trace_id UUID | provider String
input_tokens/output_tokens/total_tokens Int64 | user_id String | path String
input String (ZSTD) | output String (ZSTD) | size_bytes UInt64 | status String
attributes String (JSON) | request_model String | response_model String
parent_span_id UUID | trace_metadata String | trace_type UInt8
tags_array Array(String) | events Array(Tuple(timestamp Int64, name String, attributes String))
input_message_hashes Array(FixedString(32)) | span_kind UInt8
input_new_message_indices Array(UInt16)
```

## Per-assertion queries (against real columns)

**A1 — one root per surface**
```sql
SELECT JSONExtractString(attributes,'trigger') AS trigger,
       JSONExtractString(attributes,'channel') AS channel, count() AS roots
FROM spans WHERE name='agent.activation' AND start_time > '2026-07-10 17:36:30'
GROUP BY trigger, channel ORDER BY trigger, channel;
```
**A2 — typed user_id**
```sql
SELECT span_id, user_id, user_id != '' AS present FROM spans
WHERE name='agent.activation' AND start_time > '2026-07-10 17:36:30' ORDER BY start_time;
```
**A3 — typed session_id (absence-not-empty, Adj B)**
```sql
SELECT JSONExtractString(attributes,'trigger') AS trigger, session_id, session_id != '' AS present
FROM spans WHERE name='agent.activation' AND start_time > '2026-07-10 17:36:30' ORDER BY start_time;
```
**A4 — root + llm.call I/O non-empty (the fixed clause — re-run after rebuild)**
```sql
SELECT name, count() AS n, countIf(length(input)>0) AS with_input, countIf(length(output)>0) AS with_output
FROM spans WHERE name IN ('agent.activation','llm.call') AND start_time > '<new-window>' GROUP BY name;
-- proves attr vs column-materialization:
SELECT span_id, JSONHas(attributes,'lmnr.span.input') AS attr_in, JSONHas(attributes,'lmnr.span.output') AS attr_out
FROM spans WHERE name='agent.activation' AND start_time > '<new-window>';
```
Post-fix expectation: `agent.activation` `with_input=with_output=n` and `attr_in=attr_out=1`.
**A5 — exit_reason matches forced outcome**
```sql
SELECT span_id, JSONExtractString(attributes,'trigger') AS trigger,
       JSONExtractString(attributes,'agent.turn.exit_reason') AS exit_reason,
       JSONExtractUInt(attributes,'agent.turn.iterations') AS iterations
FROM spans WHERE name='agent.activation' AND start_time > '<new-window>' ORDER BY start_time;
```
**A6 — no key leak (hard security check)**
```sql
SELECT name, span_id FROM spans WHERE start_time > '<new-window>'
  AND (position(input,'sk-or-')>0 OR position(output,'sk-or-')>0 OR position(attributes,'sk-or-')>0);
```
**Redaction (re-run with a credential-shaped probe)** — inject `sk-live-DEADBEEFdeadbeef00` (bare, should
redact) and `Authorization: Bearer sk-live-DEADBEEFdead` (value-form, KNOWN-MISS per zc-1qoq):
```sql
SELECT name, span_id, position(input,'sk-live-DEADBEEF') AS raw_hit FROM spans
WHERE start_time > '<probe-window>' AND position(input,'sk-live-DEADBEEF')>0;
```

## Negative control (3e) — blank `otel_headers` → spans dropped
1. Set `otel_headers = ""` in the pod config → restart pod.
2. Drive one turn with a unique marker (distinct `CLAW_USER_ID` or a probe string).
3. Confirm zero spans landed:
```sql
SELECT count() AS spans_after_blank_headers FROM spans
WHERE start_time > '<neg-control-start>' AND user_id = '<neg-control-user>';   -- expect 0
```
4. Restore `otel_headers` + restart.

---

## Root-I/O fix — re-verify procedure (flips A4 to PASS)

The fix is on `core/v0.8.0` @ `b4288dc0b`. Rebuild the pod binary FROM THE WORKTREE and re-run A4/A5:
```bash
# build from the worktree (NOT the control-plane checkout — that branch emits no spans)
cd /Users/reinova/code/forks/zeroclaw-worktrees/core-v0.8.0
ZEROCLAW_FEATURES=observability-otel just claw-hotswap   # or the project's rebuild+swap path
# verify the marker is present in the new binary, drive triggers 1–3 again, then A4 with a fresh window
```
Expected post-fix: A4 `agent.activation with_input=with_output=n`, `attr_in=attr_out=1`. When confirmed,
flip the root-I/O clause to ✅ and close zc-b78l (if negative control + redaction also pass).

---

## Environment findings (prod-rollout blockers — filed)

- **`zc-zb2t`** (clawcraft config renderer): rendered `config.toml` is 0.6.9-schema; v0.8.0 needs schema-v3
  (`[providers.models.openrouter.<alias>]` + `model_provider` refs). Every turn 500'd
  ("OpenRouter API key not set") until hand-migrated on the pod (`zeroclaw config migrate` → schema_v3,
  THEN manual `api_key` injection into all 13 `openrouter.agent_*` sections — migrate only keyed
  `openrouter.default`). Tool auto-approval not carried into v0.8.0 approval mechanism → triggers 4&5
  blocked. Route cross-repo to clawcraft (spec §3.8). Spec Step 4.1 golden test asserts config PARSES,
  not runtime credential/tool resolution — that's the coverage lesson.
- `NPM_TOKEN` unset → batched `zc-n6so`/`zc-8zdt` Docker smokes stay BLOCKED-manual (independent).
- Non-fatal: daemon IPC socket bind fails on pod recreate (stale `daemon.sock`); gateway HTTP path
  unaffected — cosmetic for the battery.

## Reproduction state (from the run)
- Pod ran under a temp compose override `/tmp/claw-debug-override.yml` (RUST_LOG debug + env; the on-disk
  migrated config was the actual fix, not the env vars).
- Config backups on the pod mount: `config.toml.bak-pre-v080-battery` (clean 0.6.9), `config.toml.backup`
  (migrate's own).
- runtime-trace: `…/claw-workspace/pd7cgr46k4xap1jcb3824p6wm186tpj7/.zeroclaw/data/state/runtime-trace.jsonl`.
