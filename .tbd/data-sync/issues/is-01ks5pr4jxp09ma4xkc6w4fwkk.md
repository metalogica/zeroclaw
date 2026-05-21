---
type: is
id: is-01ks5pr4jxp09ma4xkc6w4fwkk
title: "Step 1 — Foundations: session_key_for_thread() + Agent::replace_history()"
kind: task
status: open
priority: 1
version: 3
labels:
  - ws-context-fix
dependencies:
  - type: blocks
    target: is-01ks5prcn71t0a611f43gbp2ct
created_at: 2026-05-21T16:43:54.333Z
updated_at: 2026-05-21T16:44:47.184Z
---
Purely additive scaffolding. No callers wired yet. Lays the helper + Agent method that subsequent steps depend on.

DELIVERABLES
- ws.rs: GW_THREAD_PREFIX const + session_key_for_thread(thread_id) helper near existing GW_SESSION_PREFIX (ws.rs:228).
- agent.rs: pub fn replace_history(&mut self, messages: &[ChatMessage]) — 3 lines: self.history.clear(); self.seed_history(messages); Place immediately after seed_history (agent.rs:406).
- Unit tests in agent.rs's existing #[cfg(test)] mod: replace_history_clears_existing_messages, replace_history_preserves_system_prompt_seeding.

VERIFY: cargo test agent::tests::replace_history — must be green before merging or moving on.

RISK: trivial. No production code reads from these yet.
