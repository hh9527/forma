# RFC 0018: Explicit Placeholder Application

- Status: Implemented

## Summary

XL supports explicit partial application through a visibly distinct call
section introduced by `\(`:

```xl
f\(_, 1)
f\(_1, 1, _0)
```

These call sections elaborate to ordinary closures:

```xl
fn(arg0) { f(arg0, 1) }
fn(arg0, arg1) { f(arg1, 1, arg0) }
```

Bare `_` creates a fresh parameter for each occurrence in source order.
Indexed `_N` refers to generated parameter `N`, allowing reorder and reuse.
The two forms cannot be mixed in one call section.

This RFC does not add automatic curry, runtime partial-callable objects,
under-application, over-application, or general expression holes.

## Motivation

RFC 0017 defines `value |> function` as uniform reverse application. Configured
pipeline stages therefore need an ordinary expression that evaluates to a
unary function. Explicit closures are correct but verbose:

```xl
values |> fn(items) { arrays.map(items, normalize) }
```

A call section preserves the same semantics while exposing where pipeline data
will be placed:

```xl
values |> arrays.map\(_, normalize)
base |> dicts.merge\(_, overrides)
```

Unlike automatic curry, an ordinary call with too few arguments remains an
error. Partial application occurs only when the programmer writes a visible
placeholder. Indexed placeholders additionally express argument permutation
and duplication without privileged helpers such as `flip`.

## Lexical forms

The lexer recognizes a section opener and two dedicated placeholder token
categories:

```text
SectionLParen     = "\("
Placeholder        = "_"
IndexedPlaceholder = "_" [0-9]+
```

They are not identifiers. `_` and `_N` cannot be declared as binding or
parameter names. Names containing other characters remain identifiers:

```xl
let _ignored = value; # valid
let _0 = value;       # invalid
```

The numeric suffix is decimal and zero-based. It is decoded during semantic
lowering with checked conversion; an index too large for the implementation's
node limits is diagnosed rather than truncated or panicked.

Dedicated tokens keep the lossless CST explicit and avoid deciding from an
Identifier's text whether it is a placeholder. Outside strings, a bare `\` is
otherwise invalid, so `\(` is unambiguous and does not overlap string escapes.

## Call-section syntax

A normal call and a call section are separate grammar forms:

```text
call_expr:    expression '(' arguments ')'
section_expr: expression '\(' section_arguments ')'
```

A placeholder is accepted only as a direct argument of a call section:

```xl
f\(_, value)
object.method\(_1, value, _0)
```

It is not a general expression and is rejected where no call directly owns it:

```xl
let value = _;
f(_, value);
f({ item: _ });
f(if condition { _ } else { value });
```

The restriction gives every placeholder one unambiguous owning call and keeps
the initial grammar, lowering, and diagnostics local. A later RFC may define
general expression holes if a compelling use appears.

Nested call sections compose normally because each placeholder belongs to its
nearest section opener:

```xl
f(g\(_), value)
```

Here `g\(_)` elaborates to a unary closure which is then passed as an ordinary
argument to `f`.

A call section must contain at least one direct placeholder. `f\(value)` is a
located frontend error rather than an alternative spelling of either `f(value)`
or `fn() { f(value) }`.

## Bare placeholders

Each bare `_` creates one distinct generated parameter, ordered by occurrence:

```xl
f\(_, fixed, _)
```

elaborates to the semantic equivalent of:

```xl
fn(arg0, arg1) { f(arg0, fixed, arg1) }
```

Bare placeholders cannot refer to the same parameter twice; indexed form is
used for reuse.

## Indexed placeholders

`_N` references generated parameter `N`, independent of occurrence order:

```xl
f\(_1, fixed, _0)
```

elaborates to:

```xl
fn(arg0, arg1) { f(arg1, fixed, arg0) }
```

An index may occur multiple times:

```xl
equal\(_0, _0)
```

elaborates to a unary closure. The set of referenced indices must be exactly
the continuous range `0..=max`; gaps are errors:

```xl
f\(_2, _0) # error: missing _1
```

Continuity prevents a call section from silently creating unused parameters.

## Mixing modes

One call section may use bare placeholders or indexed placeholders, but not
both:

```xl
f\(_0, _) # error
```

Nested call sections are independent. Mixing and index continuity are checked
separately for each owning call.

## Elaboration and evaluation

Elaboration produces the existing semantic `Closure` and `Call` nodes. It does
not create a new runtime value category or opcode. Generated parameter names
are compiler-internal and cannot collide with source identifiers.

The elaboration is semantically equivalent to a closure written at that source
position. Consequently:

- the callee and every non-placeholder argument are evaluated when the
  generated closure is invoked, not when it is created;
- free variables are captured through ordinary closure capture;
- each invocation reevaluates the call body;
- arity, fuel, tail-call, stack, quota, trace, and promotion behavior is exactly
  ordinary closure behavior;
- bytecode and native callees require no special handling.

For example:

```xl
let apply = f\(_, make_value());
apply(a);
apply(b);
```

evaluates `make_value()` once per invocation, matching the explicit closure.

## Pipelines

Call sections are ordinary function-valued expressions under RFC 0017:

```xl
value |> f\(_, option)
```

first elaborates `f\(_, option)` to a unary closure, then applies `value` to it.
No pipeline-specific placeholder handling or argument insertion occurs.

Indexed placeholders permit explicit permutation:

```xl
value |> consume\(_1, _0)
```

Here the section is binary, so the pipeline supplies only its first generated
argument and the ordinary arity checker reports that two arguments are
required. The useful pipeline stage must itself be unary; indexing does not
implicitly curry the generated closure.

## Locations and diagnostics

The generated closure carries the complete call-section location. Generated
parameters and replacement variable reads carry the corresponding placeholder
locations. The inner call retains the original call location, so runtime errors
point to the source call section rather than synthetic text.

Frontend diagnostics cover:

- placeholder outside a direct call-section argument;
- a placeholder in an ordinary `(...)` call;
- a `\(...)` section with no placeholder;
- mixing bare and indexed placeholders;
- missing indexed placeholders;
- numeric index overflow;
- use of reserved `_` or `_N` where an identifier is required.

Parser recovery retains placeholder tokens in the CST and continues after all
of these errors.

## Static analysis

Because lowering produces an ordinary closure, the focused analyzer observes
the generated parameter count and existing call result. No new type category
is introduced. Parameter types may remain `Any` where the current analyzer
cannot propagate the callee's contract backward into the section.

## Rejected alternatives

### Automatic curry on under-application

It makes accidental arity mistakes silently produce functions and requires a
runtime partial-callable representation spanning bytecode, native functions,
core functions, promotion, and tail calls. Explicit placeholders preserve
ordinary arity errors.

### Use `?N` or `@N`

`_N` forms one visual family with bare `_`. `?` remains available for possible
error propagation or optional access, and `@` remains available for attributes
or pattern aliases.

### Allow arbitrary expression holes

General holes require ownership and scope rules across every expression form.
Nearest-call ownership covers nested call sections while keeping holes out of
Dicts, blocks, conditionals, and other arbitrary expression positions.

### Evaluate fixed arguments when creating the section

That would require hidden captures and would not be equivalent to the stated
ordinary closure expansion. XL uses the direct syntactic elaboration semantics.

## Deferred work

- general expression holes;
- backward parameter-type inference from function contracts;
- automatic curry or runtime partial callable values;
- source-level names for generated parameters;
- section syntax for operators or field access.

## Implementation plan

1. Add dedicated lossless lexer tokens and separate grammar slots for ordinary
   calls and `\(...)` call sections.
2. Add tolerant CST argument views that distinguish expressions, bare holes,
   and indexed holes.
3. Elaborate call sections into existing located closure and call AST nodes,
   using collision-proof internal parameter names.
4. Diagnose invalid placement, mixed modes, gaps, and overflow while preserving
   CST recovery.
5. Add parser, compiler, capture, pipeline, native/core-call, location, and
   malformed-input tests.

## Acceptance criteria

1. `f\(_, fixed)` behaves exactly like `fn(arg) { f(arg, fixed) }`.
2. Multiple bare placeholders create distinct parameters in occurrence order.
3. Indexed placeholders support reordering and repeated use.
4. Mixed modes, index gaps, overflow, and non-direct placement produce located
   diagnostics without panics.
5. Non-placeholder arguments are reevaluated for every invocation.
6. Sections capture free variables and call bytecode, native, and core functions
   through existing machinery.
7. `value |> f\(_, option)` follows uniform reverse application with no pipeline
   special case.
8. CST reconstruction and tolerant parsing retain all placeholder source text.
9. Existing calls, closures, pipelines, VM behavior, and CLI tests remain
   unchanged.

## Implementation result

Logos now emits dedicated `SectionLParen`, `Placeholder`, and
`IndexedPlaceholder` tokens. Lelwel gives ordinary calls and explicit call
sections separate grammar rules, while retaining bare `_` as the existing
match wildcard. Lossless CST reconstruction preserves the complete `\(` and
placeholder source text.

The semantic lowerer validates each section independently, rejects sections
without holes, mixed modes, index gaps, overflow, ordinary-call holes, and
reserved placeholder names, then produces only existing located `Closure`,
`Block`, `Call`, and `Variable` nodes. Generated names contain a source-
unrepresentable prefix, so they cannot capture or shadow user bindings.

Tests cover bare multi-argument sections, indexed reorder and reuse, nested
sections, pipelines, free evaluation timing, native and `core:array` calls,
match-wildcard compatibility, malformed sections, source-positioned
diagnostics, and lossless CST reconstruction. No AST variant, LIR operation,
opcode, runtime callable, heap representation, or native ABI change was
required.
