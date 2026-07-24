# RFC 0040: Unified type observation

- Status: Proposed
- Depends on: RFC 0039

## Summary

`xl types <module.xl>` becomes a compact presentation over the root module in
`WorkspaceSnapshot`. It no longer reads `LoadedModule::analysis` directly.

The command keeps its existing user-facing purpose and output shape:

```text
type Name = ...
let binding: ...
result: ...
```

`xl show` remains the complete workspace inspection surface. `xl types` is a
convenience projection, not an independent semantic API.

## Motivation

RFC 0039 introduced the query boundary intended for CLI inspection, tests, and
LSP. Retaining a second CLI path over root `Analysis` creates two observable
type presentations and allows future query changes to drift from the legacy
command.

Routing both commands through the snapshot proves that the detached workspace
graph is sufficient for the earlier type-summary use case and removes the last
CLI dependency on compiler analysis internals.

## Semantics

The command resolves the loaded root path to its `WorkspaceModule` and emits:

1. root-module definitions of kind `Type` with known workspace type IDs;
2. other root-module top-level definitions with known workspace type IDs using
   the historical `let` label;
3. the root module result type.

Definitions without a semantic type are omitted, matching the fact that the
old `Analysis::binding_types` contained only analyzed top-level bindings.
Nested definitions currently have no type facts and are consequently omitted.

Display uses `WorkspaceTypeGraph::display`. Recursive types therefore use the
same terminating graph presentation as `xl show` and future hover queries.

Failure to find the loaded root or its result type is an internal snapshot
invariant violation reported as a CLI error rather than a panic.

## Acceptance criteria

1. `xl types` does not access `LoadedModule::analysis`.
2. declared types, top-level bindings, imports, and module result retain their
   compact summary.
3. recursive type display terminates through the workspace graph.
4. `xl types` and `xl show` display each shared type through the same method.
5. run, check, show, and library semantics are unchanged.

## Implementation plan

1. Resolve the root `WorkspaceModule` from `LoadedModule::path`.
2. Select its typed definitions and result from `WorkspaceSnapshot`.
3. Render every type with `WorkspaceTypeGraph::display`.
4. Extend CLI coverage for the unified path and recursive types.

## Implementation result

Pending.
