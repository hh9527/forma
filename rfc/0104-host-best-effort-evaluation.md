# RFC 0104: Host best-effort evaluation

- Status: Proposed
- Depends on: RFC 0088, RFC 0100, RFC 0102, RFC 0103

## Summary

Forma adds an explicit Host evaluation policy:

```text
EvaluationPolicy = Strict | BestEffort
```

Strict keeps the existing single-program fail-fast VM behavior. BestEffort
executes a compiler-produced DAG of atomic evaluation units. Each unit is still
evaluated by the ordinary strict bytecode VM in a fresh WorkWorld. The Host
records recoverable failures as RFC 0103 Never outcomes, skips dependent units,
and continues ready independent units in deterministic source order.

Successful intermediate values live only in an evaluation-session world. A
module result is published to the shared Main world and caches only when every
dependency required by that result succeeded. There is no VM instruction-level
recovery and no partial Forma module.

## Motivation

The current VM owns one mutable WorkWorld and unwinds it on any RuntimeError.
Resuming that VM at an arbitrary instruction would require every opcode,
register read, branch, native continuation, and call frame to accept Never. It
would also make correctness depend on partially executed allocation and native
state.

Forma computations are pure and module bindings already form a semantic
dependency graph. The Host can recover at a stronger boundary: compile a
binding or structural diagnostic target into an atomic unit, execute it
strictly, and preserve only a complete successful root. This reuses the one VM
semantics while allowing independent work to continue.

## Evaluation units

An evaluation plan contains stable unit IDs and explicit dependencies:

```rust
struct EvaluationUnit {
    id: EvaluationUnitId,
    kind: EvaluationUnitKind,
    location: Location,
    dependencies: Box<[EvaluationUnitId]>,
    function: BytecodeFunction,
}
```

Initial kinds are:

- Binding for an ordinary non-recursive binding;
- DefinitionGroup for one recursive SCC;
- ContainerChild for a direct Array/Tuple/Tagged/Dict child selected as an
  independent diagnostic target;
- ModuleResult for the final requested expression; and
- Metadata for an existing tool-stage metadata computation.

IDs and dependency order come from source/HIR identity, not hash-map iteration.
The plan contains no effectful unit. Native functions reachable from a unit
must obey the existing deterministic, quota-accounted Host contract.

## Unit compilation

Each unit compiles to an ordinary closed bytecode Function whose external links
name successful dependency roots. A recursive definition SCC remains one unit
so up-links are initialized atomically. Nested ordinary Function calls remain
inside their owning unit and retain normal call stacks.

The compiler may split direct container children into units when this preserves
ordinary eager order and does not duplicate computation. The container root
unit depends on every child and constructs no partial container if one is
Never. Conditions and match arms are not split speculatively: the condition or
scrutinee unit must succeed before the selected branch is planned or executed.

Strict compilation may continue producing the current monolithic program
Function. Best-effort plans are additional compiler output, not a changed
bytecode ABI or a requirement that ordinary execution schedule every binding
separately.

## Session world

Best-effort evaluation owns an isolated session world layered over the frozen
Main world:

```text
Frozen Main
    -> best-effort Session roots
        -> per-unit WorkWorld
```

A successful unit is promoted atomically into the session world and becomes
available to later units. A failed unit discards its WorkWorld. Session roots
may include closures, recursive links, descriptors, and source provenance using
the existing heap-copy rules.

The session world is never installed as the shared Main world. When the
ModuleResult succeeds and its complete dependency closure contains no Never,
the result may be exported to the Host. Publication to shared module caches uses
the existing all-or-nothing module path. Failed, cancelled, stale, or
diagnostic-only sessions are dropped wholesale.

## Scheduling

The Host scheduler repeatedly selects ready units by stable source order:

1. if all dependencies are Value, execute the unit once;
2. if any dependency is Never, do not execute it and create a propagation node;
3. if execution produces a recoverable RuntimeError, create one Root and record
   one diagnostic candidate;
4. if execution produces a terminal error, stop and discard the session; and
5. continue until all reachable units are Value, Never, or cancelled.

Independent units after a recoverable failure continue. A dependency blocked
unit is silent. The ModuleResult receives `FailureOperation::ModuleResult`, so a
Host may explain why requested output is unavailable without publishing a
partial value.

## Recoverability

RuntimeErrorKind is classified exhaustively in code rather than by message:

Recoverable data/program failures:

- DivisionByZero and IntegerOverflow;
- MissingField, NoPatternMatched, and TypeMismatch; and
- UninitializedDefinition or DuplicateDefinition only when they arise from
  user program/data semantics rather than a compiler invariant.

Terminal failures:

- Cancelled;
- FuelExhausted, AllocationQuotaExceeded, StackLimitExceeded, and
  CallDepthExceeded;
- InvalidBytecode and internal heap/link invariants; and
- Host I/O/module-resolution failures outside evaluation.

Quota errors remain terminal even if independent units exist. Continuing after
a budget failure would make diagnostics depend on which unit happened to spend
the remaining global budget.

## Quotas and diagnostic budgets

All units share one `QuotaAccount` for evaluation fuel and allocations. Per-unit
VM calls do not reset limits. The Host also supplies:

- a maximum recoverable root count;
- RFC 0103 propagation/cause/render limits; and
- cancellation checkpoints before planning, before each unit, after each VM
  call, and before publication.

Reaching the root diagnostic budget stops scheduling cleanly and returns a
bounded analysis result; it does not synthesize another Forma failure. Stable
unit order makes the retained diagnostic prefix deterministic.

## Native calls and debug events

A unit's native calls use the same strict callback and continuation machinery.
A native failure either aborts the unit as one recoverable Root or terminates
the session according to its typed RuntimeErrorKind. Native callbacks never
receive Never arguments because blocked units are not entered.

Debug events from a failed unit are session observations, not committed output.
The Host may retain them only in explicit diagnostic/debug mode. Default LSP
analysis discards debug events, and effectful execution commands always use
Strict policy.

## Public and Host API

Best-effort results are Host structures, conceptually:

```text
BestEffortResult = {
    output: Option<Value>,
    diagnostics: Array<Diagnostic>,
    blocked: Option<FailureId>,
}
```

`FailureId` remains crate-private; public callers receive only rendered or
structured Host diagnostics and an output-availability state. `LoadedModule`
strict `execute` methods remain unchanged. LSP/workspace integration is
completed by RFC 0106.

## Goals

1. add an explicit strict/best-effort Host policy;
2. continue deterministic independent pure computations after local failures;
3. reuse the strict VM rather than adding a second evaluator;
4. isolate each failed unit's heap mutations;
5. propagate Never at dependency boundaries without entering blocked code;
6. share quotas, cancellation, provenance, closures, and native semantics;
7. preserve all-or-nothing module and cache publication; and
8. provide structured results for CLI/LSP publication in RFC 0106.

## Non-goals

- resuming an arbitrary VM instruction after RuntimeError;
- putting Never in VM registers, heaps, Values, or native arguments;
- partial containers, exports, module interfaces, or cache entries;
- speculative condition or match-arm evaluation;
- resetting quotas for each independent unit;
- retaining debug output as an external effect;
- using BestEffort for `run`, `exec`, or build effects;
- changing strict execution behavior; or
- publishing diagnostics directly from the evaluator.

## Acceptance criteria

1. strict LoadedModule execution remains behavior compatible and fail-fast;
2. plans use stable IDs, source order, and explicit dependencies;
3. a recoverable unit failure produces one Root and discards its WorkWorld;
4. dependent units are skipped and receive deterministic propagation lineage;
5. later independent units execute and may produce additional root failures;
6. recursive definition SCCs initialize atomically in one unit;
7. direct container children may be diagnosed independently without producing
   a partial container;
8. conditions and matches never speculate branches after a blocked selector;
9. terminal errors stop the session and cannot become Never;
10. all units share quota, cancellation, provenance, native, and debug policy;
11. no failed/stale/cancelled session root enters Main or a shared cache;
12. a complete successful ModuleResult may be exported normally;
13. deterministic root ordering and diagnostic budgets are tested; and
14. full Forma, CLI, LSP, formatting, and strict static checks pass.

## Implementation plan

1. add exhaustive RuntimeErrorKind recoverability classification;
2. define evaluation-plan/unit/result structures and stable DAG validation;
3. add compiler support for atomic binding/SCC/result units and external links;
4. add an isolated session world with atomic successful-unit promotion;
5. implement the deterministic Host scheduler using RFC 0103 outcomes;
6. split safe direct container children without branch speculation;
7. add strict-compatibility, independent-failure, dependency, recursive,
   container, terminal, quota, cancellation, and publication tests; and
8. run the full quality gate and record the implementation result.

## Stopping rules

Work returns to discussion if implementation requires:

1. storing Never in a Forma Value, heap object, or native ABI;
2. resuming arbitrary partially executed VM state;
3. duplicating VM semantics in an AST evaluator;
4. speculative branch execution;
5. partial module/cache publication;
6. resetting quota per unit or making order nondeterministic;
7. splitting recursive SCC initialization; or
8. permitting effectful commands to consume best-effort output.
