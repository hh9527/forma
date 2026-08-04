# RFC 0077: Explicit generic type application

- Status: Proposed
- Depends on: RFC 0049, RFC 0051, RFC 0052, RFC 0053, RFC 0076

## Summary

Forma can explicitly instantiate every parameter of a generic `for(...)`
binding:

```forma
native empty: for(A) Fn() -> Array(A);
native identity: for(A) Fn(A) -> A;

empty[Int]()
identity[String]("value")
```

Type arguments are TypeMetadata expressions checked and evaluated during
analysis. Type application produces an ordinary monomorphic function value and
is erased from runtime bytecode.

## Motivation

Forma can declare and infer explicit generic schemes, but a call whose type
parameter has no value or expected-result evidence remains impossible:

```forma
empty() # cannot infer A
```

An annotation can provide context, but it is indirect and awkward inside larger
expressions. Explicit application completes the source-level `for(...)` model
without introducing implicit generalization or runtime type arguments.

## Syntax

Type application is a postfix expression with call-level precedence:

```text
type_application := expression '['
                    expression (',' expression)* [','] ']';
```

Examples:

```forma
empty[Int]
module.identity[String]
module.pair[Int, String](1, "x")
```

Forma currently has no value indexing syntax, so the brackets are
unambiguous. This RFC requires at least one type argument.

## Resolution

The callee must statically identify a `TypeScheme`:

- a local or core binding name; or
- a field exported by a statically resolved imported module.

Aliases of already instantiated generic functions are monomorphic and cannot
be explicitly reapplied. Arbitrary runtime expressions and dynamic `Any`
values are not type-applicable.

The number of supplied arguments must exactly equal the scheme's declared
parameter count. Applying a monomorphic binding or giving too few or too many
arguments is an analysis error.

## Type arguments

Each argument is an ordinary TypeMetadata expression. It is inferred against
`Type`, evaluated with the existing tool-stage environment and shared quota,
and decoded to a `TypeDescriptor`.

The checker substitutes descriptors positionally into the scheme body:

```text
for(A, B) Fn(A) -> B
        [Int, String]
=> Fn(Int) -> String
```

Substitution is capture-free because nested `for(...)` is already rejected and
TypeParameterIds belong to the scheme. The instantiated descriptor contains no
bound variables and participates in ordinary call and expected-type checking.

## Static and runtime behavior

Type application itself does not call the function or pass metadata values at
runtime. Compiling `callee[T]` compiles exactly the value of `callee`; a
following ordinary call retains the existing argument registers and arity.

This differs from APIs such as `decode(TypeOf(A), value)`, where TypeMetadata is
an intentional runtime value. Explicit application selects a static scheme
instance and is erased.

## Diagnostics and facts

Diagnostics distinguish:

- a callee with no statically known scheme;
- applying a monomorphic scheme;
- argument-count mismatch;
- a type argument that does not evaluate to TypeMetadata;
- an ordinary value-argument mismatch after instantiation.

Hover on the application reports the instantiated monomorphic descriptor.
References and definitions still point through the callee expression to the
original binding. Type argument expressions retain their own `TypeOf(T)` facts.

## Goals

1. add postfix explicit type application syntax;
2. support local, core, and imported generic schemes;
3. require complete positional application in the first version;
4. accept computed TypeMetadata arguments;
5. substitute without creating fresh inference variables;
6. feed the monomorphic result into ordinary bidirectional checking;
7. erase applications from runtime bytecode;
8. preserve HIR navigation, semantic facts, quotas, and cancellation.

## Non-goals

- partial type application or placeholders;
- higher-rank or higher-kinded type arguments;
- applying a runtime function value dynamically;
- explicit generic lambda binders;
- inferred generic arguments written back into source;
- runtime reification of erased type arguments;
- recursive generic inference.

## Implementation plan

1. add lossless postfix type-argument syntax and an AST expression;
2. preserve callee references and index type-argument expressions;
3. collect and evaluate type arguments with local annotation machinery;
4. resolve local and imported schemes without ordinary fresh instantiation;
5. substitute all bound parameters positionally;
6. infer and record the instantiated application descriptor;
7. compile the application as its callee value only;
8. add local, imported, empty-result, multi-parameter, computed metadata,
   arity, monomorphic, dynamic, mismatch, fact, erasure, quota, and cancellation
   tests;
9. run full workspace tests and strict static checks.

## Acceptance criteria

1. `empty[Int]()` returns `Array(Int)` without surrounding context;
2. an explicitly applied identity checks its value argument;
3. multiple type parameters substitute in declaration order;
4. imported generic members can be applied;
5. computed valid TypeMetadata arguments work;
6. too few and too many type arguments fail deterministically;
7. monomorphic bindings and arbitrary runtime expressions cannot be applied;
8. invalid TypeMetadata arguments point to the argument;
9. explicit and inferred instances have equivalent monomorphic descriptors;
10. aliases remain one monomorphic instance;
11. hover reports the instantiated type and navigation reaches the callee;
12. runtime call arity and bytecode do not include type arguments;
13. type evaluation shares quotas and cancellation;
14. workspace tests and strict static checks pass.

## Deferred work

- partial type application;
- inferred placeholders such as `pair[Int, _]`;
- explicit generic lambdas;
- monomorphic recursive SCC inference;
- bounded parameters and traits.

## Rejected alternatives

### Pass TypeMetadata as hidden runtime arguments

Generic schemes are a static interface mechanism. APIs that need metadata at
runtime already express `TypeOf(A)` explicitly; hidden arguments would obscure
the ABI and duplicate that capability.

### Treat every function value as type-applicable

After a scheme is instantiated or aliased, its quantifier is gone. Recovering
genericity from a runtime function would introduce impredicative or dynamic
type application outside Forma's explicit scheme model.

### Infer omitted trailing arguments

Complete application gives the first version one clear substitution operation.
Partial application can be added later with explicit placeholder and diagnostic
rules if real use cases justify it.
