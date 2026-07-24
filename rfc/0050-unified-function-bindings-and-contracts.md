# RFC 0050: Unified function bindings and contracts

- Status: Implemented
- Depends on: RFC 0013, RFC 0048, RFC 0049

## Summary

XL removes the special named-function binding and expresses all definitions
through `def`. A `def` may carry the same root type scheme as `decl`:

```xl
def identity: for(A) Fn(A) -> A = fn(value) { value };
```

Lowercase `fn` constructs function values. Uppercase `Fn` constructs function
type metadata. `decl`, annotated `def`, and `native` all consume the same
`type_scheme` syntax, while the runtime representation and VM ABI remain
unchanged.

## Motivation

XL currently has two ways to define a function:

```xl
fn add(left: Int, right: Int) -> Int { left + right }

decl add: fn(Int, Int) -> Int;
def add = fn(left, right) { left + right };
```

The first form combines binding, parameter declaration, contract construction,
and closure construction in one grammar production. The second uses the
general single-assignment slot model and keeps the complete contract as data to
the right of `:`. RFC 0049 also made the second form the language's explicit
generic-definition mechanism.

Keeping a distinct named-function AST and HIR kind now adds semantic branches
without adding capability. It also overloads lowercase `fn` for both values and
types. XL's other type constructors use uppercase names (`Array`, `Tuple`,
`Int`), so `Fn` makes the value/type boundary visible and regular.

## Goals

1. allow an optional root `type_scheme` annotation on `def`;
2. make annotated `def` atomically declare and initialize one definition slot;
3. preserve split `decl` plus unannotated `def` for mutual recursion and
   separated contracts;
4. check annotated implementations with the same rigid scheme semantics as
   RFC 0049;
5. predeclare annotated definitions so their implementations may recurse;
6. reject a prior `decl` combined with an annotated `def` as two contracts for
   one slot;
7. remove named-function syntax and its AST, HIR, type-analysis, and compiler
   variants;
8. reserve lowercase `fn` for closure expressions and uppercase `Fn` for
   function contracts;
9. keep type metadata, inference, bytecode, closures, and VM calls unchanged.

## Non-goals

- implicit generalization or inferred scheme parameters;
- parameter annotations inside closure syntax;
- nested or higher-rank schemes;
- changing function variance, assignability, calling conventions, or runtime
  representation;
- preserving source compatibility with named-function or lowercase function
  contract syntax;
- making `Fn` an ordinary shadowable prelude binding.

## Surface syntax

The relevant grammar becomes:

```text
binding        := let_binding | decl_binding | def_binding | native_binding
                | type_binding | import_binding
decl_binding   := "decl" Identifier ":" type_scheme ";"
def_binding    := "def" Identifier (":" type_scheme)? "=" expression ";"
native_binding := "native" Identifier ":" type_scheme ";"
type_scheme    := ("for" "(" Identifier ("," Identifier)* ")")? contract
contract       := Identifier ("(" contract ("," contract)* ")")?
                | "Fn" "(" contract ("," contract)* ")" "->" contract
closure        := "fn" parameters block
```

Examples:

```xl
def answer = 42;

def increment: Fn(Int) -> Int = fn(value) {
    value + 1
};

def identity: for(A) Fn(A) -> A = fn(value) {
    value
};
```

Mutual recursion retains split declarations:

```xl
decl even: Fn(Int) -> Int;
decl odd: Fn(Int) -> Int;
def even = fn(value) { if value < 1 { 1 } else { odd(value - 1) } };
def odd = fn(value) { if value < 1 { 0 } else { even(value - 1) } };
```

The removed named-function form has a mechanical translation:

```xl
fn increment(value: Int) -> Int { value + 1 }
```

becomes:

```xl
def increment: Fn(Int) -> Int = fn(value) { value + 1 };
```

Parameter names belong only to the closure. Parameter and result types belong
only to the contract.

## Binding semantics

### Unannotated `def`

An unannotated definition has the existing behavior. If a matching `decl`
slot exists, it initializes that slot and is checked against its contract.
Otherwise it creates an inferred monomorphic binding. Without a prior
declaration it is not predeclared for recursion.

### Annotated `def`

An annotated definition performs declaration and initialization atomically. It
creates a slot before indexing its value, evaluates its contract into a
`TypeScheme`, and checks the implementation against the rigid scheme body.
References outside the implementation, and recursive references within it,
freshly instantiate the scheme.

The following is rejected because the implementation specializes universal
`A` to `Int`:

```xl
def identity: for(A) Fn(A) -> A = fn(value) { 1 };
```

### One contract per slot

A split declaration must be initialized by an unannotated definition:

```xl
decl parse: Fn(String) -> Int;
def parse = fn(text) { ... };
```

Combining a declaration with an annotated definition is a duplicate contract
and is rejected even when the two contracts are textually equal:

```xl
decl parse: Fn(String) -> Int;
def parse: Fn(String) -> Int = fn(text) { ... };
```

This keeps one authoritative scheme and avoids a contract-equivalence rule.

## Function value and type syntax

`fn` and `Fn` are distinct case-sensitive keywords:

```xl
fn(value) { value }  // expression producing a function value
Fn(Int) -> String    // contract producing Function TypeMetadata
```

`Fn` remains parser-recognized rather than a normal type binding because its
arrow and result contract have dedicated syntax. Lowering still produces the
existing function TypeMetadata constructor call; no new semantic type node or
runtime value is introduced.

Using lowercase `fn` where a contract is required, or uppercase `Fn` where an
expression is required, is a syntax error. Nested function contracts use
uppercase consistently:

```xl
for(A, B) Fn(Array(A), Fn(A) -> B) -> Array(B)
```

## Internal model

`BindingKind::NamedFunction`, the typed `NamedFunction` CST view, and
`HirDefinitionKind::NamedFunction` are removed. All former named functions
lower to ordinary `BindingKind::Def` values whose right-hand expression is a
closure.

An annotated `BindingKind::Def` retains located type parameters and an
annotation alongside its value. Slot indexing predeclares only declarations,
natives, types, imports, and annotated definitions. Analysis constructs a
scheme for annotated definitions using the same path as declarations and
natives, then checks and initializes the definition exactly once.

Directly exported annotated definitions retain their schemes in
`ModuleInterface`. Generic parameters are erased before compilation as in RFC
0048 and RFC 0049.

## Diagnostics

- duplicate contracts point to both the prior declaration and annotated
  definition;
- a missing implementation still points to its `decl`;
- an implementation mismatch points to the value and its contract;
- duplicate or unresolved scheme parameters retain existing diagnostics;
- removed named-function and lowercase function-contract forms produce normal
  parser recovery diagnostics;
- query cancellation continues through existing checkpoints.

## Compatibility

This is an intentional source-breaking grammar cleanup. All repository source,
standard-library declarations, fixtures, and examples migrate in one change.
There is no compatibility alias for lowercase function contracts or named
functions, so the language has one canonical spelling immediately.

Historical RFCs remain historical records and are not rewritten except where
an actively implemented RFC's result would otherwise claim current syntax.

## Implementation plan

1. add `Fn` to the lexer and replace lowercase function contracts in grammar;
2. add an optional `type_scheme` to `def_binding` and remove `named_function`;
3. update typed CST views, validation, and AST lowering;
4. predeclare annotated definitions in HIR and remove named-function kinds;
5. generalize scheme collection and rigid checking from `decl`/`native` to
   annotated `def` while enforcing one contract per slot;
6. simplify compiler branches to ordinary definitions;
7. mechanically migrate standard-library source, tests, fixtures, and active
   documentation examples;
8. test recursion, mutual recursion, generic annotated definitions, duplicate
   contracts, parser rejection, module schemes, semantic facts, and runtime
   equivalence;
9. run workspace tests, strict Clippy, formatting, and diff checks.

## Acceptance criteria

1. annotated monomorphic and generic definitions parse losslessly;
2. annotated definitions are recursively visible and initialize once;
3. split declarations continue to support mutual recursion;
4. duplicate split and inline contracts are rejected;
5. generic implementations are checked rigidly and instantiate freshly;
6. direct exports preserve schemes and aliases remain monomorphic;
7. named-function and lowercase function-contract syntax are rejected;
8. AST, HIR, analysis, and compiler no longer contain named-function variants;
9. `fn` closures and `Fn` contracts lower to the existing runtime and metadata
   representations;
10. workspace tests and strict static checks pass.

## Deferred work

- implicit generalization and local polymorphism;
- annotations on lambda parameters;
- higher-rank or constrained schemes;
- generic data type declarations;
- explicit type application.

## Implementation result

Implemented with one `DefBinding` CST/AST/HIR/compiler path. An optional root
scheme is lowered on `def`; annotated definitions are predeclared for recursion,
count as their slot's sole initialization, undergo the existing rigid scheme
check, and export through the existing static `ModuleInterface`. A prior
`decl` plus annotated `def` is rejected as a duplicate declaration.

The named-function grammar, typed view, `BindingKind`, `HirDefinitionKind`,
analysis branches, compiler branches, and dedicated recovery expectations were
removed. Repository XL sources now use `def name = fn(...) { ... };` or
annotated `def name: type_scheme = fn(...) { ... };`. Lowercase `fn` appears
only in closure grammar, while uppercase `Fn` appears only in function contract
grammar. Type diagnostics and semantic displays also render `Fn(...)`.

Tests cover lossless annotated definitions, generic fresh instantiation, rigid
implementation rejection, duplicate contracts, recursive annotated functions,
split mutual recursion, rejection of both removed syntaxes, standard-library
contracts, modules, CLI behavior, and LSP queries. The final workspace run
passed 187 core tests with one manual benchmark ignored, 9 CLI tests, and 19
LSP tests. Strict Clippy, formatting, and whitespace validation also pass.

## Rejected alternatives

### Keep named functions as sugar

Permanent sugar would retain parser, AST, HIR, diagnostics, and tooling paths
for a construct expressible as annotated `def` plus a closure. Removing it now
keeps later binding features orthogonal.

### Keep lowercase `fn` for contracts

Context can disambiguate the token, but readers and tools still face one word
with two semantic categories. `Fn` follows XL's uppercase type vocabulary and
makes nested higher-order contracts easier to scan.

### Allow both a `decl` and annotated `def`

Requiring equality would create two authoritative spellings and force scheme
alpha-equivalence and diagnostic precedence rules. One slot has one contract.

### Require annotations on every `def`

Inferred monomorphic values are useful and already well-defined. The optional
annotation adds an atomic explicit-contract form without making simple values
verbose.
