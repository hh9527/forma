# RFC 0072: Self-constrained closure inference

- Status: Implemented
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

If the body does not provide enough evidence, this staged RFC retains the
existing conservative fallback:

```forma
let identity = fn(value) { value }; # Fn(Any) -> Any until RFC 0073
```

Inferring that identity from a later call is intentionally deferred to RFC
0073. This RFC keeps every fresh closure variable local to one closure
expression, never generalizes it, and erases only the variables that remain
unresolved when the closure completes.

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
6. preserve the existing `Any` fallback only for closure-local variables that
   remain unresolved, pending RFC 0073;
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
They are not source-level type parameters and cannot escape unresolved; each is
either solved concretely or receives the explicit transitional `Any` fallback.

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

## Transitional unresolved closures

The closure boundary cannot yet retain a remaining variable:

```forma
fn(value) { value }
```

has the unresolved shape:

```text
Fn(?A) -> ?A
```

This RFC does not generalize that shape or confuse it with `Never`. Until RFC
0073 introduces a block-lived obligation, it retains the previous
`Fn(Any) -> Any` result. This is an explicit staging rule rather than evidence
that the source requested a dynamic boundary.

Likewise:

```forma
fn(value) { [value] }
```

temporarily becomes `Fn(Any) -> Array(Any)` even though its relationship
`Fn(?A) -> Array(?A)` is known. RFC 0073 retains that relationship through a
local binding and solves it from later uses.

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

Conflicting body evidence continues to use the smallest existing expression
diagnostic, for example `cannot unify String with Int`. Merely unresolved
closure-local evidence retains the transitional `Any` fallback and does not
introduce a new diagnostic in this RFC.

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
5. default only closure-local variables that remain unresolved at the closure
   boundary to `Any`;
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
7. `fn(value) { value }` retains `Fn(Any) -> Any` in this RFC;
8. `fn(value) { [value] }` retains `Fn(Any) -> Array(Any)` in this RFC;
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

## Implementation result

Unannotated closures now allocate fresh parameter variables and solve them from
operators, known calls, generic relationships, nested closures, and structural
body results. Closures whose bodies establish concrete evidence publish exact
monomorphic types such as `Fn(Int) -> Int` and `Fn(String) -> Int`.

Generic calls inside an unannotated closure may retain a result variable when
it remains linked to an unresolved closure argument. That allowance is scoped
to closure inference; an ordinary underconstrained call such as an all-empty
generic `concat` continues to fail immediately.

Branch joins no longer bind an external closure variable merely because one
branch returns that variable and another returns a concrete type. After the
transitional fallback, a Union containing `Any` normalizes to `Any`. This
preserves existing dynamic decorator and callback behavior while avoiding
order-dependent specialization.

The initially proposed immediate error for unresolved closure-local variables
was not implemented. A full workspace audit found 27 existing cases where a
closure body expresses a valid monomorphic relationship that is concretized by
a later call. Rejecting those programs between RFC 0072 and RFC 0073 would
create a broad temporary regression. The implementation therefore preserves
the old `Any` fallback for precisely those unresolved variables; RFC 0073 owns
removing it through delayed block-local solving.

Tests cover arithmetic, known String calls, multiple related parameters,
nested closures, expected contracts, `Never`, conservative identity/singleton
fallbacks, generic result relationships, and the existing runtime suite. The
final workspace run passed 240 Forma tests with one manual parser benchmark
ignored, 12 CLI tests, and 19 LSP tests. Strict Clippy, formatting, and
whitespace validation pass.
