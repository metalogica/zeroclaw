# Fable 5 handover prompt

Paste everything in the fenced block below into a fresh **Fable 5** session opened at the root
of this repo (`/Users/reinova/code/forks/zeroclaw`, on branch `docs/v0.8.0-migration-handoff`).
It is a one-shot: Fable reads the local research pack, re-derives the analysis, and produces one
canonical substrate spec + its bead DAG. Everything Fable needs is on disk — it makes **no web
calls**.

> Note for the operator: `/substrate:architect-spec` runs an inline Socratic Q&A and asks a few
> questions before writing the spec — answer them from the brief's Open Questions. The doctrines
> and SDD protocol are already bootstrapped into this repo, so architect-spec's doctrine
> discovery will find them.

---

```text
ROLE
You are migrating our sovereign `zeroclaw` fork onto upstream v0.8.0 and standing up a durable
upstream-sync + dev→prod release system for clawcraft. You have BOTH repos locally:
  - this fork:  /Users/reinova/code/forks/zeroclaw   (branch: docs/v0.8.0-migration-handoff)
  - clawcraft:  /Users/reinova/code/soulbound-labs/clawcraft
GROK BOTH before you design anything. This is inherently cross-repo work.

READ FIRST (all local — do NOT make web calls; the v0.8.0 tag is already in this repo):
  1. docs/tasks/ongoing/upstream-v0.8.0-migration/upstream-v0.8.0-migration-brief.md   ← the brief
  2. docs/tasks/ongoing/upstream-v0.8.0-migration/research/upstream-v0.8.0-facts.md    ← verified v0.8.0 facts (Q1–Q5 resolved)
  3. docs/tasks/ongoing/upstream-v0.8.0-migration/research/clawcraft-integration-contract.md  ← "don't break prod" constraints
  4. docs/tasks/ongoing/upstream-v0.8.0-migration/research/fork-delta-commits.md        ← the raw 84-commit ledger
  5. docs/doctrine/ (praxis §6.5, observability, claw, infra, methodology, claw-state-machine)  ← BINDING constraints
  6. docs/tasks/ongoing/upstream-v0.8.0-migration/migration-playbook.md  ← PRIOR ANALYSIS, CROSS-CHECK ONLY

RE-DERIVE (do not trust the playbook):
  - From the raw ledger (research/fork-delta-commits.md) + `git show <sha>` for detail, RE-CLUSTER
    all 84 commits into themes and RE-DECIDE each disposition (EXTRACT / REWORK / KILL-AND-REWORK /
    DROP) yourself, grounded in the verified v0.8.0 facts. Expect roughly these themes but confirm
    them from the diffs: pod preshared-token cold-start; the Laminar/observability layer (split
    across many commits); the praxis NextAction runtime edits; the hotswap devx + Dockerfile
    additions; and assorted minor bug/feature changes.
  - The migration-playbook is a first-pass cross-check. Where its claims contradict the verified
    facts or your own reading of the diffs, FLAG the contradiction explicitly in the spec.
  - Honor the RESOLVED open questions in research/upstream-v0.8.0-facts.md §3 (v0.8.0 has 15
    zeroclaw crates; NextAction is a clean net-new; OpenRouter already has image but not
    user/audio; ZEROCLAW_SYSTEM_DIR is gone → per-agent ACP sessions; generic OTel is now upstream,
    only the Laminar layer is ours to salvage).

BINDING CONSTRAINTS (from the doctrines + integration contract — a spec step that changes any of
these interfaces is a BREAKING change to clawcraft prod and must be called out):
  - Preserve the praxis NextAction {data,next_action} contract EXACTLY (praxis-doctrine §6.5):
    next_action:null = unconditional turn-end, runtime never inspects data on null; PARK = null +
    data.parked. Theme "praxis continuation auto-drive" (continuation.rs + turn/turn_streamed
    guard) is the keystone deliverable, re-homed onto v0.8.0's execute_turn/loop_.rs.
  - Keep the pod API (/api/chat, brain.db, workspace, tool policy), the SHA-pinned
    CLAW_DOCKER_IMAGE → GCP Artifact Registry flow (never :latest), the Dockerfile
    ARG PRAXIS_VERSION praxis bundling, and the config.toml [observability] Laminar
    config-carrier (no OTEL_* env).
  - Preserve the dev hotswap loop (dev/hotswap/*, Justfile, Dockerfile → clawcraft-claw-runtime:dev)
    against the new multi-crate build; land it FIRST so the rest is locally testable.
  - Anchor on the v0.8.0 tag. Upstream-first: DROP anything now covered upstream rather than
    re-homing it, to shrink the fork delta.

PRODUCE ONE SUBSTRATE SPEC:
  Run  /substrate:architect-spec docs/tasks/ongoing/upstream-v0.8.0-migration/upstream-v0.8.0-migration-brief.md
  Follow the SDD protocol in docs/protocol/sdd/. The spec MUST include a Prompt Execution Strategy
  (phases → steps → Verify → Gate) that, when executed, yields:
    (a) a mirror/`upstream` branch tracking v0.8.0;
    (b) the fork's durable delta re-landed on the sovereign/core branch clawcraft SHA-pins in prod,
        each theme its own reviewable commit behind a green gate
        (`cargo build --all-targets --message-format=short`, then the pinned fmt/clippy neutrality
        check from CLAUDE.md);
    (c) the hotswap loop preserved against the multi-crate build;
    (d) a standing fork-sync system: docs/FORK_DELTA.md ledger, a rebase cadence, a conflict-canary
        CI job, and a documented dev→prod release loop (mirror → SHA-tagged image →
        CLAW_DOCKER_IMAGE bump) that fits infra-doctrine.
  Then GRAPH it into a bead DAG under epic:upstream-v0.8.0-migration (architect-spec's graph-spec
  step). Export ALL new work as TBD beads — nothing lives only in prose.

OUTPUT: a single canonical docs/tasks/ongoing/upstream-v0.8.0-migration/upstream-v0.8.0-migration-spec.md
plus its bead DAG. Do not start implementing the migration — stop at the graphed spec.
```

---

## Why this prompt is thin

The weight lives in the brief + research pack (already on disk), not the prompt. The prompt only
sets role, points at the local files, enforces re-derivation, restates the binding constraints,
and names the exact skill to invoke and the required outputs. That keeps the handover cheap and
reproducible.
