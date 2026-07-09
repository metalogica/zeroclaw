# Ideosphere Specification Standards

**Authority**: Binding
**Applies To**: All documents in `docs/specs/`
**Version**: 1.0.0
**Date**: 2026-02-07

---

## 1. Purpose

This document defines the invariants that all specifications must uphold to enable autonomous agent execution in a spec-driven development workflow.

Specifications are the **source of truth** for system behavior. They must be complete enough that:
- An AI agent can implement them without human clarification
- An AI agent can verify its own work without human judgment
- A future maintainer can understand decisions without archeological research

---

## 2. The Agent-First Principle

> **A specification is complete if and only if an autonomous agent can:**
> 1. **Execute** it without clarifying questions
> 2. **Verify** its own work without human judgment
> 3. **Recover** from failures without data corruption
> 4. **Operate** without prior conversation context

If a spec requires "you know what I mean" or "use common sense," it is incomplete.

---

## 3. Document Hierarchy

```
docs/specs/                     # Canonical specifications (Binding)
├── _spec-standards.md          # This file (meta-spec)
├── scoring-subsystem-spec.md   # Child specs
└── [feature]-spec.md

docs/tasks/                     # Execution orders (Disposable)
├── ongoing/                    # Active work
└── completed/                  # Archived

docs/architecture/              # System design (Binding)
└── frontend.md
```

### Authority Levels

| Path | Authority | Meaning |
|------|-----------|---------|
| `docs/specs/` | **Binding** | Violations are bugs |
| `docs/architecture/` | **Binding** | Violations are bugs |
| `docs/protocol/` | Normative | How work happens |
| `docs/tasks/` | Disposable | Execution orders, not truth |

---

## 4. Required Sections

Every spec in `docs/specs/` MUST include these sections:

| Section | Purpose | Example |
|---------|---------|---------|
| **Header** | Version, status, date, relationships | `**Version**: 1.0.0` |
| **Overview** | What this spec covers | Architecture diagram |
| **Scope Table** | In vs out of scope | `\| In Scope \| Out of Scope \|` |
| **Data Model** | Full schema definitions | All columns, no `...` |
| **Invariants** | Rules with audit queries | `S-1: scoring_run_id mandatory` |
| **Verification** | Executable test requirements | `- [ ] Trigger creates event` |
| **Change Log** | Version history | `\| 1.0.0 \| 2026-02-07 \| Initial \|` |

### Header Template

```markdown
# [Feature] Specification

**Version**: X.Y.Z
**Status**: Draft | Canonical | Deprecated
**Date**: YYYY-MM-DD
**Supersedes**: [link to deprecated doc] (if applicable)
**Parent Spec**: [link] (if child)
**Child Specs**: [links] (if parent)
```

---

## 5. Invariant Categories

### 5.1 Semantic Completeness

**Principle**: An agent cannot act on ambiguity. Every term, structure, and state must be fully specified.

| Invariant | Violation | Correct Form |
|-----------|-----------|--------------|
| No abbreviations in schemas | `... \| \| Additional fields` | Full column definitions |
| All terms defined or referenced | "Use the standard pattern" | Link to pattern doc or define inline |
| State machines are exhaustive | "Handle errors appropriately" | Enumerate all error states |
| Enums are closed | "Status can be active, etc." | `CHECK (status IN ('a', 'b', 'c'))` |
| Nullability explicit | Column type only | `NOT NULL` or `NULL DEFAULT x` |
| Default values stated | "Uses default" | `DEFAULT now()` or `DEFAULT 0` |

**Litmus Test**: Can an agent write a complete implementation without asking clarifying questions?

### 5.2 Verification Completeness

**Principle**: Every action must have a machine-executable verification step with binary pass/fail outcome.

| Invariant | Violation | Correct Form |
|-----------|-----------|--------------|
| Verification is executable | "Visually confirm UI works" | `pnpm test:e2e --grep "feature"` |
| Success criteria are binary | "Should be fast enough" | `p99 < 200ms` or test times out |
| Invariants have audit queries | "Data must be consistent" | SQL query that returns 0 rows on success |
| Each phase has a gate | "Continue to next step" | `##### Verify\n- pnpm app:compile` |
| Common queries documented | "Query the data" | Example SQL in appendix |

**Litmus Test**: Can an agent verify its own work without human judgment?

### 5.3 Recovery Completeness

**Principle**: Agents will fail. Specs must define how to detect, retry, and rollback failures.

| Invariant | Violation | Correct Form |
|-----------|-----------|--------------|
| Failure modes enumerated | "Handle errors" | FMEA table with severity/mitigation |
| Idempotency guaranteed | "Run the migration" | `ON CONFLICT DO NOTHING` or unique keys |
| Rollback defined | "If it fails, fix it" | Explicit rollback SQL or compensating action |
| Retry semantics specified | "Retry on failure" | `max_attempts: 3, backoff: exponential` |
| Partial completion detectable | "Check if done" | Status column or idempotency key |
| Stuck state recovery | "Should not get stuck" | Reset query for stuck records |

**Litmus Test**: Can an agent recover from a crash at any step without data corruption?

### 5.4 Context Completeness

**Principle**: Agents don't share memory across sessions. All context must be in the spec or linked.

| Invariant | Violation | Correct Form |
|-----------|-----------|--------------|
| Parent spec linked | "See the main doc" | `**Parent Spec**: path/to/spec.md` |
| Child specs listed | Implicit children | `**Child Specs**:` with links |
| Dependencies explicit | "Requires the usual setup" | `**Prerequisites**: Table X exists` |
| Decision rationale captured | Just the decision | `> **Insight**: "Stale version" is a feature...` |
| Anti-patterns documented | "Don't do it wrong" | `**Not supported**: ❌ Direct invocation` |
| Historical context preserved | No changelog | `## Change Log` with version history |

**Litmus Test**: Can a fresh agent session execute the spec without prior conversation context?

### 5.5 Boundary Completeness

**Principle**: Agents must know where their authority ends and external systems begin.

| Invariant | Violation | Correct Form |
|-----------|-----------|--------------|
| Scope table present | "This covers everything" | `\| In Scope \| Out of Scope \|` table |
| Interface contracts defined | "Calls the other service" | Input/output schemas, error codes |
| Auth requirements explicit | "Requires permission" | `**Auth**: service_role` or `market_operator` |
| Performance constraints stated | "Should be fast" | `**Timeout**: 30s` or `**Batch size**: 5` |
| External dependencies listed | "Uses the database" | `**Dependencies**: messaging schema` |

**Litmus Test**: Can an agent refuse out-of-scope requests and correctly delegate to other systems?

---

## 6. Key Design Decision Documentation

When a spec makes a non-obvious design choice, document it with this pattern:

```markdown
## N. Key Design Decision: [Name]

> **Insight**: [One-sentence summary of the key insight]

[Explanation of the decision]

**Why this matters:**

| Approach | Behavior | Problem |
|----------|----------|---------|
| Alternative A | What it does | Why it's worse |
| **Chosen approach** | What it does | Why it's better |

This aligns with [reference to principle or FMEA item].
```

This ensures future maintainers understand **why**, not just **what**.

---

## 7. FMEA (Failure Mode and Effects Analysis)

Every spec with state changes SHOULD include an FMEA table:

```markdown
### Failure Modes & Mitigations

| # | Failure Mode | Severity | Mitigation |
|---|--------------|----------|------------|
| 1 | [What can go wrong] | Critical/High/Medium/Low | [How it's prevented] |
```

**Severity Levels**:
- **Critical**: Data loss, security breach, financial impact
- **High**: Feature broken, user-facing error
- **Medium**: Degraded experience, manual intervention needed
- **Low**: Minor inconvenience, self-healing

---

## 8. Operational Queries

Every spec with database tables SHOULD include an operational queries section:

```markdown
## N. Operational Queries

### Status Check
\`\`\`sql
-- Check if [entity] is in expected state
SELECT ... FROM ... WHERE ...;
\`\`\`

### Invariant Audit
\`\`\`sql
-- Verify [invariant name]
SELECT ... -- Expected: 0 rows
\`\`\`

### Recovery
\`\`\`sql
-- Reset stuck [entities]
UPDATE ... SET status = 'pending' WHERE status = 'processing' AND ...;
\`\`\`
```

---

## 9. Spec Completeness Checklist

Use this checklist when reviewing or creating specs:

```markdown
## Spec Completeness Checklist

### Semantic Completeness
- [ ] All data structures fully defined (no `...`)
- [ ] All terms defined or linked to definitions
- [ ] All state machines exhaustive (all states, all transitions)
- [ ] All enums closed (explicit list of valid values)
- [ ] Nullability explicit on all columns
- [ ] Default values stated where applicable

### Verification Completeness
- [ ] Each phase has executable verification commands
- [ ] All invariants have audit queries
- [ ] Success criteria are binary (pass/fail)
- [ ] Example queries for common operations included

### Recovery Completeness
- [ ] FMEA table with failure modes (if stateful)
- [ ] Idempotency guarantees documented
- [ ] Rollback/recovery procedures defined
- [ ] Retry semantics specified (if async)
- [ ] Stuck state recovery queries provided

### Context Completeness
- [ ] Parent spec linked (if child spec)
- [ ] Child specs listed (if parent spec)
- [ ] Decision rationale captured for non-obvious choices
- [ ] Change log present with version history
- [ ] Supersedes link present (if replacing another doc)

### Boundary Completeness
- [ ] In-scope / out-of-scope table present
- [ ] Auth requirements explicit
- [ ] External dependencies listed
- [ ] Interface contracts defined (inputs, outputs, errors)
- [ ] Performance constraints stated (if applicable)
```

---

## 10. Deprecation Protocol

When a spec is superseded:

1. **Add deprecation banner** to old spec:
   ```markdown
   > **DEPRECATED**: This specification is superseded by [link].
   > **Do not use this document for new implementation work.**
   ```

2. **Update status** in header:
   ```markdown
   **Status**: ~~Ready for Implementation~~ **DEPRECATED**
   **Superseded By**: [link] (YYYY-MM-DD)
   ```

3. **Link from new spec**:
   ```markdown
   **Supersedes**: [link to deprecated doc]
   ```

4. **Do not delete** deprecated specs (historical reference).

---

## 11. Versioning

Specs use semantic versioning:

| Change Type | Version Bump | Example |
|-------------|--------------|---------|
| Breaking (schema, API) | Major | 1.0.0 → 2.0.0 |
| New section, clarification | Minor | 1.0.0 → 1.1.0 |
| Typo, formatting | Patch | 1.0.0 → 1.0.1 |

**Immutability Rule**: Once a version is published (in changelog), it cannot be silently edited. All changes require a new version entry.

---

## 12. Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2026-02-07 | Initial specification standards |
