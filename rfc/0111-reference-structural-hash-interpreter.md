# RFC 0111: Reference structural hash interpreter

- Status: Implemented
- Depends on: RFC 0096 through RFC 0099, RFC 0107, RFC 0110

## Summary

Forma adds an ordinary user-space structural hash example using explicit
HashState threading:

```forma
def hash_dyn:
    Fn(Dyn, HashState) -> Result(HashState, BlameError) = ...;

def my_hash:
    for(A) Fn(TypeOf(A))
        -> Fn(A, HashState) -> Result(HashState, BlameError) =
    interpreter!(hash_dyn);
```

The adapter packs only interpreted value parameters as Dyn. HashState remains
an ordinary typed parameter and result; it is never hidden or finalized by the
interpreter mechanism.

## Structural protocol

Every supported value first writes a stable String kind marker. Composite
values then write their item/field count and recursively hash content in public
observer order. Struct and Dict fields write each field name before its value.
Tags write the tag name and a payload-presence Int before an optional payload.

The example supports Int, String, Bytes, Array, Tuple, Struct, Dict, Atom,
Tagged, Enum, WithAttributes, and resolved Ref descriptors. Dict observer order
is already canonical. The first recursive failure exits through
`array.fold_control` and preserves its BlameError.

Float has no canonical hash primitive in this phase. Function, Any, Never,
Type, TypeOf, Union, Bound, Dyn, unresolved Ref, Opaque, Float, and future
unhandled descriptor kinds return explicit BlameError. There is no fallback to
debug formatting, heap identity, or native equality.

## Placement

The implementation lives at `examples/reference-hash.forma`, imports only
public `@bim/std` modules, and exports `my_hash` plus erased `hash_dyn`. It is a
mechanism test, not a production Hash capability, operator, trait, or implicit
derivation rule.

## Acceptance criteria

1. the example is ordinary Forma code using only public APIs;
2. `my_hash` retains the authored generic explicit-state contract;
3. equal supported structures produce equal final digest Bytes;
4. changed kind, boundary, field name, tag, or payload changes the digest;
5. input HashState aliases remain unchanged;
6. recursive failures preserve sourced BlameError and stop early;
7. Function, Opaque, Float, and unknown kinds fail explicitly;
8. no VM structural-hash operation or hidden accumulator is added; and
9. full workspace tests and strict Clippy pass.

## Implementation plan

1. implement descriptor normalization and sourced blame helpers;
2. implement explicit-state sequence, field, and tag traversal;
3. lift `hash_dyn` through the existing parameter-wise interpreter adapter;
4. add exact behavioral, scheme, alias, distinction, and failure tests;
5. record the implementation result.

## Implementation result

Implemented in `examples/reference-hash.forma` using only public standard
modules. The example normalizes attributed and linked descriptors, writes
explicit structural boundaries, threads immutable HashState through
`array.fold_control`, and lifts the erased worker with `interpreter!`.

The integration test covers stable equality, changed values, field-name,
Array/Tuple, and tag-payload distinctions, unchanged input-state aliases, and
explicit Function, Float, Opaque, and recursively encountered Float failures.
Native opaque descriptors are valid leaves in runtime type graphs, including
inside Result, while JSON codecs and JSON Schema generation continue to reject
them explicitly.
