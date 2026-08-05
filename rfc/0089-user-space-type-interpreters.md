# RFC 0089: User-space type interpreters

- Status: Proposed
- Depends on: RFC 0055, RFC 0080 through RFC 0088
- Tracking issue: https://github.com/hh9527/forma/issues/4

## Summary

Forma will let ordinary Forma functions interpret runtime type metadata and
then lift those erased interpreters into explicitly typed capabilities:

```forma
def my_eq_i:
    Fn(Dyn, Dyn) -> Result(Bool, BlameError) =
    fn(left, right) {
        # Ordinary Forma code and ordinary recursion.
        ...
    };

def eq_fn:
    for(A) Fn(TypeOf(A)) ->
        Fn(A, A) -> Result(Bool, BlameError) =
    interpreter(my_eq_i);
```

`interpreter` is a contextual compiler keyword, not a runtime Function. `Dyn`
is an opaque package that keeps one runtime value bound to its precise logical
type descriptor. Recursive type graphs expose explicit `$ref` nodes; recursive
execution remains an ordinary Forma function following a finite value.

This is an umbrella RFC. Child RFCs independently specify, implement, test,
and commit each boundary. The tracking issue owns mutable progress; this
document owns the phase boundary and stopping rules.

## Motivation

Forma already makes types available as canonical runtime metadata through
`TypeOf(A)`. Native codec and equality logic can interpret that metadata, but
ordinary Forma code cannot yet inspect every matching runtime value safely or
turn such an interpreter into a statically typed reusable Function.

The missing connection should not require traits, implicit implementation
search, or runtime code generation. A caller explicitly selects one factory;
the result is an ordinary Function value:

```forma
def EqUser: Fn(User, User) -> Result(Bool, BlameError) = eq_fn(User);
```

User-defined and native interpreters should have the same public logical reach.
Native code may retain representation access and optimization privileges, but
not exclusive knowledge of public data shapes.

## Phase sequence

The planned sequence is:

1. RFC 0090: public `TypeDesc` graph and explicit recursive references;
2. RFC 0091: opaque `Dyn` packages and checked primitive projections;
3. RFC 0092: structural `Dyn` observers;
4. RFC 0093: contextual typed `interpreter` lifting; and
5. RFC 0094: reference equality interpreter and conformance validation.

The order is deliberate. Metadata must be observable before it can be paired
with values; the pair must preserve its invariant before structural traversal;
and lifting is accepted only after its erased operand ABI is executable and
testable directly.

## Semantic model

The trusted boundary starts with:

```text
TypeOf(A), A
```

and constructs the existential package:

```text
Dyn = exists A. (TypeOf(A), A)
```

User code cannot forge or destructure `Dyn`. Safe observers return either a
concrete checked leaf or another `Dyn` containing the corresponding child
descriptor and child value.

For an expected scheme:

```text
for(A) Fn(TypeOf(A)) -> Fn(A, A) -> R
```

the contextual keyword requires an erased operand:

```text
Fn(Dyn, Dyn) -> R
```

and lowers to ordinary closures that perform trusted packing. `R` may not
contain `A` in this phase.

## Recursion model

The public `TypeDesc` is a finite graph. Recursive edges are explicit `$ref`
nodes with graph-local identity and safe resolution. The public view treats a
Function descriptor as an indivisible leaf; it does not traverse parameter or
result descriptors.

Forma currently has no cyclic runtime values. An interpreter of a recursive
data type follows a finite value and its descriptor together, using an
ordinary recursive Forma definition. This phase does not require an
open-recursion dispatcher, visited set, recursive capability plan, or automatic
memoization.

## Goals

1. expose a stable logical `TypeDesc` graph without leaking VM up-links;
2. prevent a descriptor from being paired with an unrelated erased value;
3. support safe primitive and structural observation in ordinary Forma code;
4. lift a narrowly accepted erased interpreter into a type-preserving input
   capability;
5. implement one finite recursive equality interpreter in Forma;
6. compare its supported behavior with authoritative native equality;
7. preserve cancellation, quota, determinism, and atomic publication rules;
   and
8. establish a reusable foundation for later Show, Hash, Validate, and Diff
   experiments.

## Non-goals

- traits, interfaces, type classes, or implicit capability search;
- global `Unknown`, a top type, subtyping, or implicit dynamic conversion;
- quote, splice, runtime code generation, or type-specialized bytecode;
- cyclic runtime values or a universal cycle policy;
- Function signature reflection or traversal below Function descriptors;
- automatic fallback chains, open-recursion dispatch, or interpreter
  memoization;
- reconstructing, decoding, cloning, or otherwise returning an `A` from `Dyn`;
- higher-order erased parameters whose callback signature contains `A`;
- higher-rank schemes, overloads, coercions, or variadic lifting; or
- exposing raw heap identity, handles, registers, or up-links.

## Shared safety rules

1. ordinary Forma code cannot construct a mismatched `Dyn`;
2. every child observer advances descriptor and value together;
3. wrong-kind observation returns `Option` or `BlameError`, never memory
   unsafety or an unchecked VM cast;
4. `$ref` identity is local to one descriptor graph;
5. no public API converts an arbitrary `TypeDesc` into `TypeOf(A)`;
6. `interpreter` accepts only a complete explicit expected scheme;
7. the compiler mechanically derives and checks the erased operand signature;
8. cancelled, stale, or failed analysis publishes no partial adapter; and
9. native optimization cannot change public logical results.

## Shared acceptance criteria

1. recursive metadata is finite and inspectable through explicit references;
2. Function metadata is observable as a leaf but not structurally traversable;
3. a trusted `TypeOf(A), A` pair can be packed as opaque `Dyn`;
4. primitive checked projections distinguish mismatch from interpreter error;
5. Struct, Dict, Array, Tuple, Atom, Tagged, and Enum values can be observed
   without separating child metadata from child values;
6. an annotated `interpreter(my_eq_i)` rejects an incompatible erased operand;
7. accepted lifting emits ordinary closure behavior, not runtime lookup;
8. a Forma equality interpreter handles finite recursive data;
9. supported native and Forma equality cases agree;
10. explicit type application remains available while the known witness
    inference gap is closed or isolated; and
11. full workspace tests and strict static checks pass.

## Stopping rules

Work returns to discussion if a child RFC requires:

1. a general subtyping relation or implicit `Unknown` conversion;
2. user-authored unchecked casts or forgeable descriptor/value pairs;
3. a runtime type descriptor becoming a static type without a witness;
4. higher-rank or dependent typing to express the adapter;
5. cyclic runtime graphs before an operation-specific cycle contract exists;
6. exposing VM up-links or heap identity as public data;
7. implicit implementation selection or coherence rules; or
8. operation-specific machinery inside the general `interpreter` keyword.

These indicate that the phase boundary is wrong rather than that the compiler
needs a broader escape hatch.

## Delivery discipline

Each child RFC is proposed in its own commit. Its implementation, tests, and
implementation-result amendment follow in a separate commit. Child work is
validated before the next proposal is accepted, so later RFCs may narrow their
surface based on implementation evidence.

RFC 0089 becomes Implemented only after RFC 0094 records the equality
conformance result and every shared acceptance criterion is met. Any deferred
case is recorded here rather than silently treated as implemented.

## Design basis

The detailed reasoning, alternatives, and current gap audit remain in:

- `discuss/type-directed-capability-factories.md`; and
- `discuss/user-space-type-metadata-interpreters.md`.

Those documents motivated this phase. This RFC and its accepted children become
authoritative for implemented behavior.
