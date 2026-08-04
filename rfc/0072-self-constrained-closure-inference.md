# RFC 0072: Self-constrained closure inference

- Status: Proposed
- Depends on: RFC 0052, RFC 0070, RFC 0071

## Summary

An unannotated Forma closure without an expected function type assigns a fresh
monomorphic inference variable to each parameter and solves those variables
from constraints inside its own body:

```forma
let increment = fn(value) { value + 1 };
```

infers:

```text
increment: Fn(Int) -> Int
```

If the body does not provide enough evidence, the closure reports an explicit
underconstrained-inference diagnostic:

```forma
let identity = fn(value) { value }; # cannot infer value
```

Inferring that identity from a later call is intentionally deferred to RFC
0073. This RFC keeps every fresh closure variable local to one closure
expression and never generalizes it.

## Motivation

RFC 0052 propagates an expected `Fn` contract into a closure, so this already
works:

```forma
def increment: Fn(Int) -> Int = fn(value) { value + 1 };
```

Higher-order generic calls also provide callback expectations. But without an
expected function type, every parameter currently becomes `Any`:

```forma
let increment = fn(value) { value + 1 };
```

The body contains exact evidence: arithmetic with `1` requires `value` to be
`Int`. Converting the parameter to `Any` loses that relationship and publishes
a weaker function type than the checker can justify.

## Goals

1. create one fresh inference variable per unannotated closure parameter when
   no expected `Fn` type is available;
2. share each variable through every reference to that parameter in the body;
3. solve variables from calls, operators, collection structure, branches, and
   the closure result;
4. preserve relationships between multiple parameters and the result;
5. resolve the complete closure type before it leaves the closure expression;
6. report a focused diagnostic when any closure-local variable remains
   unresolved;
7. keep every inferred closure monomorphic;
8. preserve existing expected-type closure checking unchanged;
9. publish resolved expression facts for parameters, body expressions, and the
   closure itself;
10. retain cancellation checkpoints throughout body traversal and solving.

## Non-goals

- using calls after a closure binding to infer its parameters;
- block-scoped or module-scoped unresolved inference variables;
- implicit generalization or inferred `for(A)` binders;
- polymorphic local closures or let-polymorphism;
- recursive-function inference;
- field-shape inference from an unknown receiver;
- overload resolution, numeric defaulting, coercion, or subtyping;
- parameter annotation syntax.

## Fresh closure variables

When an expected function type is available, RFC 0052 remains authoritative:

```text
fn(x) { body } <= Fn(P) -> R

x: P
body <= R
```

Without that expectation, the checker creates fresh variables:

```text
fn(x, y) { body }

x: ?X
y: ?Y
body => ?R or a concrete result
```

These variables are ordinary inference obligations owned by the current
checker invocation, but their resolution boundary is the closure expression.
They are not source-level type parameters and cannot escape as `Any`.

## Evidence from the body

Ordinary directional checking and unification solve closure variables:

```forma
fn(value) { value + 1 }
```

produces:

```text
value       => ?A
1           => Int
?A == Int
closure     => Fn(Int) -> Int
```

A known callee parameter also supplies evidence:

```forma
fn(value) { strings.length(value) }
```

where `strings.length: Fn(String) -> Int` yields `Fn(String) -> Int`.

Multiple parameters may be related and then solved by reachable evidence:

```forma
fn(left, right) { left + right + 1 }
```

Both parameters become `Int`.

RFC 0070 still applies: an actual `Never` expression satisfies an expectation
but supplies no parameter evidence.

## Underconstrained closures

The closure boundary rejects any remaining variable:

```forma
fn(value) { value }
```

has the unresolved shape:

```text
Fn(?A) -> ?A
```

This RFC does not turn that into `Fn(Any) -> Any`, `for(A) Fn(A) -> A`, or
`Fn(Never) -> Never`. It reports a diagnostic identifying the unresolved
parameter and closure.

Likewise:

```forma
fn(value) { [value] }
```

remains underconstrained even though its relationship
`Fn(?A) -> Array(?A)` is known. RFC 0073 may retain that relationship through a
local binding and solve it from later uses.

## Monomorphism

Successful inference produces one concrete function type:

```forma
let increment = fn(value) { value + 1 };
```

The resulting binding is `Fn(Int) -> Int`. It does not instantiate freshly at
each use and does not add a `TypeScheme`.

Explicit generic contracts remain the only way to define source-level
polymorphism:

```forma
def identity: for(A) Fn(A) -> A = fn(value) { value };
```

## Nested closures

Each closure owns a distinct set of fresh variables. An inner closure may use
an outer parameter, and constraints from that use may solve the outer variable
while the inner closure is checked. An unresolved inner closure fails at its own
boundary before its variables can escape.

Captures do not generalize either closure.

## Diagnostics

An unresolved closure reports:

```text
cannot infer type of closure parameter `value`
```

The primary location is the parameter. The closure expression may be included
as context. If several parameters remain unresolved, the checker reports them
in source order or emits one deterministic combined diagnostic.

Conflicting body evidence continues to use the smallest existing expression
diagnostic, for example `cannot unify String with Int`.

## Semantic facts

No `InferenceVariableId` may reach the final TypeGraph or WorkspaceTypeGraph.
After closure checking completes, recorded facts are resolved through the final
substitution map. A successfully inferred parameter reference therefore hovers
as `Int` or `String`, not `?0` or `Any`.

Recovery for incomplete source remains conservative and may publish explicit
unknown states under RFC 0042. It must not publish an unresolved inference
variable as a completed fact.

## Implementation plan

1. add a checked fresh-variable allocator to `GenericInference`;
2. use fresh variables instead of `Any` for closure parameters only when no
   expected function type is available;
3. infer the body with those variables in the closure environment;
4. resolve parameter and result descriptors after body inference;
5. reject closure-local variables that remain unresolved at the closure
   boundary;
6. retain expected-contract closure behavior and rigid generic definition
   checking;
7. add arithmetic, known-call, multiple-parameter, branch, nested closure,
   conflict, `Never`, and underconstrained tests;
8. verify semantic facts, module checking, and LSP hover;
9. run full workspace tests and strict static checks.

## Acceptance criteria

1. `fn(value) { value + 1 }` infers `Fn(Int) -> Int`;
2. a known String function infers a String closure parameter;
3. body constraints can solve multiple parameters;
4. the inferred result preserves structural relationships after resolution;
5. expected function contracts behave exactly as before;
6. generic higher-order callbacks behave exactly as before;
7. `fn(value) { value }` reports an underconstrained parameter;
8. `fn(value) { [value] }` remains underconstrained in this RFC;
9. conflicting body evidence reports a deterministic incompatibility;
10. `Never` does not solve a closure parameter;
11. inferred closures remain monomorphic and create no implicit scheme;
12. final semantic facts contain no inference variables;
13. cancellation remains observable during closure inference;
14. workspace tests and strict static checks pass.

## Deferred work

- retaining unresolved monomorphic closure relationships through a local block;
- inference from later use sites;
- recursive local inference;
- implicit generalization and a value restriction;
- parameter annotations and explicit type application.

## Rejected alternatives

### Keep unannotated parameters as Any

This discards exact evidence already present in the closure body and makes a
known monomorphic function unnecessarily dynamic.

### Generalize every unresolved closure variable

That silently introduces let-polymorphism and requires a value restriction,
scheme instantiation policy, and recursive-binding rules. Forma keeps
generalization explicit through `for(...)`.

### Infer from later uses in the same RFC

That requires block-lived obligations, delayed diagnostics, environment
rewriting, and final fact publication. Keeping 0072 closure-local makes its
ownership and completion boundary independently testable before RFC 0073.

