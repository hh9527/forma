# RFC 0083: Acyclic `def` component generalization

- Status: Proposed
- Depends on: RFC 0054, RFC 0078 through RFC 0080, RFC 0082
- Tracking issue: https://github.com/hh9527/forma/issues/1

## Summary

Unannotated closure-valued `def` bindings are analyzed by resolved dependency
component. An acyclic component may infer and publish a rank-1 scheme under the
same restrictions as an eligible closure-valued `let`; a recursive component
remains monomorphic unless it has an explicit contract.

```forma
def identity = fn(value) { value };
def apply = fn(value) { identity(value) };

(apply(1), apply("text")) # (Int, String)
```

Forward references do not change the result:

```forma
def apply = fn(value) { identity(value) };
def identity = fn(value) { value };
```

The checker obtains the same component graph and analyzes `identity` before
`apply`. Runtime definition slots, initialization, and source evaluation order
are unchanged.

## Motivation

RFC 0079 generalizes eligible closure-valued `let` bindings. RFC 0078 keeps all
uncontracted closure-valued `def` bindings monomorphic because `def` supports
forward references and recursion. That is safe but broader than necessary:
most named helpers are acyclic, and forcing `let` merely to recover ordinary
rank-1 reuse makes the binding forms diverge for an implementation artifact.

The relevant property is not the keyword and not source order. It is whether a
binding belongs to a recursive strongly connected component in the resolved
definition graph. Forma already resolves references to HIR definition
identities, including shadowing, so the checker can make that distinction
without textual name scanning.

## Eligible nodes

A node enters the implicit-generalization graph when it is:

- a `def` binding;
- initialized by a closure literal;
- unannotated;
- without explicit `for(...)` parameters; and
- not paired with an explicit `decl` contract.

Other bindings may be referenced by graph nodes but are not candidates for
implicit generalization. An annotation remains authoritative. A non-closure
`def` without a contract retains the existing rejection or monomorphic rules;
this RFC does not generalize arbitrary values.

## Resolved dependency graph

Each eligible HIR definition identity is one graph node. There is an edge
`F -> G` when a reference contained in `F`'s initializer resolves to eligible
definition `G` in the same lexical block.

Containment is determined from HIR expression parent identities. Resolution is
determined from `HirResolution::Definition`, never by comparing identifier
text. Consequently:

- a shadowing parameter or local `let` creates no false edge;
- two same-spelled definitions in different scopes remain distinct;
- imported and external bindings are not local component edges; and
- nested closure references still belong to the containing initializer.

Components and their topological order are deterministic. Ties are ordered by
the source location of the component's earliest definition.

## Component classification

A component is recursive when it contains more than one node or its sole node
has a self-edge. Every recursive component follows RFC 0078:

- all members receive monomorphic closure skeletons before bodies are checked;
- every intra-component reference uses those same monomorphic identities;
- no member is implicitly generalized; and
- polymorphic recursion remains impossible.

A singleton component without a self-edge is acyclic. Its initializer is
checked after all dependency components and may be generalized immediately.
Dependents then see its scheme and instantiate it independently at each direct
reference.

## Acyclic generalization

An eligible acyclic `def` uses the RFC 0079 closure-generalization boundary:

1. allocate variables owned by the closure initializer;
2. infer the monomorphic closure descriptor using already completed
   dependencies;
3. resolve substitutions;
4. reject outstanding numeric-domain variables;
5. collect owned variables in normalized structural order;
6. replace them with fresh rigid `Bound` identities; and
7. publish one `TypeScheme` for subsequent references and module export.

Captured outer inference variables are not generalized. Aliases remain
monomorphic. A direct reference instantiates the scheme; taking the definition
as a value still creates one monomorphic instance at that use.

## Dependency ordering

Static component analysis follows dependency order, independent of textual
order. Given `F -> G`, `G` is completed before `F`. This is not runtime
reordering: `def` already denotes predeclared single-assignment slots, and the
compiler retains its existing initialization and recursion behavior.

References from an acyclic node into a recursive component see the completed
monomorphic component descriptor. Such a dependent may still generalize only
variables it owns. References from a recursive component into an acyclic
dependency see an instantiated scheme after that dependency is completed.

## Nested blocks and modules

The rule applies independently to every lexical block. A nested component may
capture outer monomorphic descriptors or instantiate outer schemes, but it
cannot generalize captured inference identities.

Top-level generalized `def` schemes are exported through the existing module
interface path exactly like inferred `let` schemes. Imported member access
instantiates the scheme. The runtime Struct member remains one closure value;
the scheme is separate static interface data.

## Diagnostics and recovery

A recursive uncontracted component that cannot complete monomorphically keeps
the RFC 0078 diagnostic. RFC 0084 may add component member labels. An acyclic
definition that remains underconstrained uses the local-generalization
diagnostic when a variable is ineligible, such as a numeric-domain variable.

Incomplete or conflicted HIR never causes a plausible scheme to be guessed.
Recovery may expose unavailable facts, but only a complete resolved component
publishes a scheme. Cancellation or a stale revision publishes no partial
component results.

## Determinism

The following do not affect inferred schemes:

- source order of acyclic definitions;
- hash-map or hash-set iteration;
- presentation names of shadowed bindings;
- cache state or query scheduling; and
- cancellation timing.

SCC membership uses semantic identities. Component traversal uses a stable
topological order, and generalized parameter ordering retains RFC 0079's
normalized structural occurrence rule.

## Runtime behavior

This RFC is static-only. It adds no closure cloning, specialization, runtime
type argument, slot, instruction, or VM operation. A generalized `def` remains
one single-assignment runtime closure. Multiple typed uses call that same
closure value.

## Goals

1. generalize eligible acyclic closure-valued `def` bindings;
2. classify recursion from resolved HIR identities;
3. support forward acyclic references independently of source order;
4. keep self-recursive and mutually recursive components monomorphic;
5. reuse RFC 0079 ownership and parameter-order rules;
6. preserve aliases and captured outer variables as monomorphic;
7. export completed top-level schemes through module interfaces;
8. apply the rule in nested lexical blocks;
9. publish component results atomically under cancellation; and
10. preserve runtime slots, closure identity, bytecode, and VM behavior.

## Non-goals

- polymorphic recursion;
- higher-rank or first-class schemes;
- generalization of arbitrary non-closure values;
- generalization across recursive SCCs;
- explicit mutual-generic syntax;
- traits, interfaces, associated types, or constraints;
- changing runtime initialization order.

## Implementation plan

1. derive eligible definition nodes and initializer containment from HIR;
2. build semantic dependency edges within each lexical block;
3. compute deterministic strongly connected components and dependency order;
4. retain RFC 0078 skeleton inference for recursive components;
5. infer and generalize acyclic singleton components in dependency order;
6. publish local and top-level scheme facts through existing paths;
7. remove textual recursion checks from the authoritative decision path;
8. add direct, forward, chained, self-recursive, mutual, shadowing, nested,
   capture, alias, export, semantic-fact, cancellation, and runtime tests;
9. run full workspace tests and strict static checks.

## Acceptance criteria

1. an independent closure-valued `def` is reusable at distinct types;
2. an acyclic chain generalizes every eligible member;
3. reversing source order preserves schemes and results;
4. self-recursive and mutually recursive definitions remain monomorphic;
5. shadowed same-spelled names create no dependency edge;
6. captured outer inference variables are not generalized;
7. aliases instantiate once and remain monomorphic;
8. nested blocks follow the same component rule;
9. exported acyclic schemes instantiate in importing modules;
10. definition hover shows a scheme and references show instances;
11. cancellation and recovery publish no partial scheme;
12. one runtime closure and the existing slot ABI are preserved; and
13. workspace tests and strict static checks pass.

## Rejected alternatives

### Generalize every unannotated `def`

That admits polymorphic recursion or requires guessing recursive instances
before their bodies are known.

### Keep every `def` monomorphic

This is safe but makes forward visibility unnecessarily disable ordinary
rank-1 reuse for independent named helpers.

### Detect recursion by identifier text

Text cannot distinguish shadowing, nested scopes, or same-spelled definitions.
The HIR already owns the authoritative semantic identity.

### Use source order instead of component order

Then a harmless forward edge changes inference while the equivalent reversed
program succeeds. `def` promises forward visibility, so static completion must
respect dependencies rather than textual placement.
