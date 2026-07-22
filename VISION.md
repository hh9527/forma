# XL Language Vision

## Purpose

XL is an experimental general-purpose programming language implemented in Rust.
It explores whether a small dynamic runtime, a mostly pure functional language,
and a closed-world toolchain can use the same ordinary language to perform both
program computation and higher-order type metadata computation.

The central hypothesis is:

> In a sufficiently closed world, types can be ordinary immutable metadata,
> and higher-order type operations can be pure functions evaluated by a
> toolchain-hosted instance of the language runtime.

XL is not a configuration DSL. Configuration, validation, normalization,
encoding, and similar domains should be expressed by general-purpose functions
and libraries rather than special merge rules or domain-specific evaluation
semantics.

## Design Principles

### One language across two stages

XL has a tool stage and a program stage. Both stages use the same value model,
function semantics, and evaluator.

- The tool stage evaluates closed, pure metadata computations for type
  checking, editor information, and compile-time derivation.
- The program stage executes ordinary program code on the dynamic VM.
- Type annotations are normally erased before program execution.
- Type metadata explicitly used as a value can be retained at runtime.

There is no separate macro language or unrestricted type-level language.
Operations such as `Partial(User)` and `Result(String, Error)` should be
ordinary pure function calls over type metadata.

### Closed world by default

The toolchain should be able to enumerate the program's code and static data:

- module paths are statically known;
- packages and inputs in the build graph are pinned;
- JSON, YAML, and TOML files can participate as data modules;
- runtime `eval` and arbitrary dynamic imports are outside the model;
- genuine external data enters through a small number of explicit boundaries,
  such as command-line input.

Closed definitions do not imply that every runtime value is known at compile
time. Tool-stage evaluation is allowed only for pure computations with known
inputs and is subject to deterministic resource limits.

### Pure functional center

- Ordinary values are immutable.
- Functions are pure by default.
- Control flow is expression-oriented.
- Blocks, conditionals, and pattern matching produce values.
- Domain behavior is composed from functions instead of hidden language rules.

The VM and trusted built-ins may use unobservable mutation for garbage
collection, interning, caches, copy-on-write, and uniqueness optimizations.
Observable effects, processes, IO, and process-local storage are deliberately
deferred until a coherent effect boundary is designed.

### Small dynamic runtime

The program runtime is a compact bytecode VM inspired by Lua's implementation
shape, with an immutable term model influenced by Erlang. The initial native
value categories are:

```text
Int(i64), Float(f64), String, Bytes,
Dict, Array, Atom, Tuple, Func
```

The initial numeric semantics use `i64` and `f64`. Integer overflow and integer
division by zero are runtime errors. Floating-point behavior follows IEEE 754.

`Dict` is the sole native string-keyed product value. Conceptually it is:

```text
Dict = (shape, values)
```

Its shape is a canonical, expression-order-independent set of string fields.
An implementation can store it as a sorted field array aligned with a value
array, and equal shapes can be shared. Static record types and homogeneous
`Dict<T>` types erase to the same runtime value.

### Atoms and tagged tuples

Atoms are symbolic runtime values. Sum values use Erlang-style tagged terms:

```text
'None
('Some, value)
('Ok, value)
('Err, error)
```

The VM assigns stable identities and language semantics to these built-in
atoms:

```text
'None, 'Some, 'Ok, 'Err, 'True, 'False
```

Boolean conditions accept only `'True` and `'False`; XL does not use general
truthy/falsy coercion. Tags remain ordinary observable atoms, and tagged values
remain ordinary tuples even when bytecode instructions optimize their use.

### Types are metadata

A type declaration provides a static constraint and a canonical metadata value.
Metadata is composed only from ordinary XL values and fixed, documented shapes.
It can be inspected, passed to functions, transformed, and retained at runtime.

The same metadata may support:

- static checking and LSP information;
- runtime validation;
- normalization and corrective transformation;
- encoding and decoding;
- documentation and schema generation.

Type metadata computation is ordinary pure computation. When a type position
requires a concrete result, the toolchain evaluates the expression in its own
VM. If it cannot produce a valid type within its resource budget, it reports a
tool-stage evaluation error rather than silently inventing a precise type.

Omitted types are inferred when practical. A type that is omitted and cannot be
inferred is `Any`. `Any` represents a loss of static guarantees, not a distinct
runtime value category.

Whether traits, row polymorphism, higher-kinded types, or higher-order types
need dedicated surface constructs is intentionally unresolved. Ordinary
functions over type metadata should be tried first; dedicated static machinery
must be justified by abstraction, diagnostics, or analysis that ordinary
tool-stage evaluation cannot provide.

### General-purpose composition

XL does not define configuration-specific merge, priority, defaulting, or
constraint semantics. Such policies are explicit functions:

```text
input
|> fn(value) { normalize(User, value) }
|> fn(value) { validate(User, value) }
|> encode_json
```

This preserves one set of evaluation rules across configuration, data
processing, application logic, and tooling.

## Surface Direction

The surface syntax is broadly Rust-like, without Rust ownership semantics.

- Named functions use `fn name(args) { ... }`.
- Closures use `fn(args) { ... }`, not `|args| { ... }`.
- `|>` is a left-associative, low-precedence pipeline operator.
- `value |> f` is exactly equivalent to `f(value)`.
- Configured pipeline stages use explicit unary closures until partial
  application is introduced.
- `if`, `match`, and blocks are expressions.
- Bindings are immutable.

Surface syntax may elaborate into a smaller core language, but elaboration must
not introduce domain-specific or stage-specific evaluation semantics.

## Static Data Modules

Static data files may be imported as immutable modules and become part of the
closed dependency graph:

- JSON, YAML, and TOML produce data values;
- text produces a `String`;
- JSON Lines produces an `Array` of data values.

Arbitrary external object keys should remain strings rather than permanently
interned atoms. Format features that cannot map deterministically to XL values
must be rejected or handled by an explicit library policy.

## MVP Thesis

The MVP is successful only if it demonstrates this vertical loop:

1. Parse and execute expression-oriented XL code on a bytecode VM.
2. Load a static data module into the same immutable value model.
3. Represent a type as canonical runtime metadata.
4. Execute an ordinary pure function over that metadata in the tool-stage VM.
5. Feed the computed metadata back into static checking.
6. Use the same metadata for runtime validation or normalization.
7. Erase unused type information before program-stage execution.
8. Admit external JSON through an explicit CLI boundary and validate it.

The MVP does not require traits, dedicated HKT syntax, effects, processes, a
JIT, package management, production garbage collection, or a complete LSP.
It should expose enough structured analysis output to prove that a future LSP
can consume the toolchain's computed types.

## Evaluation Criteria

The experiment has succeeded when the loop above works with few compiler
special cases. It has failed, or needs redesign, if the type checker must
reimplement ordinary metadata functions in a separate hidden type language.

Engineering quality for the MVP includes deterministic behavior, source-aware
diagnostics, resource-bounded tool-stage execution, focused tests, and examples
that exercise the complete two-stage path.
