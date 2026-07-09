# Spec Execution Format

**Authority**: Binding
**Version**: 1.0.0
**Date**: 2026-02-07
**Parent Spec**: docs/specs/_spec-standards.md

---

## 1. Purpose

This document defines the grammar for the **Prompt Execution Strategy** section of specifications. This section enables autonomous execution by orchestrators (human, agent, or script).

A spec without this section is a design document. A spec with this section is an executable contract.

---

## 2. Scope

| In Scope | Out of Scope |
|----------|--------------|
| Phase/Step/Verify grammar | Spec content standards (see `_spec-standards.md`) |
| Timeout specification | Brief format (see `brief-format.md`) |
| Gate definitions | Orchestrator implementation |
| Parsing rules | Agent tool permissions |

---

## 3. Section Header

The execution section MUST be titled with one of:

```markdown
## N. Prompting Strategy
## N. Prompt Execution Strategy
```

Where `N` is the section number in the document.

---

## 4. Grammar

### 4.1 Phase

```markdown
### Phase N: <Name>
```

- `N` is a 1-indexed integer
- `<Name>` is a human-readable phase title
- Phases execute sequentially

**Example:**
```markdown
### Phase 1: Schema Migration
### Phase 2: Domain Layer
### Phase 3: Integration Tests
```

### 4.2 Step

```markdown
#### Step N.M: <Title>

<Prompt content>
```

- `N` matches the parent phase number
- `M` is a 1-indexed step within the phase
- `<Title>` is a human-readable step title
- `<Prompt content>` is everything between the step header and the next section

**Example:**
```markdown
#### Step 1.1: Create Migration File

Read docs/specs/markets/scoring-subsystem-spec.md Section 3.

Create the migration file at supabase/migrations/YYYYMMDDHHMMSS_add_scoring_tables.sql.

Include all tables from Section 3.1 with exact column definitions.

Tools to use: Write
Tools to NOT use: Edit (file doesn't exist)
```

### 4.3 Verify Block

```markdown
##### Verify

- `<command>`
- `<command>`
```

- MUST appear after step content
- Each command is a backtick-wrapped shell command
- Commands execute sequentially; first failure stops verification
- All commands must exit 0 for step to pass

**Example:**
```markdown
##### Verify

- `pnpm app:compile`
- `pnpm test:unit:ci test/unit/domain/scoring.test.ts`
```

### 4.4 Timeout Block

```markdown
##### Timeout

<milliseconds>
```

- Optional; defaults to 180000 (3 minutes)
- Specifies maximum time for the step (not including verification)

**Example:**
```markdown
##### Timeout

300000
```

### 4.5 Phase Gate

```markdown
#### Gate

- `<command>`
- `<command>`
```

- Optional; runs after all steps in phase complete
- Same format as Verify block
- Failure blocks progression to next phase

**Example:**
```markdown
### Phase 1: Schema Migration

#### Step 1.1: Create Migration
...

#### Step 1.2: Apply Migration
...

#### Gate

- `pnpm app:compile`
- `pnpm db:test`
```

---

## 5. Complete Example

```markdown
## 8. Prompt Execution Strategy

### Phase 1: Database Schema

#### Step 1.1: Create Migration

Read docs/specs/markets/scoring-subsystem-spec.md Section 3.1.

Create migration file with all table definitions.

##### Verify

- `pnpm app:compile`

##### Timeout

120000

#### Step 1.2: Apply Migration

Run the migration against local Supabase.

##### Verify

- `pnpm db:reset`
- `pnpm db:test`

#### Gate

- `pnpm app:compile`
- `pnpm db:test`

### Phase 2: Domain Layer

#### Step 2.1: Create Entity

Read docs/specs/markets/scoring-subsystem-spec.md Section 4.

Create src/domain/scoring/ScoringRunEntity.ts.

##### Verify

- `pnpm app:compile`
- `pnpm test:unit:ci test/unit/domain/scoring`
```

---

## 6. Parsing Rules

For orchestrator implementers:

| Rule | Regex Pattern |
|------|---------------|
| Phase header | `^### Phase (\d+): (.+)$` |
| Step header | `^#### Step (\d+\.\d+): (.+)$` |
| Verify section | `^##### Verify$` followed by `- \`([^\`]+)\`` lines |
| Timeout section | `^##### Timeout$` followed by `^\s*(\d+)` |
| Gate section | `^#### Gate$` followed by `- \`([^\`]+)\`` lines |
| Prompt content | Everything between step header and next `####`/`#####` |

**Important:** Commands inside fenced code blocks (` ``` `) are NOT parsed as verify/gate commands. Only top-level markdown list items with backticks are parsed.

---

## 7. Execution Modes

This format supports multiple executors:

| Executor | Description |
|----------|-------------|
| Human + Claude (HIL) | Human pastes prompts, watches, intervenes |
| Claude self-orchestrating | Single session, Claude reads spec and executes sequentially |
| Orchestrator script | Automated spawning of Claude CLI per step |
| Parallel agents | Multiple Claudes on non-dependent phases (future) |

The format is executor-agnostic. The same spec works with any execution mode.

---

## 7.1 Mid-Execution Scope Expansion

Specs occasionally need to expand in scope mid-execution — a load-bearing dependency surfaces that the brief didn't anticipate, or a sibling concern becomes clearly bundled with the current work. The discipline is **triage by surface area**, not blanket permit or blanket forbid.

**Auto-detectable triggers requiring an explicit user-approval pause** (executor MUST stop and request approval before proceeding):

1. **Doctrine touches.** Any file under `docs/doctrine/` modified by the in-flight execution.
2. **Schema changes.** Any addition or removal of a table or column in `apps/clawcraft/convex/schema.ts` not declared in the spec's §3 Architecture section.
3. **New HTTP routes.** Any new entry in `apps/clawcraft/convex/http.ts` not declared in the spec.
4. **New per-user table.** Any new `userId`-bearing table — these implicate `USER_OWNED_TABLES` (backend §7.4) and historically cause silent privacy leaks.
5. **Cross-doctrine reconciliation.** Any change that touches multiple doctrines (e.g., a frontend feature that requires a backend-doctrine amendment to ratify a new pattern).

For these triggers, the executor MUST output a "Scope Expansion Request" block before any code is written for the expansion, naming:
- What's being added (specific files / new abstractions)
- Why it became load-bearing now (vs. brief time)
- What follow-up work it might displace or replace
- An explicit ask: "proceed with expansion / re-spec / drop the dependency / defer entire phase"

**Permitted without pause** (post-execution annotation in the spec's `### Post-execution notes` block is sufficient):

- Single-file additions that don't touch doctrine, schema, or routes.
- Test additions to cover code that the spec already calls out.
- Refactors of code-being-written that stay within the spec's declared file set.
- Reordering of steps within a phase if the new order preserves dependencies.

**Forbidden in all cases**:

- Expansions that contradict an explicit MUST or MUST NOT in the spec preamble or §2 Scope. Those require re-specifying via `/substrate:architect-spec`, not silent override.
- Expansions that invent doctrine carve-outs not present in the binding doctrine being expanded. (Reference precedent: praxis-spec-tab's spec attempted a praxis-doctrine §2.1 fixture-import carve-out that contradicted explicit doctrine; this required mid-execution course-correction. Don't repeat.)

**Synthesis duty**: every mid-execution scope expansion (whether paused-and-approved or post-annotated) MUST surface in the post-execution-notes block of the spec, with a one-line classification: `(intentional)`, `(opportunistic)`, or `(reactive)`. Synthesis sessions read this block to decide whether the deviation deserves a doctrine amendment or stays as a one-off precedent.

The auto-detectable triggers are intentionally narrow: a frontend component addition that doesn't touch doctrine/schema/routes is post-annotation. A new Convex schema column is a pause-and-approve. The asymmetry is calibrated to the cost of reversal — frontend changes are cheap to revert; schema and doctrine changes are not.

---

## 8. Verification Requirements

Every step SHOULD have a Verify block. Steps without verification:
- Cannot guarantee correctness
- Cannot be automatically retried on failure
- Require human judgment to proceed

Minimum verification for any code change:
```markdown
##### Verify

- `pnpm app:compile`
```

---

## 9. Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2026-02-07 | Initial execution format specification |
