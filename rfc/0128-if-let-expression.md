# RFC 0128: If-let expression

- Status: Implemented
- Depends on: RFC 0118 through RFC 0123, RFC 0127

## Summary

Forma adds `if let pattern = value { ... } else { ... }` for a single
refutable pattern branch. Pattern bindings are visible only in the then branch.
Both branches remain ordinary value-producing blocks.

After typed pattern analysis, the construct elaborates to a two-arm match whose
fallback is `_`. It adds no runtime control-flow primitive.

## Acceptance criteria

1. all existing patterns are accepted and checked against the value type;
2. bindings are scoped to the then branch;
3. branches use ordinary result-type joining;
4. incompatible patterns receive the existing pattern diagnostic;
5. the value is evaluated once; and
6. elaboration produces an ordinary match without exhaustiveness diagnostics.

## Implementation result

Implemented across grammar/CST lowering, HIR pattern scopes, typed pattern
analysis, and typed elaboration. The runtime compiler rejects residual IfLet
nodes and receives only the generated two-arm match.

Tests cover successful and fallback selection, branch binding scope, and
incompatible pattern diagnostics. The full workspace suite passed with 20 LSP
tests, 15 CLI tests, and 355 core tests passing with one ignored.
