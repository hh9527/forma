# RFC 0086: Unknown-callee Function-shape constraints

- Status: Implemented
- Depends on: RFC 0085
- Tracking issue: https://github.com/hh9527/forma/issues/3

## Summary

When an ordinary call's callee resolves to an unbound monomorphic inference
variable, Forma binds that variable to a fresh, exact-arity Function
descriptor and checks the call through the existing Function path.

```forma
let apply = fn(callback, value) { callback(value) };
apply(fn(value) { value + 1 }, 41)
```

The body call initially establishes:

```text
callback : Fn(?Parameter) -> ?Result
```

Later evidence solves the fresh positions. No scheme is created at the call;
the enclosing eligible binding may generalize only at its existing boundary.

## Semantics

For `callee(arguments...)`, infer the callee as today. If its fully resolved
descriptor is `Inference(C)`, create one fresh parameter variable per source
argument and one fresh result variable:

```text
F = Fn(?P0, ..., ?Pn) -> ?R
C = F
```

Then process `F` with the ordinary Function-call algorithm:

1. apply a surrounding expected result to `?R`;
2. infer each argument under its corresponding `?P` expectation;
3. check argument evidence against `?P`;
4. apply existing `Never`, `Any`, numeric-domain, placeholder, and completion
   rules; and
5. return the resolved `?R` descriptor.

The binding is subject to the existing occurs-check. The inferred Function
has exactly the call's source arity. Later calls through the same monomorphic
identity must use that arity.

## Boundaries

This rule applies only when the resolved callee is an unbound inference
variable. It does not change:

- declared, inferred-scheme, imported, or explicitly applied generic calls;
- calls through an already known Function descriptor;
- Atom payload construction;
- calls through explicit `Any`;
- the current recovery behavior for known non-Function descriptors; or
- runtime call lowering.

Known non-Function diagnostics are audited by RFC 0088. They are deliberately
not changed here so the first implementation has one acceptance delta.

## Generalization

The inferred shell is monomorphic while its owner is checked. An eligible
closure binding may subsequently generalize the complete descriptor:

```text
for(A, B) Fn(Fn(A) -> B, A) -> B
```

Aliases still instantiate once. Callback arguments are still monomorphic
instances. Recursive components retain their existing restrictions.

## Goals

1. replace unknown-callee `Any` fallback with exact Function evidence;
2. reuse ordinary call checking rather than add a second call solver;
3. propagate argument and expected-result evidence in both directions;
4. retain existing completion and generalization ownership; and
5. make no runtime change.

## Non-goals

- repeated-call and nested-call convergence beyond the direct rule;
- changing known non-Function or explicit-`Any` call behavior;
- field, method, container, or pattern shape inference;
- overloaded or variadic calls;
- traits, capabilities, constrained generics, or implementation search;
- higher-rank callback parameters; or
- subtyping, coercion, union-call distribution, or solver backtracking.

## Acceptance criteria

1. `fn(callback, value) { callback(value) }` infers a reusable rank-1 helper;
2. literal arguments solve inferred callback parameters;
3. a surrounding expected result solves the inferred callback result;
4. intrinsic operations on the call result constrain that result;
5. an incompatible callback argument reports an ordinary unification conflict;
6. an unresolved position follows existing binding completion rules;
7. generic, Atom, known Function, and explicit-`Any` calls do not regress;
8. emitted bytecode and VM behavior are unchanged; and
9. workspace tests and strict static checks pass.

## Implementation plan

1. recognize resolved `Inference` in the ordinary call branch;
2. allocate and bind one closed Function descriptor;
3. route it through the existing Function-call logic;
4. add positive, expected-result, intrinsic, conflict, incomplete, generic,
   constructor, and runtime regressions; and
5. record the final inferred schemes and any deliberately deferred behavior.

## Rejected alternatives

### Infer arguments first, then synthesize a Function

That loses expected parameter information needed by closure arguments and
duplicates the existing bidirectional Function-call path.

### Treat every unknown call as `Fn(Array(Any)) -> Any`

It erases source arity and positive evidence, preventing useful inference and
hiding conflicts.

### Generalize the callee at the call site

Call sites consume monomorphic instances. Generalization remains an owned
binding-boundary operation under RFC 0079 and RFC 0083.

## Implementation result

Implemented in the ordinary call checker as one pre-processing step. After the
callee is inferred and resolved, an `Inference` descriptor is bound to an
exact-arity Function whose parameters and result are fresh variables. The
existing Function-call branch then owns expected-result propagation, argument
checking, `Never`, explicit `Any`, numeric obligations, placeholders, and
completion.

No parallel call solver or descriptor kind was added. Generic bindings, Atom
constructors, known Functions, explicit dynamic calls, bytecode lowering, and
VM execution remain on their previous paths.

The implementation confirms the RFC 0084 publication distinction: an inferred
higher-order definition publishes an authoritative scheme such as
`for(A, B) Fn(Fn(A) -> B, A) -> B`, while its ordinary runtime result shape is
still explicitly erased. Tests therefore inspect the definition scheme and
the concrete types of call instances separately.
