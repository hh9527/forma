# RFC 0125: Function value return

- Status: Implemented
- Depends on: RFC 0067 through RFC 0088, RFC 0124

## Summary

Forma adds `return expression` as an early normal return from the nearest
Function boundary. A trailing semicolon is accepted when return is the final
expression of a block:

```forma
def choose = fn(condition, value, fallback) {
    if condition { return value } else { fallback }
};
```

`return` is an expression with internal type Never. It is not an exception,
failure, effect, labelled jump, or module return.

## Semantics

The operand is evaluated once and returned from the nearest enclosing
Function. Ordinary blocks, `if`, and `match` are transparent. A nested Function
starts a fresh return boundary. Module-level return is rejected.

All explicit return operands and the reachable tail expression constrain the
Function result. An authored result contract is authoritative. An unannotated
Function infers the common result from both sources; Never branches do not
widen that result.

Compilation uses the existing Return operation and VM instruction. No opcode,
runtime value, unwinding mechanism, or public control-flow protocol is added.

## Acceptance criteria

1. value return exits the nearest Function and evaluates its operand once;
2. nested Functions isolate return boundaries and ordinary blocks do not;
3. explicit returns and tail expressions participate in result inference;
4. incompatible returns receive a static diagnostic;
5. return has internal type Never in its local expression context;
6. module-level return is rejected; and
7. existing cancellation, quota, provenance, and tail-call behavior remains.

## Non-goals

- empty, labelled, module, break, or continue returns;
- exceptions, panic, catch, or recovery; or
- unreachable-code diagnostics.

## Implementation result

Implemented across syntax, AST/HIR traversal, generic inference, and compiler
lowering. Each Function inference scope collects explicit return values,
checks them against an authored result when present, and otherwise joins them
with the tail expression. Return expressions record Never locally. Compilation
uses the existing Return operation and nested Functions remain isolated.

Tests cover early and fall-through results, inferred return types, nested
Function isolation, module rejection, and incompatible authored results. The
full workspace suite passed with 20 LSP tests, 15 CLI tests, and 350 core tests
passing with one ignored.
