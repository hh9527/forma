# RFC 0087: Callable-obligation convergence

- Status: Implemented
- Depends on: RFC 0086
- Tracking issue: https://github.com/hh9527/forma/issues/3

## Summary

Function shapes inferred under RFC 0086 participate in one monomorphic,
monotonic constraint solution across repeated calls, aliases, and nested call
results. Compatible evidence converges; incompatible arity or descriptors
fail without overload selection, rollback, or union-call distribution.

```forma
let compose = fn(outer, inner, value) {
    outer(inner(value))
};
```

The eligible binding may generalize to:

```text
for(A, B, C) Fn(Fn(A) -> B, Fn(C) -> A, C) -> B
```

This RFC primarily validates that RFC 0086 composes with existing identity,
unification, and generalization rules. It adds implementation machinery only
if a constructed case demonstrates that those rules are insufficient.

## One identity, one shape

Every monomorphic binding and alias refers to one descriptor identity.
The first call through an unbound callee establishes its exact Function shell.
Subsequent calls check against that shell:

```forma
let use = fn(callback) {
    (callback(1), callback(2)) # compatible
};
```

Calling the same identity with another arity is an arity error. Calling it with
an incompatible argument or result is a unification conflict. Neither case
creates another candidate Function shape.

An ordinary alias does not copy or generalize the obligation:

```forma
let use = fn(callback) {
    let alias = callback;
    (alias(1), callback("text")) # conflict
};
```

## Nested calls

An inferred call result may itself be constrained as a Function by an enclosing
call:

```forma
let invoke_factory = fn(factory) { factory()() };
```

The equations are finite closed descriptors:

```text
factory : Fn() -> ?Produced
?Produced = Fn() -> ?Result
```

Likewise, an inner call result may provide the argument evidence for an outer
callee. Constraint propagation uses the same descriptor graph and existing
occurs-check; it does not require higher-rank types.

## Source order and diagnostics

Equivalent compatible calls must produce the same completed descriptor
regardless of their textual order. For incompatible calls, source order may
select the later conflicting expression as the primary location, but both
orders must reject with the same category.

The solver does not backtrack from the first Function shell, try another
overload, promote numeric types, or construct a Union to accept both uses.

## Recursion boundary

Recursive `def` components remain monomorphic under RFC 0078 and RFC 0083.
Callable-shape evidence may help complete a recursive skeleton, but it cannot
generalize a recursive component or infer polymorphic recursion.

Evidence-free recursion remains unresolved. Indirect recursion retains the
explicit-contract requirement established by the dependency audit.

## Goals

1. validate repeated compatible calls over one inferred Function shell;
2. reject repeated arity, parameter, and result conflicts;
3. preserve one identity through ordinary aliases;
4. converge through nested call arguments and call results;
5. preserve source-order-independent acceptance and completed types; and
6. retain monomorphic recursion and rank-1 generalization.

## Non-goals

- overload sets or multiple candidate Function shapes;
- intersection types for multiply callable values;
- union-call distribution or implicit coercion;
- partial application, optional parameters, or variadics;
- first-class schemes or higher-rank callback parameters;
- open structural constraints, traits, or capability search; or
- a new worklist, rollback, or backtracking solver unless monotonic descriptor
  propagation is proven insufficient.

## Acceptance criteria

1. repeated calls with compatible arguments converge;
2. repeated calls with incompatible argument descriptors conflict;
3. repeated calls with different arities report an arity error;
4. aliases share the original monomorphic callable obligation;
5. `outer(inner(value))` infers the expected three-parameter composition scheme;
6. `factory()()` infers a nested Function result;
7. equivalent call order produces equal schemes or equal conflict categories;
8. occurs-check and recursive-component regressions continue to pass;
9. no provisional shape reaches semantic facts or interfaces; and
10. workspace tests and strict static checks pass.

## Implementation plan

1. add constructed repeated-call, alias, arity, descriptor-conflict, compose,
   nested-result, ordering, and recursion probes;
2. confirm whether existing substitutions preserve one descriptor identity;
3. add only the smallest monotonic propagation fix required by a failing
   accepted case;
4. audit inferred schemes and call-instance facts; and
5. record whether the RFC required code beyond RFC 0086.

## Rejected alternatives

### Infer one Function per call occurrence

That would turn a monomorphic value into an implicit overload set and make
aliases accidentally polymorphic.

### Join incompatible callable uses

Union parameters do not describe a function that safely accepts each source
call, and union results would hide contradictory evidence. Existing
unification remains authoritative.

### Add a general constraint worklist now

The current descriptors already form a finite substitution graph. New solver
infrastructure is justified only by a concrete accepted program that cannot be
represented or completed monotonically.

## Implementation result

Implemented through adversarial validation without adding solver machinery.
RFC 0086's single Function-shell binding and the existing substitution graph
already preserve one monomorphic identity across repeated calls and aliases.
Nested argument and result calls add ordinary finite equations and converge
through the existing recursive descriptor resolution and occurs-check.

Tests cover compatible repeated calls in both orders, parameter conflicts in
both orders, conflicts through an alias, exact-arity disagreement, composition,
a call whose result is called again, and a concrete composed invocation. The
inferred composition scheme uses Forma's established structural-occurrence
parameter naming:

```text
for(A, B, C) Fn(Fn(A) -> B, Fn(C) -> A, C) -> B
```

No worklist, rollback, overload candidate, Union construction, runtime change,
or additional production-code path was required.
