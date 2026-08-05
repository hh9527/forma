# RFC 0082: Context-complete generic inference

- Status: Implemented
- Depends on: RFC 0052, RFC 0070 through RFC 0077, RFC 0079 through RFC 0081
- Tracking issue: https://github.com/hh9527/forma/issues/1

## Summary

A generic call collects explicit type arguments, its surrounding expected
result, ordinary arguments, callbacks, and structural literals into one
monomorphic constraint solution. The checker establishes result context before
completing argument expressions, so a callback sees every constraint already
available at the call boundary:

```forma
native recover: for(A, B) Fn(Fn(A) -> B) -> A;

let value: String = recover(fn(item) { item });
```

Here the expected result fixes `A = String` before the callback is checked. The
callback therefore has the expected shape `Fn(String) -> B`; its body then
fixes `B = String`. The program does not depend on whether the implementation
happens to walk the result or callback first.

This RFC completes propagation within one generic call. It does not add global
constraint solving, overload selection, subtyping, or higher-rank inference.

## Motivation

Forma already propagates expected types into calls, closures, collections, and
records. Generic instantiation also gives every occurrence of one scheme
parameter the same inference identity. However, the current call traversal
checks arguments before applying the surrounding expected result.

That order is observable when an argument needs the result constraint in order
to be checked precisely. Higher-order functions are the clearest example, but
the same issue appears in nested empty collections and structural callback
results. Treating these as isolated local inferences loses information that is
already present at the call boundary.

The desired model is smaller than a general solver: instantiate one rank-1
scheme, add all immediately available constraints to that instance, check its
expressions against the resulting shapes, and complete it atomically.

## Call constraint lifecycle

For a call whose callee resolves to:

```text
Fn(P0, ..., Pn) -> R
```

the checker performs these conceptual phases:

1. instantiate the callee once, including explicit and `_` type arguments;
2. verify value arity;
3. if an expected result `E` exists, check `R <= E` immediately;
4. check each argument `Ai` against the now-resolved `Pi`;
5. resolve the result and verify that no call-owned obligation remains.

These are semantic phases, not a required multi-pass architecture. A mutable
substitution table remains sufficient. The invariant is that result context is
installed before an argument can be completed without seeing it.

The final ordinary directional check of the call expression remains in place.
Repeating a compatible result check is harmless; implementations may avoid the
duplicate once both paths are proven equivalent.

## Callback propagation

When a parameter resolves to a function descriptor, its parameter and result
types are passed into closure checking even when they still contain inference
variables. Constraints from the closure body update those same identities.

```forma
native transform: for(A, B) Fn(Array(A), Fn(A) -> B) -> Array(B);

let output: Array(String) = transform([], fn(value) {
    `item: \{value}`
});
```

The surrounding result fixes `B = String`. The empty input provides no
evidence for `A`; uses of `value` may still solve it. If neither the callback
nor any other source solves `A`, the call is underconstrained.

A callback is never generalized while being checked as an argument. It is one
monomorphic value in one generic instance. Passing a previously generalized
binding instantiates that binding once before checking it against the callback
parameter.

## Structural propagation

Array, Tuple, Dict, Struct, Tagged, Enum, Function, and `TypeOf` shells retain
inference identities while expected structure flows inward. Empty containers
preserve an available expected item type but provide no positive evidence on
their own.

```forma
native flatten: for(A) Fn(Array(Array(A))) -> Array(A);

let names: Array(String) = flatten([[], []]);
```

The result context fixes `A = String`; both nested empty arrays are then
checked as `Array(String)`. The checker must not replace an unresolved inner
descriptor with `Any` merely because a literal has no members.

Record fields are matched by canonical field name, tuple members by position,
and function parameters by position. Forma does not infer row variables,
struct width subtyping, function variance, or collection covariance here.

## Evidence and boundaries

The evidence rules from RFC 0080 remain authoritative:

- explicit type arguments are rigid constraints;
- `_` and omitted generic arguments create ordinary inference variables;
- reachable concrete values and intrinsic constraints provide evidence;
- `Never` checks against an expectation but does not solve it;
- empty structural values preserve context but do not invent evidence;
- explicit `Any` follows the existing dynamic-boundary erasure rules; and
- conflicts fail rather than selecting one source by traversal order.

An expected result participates only when the language already has a checking
context, such as a binding annotation, closure result annotation, callback
result, structural member expectation, or enclosing call parameter. Forma does
not use later unrelated statements or runtime values as backwards evidence.

## Determinism

Equivalent constraints on one call must produce the same descriptor or the
same primary conflict regardless of:

- whether evidence originates in the result, an earlier argument, or a later
  argument;
- the placement of empty or `Never` arguments among concrete arguments;
- source ordering of canonical Struct fields; or
- cache state, cancellation timing, or query scheduling.

The implementation may retain source-order expression traversal for stable
effects, diagnostics, and cancellation. Tool-stage and program-stage
expressions are not speculatively executed or reordered. Order independence
applies to the static constraint result, not evaluation order.

When multiple concrete sources conflict, diagnostics remain deterministic by
the existing source traversal. RFC 0084 will improve labels and terminology;
this RFC requires only that adding compatible result context cannot change a
successful program into an order-dependent failure.

## Completion

After arguments and result context have been checked, the instantiated result
is resolved. An inference variable may continue outward only when it is owned
by an enclosing inference boundary that supplied the expected descriptor.
Otherwise an underconstrained generic result fails at the call or the more
specific RFC 0081 placeholder location.

No unresolved variable is defaulted to `Any`. No callback-local variable is
generalized by the call. Cancellation discards the entire analysis and
publishes no partial substitutions or expression facts.

## Tooling and runtime

Completed calls and argument expressions publish their resolved monomorphic
facts. A callback parameter hover therefore reflects result-derived context,
not an early `Any` fallback. Definition hover continues to show the original
scheme, while each reference and call shows its instance.

The change is static-only. It adds no runtime argument, specialization,
closure, instruction, or metadata evaluation. Source expression evaluation
order and VM calling convention are unchanged.

## Goals

1. apply surrounding result constraints before completing generic arguments;
2. propagate the shared instance through callbacks and structural literals;
3. compose explicit, placeholder, value, intrinsic, and result evidence;
4. preserve `Never`, empty-value, and explicit `Any` distinctions;
5. keep callback checking monomorphic and rank-1;
6. make compatible solutions independent of evidence position;
7. publish only resolved monomorphic expression facts;
8. preserve deterministic diagnostics and cancellation;
9. retain source evaluation order; and
10. leave runtime representation and bytecode unchanged.

## Non-goals

- whole-program or interprocedural constraint solving;
- higher-rank arguments or results;
- polymorphic callback parameters;
- bidirectional checking across arbitrary statement boundaries;
- traits, interfaces, associated types, or constrained generics;
- overload resolution, coercion, or numeric defaulting;
- structural width subtyping, rows, or variance;
- changing evaluation order.

## Implementation plan

1. apply a call's available expected result before argument inference;
2. preserve unresolved function and structural shells in argument expectations;
3. audit premature generic-result and closure completion checks;
4. retain the RFC 0081 dedicated placeholder completion path;
5. add result-to-callback, nested structural, argument-position, `Never`,
   explicit `Any`, conflict, semantic-fact, and cancellation regressions;
6. verify complete and partial explicit type application use the same path;
7. run full workspace tests and strict static checks.

## Acceptance criteria

1. result-only evidence constrains a callback parameter before its body checks;
2. callback body evidence constrains the generic result;
3. nested empty structures retain result-derived item expectations;
4. concrete evidence before or after empty and `Never` values gives one result;
5. incompatible result, argument, and callback evidence fails consistently;
6. explicit and `_` type arguments compose with result context;
7. explicit `Any` remains distinct from an unresolved obligation;
8. callbacks publish resolved parameter, body, and result facts;
9. no underconstrained instance reaches a module interface;
10. cancellation publishes no partial solution;
11. bytecode and runtime behavior are unchanged; and
12. workspace tests and strict static checks pass.

## Rejected alternatives

### Complete arguments before checking the result

This retains the existing traversal accident. A callback can lose information
that is already available from its enclosing annotation or caller.

### Re-run failed arguments after result checking

Retry requires speculative rollback for substitutions, diagnostics, numeric
constraints, and cancellation. Installing known result constraints first is
smaller and deterministic.

### Default unresolved callback positions to `Any`

That makes ordinary inference silently cross a dynamic boundary. `Any` remains
an explicit contract choice, not an ambiguity fallback.

## Implementation result

Implemented in the rank-1 call checker. A surrounding expected result is now
checked immediately after function arity and before argument inference. An
inner generic call whose result has been connected to an enclosing inference
descriptor is allowed to remain pending at that boundary; the enclosing call,
closure, binding, or program completion owns the final underconstrained check.

This removes the concrete source-order discrepancy between
`choose(empty(), 1)` and `choose(1, empty())`. Both connect the inner result to
the outer parameter identity and complete as `Int`. Calls with only empty
evidence still fail, while annotations, callbacks, partial explicit type
application, `Never`, and conflicts use the same substitution state.

Regression tests cover both argument orders, empty and `Never` evidence,
result-to-callback propagation through a structural Array result, `_`
composition, conflicting annotations, and a genuinely underconstrained outer
call. Existing expression-fact publication, cancellation, and bytecode paths
remain unchanged; the implementation neither reorders evaluation nor retries
an expression.
