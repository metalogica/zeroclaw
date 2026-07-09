# <Feature>: Technical Specification

**Version**: 1.0.0
**Status**: Draft
**Author**: Architect Agent
**Date**: YYYY-MM-DD
**Brief**: `docs/tasks/ongoing/<feature>/<feature>-brief.md`

---

## 1. Overview

### 1.1 Objective

<What this feature accomplishes>

### 1.2 Constraints

- MUST: <inherited from brief>
- MUST NOT: <inherited from brief>

### 1.3 Success Criteria

- <Binary pass/fail criterion>

---

## 2. Scope

| In Scope | Out of Scope |
|----------|--------------|
| <Feature X> | <Feature Y> |

---

## 3. Architecture / Data Model

<Schema definitions, relationships, diagrams>

---

## 4. Implementation Details

### 4.1 Domain Layer

<Entities, factories, pure functions>

### 4.2 Server Layer

<tRPC routers, RPC functions>

### 4.3 Frontend Layer

<Components, hooks, repositories>

---

## 5. Error Handling

| Error | Cause | Handling |
|-------|-------|----------|
| <Error type> | <Trigger> | <Response> |

---

## 6. Testing Strategy

| Layer | Test Focus | Command |
|-------|------------|---------|
| Domain | Invariants | `pnpm test:unit:ci test/unit/domain/<feature>` |
| Integration | Full flow | `pnpm test:integration:ci` |

---

## 7. Failure Modes (FMEA)

| # | Failure Mode | Severity | Mitigation |
|---|--------------|----------|------------|
| 1 | <What can go wrong> | Critical/High/Medium/Low | <Prevention> |

---

## 8. Prompt Execution Strategy

<!--
PROTOCOL: This section follows docs/protocol/sdd/execution-format.md
COMPLETENESS: Verify against docs/protocol/sdd/_SPEC-STANDARD.md §5 invariants
-->

### Phase 1: <Phase Name>

#### Step 1.1: <Step Title>

<Self-contained prompt for Claude Code CLI>

Include:
- Exact file paths
- Import patterns
- Code to create/modify
- Recovery guidance ("If X fails, do Y")

Tools to use: <Write/Edit/Bash>
Tools to NOT use: <Edit if file doesn't exist>

##### Verify

- `pnpm app:compile`
- `<additional verification>`

##### Timeout

120000

#### Step 1.2: <Step Title>

...

#### Gate

- `pnpm app:compile`
- `pnpm test:unit:ci`

### Phase 2: <Phase Name>

...

### Phase N: Doctrine Review

<!--
This phase is MANDATORY. It ensures learnings from implementation
feed back into the doctrine system (living documentation).
-->

#### Step N.1: Review Implementation Against Doctrines

Review all code written in this spec against the doctrines that were loaded.

Check the doctrine manifest at `docs/doctrine/doctrine-manifest.yaml` to identify which doctrines applied to this spec based on trigger keywords.

For each relevant doctrine, answer these questions:

1. **Compliance**: Did we follow all MUST/MUST NOT rules?
   - If NO: Document the violation and why it was necessary

2. **New Patterns**: Did we discover patterns that should become doctrine?
   - If YES: Document the pattern with rationale

3. **Outdated Rules**: Did we find doctrine that is wrong or outdated?
   - If YES: Document what's wrong and the correction

4. **Missing Coverage**: Did we encounter scenarios doctrine doesn't address?
   - If YES: Document the gap

If ANY amendments are needed, create:
`docs/tasks/ongoing/<feature>/doctrine-amendments.md`

Format for amendments file:
```markdown
# Doctrine Amendments: <Feature>

## Compliance Violations
- [doctrine]: [rule violated] - [justification]

## New Patterns to Add
- [doctrine]: [pattern] - [rationale]

## Outdated Rules to Update
- [doctrine]: [current rule] → [proposed update]

## Missing Coverage
- [doctrine]: [scenario not covered]
```

If no amendments needed, this step passes automatically.

##### Verify

- `test -f docs/tasks/ongoing/<feature>/doctrine-amendments.md && echo "Amendments documented" || echo "No amendments needed"`

#### Step N.2: Queue Doctrine Amendments (if any)

If `doctrine-amendments.md` exists, queue each amendment as a **bead** for human triage —
substrate is bead-only (the legacy `docs/tasks/ongoing/doctrine-updates/` `.md` queue was
retired 2026-06-03). One bead per amendment:

```bash
tbd create -t chore -l doctrine-amendment --spec docs/tasks/ongoing/<feature>/<feature>-spec.md \
  -f docs/tasks/ongoing/<feature>/doctrine-amendments.md "<amendment title>"
```

(For multiple amendments, split `doctrine-amendments.md` into one tempfile body per amendment.)

##### Verify

- `tbd list --label doctrine-amendment --spec docs/tasks/ongoing/<feature>/ 2>/dev/null | grep -q rnk- && echo "Amendments queued as beads" || echo "No amendments needed"`

---

## 9. Operational Queries

### Status Check

```sql
-- Check <entity> state
SELECT ... FROM ... WHERE ...;
```

### Invariant Audit

```sql
-- Verify <invariant> (expected: 0 rows)
SELECT ... WHERE <violation condition>;
```

---

## 10. Spec Completeness Checklist

<!-- From docs/protocol/sdd/_SPEC-STANDARD.md §9 -->

### Semantic Completeness
- [ ] All data structures fully defined (no `...`)
- [ ] All terms defined or linked
- [ ] All state machines exhaustive
- [ ] Nullability explicit on all columns

### Verification Completeness
- [ ] Each phase has executable verification
- [ ] All invariants have audit queries
- [ ] Success criteria are binary

### Recovery Completeness
- [ ] FMEA table present
- [ ] Idempotency guaranteed
- [ ] Rollback procedures defined

### Context Completeness
- [ ] Brief linked
- [ ] Decision rationale captured
- [ ] Change log present

### Boundary Completeness
- [ ] Scope table present
- [ ] Auth requirements explicit
- [ ] External dependencies listed

---

## 11. Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | YYYY-MM-DD | Initial specification |

---

<!--
TEMPLATE LOCATION: docs/protocol/sdd/templates/spec-template.md

USAGE:
This template is generated by the architect-agent from a brief.
Manual use: Copy to docs/tasks/ongoing/<feature>/<feature>-spec.md

EXECUTION:
pnpm tsx scripts/orchestrate.ts docs/tasks/ongoing/<feature>/<feature>-spec.md

OPTIONS:
--dry-run     Parse and print plan
--from 2.3    Start from specific step
--fail-fast   Stop on first error
-->
