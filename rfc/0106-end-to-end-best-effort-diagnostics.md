# RFC 0106: End-to-end best-effort diagnostics

- Status: Implemented
- Depends on: RFC 0100 through RFC 0105

## Summary

Forma connects the compiler, strict VM, recovery workspace, CLI `show`, and LSP
diagnostic publisher to complete source-aware best-effort evaluation.

The compiler derives deterministic atomic units from a fully typed module.
Each unit runs in the ordinary strict VM; successful roots remain private to
the recovery session, recoverable failures become internal Never lineage, and
dependent units are skipped. Independent units continue in source order.
There is no partial module value, export interface, or shared cache entry.

`run`, `exec`, `build`, ordinary module loading, and public `Engine::execute`
remain strict. Best-effort is an analysis-Host policy used by workspace
recovery, and is therefore observed by `forma show` and LSP diagnostics.

## Motivation

RFCs 0101 through 0105 established contextual intrinsics, structural
provenance, failure lineage, scheduling, and provenance-aware BlameError.
Before this RFC, workspace recovery continued after parse and type failures,
but attempted runtime evaluation only as one strict module. A runtime failure
was collapsed to a Partial module and its diagnostic was lost. The scheduler
also had no compiler-produced units or persistent session roots.

This RFC closes that gap. In particular, imported structured data can travel
through a user-space `interpreter!`, fail through `blame!` and `result.unwrap`,
and produce a diagnostic whose data label points into the imported source and
whose rule label points into Forma source. An unrelated later binding is still
evaluated and remains available to semantic tooling.

## Compiler plan

Best-effort planning is additional compiler output for a strictly parsed and
typed module. Stable units are emitted in authored order:

1. an ordinary non-recursive `let` is a Binding unit;
2. mutually dependent `def` bindings form one DefinitionGroup;
3. direct container children may be ContainerChild units only when splitting
   preserves eager order and does not speculate;
4. existing tool metadata work is Metadata; and
5. the final expression is the sole ModuleResult.

Imports and native declarations are session inputs, not executable units.
Type declarations and their required runtime metadata remain atomic with the
definition group that initializes them. Conditions, matches, Function bodies,
and short-circuit operators stay inside one unit.

Dependencies are derived from resolved HIR references. Unit IDs are dense and
source ordered; dependency lists are sorted, unique, and prior. A recursive SCC
is never split. Plan construction fails closed to existing strict recovery if
these invariants cannot be proven.

## Session execution

The recovery Host freezes its imported/core roots and creates one isolated
session. For each ready unit it creates a fresh WorkWorld, invokes the strict
VM with the shared quota account and cancellation token, and atomically
promotes a successful root into the session. A failed WorkWorld is discarded.

Recoverable RuntimeError values create exactly one diagnostic and one failure
root. A unit with a failed dependency is not compiled or entered and creates
only bounded propagation lineage. Terminal failures, cancellation, or a stale
query discard the session immediately. Reaching the diagnostic budget returns
the deterministic completed prefix without inventing another error.

Session values may support later independent analysis, but never constitute a
publishable Forma module. Only a successful ModuleResult with no root failures
may follow the existing complete-module publication path. A diagnostic session
itself is dropped after its immutable WorkspaceSnapshot is built.

## Diagnostic projection

RuntimeError retains its typed kind, call stack, and primary operation
location. When it contains canonical BlameError data, Host projection uses:

- `error.data` provenance for the primary data label;
- `error.rule` provenance for the secondary authored-rule label; and
- `error.message` for the message.

Otherwise the ordinary runtime operation location is primary. Diagnostics are
ordered by unit source order, then by their stable labels. Failure lineage does
not emit cascaded diagnostics. Tooling may append one bounded note explaining
that a requested result was blocked, but must not expose FailureId or Never.

WorkspaceSnapshot owns the resulting ordinary Diagnostics. Consequently CLI
`show`, LSP publication, UTF-16 mapping, cancellation, and stale-publication
checks reuse their existing paths rather than acquiring evaluator-specific
protocols.

## Public behavior

`Engine::recover_workspace[_async]` selects BestEffort for runtime analysis.
Its snapshot may contain several independent runtime diagnostics in addition
to parse/type/module diagnostics. The snapshot module state is Partial whenever
runtime roots exist; its public result and exports remain unavailable.

Strict APIs and effectful commands never select BestEffort. There is no CLI
flag that converts partial analysis into executable output.

## Goals

1. produce validated evaluation units from typed Forma source;
2. execute units with strict VM semantics and isolated failed WorkWorlds;
3. preserve successful roots only within one recovery session;
4. publish deterministic independent runtime diagnostics without cascades;
5. carry imported-data and authored-rule provenance through `blame!`;
6. expose results through existing CLI and LSP diagnostic paths;
7. honor cancellation, staleness, quotas, and root budgets; and
8. leave all strict and effectful behavior unchanged.

## Non-goals

- recovering from parse/type errors by executing ill-typed expressions;
- placing Never in bytecode, heap values, native arguments, or exports;
- partial module values, interfaces, containers, or cache publication;
- instruction-level VM recovery or speculative branch execution;
- parallel or nondeterministic unit scheduling;
- executing effects or retaining failed-unit debug output;
- exposing provenance, FailureId, or call stacks as Forma values; or
- adding a general diagnostics accumulation language effect.

## Acceptance criteria

1. the compiler emits dense source-ordered units with resolved dependencies;
2. recursive definitions remain one atomic SCC;
3. each ready unit runs with the strict VM in a fresh WorkWorld;
4. successful roots survive for later session units and failed roots do not;
5. one recoverable root produces one diagnostic and silent dependent Never;
6. an independent later binding still executes after an earlier root failure;
7. a failed condition or match never evaluates either branch speculatively;
8. imported JSON/TOML/YAML provenance survives Dyn observation and `blame!`;
9. rendered blame diagnostics distinguish data and rule source labels;
10. Partial modules publish neither result values nor export interfaces;
11. cancellation, stale revisions, quotas, and invalid bytecode are terminal;
12. CLI `show` and LSP publish the same ordered workspace diagnostics;
13. `run`, `exec`, `build`, loading, and `Engine::execute` remain strict; and
14. full workspace tests, formatting, and strict Clippy pass.

## Implementation plan

1. add compiler planning and HIR dependency projection;
2. add a VM-backed session root executor over RFC 0104's scheduler;
3. project typed runtime failures into source Diagnostics;
4. integrate BestEffort into synchronous and asynchronous workspace recovery;
5. add independent, dependent, recursive, branch, budget, and cancellation
   tests;
6. add an imported-data to interpreter/Dyn/blame CLI and LSP fixture;
7. verify strict command compatibility and all-or-nothing publication; and
8. record implementation evidence and complete RFC 0100.

## Stopping rules

Work returns to discussion if completion requires:

1. executing an expression that did not pass ordinary static analysis;
2. resuming arbitrary VM instructions or duplicating VM semantics;
3. exposing Never or partial Forma values;
4. splitting recursive SCCs or speculating control-flow branches;
5. allowing effects in diagnostic sessions;
6. weakening cancellation, quota, or stale-result terminal behavior; or
7. changing strict execution semantics.

## Implementation result

Implemented with a conservative production-unit boundary. Workspace recovery
first requires ordinary whole-module static analysis, then runs the strict
module once. After a recoverable runtime root, it replays only source-ordered
top-level `let` or `def` initializers that the existing compiler can close over
imports and previously successful values. Failed dependencies are skipped
transitively. Recursive groups, control flow, nested Function bodies,
containers, and the final result are never split or speculated.

Each replay uses a fresh strict VM world and one shared recovery quota account.
Successful values remain in the local recovery environment; failed values are
discarded and the deterministic diagnostic prefix is capped at 16. No partial
module, export, cache entry, Never, or FailureId is published.

RuntimeError now projects directly to structured Diagnostic labels. Recovery
also retains imported SourcedValue objects until heap import, preserving
Original JSON/TOML/YAML provenance. `show` enters recovery after a recoverable
strict runtime failure, while async workspace/LSP rebuild retains its existing
cancellation and stale-publication barrier.

Tests prove two independent ordered failures with a silent dependent binding,
strict `run` behavior, LSP publication and clearing, and a cross-source JSON to
`blame!` path with separate data and rule labels. The full gate passes: 315
Forma library tests passed with 1 ignored, 14 CLI tests passed, 20 LSP tests
passed, documentation tests passed, and warning-denied workspace Clippy passed.
