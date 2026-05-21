---
type: is
id: is-01ks5prcn71t0a611f43gbp2ct
title: Step 2 — Per-message key derivation at all backend touchpoints
kind: task
status: open
priority: 1
version: 3
labels:
  - ws-context-fix
dependencies:
  - type: blocks
    target: is-01ks5prny58hezqbzfhgk1hx8d
created_at: 2026-05-21T16:44:02.598Z
updated_at: 2026-05-21T16:44:47.296Z
---
Mechanical refactor — localize the choice of session key behind one helper, swap every backend touchpoint. NO BEHAVIOR CHANGE: thread-key falls back to connection session_key when thread_id is None, which matches today's flow.

DELIVERABLES
- ws.rs: pick_session_key(thread_id: Option<&str>, fallback: &str) -> String helper.
- Swap backend.append sites: ws.rs:370 (first-message user), ws.rs:479 (main-loop user), ws.rs:568 (assistant response inside process_chat_message).
- Extend process_chat_message signature: add thread_id: Option<&str> param. Update both call sites (ws.rs:372, ws.rs:482) to pass it through.
- Swap state setter sites: ws.rs:525, ws.rs:607, ws.rs:620 (set_session_state running/idle/error) — use the same derived key.
- Swap session_queue.acquire (ws.rs:461) — lock per thread, not per random connection.
- DEFER: session_name set/get at ws.rs:281, 287 — move to step 3 (no thread_id known at connection-open).

VERIFY: cargo build + cargo clippy -- -D warnings + cargo test. Behavior must be identical to pre-step-2 (random session_id path still active because hydration hasn't moved).

RISK: low. Refactor with fallback. Shippable as a standalone no-op PR if you want a safe rollback boundary.
