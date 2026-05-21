---
type: is
id: is-01ks5ps67qtqa58c1eg9yb65w8
title: "Step 5 — Acceptance: verify fix end-to-end"
kind: task
status: open
priority: 1
version: 3
labels:
  - ws-context-fix
dependencies:
  - type: blocks
    target: is-01ks5pqv4bwc7pf1mb2cqmyxkt
created_at: 2026-05-21T16:44:28.790Z
updated_at: 2026-05-21T16:44:47.619Z
---
Functional acceptance against a running daemon + clawcraft web chat. Each test gates the next.

5.1 COMPILE + LINT
- cargo build -p zeroclaw
- cargo clippy -p zeroclaw -- -D warnings
- cargo test -p zeroclaw

5.2 SAME-SOCKET CONTINUITY (regression check — must STILL work)
- WS without session_id; send {content: 'my name is Alice', threadId: 't1'}; then {content: 'what is my name?', threadId: 't1'}
- Expect: assistant says 'Alice'

5.3 CROSS-SOCKET CONTINUITY (the actual fix)
- WS connection 1: {content: 'my name is Alice', threadId: 't1'}; close
- WS connection 2 (fresh socket): {content: 'what is my name?', threadId: 't1'}
- Expect: assistant says 'Alice'. Pre-fix this returns 'I don\'t know'.

5.4 THREAD ISOLATION
- sqlite3 <workspace>/state/<session-db>.sqlite "select session_key, count(*) from messages group by session_key;"
- Expect: rows keyed gwt_t1, gwt_t2, … never gw_<random-uuid> for post-fix sessions.

5.5 MEMORY RECALL STILL GLOBAL
- On threadId=t1: store a fact via memory_store
- Switch to threadId=t2; ask the fact back
- Expect: recall succeeds. Confirms memory_session_id axis untouched.

5.6 DISAMBIGUATION ACCEPTANCE (USER-FACING — the motivating test)
- Open thread, send context-establishing msgs ('I want to spec the inbox triage workflow')
- Close tab; reopen same thread
- Send 'create a spec for this'
- Expect: agent resolves 'this' → inbox triage workflow. Does NOT ask 'what is this?'

CLOSE the epic only after 5.6 passes.

RISK: pure verification. If a test fails, reopen the relevant step bead.
