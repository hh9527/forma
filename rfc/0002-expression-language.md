# RFC 0002: Expression Language and Functions

- Status: Accepted for MVP
- Implementation: Complete

## Summary

This RFC defines the first XL source language, its parser, compilation to RFC
0001 bytecode, function calls, lexical closures, pattern matching, and the
pipeline operator.

## Source files

An XL source file is a sequence of immutable bindings followed by one result
expression:

```text
let answer = 40 + 2;
answer
```

The file is compiled as a zero-argument bytecode function named after its
source. Its result expression becomes the function result.

UTF-8 input is accepted, while identifiers in the MVP use ASCII letters,
digits, and underscores. `#` starts a line comment. A first-line shebang such
as `#!/usr/bin/env -S xl run` is therefore ordinary comment trivia.

## Literals

The surface language includes:

```text
42                       # Int
3.5                      # Float
"hello"                  # String
b"bytes"                 # Bytes
'Ready                   # Atom
[1, 2, 3]                # Array
(1, "two")               # Tuple
{name: "Ada", age: 36}   # Dict
```

Dict keys are identifiers or string literals. The compiler emits canonical
Dict construction; source field order is not observable. Duplicate fields are
a compile error.

Parentheses around one expression group it. A one-element tuple requires a
trailing comma: `(value,)`. Empty `()` is an empty Tuple. Empty `{}` is an empty
Dict.

## Blocks and bindings

A block contains zero or more bindings followed by a result expression:

```text
{
    let subtotal = price * count;
    let tax = subtotal * rate;
    subtotal + tax
}
```

Bindings are lexical and immutable. Shadowing is allowed in nested scopes.
There are no assignment statements. An empty block is not part of the MVP.

A named function declaration is binding sugar:

```text
fn add(a, b) { a + b }
```

means:

```text
let add = fn(a, b) { a + b };
```

Self-recursive and mutually recursive bindings are deferred until a later RFC.

## Functions and calls

Closures use a unified `fn` syntax:

```text
fn(value) { value + 1 }
```

They capture referenced lexical bindings by immutable value. Calls evaluate
the callee first, then arguments from left to right. Arity mismatch and calling
a non-function are runtime errors.

The bytecode VM gains closure creation, call frames, and shared instruction
budget accounting across frames. Function parameters occupy the first
registers and captures follow them.

## Operators

The MVP operators, from stronger to weaker binding, are:

```text
-value
*, /
+, -
<, ==
|>
```

`+`, `-`, `*`, `/`, `<`, and `==` compile to RFC 0001 instructions. `+` remains
numeric; string concatenation is a library operation. Unary `-` is numeric.

Amendment: comparison operators share one precedence level. Future comparison
operators (`<=`, `>=`, `>`, and `!=`) join that level. The reserved full order
is prefix, multiplicative, additive, comparison, bitwise `&`, bitwise `|`,
logical `&&`, logical `||`, then pipeline. Dict merging has no operator.
Comparisons are non-associative: chaining different or identical comparison
operators requires explicit parentheses.

## Conditionals

Conditionals are expressions and require both branches:

```text
if condition {
    left
} else {
    right
}
```

The VM accepts only `'True` and `'False` as the condition. Both branches write
to one result register, and only the selected branch is evaluated.

## Pattern matching

The MVP supports wildcard, binding, literal, and tuple patterns:

```text
match value {
    ('Ok, result) => result,
    ('Err, _) => fallback,
}
```

Patterns are attempted from top to bottom. Bindings exist only in their arm.
Failure to match any arm is a runtime error. The MVP does not yet perform
static exhaustiveness checking.

Array, Dict, or-pattern, guard, and rest patterns are deferred. Matching a
tagged tuple uses ordinary tuple length, element access, and atom equality.

## Pipeline operator

`|>` is left associative and has the lowest binary precedence:

```text
value |> f |> g
```

elaborates to `g(f(value))`.

When its right side is a call, the left value becomes the first argument:

```text
value |> transform(option)
```

elaborates to `transform(value, option)`. The left expression is evaluated
exactly once. Placeholders and non-first-argument insertion are deferred.

## Field access

`value.field` performs Dict field lookup. A missing field or non-Dict receiver
is a runtime error. Optional access belongs in the core library rather than in
special syntax.

## Diagnostics

The lexer and parser track byte offsets plus one-based line and column
locations. Frontend errors include a source name and location. Runtime errors
continue to report function and bytecode instruction until source maps are
added.

## Deferred work

- type annotations and type declarations;
- recursive bindings and tail calls;
- static exhaustiveness and unreachable-arm checking;
- short-circuit Boolean operators;
- broader patterns and destructuring bindings;
- source maps and stack traces;
- string interpolation;
- standard-library and native functions.

## Implementation plan

Add lexer, AST, parser, and compiler modules to the `xl` crate. Extend bytecode
and VM values with immutable closures and calls. Expose a `compile_source`
function and a convenience `run_source` function for tests and later CLI use.

## Acceptance criteria

1. Literals and expression precedence compile and execute correctly.
2. Blocks enforce lexical immutable binding and report unknown names.
3. Closures capture outer values and calls enforce arity.
4. `if` evaluates only its selected branch and preserves strict Boolean rules.
5. Tuple/tagged-tuple patterns bind values and fall through in source order.
6. A non-exhaustive match fails with a dedicated runtime error.
7. Pipelines elaborate to first-argument function calls and evaluate their left
   side once.
8. Dict field access works independently of source field order.
9. Lexer, parser, compiler, and runtime failures are covered by tests.
10. `cargo test --workspace` and strict Clippy pass.

## Implementation result

Implemented in the `xl` crate with lexer, parser, AST, compiler, immutable
closures, nested VM call frames, tuple-pattern lowering, and source execution
helpers. The acceptance suite passes under `cargo test --workspace`, and strict
Clippy reports no warnings.
