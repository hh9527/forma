# RFC 0094: Forma structural equality interpreter

- Status: Implemented
- Depends on: RFC 0089 through RFC 0093
- Tracking issue: https://github.com/hh9527/forma/issues/4

## Summary

Forma implements a reference structural equality interpreter in ordinary Forma
code and lifts it through `interpreter`:

```forma
def equal_dyn: Fn(Dyn, Dyn) -> Result(Bool, BlameError) = ...;

def equal:
    for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Result(Bool, BlameError) =
    interpreter(equal_dyn);
```

This is a conformance test for the public TypeDesc/Dyn model, not a replacement
for the native `==` operator. The implementation uses ordinary recursion,
pattern matching, standard-library combinators, and public observers. It has no
privileged heap or VM access.

## Minimal supporting APIs

Two general structural operations are still required for user-space traversal:

```forma
native fields:
    Fn(Dyn) -> Result(Array(Tuple(String, Dyn)), BlameError);

native zip:
    for(A, B) Fn(Array(A), Array(B)) -> Option(Array(Tuple(A, B)));
```

`dyn.fields` accepts Struct and Dict. It returns canonical field order and each
child remains paired with its exact descriptor. A descriptor/runtime field-set
mismatch is blame. It complements known-name `dyn.field`; it does not expose
raw Dict storage or descriptor internals.

`array.zip` returns `'Some(pairs)` only when both arrays have equal length and
`'None` otherwise. It is a type-preserving standard combinator, not an equality
primitive. Tuple comparison uses `dyn.tuple_items` followed by the same zip.

## Supported domain

The reference interpreter supports:

- Int, Float, String, and Bytes leaves;
- Atom tags;
- Array and Tuple elements;
- Struct and homogeneous Dict fields;
- Tagged and Enum tag/payload pairs;
- WithAttributes by following its descriptor child; and
- Ref by resolving the explicit descriptor edge.

Ref and WithAttributes normalization does not alter the Dyn package. Existing
Dyn observers already resolve references and strip attributes when validating
the package, so recursive interpretation proceeds on the finite runtime value.

Bool is covered through its canonical Enum representation. Finite recursive
Struct/Enum data terminates because Forma values are acyclic even when their
descriptors contain Ref edges.

## Unsupported domain

Function is an opaque leaf and returns `Err(BlameError)`. Any, Never, Type,
TypeOf, Union, Bound, and any newly added unhandled descriptor kind also return
explicit blame. The interpreter must not silently fall back to native `==` for
these cases.

Union branch selection and metadata-value equality need distinct public
contracts. Supporting them here would hide a reflection gap inside the example.

## Equality behavior

Primitive leaves compare their checked values with ordinary `==`. Structural
values first check matching public kinds, lengths, field names, and tags, then
recurse in canonical order. The first observer or recursive failure is returned
as blame; an ordinary inequality returns `Ok('False)`.

For every supported acyclic value pair:

```text
equal(T)(left, right) == Ok(left == right)
```

The conformance suite covers equal and unequal pairs, nested mixed shapes, and
a finite value of a recursive type. It also verifies explicit rejection of a
Function leaf.

## Goals

1. prove that ordinary Forma code can interpret public type/value structure;
2. validate typed lifting on a useful recursive capability;
3. close field enumeration and pairwise Array traversal with general APIs;
4. agree with native equality throughout the documented supported domain; and
5. make every boundary outside that domain explicit and recoverable.

## Non-goals

- replacing or reimplementing the native equality fast path;
- cyclic runtime values or cycle detection;
- Function, Union, Any, or metadata-value equality;
- operation-specific native recursion or dispatch;
- memoized capability factories or specialized bytecode; or
- a general fallback/coherence mechanism.

## Acceptance criteria

1. `dyn.fields` preserves canonical names, order, and child descriptors;
2. `array.zip` preserves both element types and rejects unequal lengths;
3. the interpreter source is an ordinary Forma module;
4. primitive, sequence, record, tag, and enum equality agrees with native `==`;
5. nested and finite recursive values execute through ordinary recursion;
6. field, length, tag, and payload differences return `Ok('False)`;
7. observer failures propagate as `BlameError`;
8. Function and every unsupported descriptor kind return explicit blame;
9. the lifted public capability retains its authored generic scheme; and
10. the full workspace quality gate passes without new VM interpreter logic.

## Implementation plan

1. add and test the type-preserving `array.zip` combinator;
2. add and test canonical `dyn.fields` observation;
3. write the erased recursive equality interpreter as Forma source;
4. expose its lifted typed capability from a standard module;
5. add supported-domain native conformance and unsupported-domain tests;
6. run the full quality gate and record the implementation result; and
7. mark umbrella RFC 0089 Implemented only after all shared criteria hold.

## Implementation result

Implemented `array.zip`, canonical `dyn.fields`, and `@bim/std/equality`.
`equal_dyn` and its descriptor normalization, sequence/field folds, payload
comparison, and recursive dispatch are Forma definitions. The module receives
the same typed native observer/combinator declarations as their public modules;
there is no native equality dispatcher or new VM operation.

The lifted `equal` capability covers Int, Float, String, Bytes, Array, Tuple,
Struct, Dict, Atom, Tagged, Enum, attributes, and explicit recursive references.
Tests exercise equal and unequal primitives, sequence lengths, Dict values,
Enum tags/payloads, and finite recursive Struct values. Function descriptors
and the other documented unsupported kinds return structured blame rather than
falling back to native equality.

Core modules still maintain a legacy tree-shaped `Value` projection alongside
their authoritative persistent heap root. Recursive equality closures cannot be
represented by that projection, so this module omits only that compatibility
preview while retaining its persistent runtime export and `ModuleInterface`.
Normal imports, execution, recovery, CLI, and LSP use those authoritative forms
and pass their full suites. General removal of the legacy preview is separate
module-loader cleanup, not an interpreter requirement.

Full Forma tests pass with 287 passed and 1 ignored; all 13 CLI and 20 LSP tests
pass, and strict workspace Clippy reports no warnings.

## Amendment: standard placement

RFC 0095 retains this implementation as the executable reference interpreter
at `examples/reference-equality.forma`, rather than as the production standard
equality module. `==` remains authoritative and `@bim/std/eq.equal` provides
the same VM behavior as a first-class Function. The reference interpreter now
uses `array.fold_control` for immediate inequality and blame exits.

Accordingly, `@bim/std/equality`, its duplicated native declarations, and its
legacy core export exception have been removed. This RFC's capability proof and
supported reflection boundary remain implemented; only its standard-library
placement was superseded.
