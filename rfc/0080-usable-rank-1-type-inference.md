# RFC 0080: Usable rank-1 type inference

- Status: Accepted
- Depends on: RFC 0048, RFC 0049, RFC 0052, RFC 0070 through RFC 0079
- Tracking issue: https://github.com/hh9527/forma/issues/1

## Summary

Forma will complete one bounded type-inference phase whose target is usable,
deterministic rank-1 polymorphism:

```forma
let identity = fn(value) { value };
let pair = fn(left, right) { (left, right) };

(identity(1), identity("text"), pair[Int, _](1, "value"))
```

The phase combines explicit generic contracts, inferred local schemes,
explicit and inferred type arguments, expected-result checking, and acyclic
definition analysis into one coherent model. It does not extend Forma to
higher-rank values, polymorphic recursion, constrained generics, or traits.

This is an umbrella RFC. It fixes the common semantics and exit criteria for a
small sequence of child RFCs. Each child RFC remains independently proposed,
implemented, tested, and committed. The tracking issue owns mutable scheduling
and task status; this document owns stable language commitments.

## Motivation

RFCs 0070 through 0079 established the individual mechanisms needed for useful
inference:

- bottom-aware directional checking;
- structural propagation of unresolved constraints;
- body and later-use inference for monomorphic closures;
- deterministic branch joins and intrinsic constraints;
- partial closure contracts;
- complete explicit type application;
- monomorphic recursive closure inference; and
- restricted rank-1 generalization of closure-valued `let` bindings.

These pieces are already useful, but their remaining edges must be designed as
one phase. A placeholder in explicit type application must obey the same
evidence rules as an omitted generic argument. Expected results must constrain
explicit and inferred schemes identically. Generalizing an acyclic `def` must
not permit polymorphic recursion. Diagnostics must explain the same conceptual
distinctions exposed by hover and module interfaces.

Without a shared contract, individually reasonable improvements can create an
inconsistent language: `_` can become a spelling of `Any`, source order can
choose a generic solution, aliases can accidentally preserve polymorphism, or
an optimization can change whether a `def` is generalized.

## Phase boundary

The phase begins after RFC 0079 and completes when all accepted child RFCs and
the shared acceptance criteria in this document are implemented.

The planned sequence is:

1. RFC 0081: partial explicit type application;
2. RFC 0082: context-complete generic inference;
3. RFC 0083: acyclic `def` component generalization;
4. RFC 0084: inference diagnostics and boundary audit.

The tracking issue may reorder or split unfinished work when implementation
evidence warrants it. Removing a shared invariant or expanding the phase into
a deferred feature requires an amendment to this RFC.

## Core model

Forma retains four distinct static entities:

```text
TypeDescriptor::Bound       rigid parameter in a TypeScheme
TypeDescriptor::Inference   temporary solver obligation
TypeDescriptor::Any         explicit dynamic boundary
TypeDescriptor::Never       uninhabited result with no positive evidence
```

The source placeholder introduced by RFC 0081 creates an inference obligation;
it is not a fifth type and is never published. A `TypeScheme` binds only its
declared `Bound` identities. Instantiation replaces those identities with fresh
inference variables while preserving unrelated rigid parameters captured from
an outer checking context.

Inference variables have a finite owner and completion boundary. They may live
through one call, initializer, lexical block, or acyclic definition component,
as specified by the responsible child RFC. They may never cross a published
module interface or final semantic snapshot unresolved.

## Rank-1 and predicativity

Generalized bindings have ordinary rank-1 schemes:

```text
for(A, B) Fn(A) -> B
```

Every use instantiates a scheme to one monomorphic descriptor. A scheme is not
itself a first-class value type and cannot appear as a function parameter,
result, collection item, Struct field, or Union variant.

Taking a generic binding as an ordinary value instantiates it once:

```forma
let alias = identity;
(alias(1), alias("text")) # conflict
```

This rule applies equally to declared, inferred, local, and imported schemes.
Preserving a scheme through an alias would require first-class polymorphism and
is outside this phase.

## Sources of evidence

One generic instance may be constrained by:

1. explicitly supplied type arguments;
2. ordinary value arguments;
3. closure parameter and result expectations;
4. structural literal members;
5. a surrounding expected result type; and
6. intrinsic expression constraints.

These sources participate in one solution. Their traversal order must not
change the accepted program, final descriptor, parameter ordering, or primary
diagnostic. Explicit type arguments are rigid requirements; omitted or
placeholder positions remain inference obligations.

`Never` satisfies a directional expectation without solving it. `Any` may
erase a variable only at an explicit dynamic boundary defined by existing
checking rules. Empty collections and unreachable branches provide no positive
evidence. Numeric-domain variables retain their finite-domain restriction and
cannot be generalized into unconstrained parameters.

## Completion

Every inference boundary has exactly three successful outcomes:

- all variables resolve to concrete descriptors;
- eligible variables generalize into declared `Bound` parameters; or
- an explicit `Any` boundary intentionally erases the remaining information.

Otherwise analysis fails. It does not default a variable to `Any`, reinterpret
it as `Never`, choose a numeric type, or publish a placeholder.

Completion is atomic. Cancellation, stale workspace revisions, conflicts, and
underconstrained inference publish neither partial substitutions nor provisional
schemes.

## Binding classes

The completed phase will expose these boundaries:

| Binding form | Generalization behavior |
| --- | --- |
| explicit `for(...)` declaration | uses its declared scheme |
| eligible closure-valued `let` | generalized under RFC 0079 |
| alias or arbitrary `let` initializer | instantiated once and monomorphic |
| acyclic eligible `def` component | generalized under RFC 0083 |
| recursive `def` component | monomorphic unless explicitly contracted |
| `native` | trusts its explicit declaration |
| import | preserves exported schemes at statically known module fields |

Whether a `def` component is acyclic is a property of resolved HIR identities,
not textual name scanning or source order. No member of a recursive component
is implicitly generalized, even when its body appears independently solvable.

## Explicitness

Forma retains both inference and explicit control:

```forma
identity(1)          # infer the instance
identity[Int](1)     # specify every parameter
pair[Int, _](1, "x") # specify one, infer one
```

Explicit syntax never changes runtime calling convention or closure identity.
It constrains static instantiation and is erased before bytecode emission.

An author can always replace inference with an explicit contract. Explicit
contracts remain authoritative for public APIs, recursive groups, dynamic
boundaries, and cases where inference-only constraints cannot be represented
as scheme metadata.

## Determinism

All child RFCs must preserve:

- stable inferred parameter order by normalized structural occurrence;
- canonical Union construction;
- source-order-independent solutions for the same dependency graph;
- no speculative substitution rollback in branch joins;
- deterministic component traversal and diagnostics; and
- stable CLI/LSP presentation names independent of semantic parameter IDs.

Hash-map iteration order, cancellation timing, cache state, or parallel query
scheduling must not affect observable inference.

## Tooling and interfaces

Definitions publish either a monomorphic type fact or a scheme fact. Individual
references and expressions publish instantiated monomorphic facts. CLI and LSP
must distinguish these views:

```text
identity definition  for(A) Fn(A) -> A
identity(1) call      Int
```

Direct module exports retain `TypeScheme`; imported member access instantiates
it. A module result's runtime Struct type may contain the erased function shape,
but that erasure must not replace or weaken its separate exported scheme.

Diagnostics use source identities and locations from HIR and the authoritative
checker. Recovery may publish explicit unknown or conflicted facts, but never a
plausible generic scheme synthesized from incomplete evidence.

## Runtime model

This phase is static-only:

- no runtime type arguments;
- no duplicated closure values per instance;
- no generic bytecode specialization;
- no VM instruction or ABI changes;
- no runtime type dictionary; and
- no change to capture or single-assignment definition slots.

Every explicit application, placeholder, inferred scheme, and instantiation is
erased after checking.

## Child RFC responsibilities

### RFC 0081: Partial explicit type application

Define placeholder syntax, mixed rigid/inferred substitution, completion, and
diagnostics for partially supplied type arguments.

### RFC 0082: Context-complete generic inference

Audit and complete propagation among arguments, callbacks, structural values,
and surrounding expected results. Specify an order-independent constraint
solution rather than a traversal accident.

### RFC 0083: Acyclic `def` component generalization

Use resolved definition dependencies to generalize eligible acyclic components
without generalizing recursive components or introducing polymorphic recursion.

### RFC 0084: Inference diagnostics and boundary audit

Unify underconstrained, conflicting, rigid, placeholder, capture, and
non-generalizable diagnostics; verify semantic facts, module interfaces, CLI,
LSP, cancellation, and recovery across every binding class.

## Goals

1. make ordinary rank-1 helpers reusable without redundant annotations;
2. retain explicit control over any or all generic arguments;
3. combine every accepted evidence source into one deterministic solution;
4. generalize only at documented value and component boundaries;
5. preserve monomorphic recursion and aliases;
6. keep dynamic erasure explicit through `Any`;
7. publish accurate scheme and instance facts;
8. preserve schemes across static module interfaces;
9. produce stable, actionable diagnostics;
10. keep all polymorphism erased at runtime.

## Non-goals

- higher-rank or impredicative polymorphism;
- polymorphic recursion;
- first-class scheme values;
- traits, interfaces, protocols, or associated types;
- constrained generic parameter metadata;
- higher-kinded types or type constructors as a distinct kind system;
- numeric defaulting or ad-hoc overloading;
- coercions, general subtyping, or flow-sensitive narrowing;
- runtime specialization or type dictionaries;
- effect inference or a generalized purity proof.

## Shared acceptance criteria

1. common identity, mapping, pairing, empty-constructor, and callback examples
   infer without redundant annotations;
2. explicit, placeholder, argument, and expected-result evidence compose;
3. equivalent evidence order produces identical results;
4. aliases instantiate once and remain monomorphic;
5. eligible `let` and acyclic `def` bindings publish stable rank-1 schemes;
6. recursive components never gain implicit polymorphism;
7. numeric and future solver-only constraints are not erased into false schemes;
8. nested generic closures preserve distinct outer rigid parameters;
9. direct exports and imports preserve independently instantiated schemes;
10. definitions show schemes while references and calls show instances;
11. unresolved obligations fail at their documented completion boundary;
12. diagnostics distinguish missing evidence from conflicting evidence;
13. cancellation and stale revisions publish no provisional facts;
14. no invalid inference or bound identity reaches the final type graph;
15. runtime values, bytecode, closure identity, and the VM ABI are unchanged;
16. every child RFC passes full workspace tests and strict static checks;
17. the completed phase records deviations and final results in this RFC.

## Implementation and tracking

The mutable task list lives in GitHub issue #1. Each child RFC follows the
repository rule: commit the accepted proposal before implementation, then
commit its tested implementation separately.

This RFC remains `Accepted` while child work is active. Once the shared exit
criteria are satisfied, its status changes to `Implemented` and an
Implementation result records the delivered model, any amendments, final test
coverage, and explicitly deferred work.

## Rejected alternatives

### Use only a mutable plan document

The phase carries stable language commitments shared by multiple changes, not
only a preferred work order. A plan alone would not make those invariants part
of Forma's accepted design history.

### Put the task list in this RFC

Scheduling, checkboxes, and implementation discoveries change frequently. They
belong in the tracking issue so this document can remain a stable contract.

### Complete HM inference without staged boundaries

Unrestricted generalization would conflict with Forma's forward-visible
definitions, explicit dynamic boundaries, inference-time numeric domains, and
future effectful native capabilities. This phase chooses useful rank-1 behavior
with visible restrictions instead of claiming full Hindley-Milner inference.

### Enter constrained generics now

Representing numeric domains or capabilities in `TypeScheme` leads toward
interfaces, evidence passing, and associated-type questions. Those deserve a
separate phase after unconstrained rank-1 behavior is complete.
