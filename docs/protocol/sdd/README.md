# Spec Driven Development Specifications

This directory contains binding specifications for the spec-driven development workflow.

## Documents

| Document | Purpose |
|----------|---------|
| [execution-format.md](./execution-format.md) | Grammar for `### Phase N:` / `#### Step N.M:` / `##### Verify` |
| [brief-format.md](./brief-format.md) | Structure for briefs that architect agents consume |

## Relationships

```
docs/protocol/sdd/
├── _SPEC-STANDARD.md             ← What goes IN a spec (content standards)
├── execution-format.md           ← How specs are EXECUTED (orchestrator grammar)
├── brief-format.md               ← What TRIGGERS spec creation (architect input)
└── templates/
    ├── brief-template.md         ← Copy-paste template for new briefs
    └── spec-template.md          ← Copy-paste template for spec format
```

## Quick Reference

### Starting New Work

1. Copy template: `docs/protocol/sdd/templates/brief-template.md` → `docs/tasks/ongoing/<feature>/<feature>-brief.md`
2. Fill in the brief per [brief-format.md](./brief-format.md)
3. Invoke `/architect` to generate spec
4. Execute spec per [execution-format.md](./execution-format.md)

### Executing Existing Spec

Read the spec's `## N. Prompt Execution Strategy` section and execute:
- Phases sequentially
- Steps within phases sequentially
- Verify after each step
- Gate at end of each phase (if defined)
