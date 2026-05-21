---
type: is
id: is-01ks5prwrm962tv0pp34h48j08
title: Step 4 — Socket-local state for thread-switch detection
kind: task
status: open
priority: 1
version: 3
labels:
  - ws-context-fix
dependencies:
  - type: blocks
    target: is-01ks5ps67qtqa58c1eg9yb65w8
created_at: 2026-05-21T16:44:19.092Z
updated_at: 2026-05-21T16:44:47.511Z
---
Adds the per-connection state that lets the WS handler distinguish first-thread-hydration from same-thread-no-op from thread-switch-mid-socket.

DELIVERABLES
- ws.rs: declare let mut hydrated_thread: Option<String> = None; near the existing session_key binding at line 240.
- ws.rs: the if hydrated_thread.as_deref() != Some(tid.as_str()) guard around the hydration block from step 3. After hydration succeeds, set hydrated_thread = Some(tid.clone()).
- Policy decision — message arriving WITHOUT threadId mid-thread. Default: treat as continuation of current hydrated_thread (append under existing key), log tracing::warn with the discrepancy. Documented in code comment. (Alternative tighter policy is to reject — explicitly chose forgiving option for v1.)
- Unit test or smoke test the branch logic if possible: first-thread / same-thread / different-thread.

VERIFY: cargo build + cargo clippy + cargo test.

RISK: low. State is one Option<String>. The forgiving no-threadId policy is reversible.
