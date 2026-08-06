# RFC 0124: Option and Result propagation

- Status: Proposed
- Depends on: RFC 0054, RFC 0067 through RFC 0088, RFC 0118 through RFC 0123

## Summary

Forma adds postfix `?` for linear success-path code over two fixed structural
Enum shapes:

```forma
def load: Fn(Input) -> Result(Output, BlameError) = fn(input) {
    let decoded = decode(InputType, input)?;
    let checked = validate(decoded)?;
    'Ok(transform(checked))
};
```

`Option(A)?` evaluates to `A` or propagates `'None`. `Result(A, E)?` evaluates
to `A` or propagates the original `'Err(error)`. Propagation targets the
nearest Function body or module body; ordinary lexical blocks are transparent.

The construct is dedicated syntax. It is not an ordinary Function, exception,
effect, user-defined macro, general return statement, or trait-based protocol.

## Structural families

Forma recognizes exact closed Enum descriptors:

```text
Option-shaped = Enum {
    None: no payload,
    Some: A,
}

Result-shaped = Enum {
    Err: E,
    Ok: A,
}
```

Because Forma's Enums are structural, a user-defined type with the same exact
shape has the same behavior. Restricting `?` to nominal standard-library names
would require type identity the language does not otherwise possess.

Standalone Atom or Tagged singleton descriptors are not enough. An authored
annotation or expected type may widen a concrete `'Some(value)` or
`'Ok(value)` into the corresponding closed Enum before `?` is applied.

No other tag set participates. In particular, arbitrary Tagged values, Bool,
and larger Enums do not acquire propagation semantics.

## Dynamic semantics

The operand is evaluated exactly once. For Option-shaped values:

```forma
option?
```

behaves as:

```forma
match option {
    'Some(value) => value,
    'None => return-boundary 'None,
}
```

For Result-shaped values:

```forma
result?
```

behaves as:

```forma
match result {
    'Ok(value) => value,
    'Err(error) => return-boundary 'Err(error),
}
```

Failure returns the original operand, preserving its payload and structural
provenance. Success selects the existing Tagged payload and therefore narrows
to the child provenance. No intermediate public value is constructed.

## Return boundaries

Every compiled Function body is a propagation boundary. Nested Functions own
their `?`; a caller's boundary is never captured. Ordinary blocks, match arms,
if branches, and interpolation expressions remain transparent.

The module body is also a boundary. A module using `?` must publish an
Option-shaped or Result-shaped result just like a Function. Imports and module
caching observe only the final propagated value; partial module publication is
unchanged.

## Static semantics

The operand must resolve to exactly one supported structural family. The type
of the `?` expression is its success payload `A`.

Every `?` in one boundary must use the same family. For Option, the boundary
result must be Option-shaped. For Result, each operand error `E1` must be
assignable to the boundary error `E2`. An unannotated boundary may infer a
common Result error type from its propagation sites.

The boundary's ordinary success expression may already have the complete Enum
type or a concrete success constructor:

```text
'Some(B) + Option propagation => Option(B)
'Ok(B)   + Result(E) propagation => Result(B, E)
```

This contextual widening is local to the return boundary. Failure-only final
expressions require an expected boundary type because they do not determine a
success payload.

Option and Result do not convert implicitly. Callers use explicit ordinary
functions such as `ok_or` or `ok` when changing channels.

## Inference and diagnostics

Each Function/module inference scope accumulates a private propagation
requirement. Entering a nested Function starts a fresh requirement; entering a
plain block does not. After inferring the body, the requirement checks or
widens the body result before the Function/module type is published.

Diagnostics point at `?` and distinguish:

- an operand that is not an exact supported closed Enum;
- incompatible Option/Result sites in one boundary;
- a boundary result with the wrong family;
- a Result error not assignable to the boundary error;
- a failure-only result without enough success-type information; and
- an unresolved operand whose propagation family cannot be chosen.

Hover and expression facts report the success payload type at the postfix
expression. Cancellation remains checked through ordinary inference and
compilation traversal.

## Compiler and VM

The compiler evaluates the operand, tests the fixed success tags `Some` and
`Ok`, reads a successful Tagged payload, and emits the existing Return operation
for the failure path. Since static checking guarantees one of the two families,
no new runtime protocol, continuation, value kind, exception unwinding, or VM
opcode is required.

Tail-position `?` uses the same lowering: failure returns the original family
value, while success becomes the boundary's ordinary returned payload only if
the surrounding authored result makes that type valid.

## Acceptance criteria

1. postfix `?` composes left-to-right with field/call/type-application postfixes
   and binds above binary operators and pipelines;
2. Option success unwraps Some and failure propagates None;
3. Result success unwraps Ok and failure propagates the original Err;
4. operands are evaluated exactly once;
5. ordinary blocks are transparent and nested Functions isolate boundaries;
6. module-level propagation produces an ordinary complete module result;
7. unannotated success constructors widen to the required structural family;
8. Result error assignability is checked at the authored `?`;
9. mixed families and unsupported shapes receive stable diagnostics;
10. expression and semantic facts expose the unwrapped success type;
11. failure and success provenance are preserved or narrowed respectively;
12. no new VM opcode, value kind, trait, effect, exception, or public Never;
13. quota, cancellation, strict/best-effort, and module publication behavior
    remain deterministic; and
14. full core, CLI, LSP, formatting, and strict static checks pass.

## Non-goals

- user-defined propagation protocols or a `Try` trait;
- implicit Option/Result conversion;
- general `return`, exceptions, throw/catch, or resumable effects;
- applying `?` to Any or Dyn through runtime tag inspection;
- propagation from native code without an ordinary Result/Option value;
- changing best-effort Never or failure lineage; or
- adding `if let`, `let else`, or other control-flow sugar.

## Implementation result

Pending.
