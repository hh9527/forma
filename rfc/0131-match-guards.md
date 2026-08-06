# RFC 0131: Match guards

- Status: Implemented
- Depends on: RFC 0121, RFC 0122, RFC 0130

## Summary

Match arms may add a Bool guard after the pattern. The guard runs only after
the pattern matches and its bindings exist; False continues with the next arm.
Guarded arms do not contribute to exhaustiveness or make later arms redundant.

No loop, mutable binding, break, continue, effect, or VM operation is added.

## Implementation result

Implemented across parsing, name resolution, type inference, elaboration, and
both ordinary and tail-position match compilation. Guards are checked as Bool
with pattern bindings in scope, then compiled as conditional fallthrough to the
next arm. They introduce no bytecode operation.

Coverage analysis ignores guarded arms when establishing exhaustiveness and
when deciding whether later arms are redundant. Prior unguarded coverage still
makes a later guarded arm unreachable.

Tests cover binding scope, false-guard fallthrough, Bool diagnostics,
short-circuit expressions inside guards, conservative exhaustiveness, and both
directions of guarded/unguarded redundancy.
