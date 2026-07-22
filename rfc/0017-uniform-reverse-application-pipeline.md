# RFC 0017: Uniform Reverse-Application Pipeline

- Status: Accepted for implementation

## Summary

XL defines the pipeline operator by one expression rule:

```text
value |> function
```

is exactly equivalent to:

```text
function(value)
```

The right operand is always evaluated as an ordinary expression and then
called with the left value as its sole argument. A call expression on the right
is not treated as a syntactic argument-insertion template.

This RFC supersedes the pipeline elaboration in RFC 0002. Automatic currying,
partial application, placeholders, and compatibility rewriting are deferred.

## Motivation

RFC 0002 gives `|>` two related but distinct elaborations:

```xl
value |> f          // f(value)
value |> f(option)  // f(value, option)
```

The second form does not preserve the ordinary meaning of `f(option)` as an
expression. Its AST is inspected and mutated by the pipeline lowerer. This is
convenient for fixed-arity APIs but makes `|>` a call-syntax macro rather than
reverse function application.

XL's expression-oriented functional core benefits from the smaller rule. A
pipeline should compose with any expression that evaluates to a function,
including a variable, field access, conditional, block, closure, or eventually
a partially applied call, without inspecting that expression's syntax.

## Semantics

For expressions `left` and `right`:

```xl
left |> right
```

lowers to the same semantic AST shape as:

```xl
right(left)
```

`left` is evaluated exactly once. Under the VM's existing call evaluation
order, `right` is evaluated before the resulting call argument window is
invoked, just as it is in the equivalent explicit call.

Pipelines remain left-associative and lower than all other expression
operators:

```xl
value |> f |> g
```

is equivalent to:

```xl
g(f(value))
```

## Calls on the right

A call on the right retains its ordinary meaning:

```xl
value |> factory(option)
```

is equivalent to:

```xl
factory(option)(value)
```

XL does not yet partially apply functions. Therefore, if `factory` requires
more arguments than `option` supplies, the inner call reports the ordinary
arity error. Code that needs configuration before curry support can write an
explicit closure:

```xl
value |> fn(item) { transform(item, option) }
```

Existing code that used `value |> transform(option)` as first-argument
insertion must use either the explicit call `transform(value, option)` or such
a closure.

## Core-library consequence

Existing core functions retain their fixed-arity APIs in this RFC. For
example:

```xl
arrays.map(values, callback)
dicts.merge(base, overrides)
```

They may appear in a pipeline through an explicit unary closure:

```xl
values |> fn(items) { arrays.map(items, callback) }
```

A later curry RFC may make configured functions naturally return unary
functions and enable forms such as:

```xl
values |> arrays.map(callback)
value |> debug.dbg_with("loaded")
```

Those forms are intentionally not valid shorthand yet.

## Diagnostics and locations

The generated call retains the complete pipeline expression location, matching
the current source-origin behavior. Errors while evaluating the right operand
retain that operand's own nested locations. Arity errors are not rewritten into
pipeline-specific messages.

## Rejected alternatives

### Retain first-argument insertion

This matches Elixir-style pipeline ergonomics, but makes a call expression on
the right a special syntactic template rather than a value-producing
expression.

### Add automatic curry in the same RFC

Partial application affects bytecode and native arity checks, closure capture,
tail calls, static analysis, and function contracts. It deserves a separate
runtime and language boundary rather than being hidden inside a parser change.

### Add placeholders now

Placeholders solve argument positioning but introduce another pipeline-only
syntax and do not establish ordinary expression composition.

### Rewrite known core calls only

Making pipeline behavior depend on callee identity would privilege the core
library and prevent user functions from following the same rules.

## Deferred work

- automatic curry and partial application;
- placeholder syntax;
- core-library data-last or curried API variants;
- function-composition helpers;
- multi-value pipeline behavior.

## Implementation plan

1. Replace call-shape-sensitive parser elaboration with an unconditional call
   of the right expression using the left expression as its only argument.
2. Replace first-argument-insertion tests with reverse-application tests over
   variables, closures, field access, and call-valued expressions.
3. Update active documentation and examples to avoid configured pipelines
   until partial application exists.
4. Preserve all source locations, precedence, and evaluation behavior outside
   pipeline elaboration.

## Acceptance criteria

1. `value |> f` and `f(value)` produce the same result.
2. `value |> fn(item) { ... }` calls the closure with `value` once.
3. `value |> object.factory(option)` means
   `object.factory(option)(value)`, without AST argument insertion.
4. Chained pipelines remain left-associative.
5. Former insertion syntax produces the same result or arity error as its
   explicit nested-call equivalent.
6. Pipeline errors retain useful source positions.
7. Existing non-pipeline parser, compiler, VM, core-library, and CLI behavior
   remains unchanged.
