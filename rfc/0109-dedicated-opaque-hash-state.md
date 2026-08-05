# RFC 0109: Module-owned opaque value boundary

- Status: Implemented
- Depends on: RFC 0051, RFC 0090, RFC 0107

## Summary

Forma adds one generic boundary for immutable values whose representation is
owned by a standard-library or Host module. A module may declare a nominal
opaque type and bind native operations over it without adding a new language
or VM value kind for every library capability.

RFC 0110 uses this boundary to define `HashState` in `@bim/std/hash`. The VM
knows only the generic `Opaque` carrier; it never knows SHA-256 or HashState.

## Type and runtime boundaries

An opaque declaration is module-scoped and nominal:

```forma
native type HashState = @3;
```

Its runtime identity is a fixed native module ID plus an explicit local type
slot. Its stable descriptive identity is the resolved module ID plus
local type name. Opaque
types participate generically in TypeDescriptor, TypeGraph, TypeOf, Function
contracts, inference, validation, display, TypeDesc observation, heap
copy/promotion, debug formatting, and equality as atomic leaves.

TypeDesc reports kind `Opaque`, a stable qualified name, and no children. Dyn
may pack an opaque value but supplies no unchecked extraction API. Ordinary
codecs, JSON, schema, and external static data reject opaque values with a
focused diagnostic unless their owning module supplies an explicit codec.

The payload contract supplies bounded debug and equality operations. Equality
is permitted only between values with the same opaque type identity and then
uses the owning implementation's logical equality. Aliases are immutable
snapshots. Payloads are shared by `Arc`; an update returns another value and
cannot mutate an observable alias.

## Surface

An owning module combines the declaration with ordinary native Functions:

```forma
native type HashState = @3;
native new: Fn() -> HashState;
native update: Fn(HashState, Bytes) -> HashState;
{HashState: HashState, new: new, update: update}
```

Forma code cannot construct or inspect an opaque payload. Native code receives
a checked typed projection through the Host ABI; it does not downcast an
arbitrary VM object or depend on heap handles.

## Goals

1. establish one reusable ordinary-value boundary for module-owned state;
2. preserve immutable aliases and heap publication;
3. expose stable Type/TypeDesc identity without representation access;
4. let a new Host module add opaque types without modifying VM enums; and
5. keep this boundary smaller than a resource or general FFI system.

## Non-goals

- hash update or finish operations;
- dynamic loading or a package-level native extension mechanism;
- unchecked arbitrary payload downcasting;
- Host resources, handles, ports, ownership, or Move semantics;
- codec or serialization support; or
- structural reflection into the context.

## Acceptance criteria

1. `native type T = @n;` links an unforgeable module-owned nominal type witness;
2. Function contracts can accept and return that type;
3. TypeDesc reports `Opaque`, its qualified name, and no children;
4. one generic runtime carrier supports all declared opaque types;
5. heap copy/promotion and public Value import/export retain type and payload;
6. equality dispatches to the payload contract and is independent of heap identity;
7. debug output is bounded and does not reveal internal context bytes;
8. codec/JSON reject HashState explicitly;
9. no resource table, invalidation, finalizer, or ownership semantics appear;
10. a second fixture opaque type requires no new VM/type enum variant; and
11. full workspace tests and strict Clippy pass.

## Implementation plan

1. add the native type declaration, module/local identity, and qualified metadata;
2. add one immutable, cloneable opaque payload carrier and checked Host API;
3. teach heap copying, equality, export/import, debug, and JSON boundaries once;
4. add generic TypeDesc observation;
5. validate two independent fixture types without VM enum growth;
6. add focused boundary tests; and
7. record the implementation result.

## Stopping rules

Work returns to discussion if implementation requires a resource table,
observable alias mutation, unsafe unchecked downcasting, dynamic native
loading, finalizers, or a general ownership/marker system.

## Implementation result

Implemented `native type T = @n;` as a top-level linked type declaration. Each
native module receives an unobservable registry ID, and each declared type an
explicit local slot; source code cannot construct either. A stable qualified
name remains available for display and persistence/rebinding. Type metadata and
TypeDesc use one generic `Opaque` node;
`type-desc.opaque_name` exposes the qualified identity and `children` is empty.
Function contracts and runtime validation compare that nominal identity.

The runtime adds one `OpaqueValue` carrier. It stores an immutable `Arc`
payload, checked Rust type projection, bounded representation-free debug
output, and a logical equality function established at construction. Heap
publication and legacy Value import/export clone the carrier without exposing
heap handles. The public synchronous native API can construct and project
typed opaque values without modifying VM dispatch enums for each library type.

JSON and codec planning reject opaque values and types by default. Tests cover
the declaration surface, deterministic qualified identity, TypeDesc
observation, codec rejection, cross-heap sharing, logical equality, nominal
separation, and checked projection. No resource table, invalidation, finalizer,
dynamic loading, or ownership semantics were introduced.
