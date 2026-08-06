# RFC 0126: Sourced panic

- Status: Implemented
- Depends on: RFC 0079 through RFC 0088, RFC 0125

## Summary

Forma adds `panic!(message)` for an unrecoverable condition in authored logic.
The message must be String and the expression has type Never.

Panic is a contextual intrinsic rather than a Function: the compiler preserves
its exact source origin. It is distinct from Result and BlameError and does not
provide catch or recovery in Forma code.

## Runtime semantics

The message is evaluated once. The VM raises a dedicated recoverable Panic
runtime failure carrying the message and authored origin. Strict execution
returns that failure immediately. Best-effort module evaluation records it in
the existing failure arena and propagates Never through dependent evaluation
units. No ordinary runtime Never value is constructed.

## Acceptance criteria

1. `panic!` accepts exactly one String expression and has type Never;
2. its argument is evaluated once;
3. strict execution reports Panic with the authored call site;
4. best-effort evaluation uses existing failure lineage;
5. Never composes with branches and generic inference; and
6. no catch, exception object, or ordinary failure value is added.

## Implementation result

Implemented as a contextual intrinsic, Never-typed AST expression, dedicated
LIR/bytecode Panic operation, and recoverable VM failure kind. The VM accepts
only the statically checked String message, retains the authored instruction
origin, and hands recoverable failures to the existing best-effort module
failure arena.

Tests cover branch typing, strict message and source reporting, arity and type
diagnostics, and best-effort deduplication without cascading diagnostics. The
full workspace suite passed with 20 LSP tests, 15 CLI tests, and 353 core tests
passing with one ignored.
