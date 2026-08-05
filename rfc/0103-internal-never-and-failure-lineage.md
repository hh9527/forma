# RFC 0103: Internal Never and failure lineage

- Status: Proposed
- Depends on: RFC 0100, RFC 0102

## Summary

Forma defines an evaluator-internal failed outcome and a bounded data-dependency
lineage:

```text
EvalOutcome(T) = Value(T) | Never(FailureId)

FailureNode =
    Root(RootFailure)
    Propagated(Operation, Location, causes)
    Truncated(causes)
```

Never is not the language's static bottom type and is never represented by a
`Value`. A root failure is diagnosed once. Operations blocked by Never add
lineage without emitting cascaded diagnostics. Multiple failed inputs produce
one DAG node with stable, deduplicated causes.

This RFC establishes the internal model, classification, propagation matrix,
and budgets. RFC 0104 connects it to Host-selected best-effort evaluation;
strict execution remains unchanged here.

## Motivation

Continuing after an evaluation error by inventing a Forma value would make
ordinary code observe invalid state. Re-emitting an error at every dependent
operation would produce diagnostic cascades. A call stack also cannot explain
why a later output is unavailable when its computation never ran.

The evaluator therefore needs a non-value outcome and a data-dependency trace:

```text
invalid imported field
  -> blocked addition
  -> blocked Function call
  -> unavailable requested output
```

This trace complements rather than replaces provenance and runtime call stacks.

## Internal representation

The implementation introduces opaque, generation-scoped identifiers:

```rust
struct FailureId(u32);

enum EvalOutcome<T> {
    Value(T),
    Never(FailureId),
}
```

`FailureId` is meaningful only within one evaluation session. It cannot cross
the public `Value` boundary, module publication, serialization, shared caches,
or Forma ABI.

The arena stores:

```rust
enum FailureNode<R> {
    Root { failure: R },
    Propagated {
        operation: FailureOperation,
        location: Option<Location>,
        causes: SmallCauseSet,
    },
    Truncated { causes: SmallCauseSet },
}
```

The generic root payload lets the VM/Host retain its authoritative runtime
failure without coupling the lineage container to diagnostic rendering.

## Failure operations

`FailureOperation` is a closed, coarse internal enum suitable for concise
explanations:

- Unary and Binary;
- Field and Index;
- Call and NativeCall;
- Condition and Match;
- Array, Tuple, Tagged, and Dict construction;
- Interpolation;
- Binding and ModuleResult; and
- Other for internal operations that must not leak implementation names.

It records semantic dependency shape, not bytecode opcodes. Generated adapter
names, registers, heap handles, and native callback identities never appear in
user output.

## Cause normalization

When an operation consumes Never inputs, the arena:

1. collects their `FailureId`s in left-to-right operand order;
2. removes duplicates while preserving first occurrence;
3. reuses an existing propagation node with the same operation, location, and
   normalized causes when available; and
4. otherwise allocates one bounded node.

Aliasing does not allocate a node and preserves the original `FailureId`.
Multiple uses of the same failed value therefore remain a DAG rather than an
exponentially expanding tree.

## Propagation matrix

The common rules are:

| Evaluation form | Never behavior |
| --- | --- |
| alias/move | preserve the same FailureId |
| unary/binary/interpolation | do not execute; propagate all Never operands |
| field/index | do not inspect the non-value; propagate receiver/index causes |
| ordinary/native call | if callee or any argument is Never, do not enter it |
| condition | do not evaluate either branch when condition is Never |
| match | do not evaluate arms when scrutinee is Never |
| container | evaluate direct children independently; any Never blocks result |
| binding | record the binding dependency and continue only by Host policy |
| module result | record why the requested result is unavailable |

For a call with valid inputs that fails inside its body, the runtime error is a
new Root with its ordinary call stack. It is not a propagation of the arguments.

Container evaluation is eager and deterministic. Direct siblings may each
produce root failures under best-effort scheduling, but no partial container is
constructed or published if any child is Never.

## Root versus terminal failures

RFC 0104 will decide which runtime errors may become recoverable Roots. This RFC
fixes the required classification boundary:

- validation, type mismatch, missing field, arithmetic domain errors, and
  non-exhaustive data matches may be recoverable roots;
- cancellation, stale revision, fuel/allocation/stack/call-depth quotas,
  out-of-memory, invalid bytecode, VM invariants, and Host I/O authority errors
  are terminal and never enter the arena.

The lineage API requires the caller to classify a failure explicitly. It does
not infer recoverability from an error string.

## Budgets and truncation

Failure tracking is bounded independently from ordinary evaluation quota:

- maximum arena nodes;
- maximum direct causes per node; and
- maximum rendered lineage depth.

When the node budget is exhausted, propagation returns a stable Truncated node
referencing as many normalized causes as the cause budget permits. Root
diagnostics are never discarded merely to retain propagation detail. Rendering
marks truncation once and remains deterministic.

The default limits are Host configuration in RFC 0104. The arena API takes
explicit limits so tests and Hosts cannot accidentally rely on global mutable
state.

## Diagnostics

Creating a Root does not itself publish a diagnostic. The Host owns root
selection, source rendering, ordering, and publication. Propagated nodes are
silent by default.

When a requested output is Never, tooling may walk the shortest stable path to
each distinct Root and render a compact "unavailable because" trace. It must
not restate each root diagnostic or turn every propagation node into an error.

## Goals

1. represent failed evaluation without constructing a Forma value;
2. retain one authoritative root failure and suppress dependent cascades;
3. record bounded, deterministic data-dependency lineage;
4. merge multiple Never causes without duplicating subgraphs;
5. define complete eager propagation behavior before Host recovery is added;
6. keep terminal failures outside best-effort recovery; and
7. provide the substrate used by RFC 0104 and RFC 0106.

## Non-goals

- exposing Never, FailureId, or lineage to Forma code;
- changing the static Never/bottom type;
- catching, matching, serializing, or exporting failed outcomes;
- language-level exceptions, accumulation, or algebraic effects;
- publishing diagnostics from the arena;
- continuing execution in this RFC;
- speculative branch evaluation;
- partial containers or partial module exports; or
- replacing runtime call stacks with data lineage.

## Acceptance criteria

1. `EvalOutcome<T>` cannot be converted into a public Value when Never;
2. roots retain an authoritative typed failure payload;
3. aliases preserve FailureId without allocation;
4. propagation normalizes duplicate causes in stable operand order;
5. identical propagated dependencies reuse one DAG node;
6. multiple causes remain distinct and deterministically ordered;
7. every form in the propagation matrix has an explicit operation category;
8. terminal failures cannot be inserted through the recoverable-root API;
9. node/cause/depth budgets truncate deterministically;
10. lineage rendering exposes no registers, handles, or generated names;
11. the model adds no public Forma syntax, type, value, or ABI; and
12. full Forma, CLI, LSP, formatting, and strict static checks pass.

## Implementation plan

1. add the private failure arena, typed IDs, outcomes, operations, and limits;
2. require explicit recoverable root classification;
3. implement stable cause normalization, propagation interning, and truncation;
4. encode the propagation matrix as reusable outcome combinators;
5. add unit tests for aliasing, DAG reuse, multi-cause order, terminal rejection,
   budgets, and bounded rendering;
6. keep all strict VM and module entry points behavior-identical; and
7. run the full quality gate and record the implementation result.

## Stopping rules

Work returns to discussion if implementation requires:

1. a public Never value or catch/match operation;
2. changing static bottom-type inference;
3. speculative branch execution or lazy evaluation;
4. partial container/module publication;
5. diagnosing every propagation node;
6. treating cancellation, quotas, or invariants as recoverable;
7. merging provenance, call stacks, and lineage into one structure; or
8. unbounded graph retention.
