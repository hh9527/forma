# RFC 0190: Host-observed domain validation

- Status: Implemented
- Depends on: RFC 0189
- Amends: RFC 0185

## Summary

Migrate the intelligent-reporting experiment from explicit diagnostic arrays
to Host-observed diagnostics. Domain functions use ordinary return types for
data dependencies and report known rule violations independently:

```forma
type DimensionLowerer =
    Fn(Measure) -> Option(GroupRequirement);

def rejected_requirement = fn(dimension, message) {
    let ignored = emit_error!(message, dimension);
    'None
};
```

`Option` means that this local lowering produced no value. `emit_error!` means
that the domain rule has authoritatively rejected the authored intent. Neither
meaning is encoded as `Result`, and diagnostics are not returned, concatenated,
or observable from Forma code.

## Compilation boundary

The public compiler becomes:

```forma
compile: Fn(ReportIntent) -> Option(ExecutionPlan)
```

A complete lowering returns Some. Missing local requirements or failed global
proofs return None. Independently, every reported Error causes the Host to
reject final evaluation, so no partial plan can cross the publication boundary.
The closed Forma world has no side effects that could execute a transient plan.

This amends RFC 0185's description of diagnostics being returned beside
`plan: None`: they are now Host-observed events, while the value-level result is
only `Option(ExecutionPlan)`.

## Causality

The old invalid fixture selected two measures and then arbitrarily used the
first one to produce measure-specific dimension errors. Those errors were not
independent of the unresolved measure choice. The fixture now uses one measure
and retains four genuinely independent failures. A separate multiple-measure
fixture demonstrates that blocked measure-dependent checks are not guessed.

Ordinary array combinators are sufficient because recoverable domain rejection
uses `emit_error!` and returns Option. `raise!` remains appropriate only where a
dependency path cannot produce any meaningful continuation. This experiment
therefore does not justify array-element VM recovery or a general accumulation
effect.

## Acceptance criteria

1. `RequirementCompilation` and value-level diagnostic arrays are removed;
2. dimension lowerers return `Option(GroupRequirement)`;
3. invalid dimensions, relationship proofs, and ordering rules report errors
   without stopping independent checks;
4. the invalid fixture publishes four independent diagnostics in one workspace
   evaluation and no successful plan;
5. valid fixtures retain their SQL and SQLite results;
6. the Host-facing adapter statically checks `Option(ExecutionPlan)` directly;
7. no fallback measure is used to invent diagnostics after measure selection
   fails.

## Implementation result

The ontology now separates local availability from reporting. Dimension
lowering, relationship proof, and order validation run through ordinary array
combinators; Error events remain outside Forma values. The public compiler and
Host adapter use `Option(ExecutionPlan)` directly. A workspace recovery test
proves that four independent domain errors are retained, while the existing
valid SQL fixtures continue to execute unchanged.

The CLI `run` command currently renders the first fatal diagnostic from a
failed execution even though workspace recovery and the LSP retain the complete
event set. Rendering the complete set in CLI failure output is a Host
presentation improvement, not a missing language accumulation mechanism.
