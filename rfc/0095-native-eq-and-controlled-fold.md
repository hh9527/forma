# RFC 0095: Native equality Function and controlled Array fold

- Status: Proposed
- Depends on: RFC 0053, RFC 0089 through RFC 0094

## Summary

Forma keeps `==` as the authoritative equality operation, exposes the same
native behavior as a first-class Function, and adds a general Array fold with
explicit normal early exit:

```forma
import arrays from "@bim/std/array";
import eq from "@bim/std/eq";

eq.equal(1, 1)

arrays.fold_control(values, initial, fn(state, value) {
    if done(state, value) {
        'Break(result(state, value))
    } else {
        'Continue(next(state, value))
    }
})
```

The user-space equality interpreter from RFC 0094 remains a conformance
example for TypeDesc, Dyn, observers, and `interpreter`. It is no longer the
standard equality implementation.

## Native equality Function

`@bim/std/eq` exports:

```forma
native equal: for(A) Fn(A, A) -> Bool;
{ equal: equal }
```

`eq.equal(left, right)` and `left == right` call the same VM equality primitive.
They therefore share structural behavior, Function identity behavior, internal
cycle handling, quota behavior, and future semantic changes. The module does
not use TypeOf, Dyn, reflection, or a second recursive implementation.

The generic contract requires both operands to have one inferred `A`. This is
the function-valued form for higher-order APIs; it is not overload resolution
or an implicit equality capability.

## Fold control

The standard Array module exports the generic control type constructor and
operation:

```forma
FoldControl(S, R) = enum {
    Continue(S),
    Break(R),
}

native fold_control:
    for(A, S, R)
    Fn(Array(A), S, Fn(S, A) -> FoldControl(S, R)) -> FoldControl(S, R);
```

The callback runs in source order. `'Continue(next)` replaces the accumulator
and advances. `'Break(result)` returns immediately without invoking the callback
for remaining elements. Empty input returns `'Continue(initial)`.

`R` is an ordinary result domain, not necessarily an error. A caller may choose
`R = Result(Bool, BlameError)` to distinguish a successful early decision from
observer failure without encoding either as Array control semantics.

Malformed callback values are runtime contract errors at the native boundary.
Fuel, allocation, call depth, stack limits, callback traces, cancellation, and
tail behavior follow the existing Array continuation machinery.

## Reference equality placement

RFC 0094 proved that public TypeDesc and Dyn operations are sufficient to write
a recursive structural equality interpreter. That proof remains valuable, but
it does not require production equality to pay reflection and packaging costs.

The implementation moves the Forma interpreter to an example/conformance
fixture. It uses public imports, including `array.fold_control`, and compares its
supported results against both `eq.equal` and `==` on the same inputs.

The move removes:

- the recursive `@bim/std/equality` core module;
- its duplicated native declarations and TypeDescKind definition; and
- its equality-specific fallback at the legacy core `Value` projection.

Unsupported reflected domains continue to return `BlameError` in the reference
interpreter. Native equality remains total over the domains already supported by
`==`, including opaque Function identity.

## Goals

1. provide a first-class Function with exactly the semantics of `==`;
2. support normal, typed, allocation-aware early termination of Array folds;
3. retain user-space equality as executable interpreter conformance evidence;
4. remove the equality-specific core bootstrap exception; and
5. keep one authoritative production equality implementation.

## Non-goals

- implicit Eq constraints, traits, dictionaries, or capability search;
- redefining, overloading, or customizing `==`;
- treating `Break` as an error or exception;
- generalized language-level algebraic effects;
- parallel, unordered, or lazy folds;
- cyclic user-visible values; or
- solving the general legacy core `Value` projection in this RFC.

## Acceptance criteria

1. `eq.equal(a, b)` equals `a == b` for scalar and nested structural values;
2. Function equality uses the same identity rule through both forms;
3. `eq.equal` is accepted wherever `Fn(A, A) -> Bool` is required;
4. empty `fold_control` returns `Continue(initial)` without a callback;
5. all-Continue traversal returns the final accumulator;
6. Break returns its result and does not invoke later callbacks;
7. callback type errors and resource failures retain Array call traces;
8. the reference interpreter uses `fold_control` for inequality and blame exits;
9. conformance cases compare reference equality, `eq.equal`, and `==` directly;
10. `@bim/std/equality` and its legacy export exception are removed; and
11. full Forma, CLI, LSP, formatting, and strict Clippy checks pass.

## Implementation plan

1. add canonical FoldControl metadata and static schemes;
2. extend the Array continuation with controlled termination;
3. add `@bim/std/eq` backed by the VM equality primitive;
4. move and adapt the RFC 0094 interpreter into conformance source;
5. remove the recursive equality core module and legacy projection fallback;
6. add direct semantic, early-exit, quota, trace, and conformance tests; and
7. run the full quality gate and record the implementation result.
