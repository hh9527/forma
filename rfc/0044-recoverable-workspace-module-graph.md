# RFC 0044: Recoverable workspace module graph

- Status: Proposed
- Depends on: RFC 0039, RFC 0042, RFC 0043

## Summary

XL builds one recoverable workspace snapshot across the complete statically
discoverable module graph. Every source is registered in one `SourceDatabase`.
Syntax, HIR, imports, diagnostics, partial TypeMetadata facts, and module state
remain observable even when one module cannot complete strict initialization.

Modules have an explicit tooling state:

```rust
enum WorkspaceModuleState {
    Known,
    Partial,
    Unavailable,
}
```

A successfully initialized dependency provides its ordinary immutable module
value to dependent partial analysis. A failed dependency provides no value.
Types that reference its import definition become
`Unknown(BlockedBy(import_definition))`; independent local types continue.

The recoverable graph is tooling data. Partial modules and their values are
never published into the runtime Main World.

## Motivation

RFC 0043 isolates TypeMetadata failures inside one source, but
`WorkspaceSnapshot::recover_source` does not follow imports. This loses the
main benefit in real XL programs, where model definitions use decorators,
constructors, and metadata exported by other modules:

```xl
import model from "./model.xl";
import json from "core:json";

@json.rename_all("camelCase")
type User = model.User;
```

If `model.xl` fails, tooling must retain `main.xl`, the import edge, independent
local facts, the failure in `model.xl`, and an explicit cause for facts that
need `model`. Treating the root as an isolated file misclassifies those facts
as unsupported operations and prevents cross-file navigation.

## Workspace builder

The engine adds a tooling-only recoverable workspace entry point:

```rust
Engine::recover_workspace(root: &Path) -> Result<WorkspaceSnapshot, ModuleError>
```

The builder owns one `SourceDatabase`, one canonical module-key table, one
diagnostic collection, and one partial-analysis quota account per XL module.
It recursively discovers imports from retained top-level import bindings.

Recovery does not require a complete executable `Program`. An import is
followed only when its binding name and literal path are both present and
valid. Missing or computed import paths produce diagnostics and no guessed
edge.

Relative paths resolve against the importing file and use the strict loader's
canonical module identity rules. Core module identities retain their
`core:name` form. Static JSON modules participate as data nodes.

## Shared sources

Partial parsing and analysis gain registered entry points accepting a shared
`SourceDatabase` and `SourceId`. Recovery must not re-register the same text in
private databases and rely on coincidentally equal source IDs.

Every diagnostic and location in the snapshot resolves through the snapshot's
database. A canonical file is registered once per build. Core source records
may also be registered when their declarations are needed for navigation.

No caching across builds is introduced. Sharing is within one immutable
workspace revision only.

## Module states

`Known` means the module completed strict loading and its immutable result is
available as an import capability. Its ordinary authoritative analysis remains
preferred when it belongs to the same recoverable build.

`Partial` means the module has recoverable HIR or semantic facts but cannot
produce a complete import value. Diagnostics explain the root failures.

`Unavailable` means no trustworthy semantic module could be constructed, for
example because the file cannot be read, its format is unsupported, or an
import cycle prevents initialization. The graph retains the module identity
and incoming edges when its canonical target is known.

State is observational tooling metadata. It does not alter strict module
initialization semantics.

## Import capabilities

Dependencies are processed before dependents. An XL dependency that completes
strict loading is evaluated to its immutable module result under the existing
module/session quota boundaries. The exported legacy `Value` is self-contained
and can be passed to the dependent RFC 0043 partial analyzer through its
explicit bindings API.

JSON dependencies are parsed once in the shared source database and their
ordinary immutable data value is supplied directly. Core modules use the same
registered declarations and native implementations as strict loading.

No value is created for a partial or unavailable module. In particular, XL
does not substitute `'None`, `{}`, `Any`, or a stale result from a previous
revision.

## Cross-module blocking

Recoverable HIR retains import definitions. For every scheduled type RHS, the
partial analyzer records references to import definitions as well as references
to local type definitions.

When an import value is unavailable, the scheduled fact is not evaluated and
records:

```text
Unknown(BlockedBy(import_definition))
```

The workspace projection maps that cause to the corresponding workspace
definition and module target. Transitive local dependents remain blocked by
their nearest local type-definition cause. Independent facts continue.

An unavailable import owns its module/read/parse diagnostic. Every blocked
dependent does not duplicate that diagnostic.

## Cycles

The recoverable builder detects module cycles using canonical module identity.
Every cycle edge remains queryable. Modules in the cycle become `Unavailable`
for import-value purposes and receive one deterministic cycle diagnostic. Their
individual CST and recoverable HIR may still be retained when readable.

This RFC does not reinterpret module cycles as recursive modules and does not
attempt a fixed point across module initializers.

## Workspace projection

`WorkspaceSnapshot` accepts complete and partial module inputs in one global ID
space. It projects:

- module state and imports;
- recovered definitions, references, and expressions;
- known and unavailable definition facts;
- per-module partial type graphs;
- all source diagnostics.

Complete `Analysis` remains authoritative when present. Partial analysis is
used only when complete analysis is absent. Type IDs and blocked causes are
remapped into workspace identities after modules are sorted deterministically.

Queries remain read-only and execute no XL code after snapshot construction.

## CLI

`xl show` uses strict loading when the complete graph succeeds. On failure it
builds the recoverable workspace rather than falling back to a single root
source. Output includes module state, import targets, diagnostics, and partial
facts across files.

`xl check`, `xl run`, and `xl types` remain strict.

## Resource model

Each partial XL module receives one module quota account shared by all of its
scheduled type definitions. A module count does not multiply a single module's
quota, while distinct modules retain the existing independent module quota
model.

Strict probing of whether a dependency can provide an import value uses the
engine's normal module quota and never relaxes publication rules. Recovery
results themselves are not promoted into Main World.

## Determinism

- module identities use canonical paths or stable core names;
- traversal and final projection are sorted by module identity;
- imports retain source order within a module;
- diagnostics sort by source identity, primary location, and message;
- blocked causes refer to identities, not formatted names;
- one canonical source is registered once per build.

## Non-goals

- open-document overlays and unsaved buffers;
- revisions, request cancellation, or stale-response suppression;
- incremental parsing, module caching, or invalidation;
- parallel module initialization;
- recursive modules or partial Main World publication;
- expression-granular retry inside one type RHS;
- ordinary runtime-binding partial execution;
- LSP protocol transport;
- introducing an LSP, async-runtime, or filesystem-watching dependency crate.

The dependency-crate selection for overlays and LSP transport is intentionally
deferred until this protocol-independent workspace boundary is validated.

## Acceptance criteria

1. one shared `SourceDatabase` owns every recovered source and diagnostic.
2. retained literal imports form a canonical deterministic module graph even
   when some nodes fail.
3. successful XL, JSON, and core dependencies can supply immutable values to a
   dependent partial analyzer.
4. failed dependencies supply no fabricated value.
5. a local type using a failed import records a workspace-visible blocked cause
   connected to that import definition/module.
6. independent local and independent-module facts continue and remain known.
7. transitive local dependents block without re-running the failed root.
8. failed imports own their diagnostics; blocked facts do not duplicate them.
9. module cycles are deterministic, retained in the graph, and unavailable for
   value linking.
10. complete analysis wins over partial analysis when both are available.
11. strict check, run, types, module initialization, quota, and publication
    behavior remains unchanged.
12. `xl show` observes all readable modules, module states, imports,
    diagnostics, known facts, and blocked facts after strict loading fails.
13. snapshot queries perform no additional evaluation.
14. no new third-party dependency crate is required by this RFC.

## Implementation plan

1. add registered partial-analysis APIs over shared sources.
2. represent module state and partial module inputs in semantic snapshots.
3. implement recoverable import discovery and canonical graph traversal.
4. obtain immutable values from successful XL, JSON, and core dependencies.
5. teach the partial scheduler to block on unavailable import definitions.
6. project partial graphs, diagnostics, imports, and causes globally.
7. route failed `xl show` through recoverable workspace construction.
8. test mixed success/failure, JSON/core linking, cycles, source identity,
   diagnostics, quota isolation, and strict behavior.

## Deferred work

- overlays, revisions, and cancellation;
- LSP transport and dependency-crate selection;
- imported partial export sets finer than whole-module availability;
- recursive component sealing across modules;
- incremental reuse and parallel scheduling;
- filesystem watching and package resolution.

## Implementation result

Pending.
