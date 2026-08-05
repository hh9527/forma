# RFC 0100: Source-aware best-effort evaluation

- Status: Implemented
- Depends on: RFC 0046, RFC 0056, RFC 0089 through RFC 0099

## Summary

Forma adds a Host-selected best-effort evaluation mode that can report several
independent errors from one module without exposing partial or invalid Forma
values. Successful values carry source provenance; failed computations produce
an internal `Never` outcome whose failure lineage suppresses cascaded
diagnostics and explains why requested output is unavailable:

```text
EvalOutcome =
    Value(Value, ProvenanceId)
    Never(FailureId)
```

The language also adopts contextual intrinsic syntax for operations that need
the authored type or source context. `interpreter!(operand)` replaces
`interpreter(operand)`, and `blame!(...)` can later construct source-aware
validation failures without pretending call-site capture is an ordinary
Function.

This is an umbrella RFC. RFCs 0101 through 0106 define the syntax, successful
value provenance, failed-value propagation, Host scheduling, Dyn/blame bridge,
and end-to-end CLI/LSP publication before this RFC becomes Implemented.

## Motivation

Forma evaluates data from files and lets ordinary user-space code verify and
transform it. A useful diagnostic must distinguish the source of the data from
the source of the rule:

```forma
import src from './a.json';

let a = src;
let a = verify(a)?;       # failures point into a.json

let b = transform(src);
let b = verify(b)?;       # failures point at the transform call
```

The operative rule is simple: an unchanged value retains its origin; a newly
computed value is attributed to the expression or authored call that generated
it. This rule must apply recursively. A new container has a generated root,
while children copied unchanged from imported data retain their more precise
origins.

Interactive analysis also needs to continue after a local evaluation failure.
Turning failures into ordinary `Result` values would change program semantics,
and language-level accumulation would force library authors to opt into an LSP
policy. Instead, the Host chooses whether evaluation is strict or best-effort.
In best-effort mode it records the root diagnostic, substitutes an unobservable
internal `Never`, propagates that outcome through dependent computation, and
continues independent work.

## One model, four channels

The implementation keeps four related but distinct structures:

1. **provenance** explains where a successful value or nested component came
   from;
2. **failure lineage** explains which failed values blocked a later operation;
3. **call stack** explains the runtime control path of an actual failure; and
4. **Host diagnostics** are the selected, ordered messages published to a user.

Provenance never represents failure. A `FailureId` never becomes a source
position or a Forma value. The call stack is not reconstructed from the data
dependency graph. Publication remains a Host decision rather than an evaluator
side effect.

Conceptually, failures live in a bounded arena:

```rust
enum FailureNode {
    Root { diagnostic: DiagnosticId },
    Propagated {
        operation: FailureOperation,
        location: Location,
        causes: SmallVec<FailureId>,
    },
}
```

A root failure produces one primary diagnostic. Dependent operations add
lineage nodes but do not emit duplicate diagnostics. When a requested output is
blocked, tooling may render a concise dependency trace from that output to the
root failure.

## Successful value provenance

Provenance follows value structure and these initial rules:

- imports assign original source provenance down to addressable fields,
  variants, and elements;
- aliases and unchanged returns preserve provenance;
- field, index, and pattern selection narrow to the selected provenance;
- computed scalars receive the generating expression's provenance;
- constructed containers receive a generated root while preserving provenance
  of unchanged children;
- generated values crossing a Function or native boundary are rebased to the
  authored call site; and
- module promotion and caching preserve canonical module identities rather
  than physical filesystem paths.

Provenance is observational metadata. It does not affect equality, hashing,
types, serialization, or user-visible pattern matching.

## Failed computation

`Never(FailureId)` is an evaluator outcome, not Forma's bottom type and not a
runtime value. It follows conservative propagation rules:

- aliasing preserves the same `FailureId`;
- an operation consuming one or more Never inputs returns Never and records a
  bounded propagation node;
- a Function call with a Never argument does not enter the Function;
- a condition or match with a Never scrutinee does not speculate branches;
- a container evaluates its direct children for independent diagnostics, but
  any Never child prevents construction of a publishable container; and
- independent later bindings may continue in best-effort mode.

Ordinary `Result.Err`, `Option.None`, and `BlameError` values retain their
language semantics. Cancellation, resource quota exhaustion, out-of-memory,
and VM invariant violations remain terminal Host outcomes and never become
Never.

## Strict and best-effort Hosts

The evaluator exposes policy equivalent to:

```text
EvaluationPolicy = Strict | BestEffort
```

Strict execution keeps current fail-fast behavior. Best-effort is intended for
analysis Hosts such as the LSP and explicit diagnostic commands. It may recover
after a local error, continue independent module bindings, and collect root
diagnostics under deterministic error and recursion budgets.

Neither policy publishes a partial module interface, partial export object, or
cache entry. Commands that perform effects, including future `exec` and build
flows, never consume a Never outcome. Cancellation and stale analysis discard
the entire pending publication.

## Contextual intrinsics

Some language constructs require context unavailable to an ordinary Function:
the expected static scheme, authored source location, or both. Forma spells
these constructs like Rust macros while deliberately not adding macros:

```forma
interpreter!(show_dyn)
blame!(data, "expected an integer")
```

An `identifier!(...)` contextual intrinsic is dedicated syntax resolved from a
closed language-defined set. It is not a value, cannot be rebound or passed,
and does not imply token trees, hygiene, expansion APIs, or user-defined
macros. `file!()` and `line!()` are reserved follow-up consumers; they are not
required by this phase. A future `file!()` returns a canonical resolved module
identity, never a physical absolute path.

`interpreter!(...)` replaces the existing spelling without a compatibility
alias. `blame!(...)` combines an explicit data anchor with the authored rule
site, allowing user-space interpreters to approach native diagnostics without
granting arbitrary access to Host locations.

## Design evidence

Nickel separates value positions into compact original, inherited, and absent
states, supporting the feasibility of structural provenance. Its contract
labels separately retain contract location, checked-value position, type path,
polarity, and diagnostic context, supporting a contextual blame construct
rather than a fake ordinary Function.

Nickel's LSP also uses a Host-only permissive traversal that records an error,
resets evaluation, and visits sibling container children. Forma adopts the
Host-policy boundary but not Nickel's lazy recovery mechanism: Forma records an
explicit internal Never and data-dependency lineage so an eager evaluator can
continue predictably and suppress cascades.

## Phase sequence

The planned sequence is:

1. RFC 0101: define contextual intrinsic syntax and migrate
   `interpreter(...)` to `interpreter!(...)`;
2. RFC 0102: attach structural provenance to values and define preservation,
   narrowing, and call-site rebasing;
3. RFC 0103: add internal Never outcomes, bounded failure lineage, and a
   complete propagation matrix;
4. RFC 0104: add strict/best-effort Host policy, evaluator recovery, budgets,
   and all-or-nothing module publication;
5. RFC 0105: preserve provenance through Dyn observation and define
   provenance-aware `blame!`; and
6. RFC 0106: validate deterministic CLI/LSP diagnostics from imported data
   through user-space interpreters and best-effort publication.

Each child RFC is proposed and implemented independently. Later children may
use internal scaffolding from earlier RFCs, but no partial public semantics are
claimed before their own acceptance criteria pass.

## Goals

1. preserve useful source locations through aliases and structural selection;
2. attribute regenerated values to a stable authored generation site;
3. let user-space interpreters produce source-aware validation diagnostics;
4. continue independent analysis after recoverable evaluation failures;
5. suppress diagnostics caused only by an already reported root failure;
6. explain blocked outputs with bounded data-dependency lineage;
7. preserve current strict execution behavior; and
8. keep effects, publication, and recovery policy under Host authority.

## Non-goals

- making Never observable, matchable, catchable, serializable, or typeable;
- publishing or exporting partial Forma containers or modules;
- changing equality, hashing, serialization, or typing based on provenance;
- preserving a complete history of arbitrary transformations;
- turning `Result` errors into Host failures automatically;
- language-level diagnostic accumulation or validation effects;
- speculative evaluation of branches blocked by Never;
- converting cancellation, quotas, or VM faults into recoverable failures;
- user-defined macros, token trees, hygiene, or compile-time expansion;
- exposing raw filesystem paths to Forma code or diagnostics; or
- implementing `file!()` or `line!()` in this phase.

## Shared acceptance criteria

1. aliases and structural selection retain or narrow original provenance;
2. computed values and container roots receive deterministic generated
   provenance while unchanged children retain their origins;
3. Function and native boundaries apply the same documented rebasing rule;
4. Dyn packing and observation preserve descriptor, value, and provenance in
   lockstep;
5. `blame!` can report both imported data and authored rule anchors;
6. every recoverable root failure emits at most one primary diagnostic;
7. dependent operations become Never without producing diagnostic cascades;
8. independent bindings continue only under explicit best-effort Host policy;
9. strict mode remains fail-fast and behavior compatible;
10. partial modules, exports, and shared cache entries are never published;
11. diagnostic ordering, lineage truncation, cancellation, and stale-result
    handling are deterministic; and
12. full Forma, CLI, LSP, formatting, and strict static checks pass.

## Stopping rules

Work returns to discussion if a child RFC requires:

1. exposing Never or failure identities to Forma programs;
2. partial module exports or effectful execution from partial results;
3. speculative branch execution or general lazy evaluation;
4. provenance-sensitive value semantics;
5. a complete arbitrary transformation graph rather than bounded anchors;
6. language-level effects, accumulation, exceptions, or resumable handlers;
7. user-defined macros or compile-time code execution;
8. unchecked Dyn casts or reconstructing a static type from erased metadata;
9. treating cancellation or resource failure as ordinary validation; or
10. publishing stale or partially evaluated analysis state.

## Delivery discipline

Each child RFC receives a proposal commit followed by a distinct implementation
commit containing tests and an implementation-result amendment. RFC 0100 stays
Proposed until RFC 0106 demonstrates the complete imported-data-to-diagnostic
path. The umbrella is then updated with final implementation evidence and only
the semantics actually delivered by its children.

## Implementation result

RFCs 0101 through 0106 are implemented. Forma now has closed contextual
intrinsics, structural Original/Generated provenance, bounded internal failure
lineage, a deterministic best-effort scheduling contract, provenance-aware Dyn
and `blame!`, and analysis-Host publication through the WorkspaceSnapshot used
by CLI `show` and the LSP.

Production recovery conservatively replays only independently closed top-level
initializers after a recoverable strict runtime root. Failed dependencies are
silent; later independent initializers continue in source order. Recursive
groups, branches, nested calls, containers, and final results remain strict
indivisible units. No partial module, export, cache entry, Never, FailureId, or
provenance object is exposed to Forma.

Imported JSON/TOML/YAML remains sourced through recovery heap import, so a
user-space interpreter can return `blame!` with distinct imported-data and
authored-rule labels. RuntimeError projects to ordinary structured Diagnostics;
effectful and executable commands remain strict. The final gate passes with
315 Forma library tests (1 ignored), 14 CLI tests, 20 LSP tests, documentation
tests, formatting, and warning-denied Clippy.
