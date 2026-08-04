# RFC 0071: Constraint-preserving structural inference

- Status: Proposed
- Depends on: RFC 0052, RFC 0070

## Summary

Forma's bidirectional checker propagates an expected structural type into a
literal even when that structure contains unresolved inference variables.

The motivating case becomes directly inferable:

```forma
native concat: for(A) Fn(Array(Array(A))) -> Array(A);

concat([[1, 2], [], [3]]) # Array(Int)
```

The expected outer item type is `Array(?A)`. That structural shell reaches each
nested Array. Non-empty members solve `?A`; an empty member preserves the same
obligation without turning it into `Any` or solving it as `Never`.

This RFC only preserves constraints supplied by an existing expected type. It
does not yet create block-lived obligations for a wholly unconstrained literal
or infer an unannotated closure from later uses.

## Motivation

RFC 0052 made expected types authoritative, but Array inference currently
propagates its expected item only when the item contains no inference variable:

```text
Array(Array(?A))
      ^^^^^^^^^ rejected as nested context
```

The checker therefore loses useful structure precisely at a generic call site.
RFC 0063 records the resulting workaround:

```forma
let nested: Array(Array(Int)) = [[1, 2], [], [3]];
arrays.concat(nested)
```

The annotation contains no information that the call contract and non-empty
members could not already establish. Requiring it is an implementation leak.

## Goals

1. propagate expected Array, Dict, Tuple, and Struct structure through nested
   literals even when descendants contain inference variables;
2. preserve one shared inference identity through every nested occurrence;
3. allow non-empty siblings to solve variables needed by empty siblings;
4. make sibling order irrelevant to the final solution;
5. retain existing TypeMetadata widening for heterogeneous metadata literals;
6. retain `Never` as no evidence rather than an element-type solution;
7. report genuinely unresolved generic results instead of replacing their
   variables with `Any`;
8. record fully resolved final expression facts;
9. retain cancellation checkpoints during nested traversal and unification.

## Non-goals

- changing the type of a completely unconstrained `[]` or `{}` expression;
- introducing block-lived inference variables for ordinary local bindings;
- inferring unannotated closure parameters without an expected function type;
- implicit generalization or polymorphic local bindings;
- collection covariance, subtyping, or coercion;
- changing heterogeneous runtime collection support;
- traits, associated types, or higher-kinded collection abstraction.

## Structural propagation

Checking a literal against a known structural constructor recursively passes
the corresponding child expectations:

```text
Array literal <= Array(E)       each item <= E
Dict literal  <= Dict(E)        each field <= E
Tuple literal <= Tuple(E...)    item i <= E[i]
Struct literal <= Struct(E...)  field n <= E[n]
```

`E` may contain inference variables. The checker must not discard the expected
shape merely because `contains_type_variable(E)` is true.

For nested Array inference:

```text
concat([[1], [], [2]])

parameter: Array(Array(?A))
outer item expectation: Array(?A)

[1] <= Array(?A)  contributes ?A = Int
[]  <= Array(?A)  contributes no new evidence
[2] <= Array(?A)  confirms ?A = Int
```

The final result is `Array(Int)`.

## Empty literals

An empty literal receiving a structural expectation retains that expected
child descriptor:

```text
[] <= Array(?A)  synthesizes Array(?A)
```

It does not solve `?A`, default it to `Any`, or replace it with `Never`.

Without any structural expectation, current conservative literal behavior is
unchanged by this RFC. Creating a fresh obligation whose lifetime extends
through a local block belongs to RFC 0073.

## Naked variables and widening

Passing a naked inference variable directly into every non-empty literal member
would make member order determine the result and would reject existing
TypeMetadata widening:

```forma
[Int, String]
```

The literal currently synthesizes the common metatype `Type`, which is useful
when passed to generic collection functions. Therefore a collection may use
the expected variable as its container obligation while still synthesizing and
joining non-empty member evidence before solving that variable.

The implementation may stage this as a focused Array rule in the first pass,
provided the observable rules above hold and Dict/Tuple/Struct retain their
existing expected-field behavior. It must not bind a shared item variable to
the first sibling merely because that sibling is visited first.

## Never

RFC 0070's directional rule applies recursively:

```text
actual Never <= expected ?A
```

succeeds without solving `?A`. A reachable sibling or surrounding expected type
must provide the solution. A collection expression whose evaluation reaches a
`Never` child never completes at runtime, but static checking may still use the
declared collection expectation, just as a call argument of `Never` fits any
parameter without becoming type evidence.

## Diagnostics

Conflicting reachable evidence reports the smallest conflicting member and the
resolved structural expectation. An empty member is never blamed merely for
providing no evidence.

If a generic result remains unresolved after all arguments and surrounding
expectations have been processed, the existing `cannot infer generic result
type` diagnostic remains authoritative.

## Implementation plan

1. replace the Array expected-item guard with structural propagation that
   preserves nested variables;
2. preserve a bare expected item variable for an empty Array without using it
   as first-member evidence for a non-empty heterogeneous literal;
3. resolve the shared descriptor only after every sibling has contributed;
4. audit Dict, Tuple, and Struct expected-child propagation for equivalent
   premature variable erasure;
5. add direct generic nested-Array tests in both sibling orders;
6. test multiple empty siblings, all-empty underconstrained calls, conflicting
   reachable evidence, `Never` evidence, and heterogeneous TypeMetadata arrays;
7. remove the RFC 0063 annotation workaround from its active regression test;
8. verify final semantic facts and cancellation remain resolved and responsive;
9. run full workspace tests and strict static checks.

## Acceptance criteria

1. `concat([[1], [], [2]])` infers `Array(Int)`;
2. `concat([[], [1]])` and `concat([[1], []])` infer the same type;
3. an expected `Array(Array(String))` reaches every nested empty literal;
4. an all-empty generic call without expected result remains underconstrained;
5. a surrounding expected result solves an all-empty nested call;
6. conflicting non-empty siblings fail deterministically;
7. `Never` members do not solve item inference variables;
8. heterogeneous TypeMetadata arrays retain their common `Type` behavior;
9. Dict, Tuple, and Struct expected fields continue to preserve nested generic
   relationships;
10. no unconstrained literal, closure, or local-binding behavior changes;
11. final semantic facts contain resolved types only;
12. workspace tests and strict static checks pass.

## Deferred work

- fresh obligations for wholly unconstrained empty literals;
- self-constrained inference for unannotated closures;
- block-scoped delayed monomorphic inference;
- generalized joins or subtyping for heterogeneous collections.

## Rejected alternatives

### Default nested empty members to Any

`Any` erases the relationship declared by the generic contract and can hide a
real conflict. Lack of evidence must remain an obligation, not become a dynamic
boundary.

### Infer nested empty members as Array(Never)

RFC 0070 reserves `Never` for computations that cannot return. An empty
collection is a real value whose element type lacks evidence; using bottom
would additionally require covariance or coercion rules.

### Bind the item variable from the first sibling

This makes order observable and rejects common widening such as heterogeneous
TypeMetadata arrays. All reachable siblings must contribute before the final
solution is fixed.

