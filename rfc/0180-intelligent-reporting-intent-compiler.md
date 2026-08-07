# RFC 0180: Intelligent reporting intent compiler

- Status: Implemented

## Summary

Drive the intelligent-reporting experiment from a closed measure/dimension
vocabulary to a trustworthy execution-plan boundary. Forma code owns the
domain ontology, validation, relationship planning, grain proof, and lowering.
The Host receives only a checked plan and retains authority to execute it.

The sequence deliberately tests language-library expressiveness before adding
kernel concepts. Higher-order capability records remain the primary mechanism:
successful lowering is the evidence that a composition is legal, rather than
a separate Boolean compatibility table.

## Pipeline

```text
ReportIntent
  -> VerifiedSemanticPlan
  -> RelationalPlan
  -> SQL AST
  -> SQLite SQL
  -> Host-authorized execution
```

Each boundary is a Forma type. A rejected stage returns structured diagnostics
and does not publish a value for the following stage.

## Child sequence

1. RFC 0181 replaces SQL fragments with a minimal Forma SQL AST and renderer;
2. RFC 0182 models relationships and selects reusable join paths;
3. RFC 0183 makes grain and cardinality explicit and rejects fan-out without
   an aggregation or allocation policy;
4. RFC 0184 expands report intent and separates semantic, relational, and SQL
   lowering stages;
5. RFC 0185 defines a checked, serializable plan at the Host execution boundary.

## Shared acceptance criteria

1. both existing valid reports continue to execute against the SQLite fixture;
2. capabilities do not concatenate SQL strings;
3. relation paths and grain decisions are inspectable Forma values;
4. independent invalid intent components are diagnosed in one compilation;
5. rejected compilation never exposes partial executable SQL or a Host plan;
6. authored intent provenance survives through diagnostics;
7. the domain library remains ordinary Forma code without traits, associated
   types, a general effect system, or compiler-specific reporting constructs;
8. every child RFC records its implementation result and lands independently.

## Non-goals

- a complete SQL grammar or optimizer;
- arbitrary cost-based relationship search;
- executing database queries inside the closed Forma world;
- nominal IDs solely to distinguish concepts that are never runtime values;
- a general trait, effect, or macro system;
- every reporting feature or SQL dialect in this sequence.

## Stopping rules

Return to discussion if a child requires a second compatibility model beside
lowering, unchecked SQL escape hatches in domain capabilities, ambient database
authority, or kernel support specific to reporting. A small missing standard
library combinator is acceptable when it has general value.

## Completion

RFCs 0181 through 0185 are implemented. The experiment now lowers typed report
intent through capability composition, deterministic relationship selection,
cardinality proof, a bounded SQL AST, and a typed inert Host plan. Both valid
queries execute against SQLite; invalid intent returns independent diagnostics
and no plan.

The principal departure is the SQL expression representation. A conventional
recursive AST cannot currently cross the legacy module-value boundary because
its recursive type metadata is cyclic. RFC 0181 therefore uses a bounded
non-recursive hierarchy while retaining structured nodes and centralized
rendering. Other remaining gaps are explicit policy support for safe fan-out,
ambiguous-path diagnostics, provenance tests through every intermediate plan,
and ergonomic diagnostic accumulation.
