# RFC 0074: Intrinsic expression type constraints

- Status: Implemented
- Depends on: RFC 0052, RFC 0070, RFC 0072, RFC 0073

## Summary

Operators and control forms contribute their own static constraints instead of
merely forwarding whatever type happened to be inferred first.

```forma
let select = fn(condition, value) {
    if condition { value } else { value }
};
```

infers `condition` as `Bool`. Numeric operators accept only `Int` or `Float`,
require both operands and the result to use the same numeric type, and preserve
that relationship until ordinary inference evidence selects one.

```forma
let increment = fn(value) { value + 1 }; # Fn(Int) -> Int
let scale = fn(value) { value * 1.5 };   # Fn(Float) -> Float
```

Forma does not default an otherwise ambiguous numeric variable:

```forma
let negate = fn(value) { -value }; # error: Int or Float remains unresolved
```

## Motivation

The current checker mostly types binary operators by inferring the left operand
and passing it as the right operand's expectation. Unary negation simply
returns its operand type, and `if` conditions receive no static expectation.
Consequently unsupported resolved operands may reach the VM, while a closure
condition remains `Any` or underconstrained even though the runtime requires a
Boolean value.

These requirements belong to the expression forms themselves. Encoding them
as first-class constraints makes bidirectional checking independent of operand
visit order and aligns static facts with existing VM behavior.

## Goals

1. check every `if` condition against normalized `Bool`;
2. constrain unary negation to one numeric type shared by operand and result;
3. constrain arithmetic operands and result to one shared numeric type;
4. constrain less-than operands to one numeric type and return `Bool`;
5. retain equality as a total heterogeneous comparison returning `Bool`;
6. reject known non-numeric operands during analysis;
7. preserve an unresolved numeric domain until evidence selects `Int` or
   `Float`;
8. reject an unresolved numeric domain at its ordinary completion boundary;
9. let surrounding expected types select the numeric member directionally;
10. keep constraint solving deterministic and cancellation-aware.

## Non-goals

- numeric promotion or mixed `Int`/`Float` arithmetic;
- choosing a default numeric type;
- user-defined operator overloading;
- traits, interfaces, type classes, or coercions;
- changing equality semantics;
- Boolean operators or new syntax;
- changing branch result joins, which belong to RFC 0075.

## Boolean conditions

`if condition` checks `condition` against the normalized built-in `Bool` Enum.
This expected type reaches variables, calls, blocks, and closures through the
ordinary checker.

A resolved non-Boolean condition is a compile-time error at the condition
expression. Explicit `Any` remains a dynamic boundary and retains the VM's
runtime check.

`if` continues to infer or check branch results independently of this RFC.

## Numeric domains

The solver may attach a finite domain to an inference variable:

```text
Numeric = { Int, Float }
```

This is a constraint on an inference variable, not a source-level Union and not
a publishable type. Unifying the variable with `Int` or `Float` satisfies the
domain. Unifying it with any other resolved type fails. Unifying two variables
merges their domains.

If a numeric-domain variable remains unresolved when its binding or generic
call completes, analysis fails rather than publishing `Int | Float` or `Any`.
The diagnostic may display the pending domain as `numeric (Int or Float)`.

## Operator contracts

The intrinsic contracts are:

```text
-x             x: N, result: N, N in Numeric
x + y          x: N, y: N, result: N, N in Numeric
x - y          x: N, y: N, result: N, N in Numeric
x * y          x: N, y: N, result: N, N in Numeric
x / y          x: N, y: N, result: N, N in Numeric
x < y          x: N, y: N, result: Bool, N in Numeric
x == y         x: Any, y: Any, result: Bool
```

An available expected numeric result is applied before operands are completed.
Literal evidence is otherwise sufficient to select the numeric type. Mixed
numeric operands fail because Forma has no implicit promotion.

Equality intentionally adds no relationship between operand types. Function,
metadata, Tagged, and heterogeneous structural values retain their existing
equality behavior.

## Diagnostics and facts

Known invalid operands report the operator and required numeric domain at the
smallest operand location. Mixed numeric operands report the existing concrete
type conflict. A statically invalid `if` condition points to the condition.

No domain marker reaches `TypeGraph`, module interfaces, CLI output, or LSP
hover. Completed expressions publish only resolved Forma types.

## Implementation plan

1. add normalized `Bool` expectations to conditional inference;
2. represent finite numeric domains alongside substitutions;
3. validate domains whenever a variable is bound or variables are merged;
4. give unary and arithmetic expressions shared numeric obligations;
5. apply surrounding result expectations before operand completion;
6. make less-than return `Bool` and equality remain unconstrained;
7. include domain obligations in delayed-binding completion checks;
8. add condition, unary, arithmetic, comparison, expected-result, invalid
   operand, mixed-number, ambiguity, semantic-fact, and cancellation tests;
9. run full workspace tests and strict static checks.

## Acceptance criteria

1. an unannotated `if` condition parameter infers as `Bool`;
2. an `Int` literal selects `Int` for both arithmetic operands and result;
3. a `Float` literal selects `Float` equivalently;
4. operand order does not change the inferred type;
5. a surrounding `Int` or `Float` expectation selects that numeric type;
6. unary negation preserves a selected operand type;
7. less-than returns `Bool` and constrains both operands;
8. equality returns `Bool` without equating heterogeneous operands;
9. String and other resolved non-numeric operands fail statically;
10. mixed `Int` and `Float` arithmetic fails without promotion;
11. a numeric-only unresolved closure fails without defaulting;
12. explicit `Any` preserves runtime checking;
13. no numeric-domain marker is published in semantic facts or interfaces;
14. workspace tests and strict static checks pass.

## Deferred work

- branch join constraints and Union normalization;
- local closure parameter and result annotations;
- explicit generic type application;
- monomorphic recursive SCC inference;
- overloads, traits, and numeric abstractions.

## Implementation result

Implemented in the RFC 0074 change set.

`GenericInference` now retains a set of numeric-constrained inference
variables. Binding one of those variables validates the resolved target
immediately; binding it to another variable transfers the domain. The domain
accepts `Int`, `Float`, and the explicit dynamic or unreachable boundaries
`Any` and `Never`, but is never interned as source-visible type metadata.

Unary negation, arithmetic, and less-than create one shared numeric obligation
for their operands and numeric result. Expected result types participate before
operand completion, so literal and contextual evidence are order-independent.
Equality remains heterogeneous and returns normalized `Bool`. Conditions are
checked against that same normalized `Bool` descriptor.

Focused tests cover condition-driven closure inference, Int and Float evidence,
operand order, expected results, unary negation, comparison and equality,
invalid and mixed operands, unresolved numeric ambiguity, and explicit `Any`.
The final workspace run passed 243 Forma tests with one manual parser benchmark
ignored, 12 CLI tests, and 19 LSP tests.

## Rejected alternatives

### Default ambiguous numerics to Int

This would make source order and an unstated default decide public monotypes.
Forma currently has no general defaulting framework, so ambiguity remains
explicit.

### Represent Numeric as Int | Float

A Union would claim that each use accepts either member independently. Numeric
operators instead require one consistent member for operands and result. The
finite domain is a solver obligation, not a runtime sum type.

### Model operators as ordinary generic natives

Their finite numeric domain cannot be expressed by current `for(...)` schemes
without traits or bounded parameters. Intrinsic constraints accurately model
the existing closed set without introducing that larger type-system feature.
