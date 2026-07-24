# RFC 0043: Dependency-scoped partial tool evaluation

- Status: Implemented
- Depends on: RFC 0035, RFC 0041, RFC 0042

## Summary

XL continues independent top-level TypeMetadata computations after one tool
expression fails. A semantic dependency graph connects HIR definition
identities. Each type definition is one atomic scheduling unit: it either
produces metadata, records a conflict or incomputable state, or remains unknown
because an upstream definition is unavailable.

The VM does not resume after a failed instruction. The scheduler starts a
separate bounded tool evaluation for each ready definition and records its
outcome:

```text
type A = broken(Int);  -> Incomputable(UnsupportedOperation)
type B = String;       -> Known(String)
type C = Array(B);     -> Known(Array(String))
type D = Array(A);     -> Unknown(BlockedBy(A))
```

Strict compilation and module publication remain all-or-nothing. Partial
results belong only to an unpublished semantic snapshot.

## Motivation

RFC 0042 can retain syntax, HIR identities, and explicit unavailable facts,
but it does not run semantic computation after a failure. The existing
analyzer evaluates top-level metadata sequentially and returns on the first
error, so one damaged decorator suppresses every independent type below it.

The runtime already provides the required isolation:

- tool calls are quota-bound;
- failed Work World allocations are not published;
- Main World publication is atomic;
- VM errors retain source origins.

The missing layer is a dependency-aware scheduler and a result model, not an
instruction-level continuation mechanism.

## Scope

The first implementation schedules top-level bindings whose kind is `Type`.
It includes their decorators because decorators are part of the elaborated type
RHS. Built-in TypeMetadata constructors and imported/prelude tool functions are
available exactly as in strict analysis.

The scheduler records ordinary lexical references from each type RHS and its
annotation/decorator expressions. An edge is created only when a reference
resolves to another top-level type definition:

```text
dependent --requires--> dependency
```

References to prelude functions, imported capabilities, and runtime bindings
are not guessed as type-definition edges. Their existing tool-stage boundary
decides whether evaluation is possible.

## Dependency graph

The graph is snapshot-owned and identity-based:

```rust
struct SemanticDependencyGraph {
    nodes: Vec<SemanticDependencyNode>,
}

struct SemanticDependencyNode {
    definition: HirDefinitionId,
    dependencies: Vec<HirDefinitionId>,
}
```

Dependencies are sorted and deduplicated. Source order is used only as the
deterministic tie-breaker for ready nodes; names and source ranges are not graph
identity.

Strongly connected type definitions form one scheduling component. Existing
recursive TypeMetadata predeclaration and up-link sealing semantics remain
authoritative. A component is evaluated with all of its names predeclared and
is considered publishable only when every definition in the component seals.
If the current implementation cannot safely isolate a recursive component, it
records `Incomputable(CyclicEvaluation)` rather than fabricating `Any`.

## Evaluation outcomes

Each scheduled definition produces `SemanticFact<TypeId>`:

- successful, decoded TypeMetadata becomes `Known(type_id)`;
- an incompatible declared contract becomes
  `Conflicted(IncompatibleContract)` and retains its diagnostic;
- quota exhaustion becomes `Incomputable(QuotaExceeded)`;
- an unavailable native or stage boundary becomes
  `Incomputable(RuntimeOnly)`;
- unsupported or malformed metadata computation becomes
  `Incomputable(UnsupportedOperation)`;
- an unsafe recursive scheduling component becomes
  `Incomputable(CyclicEvaluation)`;
- a definition whose prerequisite is unavailable becomes
  `Unknown(BlockedBy(definition))` without invoking the VM.

The original failure owns the primary diagnostic. Blocked dependents retain a
cause edge but do not duplicate the upstream diagnostic.

`Any` remains an ordinary successful TypeMetadata result and is represented as
`Known(Any)`.

## Quota and worlds

All evaluations in one partial analysis share the module's tool-stage quota
account. Splitting work into scheduling units must not multiply fuel, stack, or
allocation limits by the number of definitions.

Each attempt is atomic with respect to semantic publication. Successful values
may be retained by the unpublished analysis Work World for later dependent
computations. A failed attempt does not publish roots to Main World. The strict
module initializer continues to perform the existing once-only authoritative
promotion only after complete success.

## Partial analysis result

The tooling API returns a partial analysis even when some scheduled definitions
are unavailable:

```rust
struct PartialAnalysis {
    hir: HirProgram,
    dependencies: SemanticDependencyGraph,
    definition_facts: BTreeMap<HirDefinitionId, SemanticFact<TypeId>>,
    diagnostics: Vec<Diagnostic>,
    types: TypeGraph,
}
```

This is not accepted by the compiler. It contains no executable bytecode and
cannot be promoted into Main World. Complete `Analysis` remains the compiler
input.

Workspace construction projects partial type facts into the same definition
IDs used by normal snapshots. Queries do not execute additional XL code.

## Determinism

Scheduling is deterministic:

1. build and normalize the graph from resolved HIR;
2. collapse strongly connected components;
3. visit ready components in source definition order;
4. sort diagnostics by primary source location and stable diagnostic kind;
5. store dependency and cause IDs in ascending identity order.

Changing an unrelated source-order-independent definition must not change the
semantic outcome of another definition, although compact snapshot IDs may be
renumbered on rebuild.

## Strict behavior

`xl check`, `xl run`, compilation, module initialization, and Main World
publication retain their existing strict behavior. They may share graph and
scheduler helpers, but success still requires all mandatory metadata and
runtime compilation to succeed.

`xl show` and future LSP snapshots may use partial analysis. They report all
root diagnostics plus unavailable states and known independent facts.

## Non-goals

- resuming a VM frame after an instruction error;
- partial execution of ordinary runtime bindings or `main`;
- expression-level scheduling inside one type definition;
- evaluating arbitrary editor-selected expressions;
- parallel scheduling;
- incremental or cross-revision memoization;
- publishing partially initialized modules;
- cross-module failure recovery beyond representing an unavailable imported
  dependency.

## Acceptance criteria

1. dependency edges use resolved HIR definition IDs rather than names.
2. independent type definitions continue after one definition fails.
3. transitive successful dependencies produce known metadata facts.
4. a dependent of a failed definition is not evaluated and records
   `Unknown(BlockedBy(definition))`.
5. the root failure retains one source-located diagnostic; blocked dependents
   do not duplicate it.
6. quota is shared across all scheduled definitions and cannot be multiplied by
   adding definitions.
7. known `Any`, conflict, incomputable, and blocked states remain distinct.
8. recursive components either use existing safe recursive metadata semantics
   or explicitly record `Incomputable(CyclicEvaluation)`.
9. strict compiler and module publication behavior is unchanged.
10. partial analysis allocates no persistent Main World roots and produces no
    executable bytecode.
11. workspace and `xl show` expose independent known facts and unavailable
    causes from one failed source.
12. scheduling and diagnostics are deterministic.

## Implementation plan

1. derive a normalized type-definition dependency graph from RFC 0041 HIR.
2. add a tooling-only `PartialAnalysis` result and diagnostic ownership.
3. extract one-definition metadata evaluation from strict analysis without
   changing its VM semantics.
4. schedule acyclic definitions with one shared quota account.
5. classify tool errors into conflict, incomputable, and blocked states.
6. project partial facts through `WorkspaceSnapshot` and `xl show`.
7. test independent success, transitive success, blocking, quota sharing,
   recursion handling, diagnostics, and strict-path isolation.

## Deferred work

- recursive component evaluation beyond the safe shapes supported by the
  extracted evaluator;
- partial ordinary-binding analysis;
- expression-granular scheduling and retry;
- imported-module partial evaluation;
- parallel scheduling and cancellation;
- revision-aware dependency invalidation and caching.

## Implementation result

Implemented for the top-level TypeMetadata scope defined by this RFC.

HIR expressions now retain an optional parent expression ID. Parent identities
are assigned during the shared RFC 0041 resolver walk and remapped during HIR
normalization. A type definition's existing RHS expression ID therefore owns a
precise expression subtree. Dependency extraction walks that identity
relationship and reads resolved reference targets; it does not associate
references with definitions through names or source-range containment.

`SemanticDependencyGraph`, `SemanticDependencyNode`, and `PartialAnalysis` are
public tooling records. `analyze_partial_types` parses recoverably, constructs
HIR, selects retained top-level type definitions, normalizes and deduplicates
their definition-ID edges, and schedules them in deterministic HIR order.
`analyze_partial_types_with_bindings` additionally accepts explicitly linked
tool values, so host/module integration can supply imported decorators or
metadata capabilities without making the analyzer guess. Such names resolve as
HIR externals.

The scheduler uses one `QuotaAccount` and one successful-value environment for
the complete partial analysis. Every ready RHS is compiled and evaluated by
the existing atomic tool-expression path. A successful metadata value is
decoded, interned in the partial `TypeGraph`, and made available to later
dependents. A failed value is not inserted. Independent ready definitions
continue; a dependent of any non-known fact is skipped and records
`Unknown(BlockedBy(HirDefinitionId))` with a cause but no duplicate diagnostic.

Tool failures are classified into conflict, quota, runtime-only, and
unsupported states. Diagnostic IDs are assigned to root facts and remapped
after deterministic source-order sorting. Quota is not reset between
definitions. The initial partial evaluator deliberately does not attempt the
strict analyzer's recursive up-link sealing protocol: every cyclic graph
member records `Incomputable(CyclicEvaluation)`, after which acyclic downstream
nodes become blocked. Strict recursive TypeMetadata evaluation and promotion
remain unchanged.

`WorkspaceSnapshot::recover_source` now projects partial type graphs and facts
into workspace type and definition IDs, including remapping blocked causes.
`xl show` first attempts normal strict module loading. Only after that fails
does it construct the partial snapshot, which avoids evaluating successful
programs twice. It displays independent known types, incomputable roots,
blocked dependents, and source diagnostics. `check`, `run`, compilation,
module initialization, and Main World publication still use only complete
`Analysis` and retain fail-fast behavior.

Tests cover an unsupported root with independent and transitive success,
definition-ID dependency edges, blocked downstream facts without duplicate
diagnostics, one shared fuel account, explicit external capability linking,
recursive-cycle classification, workspace ID/cause projection, CLI display,
and unchanged strict failure. No partial path creates bytecode for publication
or persistent Main World roots.

The remaining boundary is cross-module recovery orchestration. The evaluator
can consume already linked external values, but `recover_source` is still the
single-source constructor introduced by RFC 0042; it does not build a partial
module graph when an imported source itself fails. Expression-granular retry,
recursive component sealing in partial mode, ordinary-binding scheduling,
parallelism, cancellation, and caching remain deferred.
