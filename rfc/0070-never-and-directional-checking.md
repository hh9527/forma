# RFC 0070: Never and directional checking

- Status: Implemented
- Depends on: RFC 0052

## Summary

Forma adds the uninhabited `Never` type and separates directional expression
checking from equality unification in the authoritative bidirectional checker:

```text
expression => actual
actual <= expected
left == right
```

`Never` is assignable to every expected type because an expression of type
`Never` cannot return a value that violates the expectation. It is not an
inference variable and supplies no evidence for an unresolved generic type.
Branch joins discard `Never` when another branch has a reachable result.

This RFC establishes only the bottom-type and checking foundation. It does not
change empty collection inference, infer unannotated closure parameters, or add
implicit generalization.

## Motivation

RFC 0052 uses one bidirectional checker for ordinary and generic expressions,
but its `unify` operation currently serves two different purposes:

```text
the actual expression must be assignable to an expected type
two types must describe the same generic relationship
```

Those relations coincide for many existing structural types but differ for a
bottom type. Given:

```forma
native stop: Fn() -> Never;
native choose: for(A) Fn(A, A) -> A;
```

this call must infer `Int`:

```forma
choose(stop(), 1)
```

`stop()` is valid where `A` is expected, but it must not solve `A = Never`.
The reachable argument supplies the only evidence and solves `A = Int`.

Similarly:

```forma
if condition { 1 } else { stop() }
```

has type `Int`, not `Int | Never`, while:

```forma
choose(stop(), stop())
```

remains underconstrained. The absence of a returned value is not positive
evidence for a generic result type.

## Goals

1. publish `Never` as ordinary built-in TypeMetadata;
2. represent `Never` distinctly in semantic descriptors and final type graphs;
3. define directional assignability from `Never` to every type;
4. keep assignability to `Never` restricted to `Never` itself;
5. prevent an actual `Never` expression from solving inference variables;
6. make branch joins ignore unreachable `Never` results when a reachable result
   exists;
7. retain `Never` when every branch is `Never`;
8. preserve resolved `Never` expression facts for LSP and CLI observation;
9. keep runtime values, bytecode, and the VM ABI unchanged;
10. retain asynchronous cancellation checkpoints in checking and unification.

## Non-goals

- a `panic`, `throw`, `return`, `break`, `continue`, or process-exit syntax;
- a new built-in runtime function that constructs `Never`;
- interpreting an empty Array, Dict, Tuple, Struct, Enum, or Union as bottom;
- changing unresolved empty literal types to `Never`;
- local binding constraint graphs or inference from later uses;
- unannotated closure parameter inference;
- implicit generalization, let-polymorphism, or a value restriction;
- general subtyping, variance, coercions, or union normalization;
- flow-sensitive reachability, exhaustiveness, or unreachable-code warnings.

Native and externally declared functions can already provide a `Never` return
contract, which is sufficient to exercise and use the type without adding a
new runtime termination primitive in this RFC.

## TypeMetadata

`Never` is a built-in TypeMetadata value:

```forma
Never
```

Its normalized data representation is:

```forma
{ kind: 'Never }
```

It has the metatype witness `TypeOf(Never)`, participates in metadata
round-tripping, can appear inside ordinary type constructors, and is preserved
across module interfaces:

```forma
native stop: Fn() -> Never;
native impossible: for(A) Fn(A) -> Never;
```

There is no runtime Forma value whose instance type is `Never`.

## Three distinct concepts

The checker must keep these representations separate:

```text
Never  no normal value can be produced
?A     a temporary inference obligation awaiting evidence
Any    an explicit dynamic boundary with erased static information
```

None is a fallback spelling for another. In particular:

- an unresolved `?A` is not converted to `Never`;
- `Never` is not converted to `Any` when final semantic facts are recorded;
- an empty collection does not synthesize a collection of `Never` under this
  RFC.

## Directional checking

The authoritative checker introduces an explicit directional operation:

```text
check(actual, expected)
```

For fully resolved types it succeeds exactly when `actual` is assignable to
`expected`. It must not accept a value merely because the reverse relation is
valid.

For types containing inference variables, checking may use structural
unification to solve equality obligations introduced by generic contracts.
There is one bottom-specific rule:

```text
check(Never, T) = success without substitutions
```

This applies when `T` is concrete, contains inference variables, or is itself
an inference variable. The actual `Never` expression contributes no type
evidence.

Equality unification remains a separate relation:

```text
unify(left, right)
```

It may bind an inference variable to an explicitly required `Never`, for
example when checking a value against `Array(Never)`. What it must not do is
treat an unreachable actual expression as evidence that a generic variable is
`Never`.

## Assignability

The descriptor and final graph relations add:

```text
assignable(Never, T) = true
assignable(T, Never) = false, unless T is Never
```

The first rule precedes structural cases. The second follows naturally by
providing no reverse wildcard rule.

`Any` retains its existing gradual behavior in this RFC. Consequently both
`Never <= Any` and the existing `Any <= Never` relation are accepted at an
explicit dynamic boundary. Tightening `Any` is independent work.

Function compatibility continues to use Forma's existing function rules. This
RFC only ensures that a function result of `Never` is accepted where the
corresponding expected result permits any ordinary value, while the reverse
direction is not accepted by directional checking.

## Branch joins

When `if` or `match` has no surrounding expected type, its result join follows:

```text
join(Never, T)     = T
join(T, Never)     = T
join(Never, Never) = Never
```

For more than two arms, all `Never` members are removed before the existing
common-type or Union behavior is applied. If every member is `Never`, the join
is `Never`.

This is a result-type rule, not a reachability analysis. The checker still
visits every branch, records its facts, reports its errors, and honors
cancellation.

## Generic evidence

An actual `Never` argument satisfies any parameter expectation without solving
variables in that parameter:

```forma
native stop: Fn() -> Never;
native choose: for(A) Fn(A, A) -> A;

choose(stop(), 1)       # Int
choose(1, stop())       # Int
choose(stop(), stop())  # error: cannot infer A
```

An available surrounding expected result can still solve the last call:

```forma
let value: String = choose(stop(), stop());
```

Here `A = String` comes from the explicit expected result, not from either
`Never` argument.

## Empty literals

This RFC deliberately does not use `Never` to represent missing element
evidence:

```forma
[]
```

Conceptually, future constraint-preserving inference should treat this as
`Array(?A)` until context solves `?A`, and should report an inference error if
the obligation remains unresolved. Implementing that behavior requires a
separate RFC because the current literal checker still has established `Any`
fallbacks outside generic call obligations.

Using `Array(Never)` as the fallback would require collection covariance or
special coercion rules and would incorrectly turn lack of evidence into an
assertion that the element type is uninhabited.

## Diagnostics and semantic facts

`Never` displays exactly as `Never` in type output, hover, module interfaces,
and incompatibility diagnostics. Complete expressions with a `Never` contract
record `Never`, not `Any` or an empty Union.

An underconstrained generic call involving only `Never` evidence retains the
existing deterministic `cannot infer generic result type` diagnostic.

## Implementation plan

1. add `Never` to `TypeDescriptor`, `TypeNode`, display, interning, metadata
   encoding, and both metadata decoders;
2. publish `Never` through the runtime and static core preludes;
3. add directional `check(actual, expected)` to the inference-variable checker;
4. use directional checking for expression expectations and call arguments;
5. add bottom rules to descriptor and final graph assignability;
6. make branch result joining remove `Never` unless every result is `Never`;
7. preserve `Never` through scheme instantiation, resolution, occurs checks,
   variable erasure, witness erasure, validation, and semantic recording;
8. add metadata, assignability, branch, generic-evidence, interface, and
   observation tests;
9. run the full Forma, CLI, and LSP suites, strict Clippy, formatting, and
   whitespace validation.

## Acceptance criteria

1. `Never` evaluates as `{ kind: 'Never }` TypeMetadata and has static type
   `TypeOf(Never)`;
2. metadata conversion round-trips `Never` without using `Any`;
3. final type graphs report `Never` assignable to `Int`, String, structured
   types, and function expectations;
4. ordinary concrete values are not assignable to `Never`;
5. `if` and `match` joins absorb `Never` into a reachable result type;
6. all-`Never` branches retain `Never`;
7. `choose(stop(), 1)` and `choose(1, stop())` infer `Int`;
8. `choose(stop(), stop())` remains underconstrained without an expected result;
9. an expected result can solve a generic call whose arguments are all
   `Never`;
10. final expression facts and imported/exported contracts preserve `Never`;
11. empty literal inference and unannotated closure behavior do not change;
12. no runtime representation or VM instruction is added for an inhabitant of
    `Never`;
13. all workspace tests and strict static checks pass.

## Deferred work

- constraint-preserving inference through nested collection literals;
- delayed monomorphic inference for unannotated local bindings;
- explicit source constructs whose control flow has type `Never`;
- unreachable-code diagnostics and exhaustive pattern analysis;
- effect typing and typed accumulation channels.

## Rejected alternatives

### Infer empty literals as collections of Never

An empty literal lacks element evidence; it does not prove its element type is
uninhabited. Making `Array(Never)` useful as every empty Array would also need
variance or coercion behavior that Forma does not otherwise have.

### Treat Never as an unresolved inference variable

This would allow unreachable expressions to determine generic parameters and
would make `choose(stop(), 1)` conflict between `Never` and `Int`. Bottom is a
known type with no values, not an unknown type.

### Keep symmetric unification as expression checking

Symmetric compatibility can accept the reverse of the intended assignment and
cannot express that an actual `Never` supplies no inference evidence. A small
directional checking operation is required even without general subtyping.

### Add a termination primitive in the same RFC

Native contracts already provide a producer for static tests and embedding
interfaces. Runtime failure semantics, diagnostics, and whether termination is
an effect deserve independent design.

## Implementation result

`Never` is now a normalized built-in TypeMetadata value and a distinct node in
the module and workspace type graphs. Both metadata decoders, persistent graph
publication, display, validation, semantic facts, and module interfaces retain
it without erasing it to `Any`.

The authoritative inference checker now uses a directional checking operation
for expression expectations and call arguments. An actual `Never` succeeds
without adding substitutions, while resolved non-bottom types are accepted
only in the actual-to-expected direction. Equality unification remains the
solver for structural generic obligations.

Branch result normalization removes `Never` when another reachable result type
exists and retains it when all results are `Never`. Tests verify asymmetric
assignment, `TypeOf(Never)`, metadata round-tripping, mixed and all-bottom
branches, generic calls with bottom arguments, underconstrained all-bottom
evidence, and expected-result solving.

Directional checking exposed a pre-existing bootstrap dependency in type
declarations: a provisional `Type` RHS had been symmetrically unified with the
declaration's already evaluated precise `TypeOf(T)` witness. Type declaration
expressions are now checked against `Type`, matching RFC 0051, while their
published binding still retains `TypeOf(T)`.

The final workspace run passed 235 Forma tests with one manual parser benchmark
ignored, 12 CLI tests, and 19 LSP tests. Strict Clippy, formatting, and
whitespace validation pass.
