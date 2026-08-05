# RFC 0085: Bounded callable-shape inference

- Status: Proposed
- Depends on: RFC 0080 through RFC 0084
- Tracking issue: https://github.com/hh9527/forma/issues/3

## Summary

Forma will complete one narrowly bounded extension to rank-1 bidirectional
inference: an ordinary call may constrain an otherwise unknown monomorphic
callee to a closed Function shape.

```forma
let apply = fn(callback, value) { callback(value) };
let increment = fn(value) { value + 1 };
apply(increment, 41)
```

Checking `callback(value)` may establish:

```text
callback : Fn(type(value)) -> ?Result
```

The obligation remains monomorphic, participates in the existing finite
unification model, and must complete or generalize at an existing RFC 0080
boundary. This phase does not infer open object shapes, search for behavior,
or add a general-purpose constraint language.

This is an umbrella RFC. Child RFCs remain independently proposed,
implemented, tested, and committed. The tracking issue owns mutable progress;
this document owns the phase boundary and stopping rules.

## Motivation

RFCs 0070 through 0084 can propagate a known Function expectation into a
closure and can infer a closure from intrinsic operations in its body. They do
not currently turn a call through an unknown value into Function evidence.
Consequently, a small higher-order helper loses information even though every
required constraint is local and rank-1:

```forma
let use = fn(callback) { callback(1) + 2.0 };
```

The missing step is structural, not ad-hoc dispatch: the syntax of a call
requires one callable value with a known arity and a fresh result obligation.
Adding that one source of evidence makes ordinary higher-order composition
usable without introducing traits or implicit capability resolution.

## Phase sequence

The planned sequence is:

1. RFC 0086: unknown-callee Function-shape constraints;
2. RFC 0087: callable-obligation convergence across repeated and nested calls;
3. RFC 0088: callable-inference diagnostics and publication audit.

The sequence may be split when implementation evidence reveals a smaller
boundary. Any expansion beyond callable Function shapes requires an amendment
and explicit discussion.

## Core rule

For an ordinary call with `n` value arguments:

```text
callee(argument_0, ..., argument_n)
```

if the callee resolves to an unbound monomorphic inference variable, checking
may bind it to:

```text
Fn(?Parameter_0, ..., ?Parameter_n) -> ?Result
```

Argument evidence, a surrounding expected result, and intrinsic constraints
inside callback bodies solve those fresh variables through the existing
directional checker. If the callee is already known, ordinary call checking is
unchanged. A known non-Function remains an error.

This rule does not construct a scheme. Generalization, when eligible, happens
only at the existing binding boundary after all body constraints have been
collected.

## Closed structural scope

The only inferred shell in this phase is `Function` with exact source arity.
The phase does not infer:

- a Struct from `value.field`;
- an Array or Dict from an operation name;
- a Tagged or Enum shape from pattern use;
- an interface, trait, protocol, or capability implementation;
- optional, variadic, overloaded, or keyword-parameter call shapes.

Existing literal and expected-type propagation for concrete Array, Dict,
Tagged, Tuple, and Struct descriptors remains available inside parameters and
results. It does not become open-shape inference.

## Monomorphism and rank

An inferred callable obligation is one monomorphic identity. Repeated uses
must agree on arity, parameter descriptors, and result descriptor:

```forma
let use = fn(callback) {
    (callback(1), callback("text")) # conflict
};
```

An eligible closure-valued binding may generalize only after its complete body
has been checked under RFC 0079 or RFC 0083. No argument position accepts a
scheme as a first-class value, and no inferred parameter is higher-rank.

Recursive components retain RFC 0078 and RFC 0083 behavior. This phase must
not infer polymorphic recursion or use recursive calls to manufacture
evidence-free `Any`.

## Direction and completion

Call syntax supplies evidence that the callee is callable; it does not supply
positive evidence for every parameter or result. An unused parameter or an
unobserved result may therefore remain unresolved and must obey the existing
completion rules.

`Never` remains directional bottom evidence. Explicit `Any` remains the only
intentional dynamic erasure. Numeric-domain obligations remain solver-only and
cannot generalize as unconstrained parameters.

## Determinism

Repeated calls form ordinary equations over one monomorphic descriptor.
Source traversal may determine where a conflict is reported, but it may not
change acceptance, the completed descriptor, or diagnostic category.

The implementation must use existing stable HIR identities and canonical
descriptor construction. It must not depend on hash iteration, speculative
rollback, cache state, or query scheduling.

## Runtime model

This phase is static-only. It adds no runtime dispatch, dictionaries, type
arguments, specialization, bytecode instruction, or VM state. A source call
continues to compile as the same ordinary call after checking.

## Goals

1. infer an exact Function shell from a call through an unknown monomorphic
   value;
2. propagate argument, callback-body, and expected-result evidence through
   that shell;
3. keep repeated and nested calls on one deterministic constraint solution;
4. preserve rank-1 generalization and monomorphic aliases and recursion;
5. reject known non-callable values and incompatible callable uses clearly;
6. publish no unresolved callable obligation or solver identity; and
7. leave runtime behavior unchanged.

## Non-goals

- traits, interfaces, type classes, protocols, or implicit instance search;
- constrained generics or parameterized capability metadata;
- associated types, higher-kinded types, or higher-rank polymorphism;
- row polymorphism, open Struct types, structural subtyping, or field-shape
  inference;
- overload resolution, coercions, union-call distribution, or intersection
  types;
- flow-sensitive narrowing or control-flow typing;
- variadic, optional, named, or partially applied calls;
- effect inference or generalized constraint solving.

## Stopping rules

Work on this phase stops and returns to design discussion if any accepted child
RFC requires:

1. solving an open set of fields or methods;
2. searching a global or lexical implementation environment;
3. choosing among multiple candidate Function shapes;
4. subtyping or coercion to make call evidence converge;
5. quantification inside a parameter or result descriptor;
6. solver backtracking rather than monotonic unification; or
7. runtime representation changes.

These are not implementation inconveniences. They indicate that the proposed
program lies outside this bounded phase.

## Shared acceptance criteria

1. a higher-order helper can infer a callback's exact arity and monomorphic
   Function shape from its body;
2. argument and expected-result evidence solve parameter and result variables;
3. intrinsic constraints reached through a callback remain enforced;
4. repeated compatible calls converge and incompatible calls conflict;
5. nested calls propagate evidence without traversal-order-dependent results;
6. known non-Functions retain a dedicated call error;
7. underconstrained callable obligations fail or generalize only at existing
   completion boundaries;
8. aliases and recursive components gain no additional polymorphism;
9. CLI, LSP, module interfaces, recovery, cancellation, and stale revisions
   publish no provisional callable shape; and
10. full workspace tests and strict static checks pass.

## Rejected alternatives

### Infer every structural shell from use

Calls have an exact syntactic arity and one existing closed descriptor. Field
access would require deciding whether a partial field set is an open Struct,
subtype, row, or capability. Combining those questions would turn a local
rank-1 improvement into a new type-system foundation.

### Introduce traits before callable inference

No behavior selection is needed here. A call constrains one value already in
scope; it does not search for an implementation. Traits would add coherence,
resolution, and associated-type questions without addressing this local gap.

### Default unresolved callable positions to `Any`

That would hide missing evidence and make generalization depend on erasure.
Forma retains explicit `Any` as an authored dynamic boundary.

## Amendment policy

The umbrella may be marked Implemented after all accepted child RFCs meet the
shared criteria and an implementation-result section records any narrowed
semantics. Adding field-shape inference, capabilities, subtyping, overloads,
or higher-rank behavior requires a separate RFC rather than an implementation
note.
