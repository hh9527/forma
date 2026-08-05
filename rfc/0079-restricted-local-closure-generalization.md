# RFC 0079: Restricted local closure generalization

- Status: Implemented
- Depends on: RFC 0049, RFC 0052, RFC 0073, RFC 0076, RFC 0077, RFC 0078

## Summary

An unannotated, non-recursive `let` whose initializer is a closure literal may
generalize initializer-owned inference variables into one rank-1 `TypeScheme`:

```forma
let identity = fn(value) { value };
(identity(1), identity("text"))
```

infers:

```text
identity: for(A) Fn(A) -> A
(Int, String)
```

Every reference to `identity` instantiates fresh monomorphic variables. The
closure itself remains one ordinary runtime value; generalization and
instantiation exist only in static analysis.

This first version deliberately retains RFC 0073's monomorphic behavior for
`def`, aliases, non-closure initializers, captured unresolved obligations, and
inference constraints that cannot be represented by an ordinary `for(...)`
scheme.

## Motivation

RFCs 0072 and 0073 preserve relationships such as:

```text
Fn(?A) -> ?A
Fn(?A) -> Array(?A)
```

but require one later use to choose a single monomorphic solution. That is the
right behavior for general computations and recursive definitions, yet it
makes the most elementary pure helper unnecessarily single-use:

```forma
let identity = fn(value) { value };
(identity(1), identity("text")) # currently conflicts
```

Forma already represents declared rank-1 schemes with `TypeScheme`,
instantiates them per reference, preserves them through module interfaces, and
supports explicit application. The missing operation is a conservative,
deterministic conversion from suitable local inference variables to bound type
parameters.

## Eligible bindings

A binding is eligible only when all of these conditions hold:

1. its kind is `let`;
2. it has no type annotation or explicit type parameters;
3. its initializer is syntactically a closure literal;
4. the initializer is analyzed without a surrounding expected function type;
5. every generalized variable was created while analyzing that initializer;
6. no generalized variable is free in the surrounding environment;
7. no generalized variable carries an inference-only constraint that the
   resulting `for(...)` scheme cannot express.

Partial parameter and result annotations from RFC 0076 are allowed. Concrete
annotated positions remain concrete while eligible unannotated positions may be
generalized:

```forma
let keep_left = fn(left: Int, right) { left };
# for(A) Fn(Int, A) -> Int
```

The syntactic closure restriction is Forma's first value restriction. It is
easy to explain, stable under future effectful native capabilities, and does
not require the checker to prove that an arbitrary call or block is pure.

## Generalization

After checking an eligible initializer, the checker resolves all substitutions
already supplied by its body. It then collects remaining inference variables
from the closure descriptor in deterministic structural order.

A variable is generalizable when it:

- belongs to the initializer's allocation interval;
- does not resolve to, or share identity with, a variable free in the outer
  environment; and
- has no outstanding numeric-domain or other solver-only restriction.

The checker replaces each such variable with a fresh bound parameter:

```text
Fn(?7) -> Array(?7)
    |
    +-- generalize --> for(A) Fn(A) -> Array(A)
```

Repeated occurrences preserve identity. Parameter order follows first
structural occurrence in the normalized descriptor, not hash-map order or use
order. Inferred parameter names are presentation-only and deterministically
chosen as `A`, `B`, and so on; semantic identity remains `TypeParameterId`.

If the descriptor contains no eligible variable, the binding remains an
ordinary monomorphic closure. If it retains an unresolved non-generalizable
variable, it stays under RFC 0073's delayed monomorphic completion rule and
must be solved by later evidence or rejected at the block boundary.

## Instantiation

Each direct reference to a generalized binding creates fresh inference
variables exactly as a declared `for(...)` scheme does:

```forma
identity(1)      # Fn(Int) -> Int instance
identity("text") # Fn(String) -> String instance
```

The instances share no substitutions. Constraints from one call cannot solve
or conflict with another instance.

Taking a generalized function as a value instantiates it once. Therefore an
alias remains monomorphic, matching RFC 0077's rule for declared generics:

```forma
let alias = identity;
(alias(1), alias("text")) # conflict
```

This RFC does not introduce first-class polymorphic values or impredicative
types.

## Explicit type application

An inferred local scheme is a statically known scheme for RFC 0077:

```forma
let identity = fn(value) { value };
identity[Int](1)
```

Type arguments remain erased. Applying type arguments to an alias or another
monomorphic value remains invalid.

## Captures and outer obligations

Capturing a concrete outer value does not prevent generalization:

```forma
let prefix: String = "value:";
let pair_with_prefix = fn(value) { (prefix, value) };
# for(A) Fn(A) -> (String, A)
```

An inference variable shared with an outer delayed binding is not owned by the
closure and cannot be generalized. It retains one monomorphic identity so the
outer block can complete it consistently. This is the standard environment
free-variable boundary, expressed in Forma's existing delayed-inference model.

## `def` and recursion

Unannotated `def` closures remain governed by RFC 0078 and are monomorphic,
including definitions that turn out to be acyclic. `def` has forward-visible
single-assignment semantics, so generalizing only selected acyclic components
would require a separate component-level rule and more complex source-order
diagnostics.

Authors can continue to give a `def` an explicit generic contract:

```forma
def identity: for(A) Fn(A) -> A = fn(value) { value };
```

Implicit generalization of non-recursive `def` components is deferred.
Polymorphic recursion is not implied and remains out of scope.

## Constraints without scheme metadata

RFC 0074's numeric domain is an inference-time restriction, not a source-level
type bound:

```forma
let negate = fn(value) { -value };
```

Generalizing its unresolved numeric variable as unconstrained `A` would be
unsound because `negate[String]` would then appear valid. Until Forma has a
data model for constrained type parameters, such a variable is not
generalizable and must receive concrete evidence or an explicit contract.

The same rule applies to any future solver-only constraint that cannot be
faithfully represented in `TypeScheme`.

## Interfaces and semantic facts

A generalized top-level `let` exported directly from the module result retains
its inferred scheme in `ModuleInterface`, exactly like an explicitly generic
definition. Imported field references instantiate it independently.

Binding hover and CLI type observation display the inferred scheme rather than
erasing bound parameters to `Any`. Individual reference and call expression
facts remain monomorphic instantiated types. No inference variable may reach a
published fact or interface.

## Diagnostics and cancellation

Generalization traversal, instantiation, and block completion retain query
cancellation checkpoints. Cancellation publishes neither a partial scheme nor
provisional instantiated facts.

Underconstrained non-generalizable bindings keep RFC 0073's diagnostic. A
future diagnostic may explain the particular capture or solver-only constraint
that prevented generalization, but this RFC requires deterministic failure and
the binding initializer as the primary location.

## Goals

1. infer rank-1 schemes for eligible closure-valued `let` bindings;
2. instantiate each direct reference independently;
3. preserve relationships among repeated variables in parameters and results;
4. exclude variables free in the surrounding environment;
5. exclude constraints not representable in `TypeScheme`;
6. preserve monomorphic alias behavior;
7. support RFC 0077 explicit application of inferred schemes;
8. preserve inferred schemes across direct module exports;
9. expose schemes accurately through semantic tooling;
10. keep runtime values, closure capture, and bytecode unchanged.

## Non-goals

- generalizing `def`, recursive components, or arbitrary expressions;
- polymorphic recursion;
- first-class or impredicative polymorphism;
- higher-rank function parameters or results;
- generic closure binder syntax;
- constrained generics, traits, interfaces, or associated types;
- generalizing empty Array or Dict literals;
- purity or effect inference;
- changing explicit `for(...)` contracts.

## Implementation plan

1. add deterministic inference-variable collection and substitution helpers;
2. track scoped inferred schemes alongside monomorphic environments;
3. recognize eligible closure-valued `let` initializers;
4. subtract outer free variables and solver-only constrained variables;
5. convert eligible variables to stable bound parameters;
6. instantiate inferred schemes on direct variable references;
7. expose inferred schemes to RFC 0077 type application;
8. retain top-level inferred schemes in direct module exports;
9. publish scheme-aware binding facts for hover and CLI observation;
10. add local, nested, partial-annotation, capture, alias, explicit-application,
    export/import, rejection, runtime, deterministic-order, and cancellation
    tests;
11. run full workspace tests and strict static checks.

## Acceptance criteria

1. one inferred identity closure accepts both `Int` and `String` calls;
2. parameter/result relationships survive independent instantiation;
3. multiple inferred parameters have deterministic identities and order;
4. concrete partial annotations remain concrete;
5. concrete captures do not prevent unrelated generalization;
6. outer unresolved variables are not generalized;
7. unresolved numeric-domain variables are not generalized as unconstrained;
8. aliases instantiate once and remain monomorphic;
9. inferred schemes support complete explicit type application;
10. unannotated `def` and recursive closures remain monomorphic;
11. arbitrary value initializers and empty collections are not generalized;
12. nested scopes shadow and restore inferred schemes correctly;
13. direct module exports preserve and independently instantiate the scheme;
14. binding facts display the inferred scheme while use facts are monomorphic;
15. runtime closure identity and capture behavior are unchanged;
16. no inference or bound variable leaks into an invalid published position;
17. cancellation prevents provisional publication;
18. workspace tests and strict static checks pass.

## Deferred work

- implicit generalization of acyclic `def` components;
- a richer value restriction informed by effects;
- constrained generic parameter metadata;
- partial explicit application such as `pair[Int, _]`;
- higher-rank and impredicative polymorphism;
- polymorphic recursion.

## Rejected alternatives

### Generalize every unresolved delayed binding

That would silently turn empty collections and arbitrary computations into
polymorphic values, erase useful later-use constraints from RFC 0073, and leave
future effects without a defensible value restriction.

### Generalize every closure, including `def`

Forward-visible `def` groups require dependency-component analysis to separate
acyclic definitions from recursive ones. Treating all of them as polymorphic
would accidentally introduce polymorphic recursion.

### Generalize numeric-domain variables as ordinary parameters

`for(A)` currently expresses no `A is Int or Float` bound. Dropping that bound
would publish a stronger and false contract.

### Make aliases preserve polymorphism

That requires first-class scheme values or special propagation rules through
arbitrary expressions. Direct-reference instantiation keeps polymorphism
predicative and matches existing declared-generic behavior.

## Implementation result

Implemented in the authoritative bidirectional checker. `GenericInference`
maintains lexical scheme scopes alongside monomorphic descriptor environments.
An eligible `let` closure is first inferred under RFC 0073's delayed boundary;
the checker then resolves body constraints, collects initializer-owned variables
in structural order, excludes outstanding numeric-domain variables, and
replaces the remaining variables with stable bound parameters.
Inferred parameter identities avoid rigid parameters already present in the
descriptor, and instantiation replaces only parameters declared by that scheme;
an inner generic closure can therefore capture an outer generic parameter
without conflating the two.

Direct variable references and RFC 0077 type applications consult the same
scoped scheme table. Each direct reference receives fresh inference variables,
while a non-closure alias records one instantiated descriptor and consequently
remains monomorphic. Scheme scopes explicitly record monomorphic shadows, so a
local binding cannot accidentally expose a same-named core or outer generic.

Inferred schemes are retained for nested definition facts and direct top-level
module exports. Workspace definitions carry their scheme presentation
separately from the erased runtime type graph; CLI `show` and LSP hover therefore
report `for(A) Fn(A) -> A`, while ordinary call expressions retain their
instantiated monomorphic facts.

Coverage includes independent Int/String instances, repeated parameter/result
relationships, partial annotations, concrete and unresolved captures,
monomorphic aliases, numeric-domain rejection, lexical shadowing, explicit type
application, cross-module export/import, runtime closure identity, CLI output,
and LSP hover. Full workspace tests and strict Clippy checks pass.
