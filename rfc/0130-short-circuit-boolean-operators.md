# RFC 0130: Short-circuit boolean operators

- Status: Implemented
- Depends on: RFC 0127

## Summary

Forma adds `&&` and `||` over structural Bool values. Both operands must be
Bool and the right operand is evaluated conditionally. Typed elaboration
rewrites them to ordinary `if`; no VM operation is added.

`&&` binds tighter than `||`; both bind below comparisons and above pipeline.

This control flow does not mutate or rebind anything. Forma does not add
`while`, mutable bindings, `break`, or `continue`; repetition remains explicit
state transformation through recursion and combinators.

## Implementation result

Implemented in lexer/parser precedence, Bool checking, and typed elaboration to
ordinary If nodes. Tests cover both short-circuit directions, precedence, and
operand diagnostics. No compiler, LIR, bytecode, or VM boolean operation was
added.
