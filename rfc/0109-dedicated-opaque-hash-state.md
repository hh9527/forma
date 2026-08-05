# RFC 0109: Dedicated opaque HashState

- Status: Proposed
- Depends on: RFC 0051, RFC 0090, RFC 0107

## Summary

Forma adds one dedicated nominal builtin leaf type, `HashState`, exported as a
Type witness by `@bim/std/hash`. Its runtime representation is a heap-owned
immutable hash context. Forma code can pass, return, alias, compare, and place
it in ordinary containers, but cannot construct or structurally inspect it.

This is a focused closed-enum extension, not a general native opaque payload
ABI and not a Host resource.

## Type and runtime boundaries

HashState participates in TypeDescriptor, TypeGraph, TypeOf, Function
contracts, inference, validation, display, TypeDesc kind observation, heap
copy/promotion, debug formatting, and structural equality as an atomic leaf.

It has no fields or children. Dyn can pack it, report an opaque HashState kind,
and return no structural children, but supplies no unchecked extraction API.
Ordinary codecs, JSON, schema, and external static data reject HashState with a
focused unsupported-opaque diagnostic.

Equality compares logical context state, not heap or Arc identity. Aliases are
immutable snapshots. There is no invalidation, generation, finalizer, session
lifetime, or resource-table entry.

## Surface

`@bim/std/hash` exports:

```forma
{
    HashState: HashState,
    # existing and later hash Functions
}
```

HashState is not structurally constructible. The state constructor and update
operations arrive in RFC 0110; RFC 0109 validates the type/value plumbing with
internal fixtures.

## Goals

1. establish a precise ordinary-value boundary for hash state;
2. preserve immutable aliases and heap publication;
3. expose stable Type/TypeDesc identity without representation access; and
4. avoid premature general native-object machinery.

## Non-goals

- hash update or finish operations;
- a user declaration syntax for opaque/native types;
- arbitrary native payloads or downcasting;
- Host resources, handles, ports, ownership, or Move semantics;
- codec or serialization support; or
- structural reflection into the context.

## Acceptance criteria

1. HashState has a stable displayed type and TypeOf witness;
2. `@bim/std/hash` exports that witness;
3. Function contracts can accept and return HashState;
4. TypeDesc reports a dedicated opaque leaf with no children;
5. heap copy/promotion and public Value export retain logical state;
6. equality is logical and independent of heap identity;
7. debug output is bounded and does not reveal internal context bytes;
8. codec/JSON reject HashState explicitly;
9. no resource table or generic native payload ABI is introduced; and
10. full workspace tests and strict Clippy pass.

## Implementation plan

1. add the dedicated type and runtime leaf across closed enums;
2. add an internal immutable HashContext representation;
3. teach heap copying, equality, export/import, debug, and JSON boundaries;
4. add Type metadata and TypeDesc observation;
5. export the witness from `@bim/std/hash`;
6. add focused boundary tests; and
7. record the implementation result.

## Stopping rules

Work returns to discussion if implementation requires a resource table,
observable alias mutation, user-extensible native payload vtables, unsafe
downcasting, or a general ownership/marker system.
