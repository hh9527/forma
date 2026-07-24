# RFC 0049: Explicit generic definition contracts

- Status: Implemented
- Depends on: RFC 0013, RFC 0041, RFC 0048

## Summary

XL permits a root prenex type scheme on `decl` bindings:

```xl
decl identity: for(A) fn(A) -> A;
def identity = fn(value) { value };
```

The declaration is the explicit polymorphic contract for the single-assignment
definition slot. Its implementation is checked once with rigid bound
parameters, while every reference to the completed binding instantiates fresh
inference variables. The scheme remains erased at runtime and is preserved in
module static interfaces independently of exported runtime values.

This extends the scheme mechanism introduced for native capabilities to
ordinary XL definitions without adding implicit generalization, polymorphic
local values, or higher-rank types.

## Motivation

RFC 0048 deliberately introduced the smallest useful scheme mechanism at the
native boundary. Its data model and inference rules are not inherently native:
`TypeScheme`, stable bound-parameter identities, fresh use-site instantiation,
and `ModuleInterface` are language-level concepts.

Ordinary definitions need the same relationships. A monomorphic declaration
cannot express that identity returns exactly its argument type or that a
higher-order function relates callback inputs and results:

```xl
decl map: for(A, B) fn(Array(A), fn(A) -> B) -> Array(B);
```

Merely instantiating calls is insufficient. The implementation must be checked
for every possible choice of its declared parameters. Treating `A` as a fresh
inference variable while checking the body could incorrectly specialize a
purported generic definition. The declaration parameters are therefore rigid
inside the corresponding definition check.

## Goals

1. allow the existing root `type_scheme` grammar after `decl Name:`;
2. retain located parameters through CST, AST, and HIR;
3. represent a generic declaration with the existing `TypeScheme` data;
4. check the corresponding `def` against the scheme body with rigid bound
   parameters;
5. instantiate fresh inference variables at each local and imported use;
6. preserve directly exported declaration schemes in `ModuleInterface`;
7. keep ordinary local aliases monomorphic after their one instantiation;
8. erase scheme parameters before compilation and retain the current VM ABI;
9. preserve cancellation checks and finalized semantic facts.

## Non-goals

- a `for(...)` annotation directly on `def`, `let`, named functions, types, or
  imports;
- implicit generalization or inferred type parameters;
- nested or higher-rank schemes and first-class polymorphic values;
- polymorphic recursion beyond calls governed by an explicit prior `decl`;
- bounded or higher-kinded parameters, traits, constraints, or specialization;
- runtime type arguments, dictionaries, or dynamic generic validation.

## Surface syntax

The declaration grammar reuses the root scheme grammar from RFC 0048:

```text
decl_binding := "decl" Identifier ":" type_scheme ";"
type_scheme := ("for" "(" Identifier ("," Identifier)* ")")? contract
```

Examples:

```xl
decl identity: for(A) fn(A) -> A;
def identity = fn(value) { value };

decl apply: for(A, B) fn(fn(A) -> B, A) -> B;
def apply = fn(function, value) { function(value) };
```

`for(...)` remains valid only at the root of a declaration contract. Parameter
names must be unique and scope only over the contract and the static check of
the corresponding definition. They do not introduce runtime names.

A monomorphic declaration remains unchanged:

```xl
decl parse: fn(String) -> Int;
```

## Static semantics

### Declaration

Evaluating the contract metadata occurs in an environment extended with one
distinct `Bound(TypeParameterId)` value per declared parameter. The result is
stored as a `TypeScheme { parameters, body }`. Duplicate parameter names and
invalid or unresolved contract metadata remain declaration errors.

### Definition checking

The matching `def` is checked against the scheme body without instantiating
its bound parameters. Expected function types flow into closure parameters and
the expected result flows into the body. A bound parameter unifies only with
the same bound identity or through already permitted `Any` behavior; it is not
solved to a concrete type.

Thus this implementation is valid:

```xl
decl identity: for(A) fn(A) -> A;
def identity = fn(value) { value };
```

and this one is rejected because `Int` cannot satisfy rigid `A`:

```xl
decl identity: for(A) fn(A) -> A;
def identity = fn(value) { 1 };
```

Recursive references in the definition use a fresh instantiation of the
explicit scheme. This permits ordinary explicitly typed recursion but does not
infer or generalize recursive schemes.

### Use sites and aliases

Every reference resolved directly to the generic definition slot instantiates
fresh inference variables, using the same bidirectional solver as generic
natives. Two direct uses may therefore select different monotypes.

An ordinary alias is still monomorphic:

```xl
let local = identity;
```

The reference on the right instantiates once; subsequent uses of `local` share
that monotype. This RFC does not make schemes first-class runtime or Dict
values.

### Modules and runtime

When a module directly exports a generic declared definition, its static
`ModuleInterface` exports the scheme beside the ordinary runtime Dict member.
Each direct imported member access instantiates freshly. Runtime compilation,
closure arity, bytecode, and VM calls contain no type parameters.

## Diagnostics

Diagnostics retain the existing RFC 0048 distinctions and locations:

- duplicate binders label the repeated parameter;
- an invalid scheme body labels the declaration contract;
- an implementation incompatible with a rigid scheme labels the definition
  and points back to the declaration;
- conflicting or underconstrained calls label the use site;
- cancellation exits through the existing query error path.

## Implementation plan

1. change `decl_binding` to consume `type_scheme`;
2. make the typed declaration view expose scheme parameters and its contract;
3. lower located declaration parameters into the existing binding data;
4. reuse the existing HIR parameter scope and `TypeScheme` construction;
5. ensure definition inference receives the rigid scheme body as its expected
   type and direct references instantiate the scheme;
6. verify direct local uses, aliases, recursion, module interfaces, semantic
   facts, diagnostics, and cancellation;
7. run workspace tests, strict Clippy, formatting, and diff checks.

## Acceptance criteria

1. generic `decl` syntax is lossless and exposes located binders;
2. identity and higher-order definitions pass rigid body checking;
3. a concretely specialized implementation of a generic result is rejected;
4. separate direct references instantiate independently;
5. an ordinary alias remains monomorphic;
6. exported schemes instantiate independently across module member accesses;
7. monomorphic declarations behave exactly as before;
8. generated bytecode and runtime calling conventions are unchanged;
9. workspace tests and strict static checks pass.

## Deferred work

- scheme annotations directly on implemented bindings;
- implicit let-generalization and a value restriction;
- richer bidirectional inference for unannotated function bodies;
- higher-rank, constrained, and higher-kinded polymorphism;
- explicit type application syntax.

## Implementation result

Implemented by making `decl_binding` consume the same root `type_scheme` node
as `native_binding`. Typed CST views and AST lowering retain located declaration
parameters; the existing HIR bound-parameter scope, `TypeScheme` construction,
rigid expected-type flow, fresh reference instantiation, and static
`ModuleInterface` propagation then apply without a second representation.

Tests cover lossless syntax and locations, identity and higher-order
definitions, rejection of a concretely specialized implementation, fresh
direct uses, monomorphic aliases, exported scheme data, and independent
cross-module member instantiation. Monomorphic declaration and generic native
tests remain unchanged. The final workspace run passed 185 core tests with one
manual benchmark ignored, 9 CLI tests, and 19 LSP tests. Strict Clippy,
formatting, and whitespace validation also pass.

## Rejected alternatives

### Put binders on the declaration name

`decl identity[A]: ...` would split the semantic `TypeScheme` across both
sides of `:`. Reusing `for(A) ...` keeps the entire contract represented by the
right-hand metadata and remains consistent with native declarations.

### Instantiate parameters while checking the definition

Inference variables are solvable. Using them for definition checking would
allow an implementation to specialize a supposedly universal declaration.
Rigid bound parameters preserve the meaning of `for(A)` as "for every A".

### Generalize every definition implicitly

That requires decisions about value restriction, effects, recursive groups,
annotation subsumption, and polymorphic local storage. Explicit declaration
schemes provide user-defined generic functions without committing to those
semantics.

### Reify type parameters in the VM

The declared relationships are needed only for static checking. Runtime type
arguments would change calling conventions and solve no requirement in this
RFC.
