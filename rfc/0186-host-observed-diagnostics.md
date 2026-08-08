# RFC 0186: Host-observed diagnostics

- Status: Accepted
- Amends: RFC 0105, RFC 0112, and RFC 0167

## Summary

Add a narrow diagnostic observation channel without introducing a general
effect system. `BlameError` remains ordinary data. Forma code explicitly
reports that data or raises it; the Host alone chooses strict or best-effort
evaluation after a raised failure.

```forma
let error = blame!("incompatible values", left, right);
let error = report('Warn, error);
raise!(error);
```

The primitive surface is:

```text
blame!(String, subjects...) -> BlameError
report(Severity, BlameError) -> BlameError
raise!(BlameError) -> Never
```

Convenience syntax composes those primitives:

```text
emit_info!(message, subjects...)
emit_warn!(message, subjects...)
emit_error!(message, subjects...)
fail!(message, subjects...)
```

`report('Error, error)` invalidates the final evaluation but permits the
current validation code to continue. `raise!(error)` reports Error and prevents
the current expression from producing a value. Best-effort may explore
independent siblings but can never publish a partial value.

## Child sequence

1. RFC 0187 changes blame to message-first variadic subjects and replaces
   `reraise!` with `raise!`;
2. RFC 0188 adds severity, the ordinary `report` BIF, diagnostic events, and
   final-success invalidation;
3. RFC 0189 adds the four convenience intrinsics and Host-controlled
   best-effort sibling exploration;
4. RFC 0190 migrates the reporting experiment away from explicit diagnostic
   arrays and records the resulting boundary.

## Invariants

1. `Result.Err` remains ordinary data and is never interpreted by the Host;
2. strict and best-effort have identical success/failure and successful values;
3. best-effort changes only the deterministic set of discovered diagnostics;
4. Info and Warn do not invalidate success;
5. Error invalidates success even when reporting code continues;
6. a raised dependency never becomes a shortened collection or fallback value;
7. diagnostics are write-only and cannot be observed by Forma code;
8. cache hits must eventually replay cached diagnostic events;
9. no user-defined handlers, ports, effect rows, or partial values are added.

## Non-goals

- string-internal blame spans;
- a general algebraic effect or logging system;
- dynamically selecting evaluation policy from Forma code;
- making fatal Host/runtime failures constructible as domain diagnostics.
