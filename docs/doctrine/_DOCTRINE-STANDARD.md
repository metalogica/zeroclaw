# {Doctrine Title}

**Version**: {x.y.z}
**Status**: Binding | Draft | Advisory
**Date**: {YYYY-MM-DD}

---

## 1. Authority

This document is **{Status}**. Violations are architectural bugs.

Keywords MUST, MUST NOT, SHOULD, MAY follow RFC 2119.

**Reference Implementation**: `{path}`

---

## 2. Model / Layer Architecture

```

[ High-Level Diagram Here ]

```

### 2.1 Import / Dependency Rules

| Layer | MUST NOT | MAY |
|-------|----------|-----|
| {Layer A} | | |
| {Layer B} | | |
| {Layer C} | | |

### 2.2 Boundary Rules

- MUST …
- MUST NOT …
- SHOULD …

---

## 3. Structural Conventions

### 3.1 Directory / Schema Layout

```

{directory tree}

````

### 3.2 Naming Conventions

- Prefix:
- Suffix:
- Pattern:

---

## 4. Core Patterns

### 4.1 Pattern Name

```ts
// Canonical pattern
````

Rules:

* MUST …
* MUST NOT …
* Rationale:

### 4.2 Pattern Name

(Repeat as needed)

---

## 5. Operational Rules

* MUST …
* MUST NOT …
* SHOULD …
* NEVER …

---

## 6. Trust Boundaries

| Boundary | Enforcement |
| -------- | ----------- |
|          |             |
|          |             |

---

## 7. Error Handling Model

* Pattern:
* Invariants:
* Failure semantics:

---

## 8. Invariants

* Invariant 1:
* Invariant 2:
* Invariant 3:

---

## 9. Testing Expectations

| Layer | Test Focus |
| ----- | ---------- |
|       |            |

---

## 10. Change Protocol

* Modifications REQUIRE:

  * …
  * …
* Security review required when:

  * …
* Backwards compatibility rules:

  * …

---

## Document History

| Version | Date       | Changes         |
| ------- | ---------- | --------------- |
| x.y.z   | YYYY-MM-DD | Initial version |

```

---

### Core Structural Elements Extracted

Every doctrine document should contain:

1. Authority section (binding status)
2. Model/Architecture diagram
3. Explicit rule tables
4. Canonical pattern examples
5. Invariants
6. Trust boundaries
7. Change protocol
8. Version history
