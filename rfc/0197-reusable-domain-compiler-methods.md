# RFC 0197: Reusable domain-compiler methods

- Status: Implemented
- Depends on: RFC 0166, RFC 0193

## Summary

Test whether ordinary Telora higher-order types, higher-order functions, and
combinators can separate a reusable domain-compilation method from a concrete
domain model and a concrete intent.

```text
domain-compilation method
    + domain knowledge model
    + authored intent
    -> verification + lowering
    -> complete plan or domain diagnostics
```

The phase uses two deliberately different existing applications:

- intelligent reporting: entities, relations, grain, aggregation, SQL, and
  rendering;
- GCC wrapper: packages, Host platform, TARGET, installations, argv policy,
  and executable plans.

The goal is not a universal ontology framework or a score of 100. The goal is
to find a small body of visibly useful reusable method, demonstrate where
industry-level libraries can grow above it, and record the remaining language,
diagnostic, modeling, and engineering gaps without hiding fallback.

## Knowledge layers

The target dependency direction is:

```text
Telora language and standard combinators
        ↓
cross-industry domain-compiler method
        ↓
industry method library
        ↓
organization or application domain model
        ↓
concrete intent
        ↓
typed executable plan
```

The cross-industry layer may know about rules, capabilities, requirements,
policies, independent lowering, completion, and staged transformation. It may
not know about SQL, grain, GCC, packages, tables, targets, or argv.

Industry libraries are allowed to differ. An analytics library may define
relation planning, cardinality, grain alignment, result schema, and render
binding. A toolchain library may define package selection, installation,
platform/target policy, deterministic argument rewriting, and executable-plan
finalization. Forcing either vocabulary into the cross-industry layer is a
failed abstraction, not reuse.

Concrete models supply company tables and measures, or particular GCC package
sources and target support. Concrete intents select a report or command and do
not repeat the model's rules.

## Candidate shared algebra

This RFC does not freeze exact syntax before extraction, but the candidate
shapes are intentionally small:

```text
Rule(A, B)             A -> Option(B), with Host-observed diagnostics
Capability(K, A, B)    keyed Rule supplied by a domain
Policy(A, B)           explicit semantic transformation
lower_each             continue independent rules
collect_complete       publish B only when every required result exists
compose                connect typed lowering stages
finalize               construct one complete Host-facing plan
```

`Option + emit_error!` remains the baseline. This phase does not assume an
accumulation effect or `call_with_diagnostics!`. If real cross-domain use shows
that independent composition cannot be expressed precisely, the gap is
recorded before proposing a language mechanism.

The extraction should prefer functions over framework-owned control flow. A
domain author should still be able to read the order of lowering stages and
call ordinary helpers directly.

## Child sequence

1. RFC 0198 extracts the minimum cross-industry rule, capability, policy, and
   completeness combinators from the two working applications. It adds focused
   examples and rejects domain vocabulary in the shared module;
2. RFC 0199 builds an analytics industry-method module from relation, grain,
   alignment, schema, and render composition, then rewrites intelligent
   reporting as a concrete model plus intent without changing SQLite results or
   diagnostic coverage;
3. RFC 0200 builds a toolchain industry-method module around package selection,
   installation, input policy, argv rewriting, and `ExecEnv` finalization, then
   rewrites the GCC fixture without changing canonical dry-run behavior;
4. RFC 0201 introduces an ordinary imported restriction data source in one
   model and verifies cross-source diagnostics through intent, domain rule, and
   external fact provenance. The data source receives no special Context
   status;
5. after all children, this umbrella records a comparative evaluation: what
   was genuinely shared, what remained industry-specific, what fallback was
   used, and which Telora gaps were exposed.

Each implementation child lands independently. A child may narrow its proposed
abstraction when code demonstrates that a candidate is not reusable.

## Comparative acceptance criteria

1. both applications use one ordinary cross-industry method module without
   that module importing either industry's types;
2. reporting-specific relation/grain logic and toolchain-specific package/argv
   logic live in separate industry modules;
3. changing a concrete report, metric, package catalog, or supported target
   does not require editing the cross-industry module;
4. concrete intents contain domain vocabulary and assembly, not copied
   verification or lowering algorithms;
5. successful reporting and GCC plans remain byte- or structurally equivalent
   at their established Host boundaries;
6. existing invalid cases remain rejected near their domain causes, with no
   Host-side parallel checker;
7. imported restriction data can influence lowering deterministically, and a
   rejection retains meaningful intent, rule, and data provenance;
8. no child adds a reporting-, ontology-, or toolchain-specific VM operation;
9. the final evaluation reports limitations honestly rather than treating use
   by two examples as proof of universal applicability.

## Evaluation rubric

The final comparison rates each extracted abstraction qualitatively:

| Dimension | Evidence sought |
|---|---|
| Reuse | exercised by both industries, or explicitly classified industry-only |
| Simplification | removes repeated orchestration or invariant maintenance |
| Readability | domain rules remain understandable without framework internals |
| Local extension | a capability or rule can be added without rewriting the compiler |
| Diagnostics | errors remain domain-specific, causal, and source-linked |
| Plan integrity | success still yields one complete executable plan |
| Cost | type annotations and framework concepts do not dominate domain logic |

No aggregate numeric score is required. A partially reusable method is a
successful result when its value is visible and its boundary is explicit.

## Comparative implementation result

The experiment found a useful but deliberately small cross-industry layer.
`domain-method/method.telora` contains independent lowering, completeness,
composition, fallback, and finalization combinators. Both reporting and the GCC
wrapper use it without leaking either domain's vocabulary into that module.

The larger reuse boundary is industry-specific. `analytics-method` owns graph
closure, connecting-edge selection, and missing-field analysis while remaining
generic over the concrete Entity, Relation, and result-field types.
`toolchain-method` owns package lookup and deterministic archive preparation;
the GCC model still owns TARGET selection, tool choice, sysroot policy, and
argument rewriting. These different boundaries are evidence for the layered
model, not a failure to discover one universal framework.

Concrete reporting intents remain short domain values. Concrete catalogs,
measure semantics, relations, and SQL payloads remain in the reporting model.
Likewise, package sources and command policy remain outside the shared method
module. Changes to those facts do not require editing the cross-industry core.

RFC 0201 adds an ordinary JSON restriction. The domain model decodes and
interprets it, successful plans record its revision, and rejected requirements
carry source labels from the authored intent, JSON data, and Telora rule. This
work also repaired recovery evaluation to preserve authoritative persistent
module roots: sourced data and recursive closures no longer cross a lossy
legacy `Value` boundary before diagnostic evaluation.

The established reporting SQL results, four-diagnostic invalid fixture, GCC
dry-run shape, and deterministic installation hashes remain unchanged.

## Honest boundaries

- The shared algebra is orchestration, not an ontology representation. Its
  value is consistent control flow and completion policy, not domain meaning.
- Rank-1 generics are sufficient for these combinators, but Telora still lacks
  user-defined parameterized type constructors; therefore abstractions such as
  `Capability(K, A, B)` remain domain-owned records rather than one generic
  framework type.
- Analytics reachability uses a documented six-round closure suitable for the
  bounded example. It is not an unbounded graph algorithm or proof of arbitrary
  ontology traversal.
- Diagnostics can be emitted independently and recovered by the Host, but
  user code still cannot explicitly observe one call's diagnostic stream;
  `call_with_diagnostics!` remains only a possible future boundary.
- Restriction freshness, database execution, downloads, and process effects
  remain Host responsibilities. The plan records enough data to make those
  responsibilities explicit; Telora does not claim to eliminate them.
- Two industries demonstrate a stable layering technique, not universal domain
  coverage. A third industry should be added only when it provides new pressure
  rather than another tailored success example.

## Gap taxonomy

Every child and the final evaluation classify observed gaps as one of:

```text
method-library gap
    a reusable combinator is missing or shaped too narrowly

Telora language gap
    rank-1 generics, type construction, higher-order composition, modules,
    or pattern matching prevent a natural reusable expression

diagnostic gap
    provenance, causality, independent coverage, or repair information is lost

domain-essential complexity
    the industries legitimately differ and should not share the abstraction

engineering gap
    performance, caching, debugging, LSP, packaging, or test ergonomics lag
```

Fallback is permitted, but the owning layer, lost guarantee, and reason must be
recorded.

## Non-goals

- a universal ontology object model;
- RDF, OWL, open-world inference, or a knowledge-graph runtime;
- abstracting every line shared by two examples;
- requiring relation graphs, grain, SQL, packages, or argv in every domain;
- traits, associated types, higher-kinded types, dependent types, or a general
  effect system solely to make the framework appear uniform;
- remote package acquisition, database execution, or other new effects;
- publishing a stable public ecosystem package before the experiment settles
  names and ownership boundaries; or
- claiming that two industries establish universal domain coverage.

## Stopping rules

Return to discussion when:

- the shared module needs either industry's vocabulary or a VM special case;
- a generic API is harder to understand than both direct implementations;
- diagnostics lose authored domain locations or become generic framework
  failures;
- industry code must encode dynamic typing solely to satisfy the abstraction;
- the Host must duplicate rules previously owned by Telora; or
- adding a second consumer repeatedly changes the supposed stable method API.

In those cases the correct result may be a smaller shared core, two independent
industry libraries, and a documented language gap. That outcome is preferable
to preserving an unjustified abstraction.
