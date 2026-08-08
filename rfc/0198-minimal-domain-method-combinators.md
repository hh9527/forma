# RFC 0198: Minimal domain-method combinators

- Status: Implemented
- Depends on: RFC 0197

## Summary

Extract the smallest cross-industry orchestration functions already exercised
by intelligent reporting and the GCC wrapper. The module is an ordinary Path
dependency and contains no reporting, SQL, toolchain, package, target, or argv
vocabulary.

The initial functions are:

```text
lower_independent   map independent A -> Option(B) rules without fail-fast
completed           retain the reliable values after all rules ran
collect_complete    publish an Array only when every required rule succeeded
compose             connect two typed Option-producing lowering stages
or_else             let a domain choose its own terminal fallback
finalize            conditionally publish one complete value
```

## Type boundary

Telora currently has rank-1 generic functions but no user-defined parameterized
type constructors. This RFC therefore does not invent a generic
`Capability(K, A, B)` record. Industry libraries define their own readable
capability records and use generic orchestration functions across them.

This is an explicit language boundary, not a reason to add higher-kinded types.
The experiment first measures how far functions alone go.

## Acceptance criteria

1. the shared module imports only ordinary standard combinators;
2. reporting uses it for independent measure/dimension lowering and reliable
   result collection;
3. the GCC wrapper uses it to separate generic optional fallback from its
   domain-specific missing package/TARGET failure;
4. existing report SQL, diagnostics, and GCC dry-run output do not change;
5. no VM, analyzer, or Host special case is added.

## Implementation result

`examples/domain-method/src/method.telora` is a normal dependency used by both
applications. Reporting retains its concrete `MeasureCapability` and
`DimensionCapability`; GCC retains package and command policy. The shared
module owns only higher-order control structure.

The result is intentionally modest. `lower_independent`, `completed`, and
`or_else` remove repeated, semantically important orchestration. `compose`,
`collect_complete`, and `finalize` establish the candidate surface for the next
industry refactors but are not claimed valuable until exercised there.
