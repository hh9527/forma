# RFC 0055: Typed TypeMetadata witnesses

- Status: Implemented
- Depends on: RFC 0003, RFC 0020, RFC 0034, RFC 0048, RFC 0051, RFC 0052, RFC 0053

## Summary

XL refines the static type of TypeMetadata values from the erased `Type` to
`TypeOf(A)`, meaning metadata that describes values of type `A`:

```xl
Int          // TypeOf(Int)
Array(Int)   // TypeOf(Array(Int))
Option(Int)  // TypeOf(Option(Int))
```

This witness relationship allows ordinary generic contracts to recover an
instance type from a metadata argument without dependent function types:

```xl
native decode: for(A) Fn(TypeOf(A), Any) -> Result(A, Any);
native encode: for(A) Fn(TypeOf(A), A) -> Result(Any, Any);
native validate: for(A) Fn(TypeOf(A), Any) -> Result(A, String);
```

`TypeOf(A)` is erased at runtime. Its values remain the existing canonical
TypeMetadata Dicts, including attributed and recursive metadata.

## Motivation

The existing `Type` metatype proves only that a value is valid TypeMetadata.
It forgets which instance type the metadata describes, so codec and validation
boundaries return `Any` even when their metadata argument is statically known.
The generic and bidirectional type machinery can already propagate the needed
relationship once it is represented explicitly.

## Static semantics

`TypeOf` is a unary static type constructor:

```text
TypeOf(A) assignable to Type
Type is not assignable to TypeOf(A)
TypeOf(A) unifies with TypeOf(B) by unifying A with B
```

Primitive metadata bindings have precise witness types. A type constructor
maps metadata witnesses to another metadata witness:

```xl
Int    : TypeOf(Int)
Array  : for(A) Fn(TypeOf(A)) -> TypeOf(Array(A))
Option : for(A) Fn(TypeOf(A)) -> TypeOf(Option(A))
Result : for(A, E) Fn(TypeOf(A), TypeOf(E)) -> TypeOf(Result(A, E))
```

Named type bindings expose `TypeOf(T)` to ordinary expressions. Generic type
parameters used while evaluating explicit type schemes also retain witnesses.

When control flow combines distinct metadata witnesses, the first
implementation may conservatively widen the result to `Type`. It must not
confuse `TypeOf(A) | TypeOf(B)` with `TypeOf(A | B)`.

## Runtime semantics

No runtime value, heap object, bytecode instruction, native ABI, or codec wire
format is added. `TypeOf(A)` validates and executes exactly as `Type`; `A` is
compile-time evidence erased before execution. Native declarations remain
trusted contracts.

Recursive TypeMetadata continues to use hidden up-links. Static type graphs
represent `TypeOf` with an edge to the described type, so recursive witnesses
do not require infinitely expanded descriptors.

## Boundary contracts

The built-in validation function becomes:

```xl
validate: for(A) Fn(TypeOf(A), Any) -> Result(A, String)
```

The codec module becomes:

```xl
native decode: for(A) Fn(TypeOf(A), Any) -> Result(A, Any);
native encode: for(A) Fn(TypeOf(A), A) -> Result(Any, Any);
```

The error parameter remains `Any` in this RFC because the existing diagnostic
Dict has not yet been assigned a public structural alias. Runtime behavior and
located error payloads remain unchanged.

APIs that intentionally accept arbitrary metadata continue to use `Type`.
Passing a `TypeOf(A)` to such an API is valid but erases the witness from its
result unless the API contract preserves it.

## Implementation plan

1. add `TypeOf` nodes to local and workspace type graphs;
2. implement display, persistence, substitution, occurs checks, unification,
   assignability, and variable erasure;
3. infer precise witnesses for primitive, constructed, and named metadata;
4. expose precise generic schemes for metadata constructors;
5. refine `validate`, `codec.decode`, and `codec.encode` contracts;
6. retain erased `Type` behavior at runtime;
7. test direct, aliased, generic, recursive, and widened metadata flows.

## Acceptance criteria

1. `Int` is observed as `TypeOf(Int)` while remaining assignable to `Type`;
2. metadata constructors preserve their instance relationship;
3. `decode(User, input)` has type `Result(User, Any)`;
4. `encode(User, user)` statically checks the value against `User`;
5. `validate(User, input)` has type `Result(User, String)`;
6. generic wrappers can pass a `TypeOf(A)` without evaluating its concrete
   metadata value;
7. erased `Type` cannot fabricate a specific witness;
8. recursive named metadata remains finite and publishable;
9. runtime codec, validation, and diagnostic behavior is unchanged;
10. workspace tests and strict static checks pass.

## Deferred work

- a `typeof(metadata)` type-projection keyword;
- preserving unions of different metadata witnesses across control flow;
- richer machine-readable fields beyond RFC 0056's public `BlameError`;
- singleton metadata values and general dependent function contracts;
- reflection over arbitrary runtime values.

## Rejected alternatives

### Dependent return contracts

`Fn(ty: Type, Any) -> Result(typeof(ty), Error)` directly references a value
parameter from its result type. It requires named dependent function
parameters and tool-stage value evaluation at every call. `TypeOf(A)` expresses
the required relation through existing parametric polymorphism instead.

### Keep returning Any

This preserves the current implementation but discards type information at the
most important external-data boundary and prevents Result combinators from
carrying decoded instance types.
