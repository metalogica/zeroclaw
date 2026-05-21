---
type: is
id: is-01ks5pqv4bwc7pf1mb2cqmyxkt
title: ThreadId-keyed WS history persistence (fixes context-loss bug)
kind: epic
status: open
priority: 1
version: 2
labels:
  - ws-context-fix
dependencies: []
created_at: 2026-05-21T16:43:44.650Z
updated_at: 2026-05-21T16:44:47.065Z
---
Fix the root-cause bug where WS chat history is keyed by an ephemeral random session_id instead of the stable threadId. clawcraft:relay.ts:43 deliberately omits session_id (to preserve global memory recall), which makes every new WS connection create a fresh empty history bucket. The threadId field is already plumbed end-to-end (commit 8ee05706) but only used as a [thread_id: …] prose hint, not as a history key.

Goal: decouple memory-recall scope from history persistence. Memory recall stays global; history becomes per-thread and survives WS reconnects.

Scope: src/gateway/ws.rs + ~3 lines in src/agent/agent.rs. ~50 LOC total. Single file. Single commit (or two: refactor + behavior change).

Acceptance: opening a WS thread, sending msg, closing socket, reopening with same threadId, sending follow-up → the agent sees the original msg in its history and disambiguates references correctly.

Unblocks: [ASK_USER] directive UX, multi-turn workflows, spec authoring across messages, validation resolution on WS surface. Without this fix, NO multi-turn feature on WS works.
