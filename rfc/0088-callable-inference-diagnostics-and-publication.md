# RFC 0088: Callable-inference diagnostics and publication

- Status: Proposed
- Depends on: RFC 0086, RFC 0087
- Tracking issue: https://github.com/hh9527/forma/issues/3

## Summary

Forma completes bounded callable-shape inference with stable failure ownership
and a cross-surface publication audit. Known static descriptors that are
neither Function nor Atom constructors are rejected as non-callable. Explicit
`Any` retains dynamic-call behavior. Inferred definition schemes and
monomorphic call instances follow the publication model established by RFC
0084.

This RFC closes the RFC 0085 phase. It does not add a general explanation
engine, new fact state, or runtime callable protocol.

## Call categories

After callee inference and RFC 0086 Function-shell construction, a call has
exactly one of four static categories:

| Callee descriptor | Behavior |
| --- | --- |
| `Function` | check exact arity, arguments, and expected result |
| `Atom` | construct one-payload Tagged value |
| explicit `Any` | retain dynamic call and `Any` result |
| every other completed descriptor | reject as statically non-callable |

An unbound `Inference` does not remain a fifth category: RFC 0086 immediately
binds it to an exact Function shell.

## Non-callable diagnostic

A statically known non-callable descriptor reports:

```text
cannot call value of type Int
```

The primary location is the call expression. Arguments are still analyzed for
independent recovery facts before the call fails when recovery supports that
ordering; complete analysis returns the call error and publishes no partial
call result.

This diagnostic does not apply to explicit `Any`. `Any` is the authored escape
hatch for values whose callability is known only at runtime.

## Existing call diagnostics

Known and inferred Functions use the same existing categories:

- exact-arity disagreement: `call expects N arguments, found M`;
- incompatible parameter or result evidence: ordinary unification conflict;
- unresolved owned variables: existing binding or generic-result completion
  failure;
- recursive incompleteness: existing monomorphic recursive-component failure;
- cancellation or stale revision: no publication.

The checker does not mention internal inference-variable IDs in a completed
diagnostic or fact.

## Publication model

An eligible higher-order definition publishes its `TypeScheme`:

```text
apply definition  for(A, B) Fn(Fn(A) -> B, A) -> B
```

Each reference and call publishes one monomorphic instance:

```text
apply(increment, 1) call  Int
```

The module runtime result may retain RFC 0084's explicitly erased Function
shape. That erased shape does not replace the authoritative scheme in module
interfaces, CLI type output, LSP hover, or workspace facts.

No `InferenceVariableId`, numeric-domain marker, unresolved Function position,
or provisional scheme reaches a complete `Analysis`, `TypeGraph`, module
interface, workspace snapshot, CLI response, or LSP response.

## Recovery, cancellation, and determinism

Recoverable analysis uses existing unavailable or conflicted fact states for a
failed call. It does not guess a Function shape after contradictory evidence
or publish a partially solved shape.

Cancellation and stale revisions retain the previous complete workspace
snapshot. Callable inference state is owned by the abandoned analysis and is
not shared.

Equivalent compatible evidence orders publish equal schemes and instances.
Equivalent conflicts retain the same diagnostic category. Existing source
ownership may place the primary label on the later conflicting use.

## Goals

1. reject statically known non-callable descriptors;
2. preserve explicit `Any` as the dynamic-call boundary;
3. keep arity, conflict, incompleteness, and non-callability distinct;
4. publish schemes at definitions and instances at calls;
5. prevent provisional callable shapes from crossing any semantic surface;
6. preserve recovery, cancellation, stale-revision, and determinism rules; and
7. close RFC 0085 without runtime changes.

## Non-goals

- proving runtime callability of `Any`;
- overloaded-call diagnostics or candidate explanations;
- multi-label constraint traces or a new diagnostic object model;
- warning when an explicit `Any` call may fail;
- field or method lookup inference;
- trait, capability, or implementation resolution;
- subtyping, coercion, flow narrowing, or Union-call distribution; or
- runtime callable wrappers or dispatch changes.

## Acceptance criteria

1. calling `Int`, `String`, Array, Dict, Tuple, Struct, metadata, or a completed
   non-Function descriptor fails statically;
2. the message names the completed non-callable descriptor;
3. explicit `Any` calls retain `Any` result behavior;
4. Function arity and unification diagnostics do not regress;
5. higher-order definitions expose stable schemes in analysis and tooling;
6. calls expose completed monomorphic instance facts;
7. module exports and imports preserve inferred higher-order schemes;
8. recovery publishes unavailable/conflicted state rather than a guessed shape;
9. cancellation and stale revisions retain the prior complete snapshot;
10. equivalent evidence order remains deterministic;
11. bytecode and VM behavior are unchanged; and
12. workspace tests and strict static checks pass.

## Implementation plan

1. split explicit `Any` from completed non-callable descriptors in call checking;
2. add descriptor-category, explicit-dynamic, arity, and conflict regressions;
3. add scheme/instance and module-interface probes;
4. reuse existing CLI/LSP, recovery, cancellation, and stale-revision boundary
   suites with a higher-order inferred scheme where useful;
5. run the full quality gate; and
6. record the final boundary in RFC 0085 and close the tracking issue.

## Rejected alternatives

### Preserve silent `Any` recovery for every non-Function

That conflates explicit dynamic behavior with a statically impossible call and
lets a typo erase surrounding inference. `Any` remains available when dynamic
callability is intentional.

### Add a `Callable` trait

The runtime has one Function representation plus Atom construction. No
user-selected implementation is involved, so trait search would solve a
different problem.

### Publish the inferred Function shell before completion

A provisional shell can have unresolved parameters or contradictory later
uses. Existing atomic publication rules remain authoritative.
