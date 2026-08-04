# RFC 0061: Homogeneous Dict TypeMetadata

- Status: Accepted
- Depends on: RFC 0016, RFC 0022, RFC 0033, RFC 0035, RFC 0051, RFC 0055

## Summary

Forma adds a homogeneous dictionary type constructor:

```forma
Dict(String)
Dict(User)
```

`Struct` continues to describe a closed set of statically named fields with
potentially different types. `Dict(Item)` describes arbitrary String keys
whose values all have type `Item`. It is ordinary first-class TypeMetadata and
participates in generic witnesses, validation, codecs, JSON Schema, workspace
facts, and tooling.

## Motivation

Forma already has immutable Dict runtime values, Dict literals, JSON objects,
and `@bim/std/dict` operations. The type world can currently describe only a
fixed-shape Struct. Dynamic homogeneous maps consequently degrade to `Any`.

This gap appears at ordinary data boundaries, not only in executable plans:

```forma
@struct type ExecRequest = {
    args: Array(String),
    env: Dict(String),
    cwd: String,
};
```

Environment snapshots, labels, headers, lookup tables, and JSON objects with
dynamic keys all require the same distinction. Modeling each as a Struct is
incorrect because its key set is not statically closed.

## Metadata representation

`Dict(Item)` evaluates through the normal tool-stage function call and returns
canonical metadata equivalent to:

```forma
{kind: 'Dict, item: Item}
```

The static contract is generic and preserves the instance witness:

```forma
Dict: for(A) Fn(TypeOf(A)) -> TypeOf(Dict(A))
```

`Dict` is available in the ordinary runtime and static preludes beside
`Array`. Attribute wrappers remain transparent. Recursive item metadata uses
the existing hidden up-link machinery without a Dict-specific recursion
mechanism.

## Struct and Dict

Struct and Dict remain distinct:

- `{name: String, age: Int}` is a closed heterogeneous Struct;
- `Dict(String)` is an open homogeneous String-keyed map;
- an unannotated Dict literal retains its exact Struct type;
- when a Dict literal is checked against `Dict(T)`, every field expression is
  checked against `T` and the expression has type `Dict(T)`;
- an empty literal checked against `Dict(T)` has that expected type;
- a Struct value is assignable to `Dict(T)` exactly when every field value is
  assignable to `T`;
- `Dict(T)` is not assignable to a Struct because its key set is unknown;
- `Dict(A)` is assignable to `Dict(B)` when `A` is assignable to `B`.

No implicit widening changes the inferred type of an unannotated literal.
Field completion therefore remains available for ordinary records. Explicit
annotations and generic function expectations provide the widening point.

## Runtime validation

Validation of `Dict(T)` requires a runtime Dict and visits every key in
canonical order. Every value is validated against `T`; failures use a path
containing the actual key. The key domain remains String because runtime Dict
shapes already use String keys.

The container and its entries use the existing value locations. There is no
new mutable map representation and no conversion of Struct-shaped runtime
values: Struct and Dict are type descriptions over the same immutable Dict
value kind.

## Generic core Dict functions

The built-in Dict module exposes typed contracts:

```forma
native keys: for(A) Fn(Dict(A)) -> Array(String);
native values: for(A) Fn(Dict(A)) -> Array(A);
native pairs: for(A) Fn(Dict(A)) -> Array(Tuple(String, A));
native from_pairs: for(A) Fn(Array(Tuple(String, A))) -> Dict(A);
native merge: for(A) Fn(Dict(A), Dict(A)) -> Dict(A);
```

Exact Struct arguments may flow into these functions through Struct-to-Dict
assignability. Their results are open Dict values because enumeration,
construction, and merge do not preserve a statically closed field set.

## Codec behavior

`codec.decode(Dict(T), input)` requires a JSON-shaped Dict and decodes every
value using `T`. Keys are retained exactly. `codec.encode` validates the same
shape and emits an ordinary JSON object. Unknown-field rejection does not
apply because every String key belongs to the Dict domain.

Dict metadata does not accept Struct field decorators such as `rename`,
`default`, `flatten`, or `skip_serializing_if`. Those policies require named
model fields. Attributes on the item metadata continue to apply normally.

Recursive values, allocation accounting, source provenance, BlameError data
and rule locations, and cancellation follow the existing codec machinery.

## JSON Schema

`json.schema(Dict(T))` produces:

```json
{
  "type": "object",
  "additionalProperties": "<schema(T)>"
}
```

Here `<schema(T)>` denotes the ordinary nested schema object, not a String.
Recursive item types use the existing `$defs` and `$ref` graph handling.

## Tooling

Type graphs and workspace snapshots retain a dedicated Dict node. User-facing
display uses `Dict<T>` consistently with the existing `Array<T>` display.
Hover, `forma show`, definition facts, and LSP facts therefore preserve the
item type instead of reporting `Any`.

Dynamic keys do not produce Struct-field completion. This is deliberate: the
type states the value type but provides no finite key vocabulary.

## Non-goals

- non-String dictionary keys;
- mutable maps or insertion-order semantics;
- row polymorphism or open Structs;
- key-pattern or key-enum constraints;
- per-key decorators for Dict metadata;
- changing unannotated Dict literals from exact Struct inference;
- implementing the executable effect protocol or effectful `forma exec`.

## Implementation plan

1. Add Dict nodes to runtime, persistent, static, and workspace type graphs.
2. Add the generic `Dict` TypeMetadata constructor and prelude contracts.
3. Extend decoding, display, unification, assignability, substitution,
   variable checks, erasure, and runtime validation.
4. Contextually check Dict literals against expected `Dict(T)` while retaining
   exact Struct inference without that expectation.
5. Type the existing `@bim/std/dict` functions.
6. Add Dict codec planning, bidirectional traversal, allocation accounting,
   recursive links, and JSON Schema generation.
7. Add vertical runtime, static analysis, schema, `show at`, and workspace/LSP
   tests.

## Acceptance criteria

1. `Dict(String)` evaluates to canonical first-class TypeMetadata.
2. `Dict` has a generic TypeOf-preserving static contract.
3. an annotated heterogeneous literal fails at the precise incompatible value;
4. an annotated empty literal has the expected Dict type;
5. unannotated literals retain exact Struct fields and completion behavior;
6. Struct-to-Dict assignment accepts homogeneous fields and rejects an
   incompatible field; Dict-to-Struct assignment is rejected;
7. core Dict combinators preserve their generic item types;
8. validate, encode, and decode traverse arbitrary keys with precise paths and
   BlameError locations;
9. JSON Schema uses object `additionalProperties` with the item schema;
10. recursive Dict item metadata terminates through existing graph links;
11. `forma show` and workspace/LSP hover display `Dict<T>` without flattening
    to `Any`;
12. existing Struct, JSON, codec, schema, module, quota, and cancellation
    behavior remains unchanged;
13. formatting, strict Clippy, and the full workspace test suite pass.

## Rejected alternatives

### Treat every Dict literal as homogeneous

This loses exact field types and Struct-field completion, and forces unrelated
field values toward a broad common type. Forma already relies on fixed Dict
literals as record values.

### Reuse Struct with an open flag

An open heterogeneous row and a homogeneous dynamic map have different type
operations and inference requirements. Combining them would prematurely add
row-polymorphic semantics.

### Keep dynamic maps as Any

This discards the value type at JSON, environment, and standard-library
boundaries and directly prevents typed executable requests.

### Make Dict a codec-only schema

The need exists in ordinary functions, annotations, generic combinators, and
tooling. A privileged codec node would violate the first-class metadata model.
