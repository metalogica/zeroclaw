---
type: is
id: is-01ks5prny58hezqbzfhgk1hx8d
title: Step 3 — Move hydration from connection-open to first-message-with-threadId
kind: task
status: open
priority: 1
version: 3
labels:
  - ws-context-fix
dependencies:
  - type: blocks
    target: is-01ks5prwrm962tv0pp34h48j08
created_at: 2026-05-21T16:44:12.101Z
updated_at: 2026-05-21T16:44:47.404Z
---
The actual behavior change. Defers history hydration from socket-open to the first inbound message that carries a threadId, then loads gwt_<tid> instead of gw_<random-uuid>.

DELIVERABLES
- ws.rs:271–289: gate the connection-open backend.load. Track has_explicit_session_id = params.session_id.is_some() before the unwrap at line 221. Only run the legacy hydration when has_explicit_session_id is true (preserves CLI/test-harness direct connects).
- ws.rs: after parse_thread_id at line 365 AND line 474, add a first-message hydration block. On Some(tid):
    1. Load backend.load(&session_key_for_thread(tid))
    2. If hydrated_thread is None → seed_history (covers empty + populated)
    3. If hydrated_thread.is_some() && != tid → replace_history (thread switch mid-socket)
    4. Optionally send {type: 'thread_resumed', thread_id, message_count} frame to client
- Move session_name set/get (deferred from step 2) into the first-message hydration block, keyed by gwt_<tid>.
- Document the legacy path: comment block referencing clawcraft:relay.ts:43 explaining why connection-open hydration is normally a no-op.

VERIFY: connect WS without session_id, send msg with threadId=t1, verify backend.load('gwt_t1') was called (instrument with tracing::debug if needed). cargo test must pass.

RISK: medium. Hydration ordering change. The session_start frame at ws.rs:292 now goes out with resumed: false / message_count: 0 always — clients that relied on those values must update.
