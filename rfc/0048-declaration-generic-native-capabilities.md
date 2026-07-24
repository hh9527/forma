# RFC 0048: Declaration-generic native capabilities

- Status: Proposed
- Depends on: RFC 0013, RFC 0024, RFC 0038, RFC 0041, RFC 0042

## Summary

XL adds explicit declaration-level type parameters to native bindings:

```xl
native map[A, B]: fn(Array(A), fn(A) -> B) -> Array(B);
```

The declaration is the sole static contract exposed by a library. Each use of
the binding instantiates fresh inference variables for its declared type
parameters, and a small bidirectional constraint solver relates arguments,
callbacks, and the expected result type. Type parameters are erased before
compilation and do not change the native registry or VM ABI.

The VM continues to trust native implementations. Linking verifies only the
existing runtime requirements, such as symbol presence and value arity; it
does not obtain a second type signature from Rust or dynamically validate each
call. A host implementation that violates its XL declaration is a trusted-host
bug rather than an ordinary XL type error.

## Motivation

RFC 0024 made native capabilities explicit but required monomorphic
contracts. Standard functions consequently erase useful relationships through
`Any`:

```xl
native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any);
```

This cannot express that the callback consumes the array element type or that
its result becomes the output element type. The same problem affects
`identity`, `empty`, `filter`, `flat_map`, `fold`, and data-independent Dict
operations.

Making the host registry authoritative for these types would create two
contracts and require agreement between them. XL libraries already own their
public declarations, while the VM deliberately treats registered native code
as trusted. Genericity therefore belongs to the XL declaration and semantic
analysis, not to the native ABI.

This feature also establishes the minimum bidirectional machinery needed for
later ordinary-function inference without committing XL to full
Hindley-Milner inference, implicit generalization, subtyping, or higher-rank
polymorphism.

## Goals

1. parse and retain type parameters on top-level native declarations;
2. make the declaration the only authoritative static native contract;
3. represent quantified binding schemes separately from monomorphic types;
4. instantiate a fresh monotype at every reference to a generic native;
5. infer type arguments from ordinary arguments, higher-order callbacks, and
   an available expected result type;
6. diagnose inconsistent and underconstrained instantiations without falling
   back to `Any`;
7. erase type parameters before bytecode generation and preserve the existing
   native symbol, arity, closure, quota, and calling conventions;
8. expose instantiated expression facts through the existing semantic
   snapshot and asynchronous query layers.

## Non-goals

- implicit type parameters or implicit generalization of `let` bindings;
- generic `def`, `decl`, named-function, type, or import syntax in this RFC;
- higher-rank types, first-class polymorphic values, or `forall` inside type
  expressions;
- bounded parameters, traits, type classes, interfaces, associated types, or
  overload resolution;
- higher-kinded type parameters such as `F[_]`;
- subtyping, variance-based coercion, union distribution, or intersection
  types;
- specialization, monomorphized bytecode, reified type arguments, or hidden
  runtime dictionaries;
- deriving an XL type from a Rust callback or dynamically checking trusted
  native arguments and results in the VM.

## Surface syntax

The native grammar becomes conceptually:

```text
native_binding := "native" Identifier type_parameters? ":" contract ";"
type_parameters := "[" Identifier ("," Identifier)* "]"
```

Examples:

```xl
native identity[A]: fn(A) -> A;
native empty[A]: fn() -> Array(A);
native map[A, B]: fn(Array(A), fn(A) -> B) -> Array(B);
native fold[A, B]: fn(Array(A), B, fn(B, A) -> B) -> B;
```

The brackets are part of the binding declaration, not an application operator
and not a general type expression. The quantified scope begins after `:` and
ends with the declaration contract. A parameter may appear any number of times
or not at all; an unused parameter is accepted syntactically but makes an
unconstrained call impossible unless an expected type determines it.

Parameter names must be unique within the list. They occupy the type namespace
inside the contract and shadow type declarations with the same spelling.
Names not declared as parameters continue to resolve as ordinary type names;
an unresolved name never becomes an implicit type variable.

The CST and AST retain each parameter and its source range. Diagnostics for a
duplicate parameter, unresolved contract name, inconsistent constraint, or
undetermined parameter label the declaration or call ranges that introduced
the relevant facts.

## Static model

The semantic type layer distinguishes monotypes from binding schemes:

```text
TypeScheme = parameters + body
Type        = concrete node | inference variable
```

Conceptually, the declaration:

```xl
native map[A, B]: fn(Array(A), fn(A) -> B) -> Array(B);
```

creates the scheme:

```text
map : [A, B] fn(Array(A), fn(A) -> B) -> Array(B)
```

The named parameters are rigid while checking the declaration. On every value
reference to `map`, analysis replaces them with distinct fresh inference
variables. Two references in the same expression therefore do not share an
instantiation.

Schemes belong to bindings. `TypeGraph` and workspace expression facts remain
monomorphic: after successful solving, each reference, call, and containing
expression records its instantiated type. A polymorphic binding definition may
retain a scheme identity for tooling, but no unresolved inference variable is
published as an authoritative `WorkspaceTypeId`.

`Any` remains an explicit dynamic type and is not an inference variable. An
unknown semantic fact remains unavailable evidence and is not unified as a
wildcard. Generic inference must not silently convert either condition into
the other.

## Bidirectional call inference

The initial solver operates at generic call sites. It supports equality
constraints over the existing structural type forms and follows named/ref
nodes with cycle protection. The call is checked in three directions:

1. instantiate the callee scheme with fresh variables;
2. check each argument against the corresponding parameter type, propagating
   an expected function type into closure parameters and results;
3. when the surrounding expression supplies an expected type, constrain the
   call result against it before finalizing the instantiation.

For example:

```xl
map([1, 2], fn(x) { x + 1 })
```

produces `A = Int` from the first argument, checks the closure with the expected
type `fn(Int) -> B`, derives `B = Int` from its body, and returns `Array(Int)`.

Expected types permit result-driven inference:

```xl
let values: Array(Int) = empty()
```

Without that expected type, `empty()` leaves `A` unsolved and is rejected with
an inference diagnostic. It does not become `Array(Any)`.

Conflicting constraints are rejected at the call site:

```xl
native choose[A]: fn(A, A) -> A;
choose(1, "x")
```

The first implementation requires equality of inferred structural types. It
does not search for a common supertype or construct a Union automatically.

Occurs checks reject infinite inference substitutions. Recursive named types
already represented by stable graph references remain valid; only an
inference variable occurring inside its own proposed solution is invalid.

## Native trust and runtime erasure

Native type parameters have no runtime representation. The declaration above
still links one symbol named `map` whose runtime arity is two. No type argument,
descriptor, witness, or dictionary is passed to the callback.

The existing host registry continues to contain runtime implementations and
arity. Link-time arity checking protects the calling convention but is not a
second type contract. The compiler emits an ordinary external native closure
link, and the VM invokes it through the existing trusted callback or
continuation path.

A generic native implementation must consequently be representation
independent over XL values. A capability that genuinely needs runtime type
metadata must declare an explicit ordinary XL parameter for that metadata; the
compiler must not add a hidden one.

The VM performs no per-call static-type validation. Native callbacks retain
their existing responsibility for operational errors such as invalid indices,
quota exhaustion, or malformed explicitly dynamic `Any` data. Returning a
value that contradicts a declared generic relationship is a defect in the
trusted native library.

## Semantic recovery and tooling

A complete generic native declaration contributes a resolved HIR definition
and a type scheme even when a later sibling is damaged. Duplicate parameters
or an invalid contract produce explicit partial semantic facts rather than an
`Any` scheme.

Hover and completion display the instantiated monotype for expressions. A
definition-oriented query may display declaration syntax or a deterministic
scheme form, but LSP adapters do not instantiate or solve types themselves.
All inference happens during workspace analysis and is published through one
immutable snapshot.

Async cancellation and revision semantics remain unchanged. Generic analysis
uses the existing quota and query checkpoints during contract evaluation,
constraint generation, substitution traversal, and graph publication.

## Compatibility

Existing declarations without brackets are monomorphic and retain their
current behavior:

```xl
native length: fn(Array(Any)) -> Int;
```

The runtime ABI and registry format do not change. Standard-library modules may
migrate individual contracts from `Any` to explicit parameters without
requiring changes to their native implementations.

Because `[` immediately after a native binding name was not previously valid,
the syntax addition is backward compatible.

## Acceptance criteria

1. the lexer, lossless CST, typed syntax views, parser, AST, and recovered HIR
   retain located native type parameters;
2. duplicate parameters and unknown contract type names have precise
   diagnostics;
3. each native reference receives a fresh instantiation of its declaration
   scheme;
4. argument inference preserves relationships through Array, Tuple, Struct,
   Function, and named/ref types supported by the existing graph;
5. expected result types can solve otherwise undetermined parameters;
6. higher-order callbacks receive expected parameter types and contribute
   their result constraints;
7. conflicting constraints, occurs-check failures, and unsolved parameters are
   diagnosed and never degrade to `Any`;
8. instantiated types are recorded for references, calls, and enclosing
   expressions and appear in semantic queries;
9. existing monomorphic native declarations behave unchanged;
10. generic parameters are erased and the native registry, linker arity check,
    bytecode call path, VM, quotas, and continuation behavior remain unchanged;
11. generic native analysis honors cancellation and stale workspace revisions;
12. core, CLI, semantic, recovery, and LSP test suites remain green under
    strict Clippy and formatting checks.

## Implementation plan

1. extend native CST and AST bindings with located declaration parameters;
2. resolve parameter names in native contract type expressions;
3. introduce internal type schemes and fresh inference variables without
   publishing unresolved variables in `TypeGraph`;
4. instantiate schemes at references and add equality constraints at calls;
5. propagate expected function types into closures and expected result types
   into calls and annotated bindings;
6. solve substitutions with occurs checks and deterministic diagnostics;
7. record finalized monotypes in existing analysis and workspace facts;
8. migrate representative Array natives such as `map`, `filter`, and `fold` to
   generic declarations without changing their Rust implementations;
9. verify recovery, cancellation, CLI output, semantic queries, and LSP hover;
10. run workspace tests, strict Clippy, formatting, and diff checks.

## Deferred work

- declaration parameters on `decl`, `def`, and named-function syntax;
- inferred generic parameters and let-polymorphism;
- bounded parameters and capability constraints;
- higher-rank and first-class polymorphism;
- higher-kinded parameters and generic type constructors;
- richer bidirectional checking outside generic call sites;
- explicit type application syntax;
- reified runtime type arguments or specialization.

## Rejected alternatives

### Put `forall` in the type expression

`native map: forall[A, B] ...` makes polymorphism a general type-expression
feature and raises higher-rank representation questions immediately. Binding
parameters express the required prenex scheme without promising first-class
polymorphic values.

### Use `for(A, B)` in the type expression

This is compact but has the same semantic expansion as `forall` and overloads
a likely value-level control-flow word. `map[A, B]` keeps quantification visibly
attached to the declaration that owns the scheme.

### Obtain generic signatures from the host registry

That would introduce a second source of truth and require XL and Rust contracts
to agree. Library declarations are the public static authority; the registry
only supplies trusted runtime implementations.

### Dynamically validate every native call

Per-call checks would duplicate the static type system inside the VM, add cost,
and blur responsibility for trusted host defects. Explicitly dynamic `Any`
operations may validate their own inputs, but generic declarations do not add
automatic VM checks.

### Encode relationships with `Any`

`Any` cannot state that two arguments share a type or that a callback result
determines an output element. Treating it as an inference variable would also
destroy the existing distinction between known dynamic types and unavailable
semantic facts.

### Implement full Hindley-Milner inference first

Implicit generalization, value restrictions, polymorphic recursion, and the
interaction with XL's evaluated type metadata are materially larger decisions.
Explicit native schemes plus call-site bidirectionality provide immediate
standard-library value while keeping those choices open.
