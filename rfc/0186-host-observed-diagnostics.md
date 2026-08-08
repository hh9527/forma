# RFC 0186: Host-observed diagnostics

- Status: Implemented
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
3. RFC 0189 adds the four convenience intrinsics and records the precise
   boundary of ordinary control-flow and retained-binding recovery;
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

## Completion

RFCs 0187 through 0190 are implemented. The intelligent-reporting experiment
now reports independent domain violations without returning or concatenating
diagnostic arrays. Ordinary `Option` values represent unavailable local
lowering results, while write-only Host events carry authoritative violations.
The experiment did not require an accumulation effect or generic recovery
inside array combinators. Workspace recovery retains all four independent
fixture errors; CLI rendering of the complete event set remains a Host-facing
presentation improvement.

## Future scoped observation

The model intentionally leaves room for a future explicit observation
boundary:

```forma
call_with_diagnostics!(compiler(intent))
```

This would be compiler-known syntax at the same privilege level as
`interpreter!`, not an ordinary function, user-defined macro, or implicit call
mode. Its operand would be syntactically restricted to a call expression. The
intrinsic could establish a child diagnostic scope, evaluate that call once,
and reify the reports produced along its dynamic call path for the caller.
This is useful when one intent compiler embeds another and must deliberately
decide whether to preserve, translate, or raise the nested diagnostics.

No such intrinsic is implemented by this RFC. A future RFC must define at
least:

1. the typed result protocol, including the distinction between a value with
   Info/Warn reports and rejection by Error;
2. whether captured Error reports are consumed or automatically propagated;
3. the treatment of `raise!` inside the child scope;
4. which failures remain uncatchable, including cancellation, fuel, stack,
   and allocation exhaustion;
5. deterministic ordering, nesting, provenance, and cache replay;
6. why the concrete use case cannot remain at the current Host boundary.

Introducing this form would explicitly amend the current write-only invariant.
It must remain a narrow, call-site-selected bridge rather than grow into
user-defined handlers or a general effect system.
