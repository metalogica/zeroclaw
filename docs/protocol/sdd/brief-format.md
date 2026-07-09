# Brief Format Specification

**Authority**: Binding
**Version**: 1.0.0
**Date**: 2026-02-07
**Parent Spec**: docs/specs/_spec-standards.md

---

## 1. Purpose

A **brief** is the input to the architect agent. It captures the human's intent in enough detail to enable Socratic Q&A and spec generation.

Briefs are disposable. Once a spec is generated, the brief is archived (not deleted) for historical reference.

---

## 2. Scope

| In Scope | Out of Scope |
|----------|--------------|
| Brief structure and required fields | Spec content (see `_spec-standards.md`) |
| Architect agent entry point | Execution format (see `execution-format.md`) |
| Socratic Q&A triggers | Implementation details |

---

## 3. File Location

```
docs/tasks/ongoing/<feature>/
├── <feature>-brief.md      # Input to architect agent
├── <feature>-spec.md       # Output from architect agent
└── [slice files]           # Optional decomposition
```

After completion, move to `docs/tasks/completed/<feature>/`.

---

## 4. Required Sections

### 4.1 Header

```markdown
# <Feature> Brief

**Author**: <Human name>
**Date**: <YYYY-MM-DD>
**Status**: Draft | In Review | Approved
```

### 4.2 User Story

```markdown
## User Story

As a <role>,
I want <goal>,
so that <benefit>.
```

This is the **why**. It must be specific enough that an agent can ask clarifying questions.

### 4.3 Constraints

```markdown
## Constraints

- MUST: <hard requirement>
- MUST NOT: <prohibition>
- SHOULD: <soft preference>
```

Constraints bound the solution space. They prevent the architect from over-engineering or missing requirements.

### 4.4 References

```markdown
## References

- Spec: docs/specs/<domain>/<spec>.md
- Architecture: docs/specs/architecture/frontend.md
- Existing code: src/domain/<feature>/
```

References tell the agent where to look for context. Be specific—don't say "see the codebase."

---

## 5. Optional Sections

### 5.1 Acceptance Criteria

```markdown
## Acceptance Criteria

- [ ] <Observable behavior>
- [ ] <Observable behavior>
```

Binary pass/fail criteria. If present, these become verification targets in the spec.

### 5.2 Out of Scope

```markdown
## Out of Scope

- <Thing that might seem related but isn't>
- <Future enhancement to defer>
```

Explicitly stating what's out prevents scope creep during Socratic Q&A.

### 5.3 Open Questions

```markdown
## Open Questions

1. Should we use approach A or B?
2. What happens when X?
```

Questions the human wants the architect to explore. Triggers for Socratic dialogue.

---

## 6. Complete Example

```markdown
# Market Scoring Brief

**Author**: Rei Jarram
**Date**: 2026-02-07
**Status**: Draft

---

## User Story

As a market operator,
I want markets to be automatically scored when they resolve,
so that users receive their payouts without manual intervention.

---

## Constraints

- MUST: Use existing ledger infrastructure for payouts
- MUST: Support partial resolution (resolved before end_date)
- MUST NOT: Allow double-scoring of the same market
- SHOULD: Complete scoring within 30 seconds of resolution

---

## References

- Spec: docs/specs/markets/market-lifecycle.md
- Ledger: docs/specs/domain/ledger-design.md
- Existing: src/domain/markets/

---

## Acceptance Criteria

- [ ] Resolving a market triggers automatic scoring
- [ ] All positions are settled via ledger transactions
- [ ] Scoring is idempotent (re-running produces same result)
- [ ] Failed scoring can be retried without data corruption

---

## Out of Scope

- UI for manual score override
- Historical score recalculation
- Multi-outcome markets (binary only for v1)

---

## Open Questions

1. Should scoring run synchronously or via background job?
2. What happens if the treasury has insufficient funds?
```

---

## 7. Architect Agent Invocation

When a brief exists at `docs/tasks/ongoing/<feature>/<feature>-brief.md`:

```
/architect docs/tasks/ongoing/<feature>/<feature>-brief.md
```

The architect agent will:
1. Read the brief
2. Introspect referenced specs and code
3. Engage in Socratic Q&A to clarify ambiguities
4. Generate `<feature>-spec.md` with Prompt Execution Strategy

---

## 8. Brief vs Spec

| Aspect | Brief | Spec |
|--------|-------|------|
| Authority | Disposable | Binding (during execution) |
| Author | Human | Architect agent (reviewed by human) |
| Purpose | Capture intent | Define implementation |
| Detail level | High-level requirements | Complete, executable |
| Lifecycle | Archived after spec | Versioned, may become canonical |

---

## 9. Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2026-02-07 | Initial brief format specification |
