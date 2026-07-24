# RFC 0039: Workspace semantic queries

- Status: Implemented
- Depends on: RFC 0035, RFC 0038

## Summary

A successful module load retains a read-only, workspace-wide semantic snapshot.
The snapshot gives stable-for-one-snapshot IDs to modules, definitions,
references, and type nodes, and exposes protocol-independent queries over
them. `xl show` renders the same query data for inspection and golden tests.

This RFC covers complete, valid workspaces. Recoverable lowering, open-document
overlays, revisions, cancellation, and LSP transport remain subsequent RFCs.

## Snapshot ownership

`LoadedModule` retains one `WorkspaceSnapshot` built from all user XL and static
data modules visited by its loader. Core modules remain addressable import
targets but are not expanded into user workspace syntax in this RFC.

The snapshot owns cloned source text and detached semantic records. It exposes
no AST references, VM values, heap handles, persistent roots, quota accounts,
or mutable compiler state. Constructing it is part of module loading; querying
it executes no XL code.

All compact IDs are meaningful only within their owning snapshot:

```rust
struct WorkspaceModuleId(u32);
struct DefinitionId(u32);
struct ReferenceId(u32);
struct WorkspaceTypeId(u32);
```

The implementation rebuilds the entire snapshot on every module load and does
not preserve IDs across loads.

## Modules and sources

Each observed module record contains:

- its canonical path or core module name;
- source identity and complete source range when source-backed;
- XL or static-data kind;
- resolved import edges;
- module result type for XL modules;
- statically known result fields when the result type is a Struct.

The snapshot exposes source lookup by ID and path plus UTF-aware conversion
between line/column positions and internal byte locations. JSON modules appear
in module and source inventories but do not produce XL definitions.

## Definitions and references

The semantic index walks the same located AST accepted by normal compilation.
It assigns definitions to:

- top-level and nested `let`, `decl`, `def`, named function, type, import, and
  native bindings;
- closure parameters;
- pattern bindings.

References are variable occurrences. Field names are not variable references.
Lexical scopes, sequential `let`, definition slots, recursive named functions,
type predeclarations, closure parameters, and pattern scopes follow existing XL
semantics. An import definition records its resolved module target when one is
available. Uses of the imported local name still resolve to that local import
definition.

Every definition and reference retains its source location. Position queries
choose the narrowest containing name location; they do not infer a result from
nearby trivia.

## Workspace type graph

The loader currently derives one authoritative `TypeGraph` per analyzed XL
module. Snapshot construction remaps those graph nodes into one detached
workspace graph and remaps all module result and top-level binding/type roots.
This merge is an observation representation, not another inference pass.

Nodes retain graph structure rather than formatted strings, including recursive
references, Struct fields, Enum payloads, unions, and function signatures.
Display is a terminating presentation over the workspace graph. Names are
qualified by module in the merged name index to avoid collisions.

The graph may initially duplicate structurally equal nodes from different
module analyses. Cross-module interning and persistent identity exposure are
not required.

`type_at` covers:

- top-level binding and type definition names;
- references resolving to those definitions;
- complete module result expressions.

Nested definitions without an existing authoritative analysis type and
arbitrary expression subtrees return no type. Recording expression-level
semantic facts belongs to a later focused RFC and must extend this query rather
than add a second API.

## Query API

The initial read-only API provides the equivalent of:

```text
modules()
module(id)
module_by_path(path)
definitions()
definition_at(location)
references()
reference_at(location)
references_of(definition_id)
type_at(location)
type_node(type_id)
display_type(type_id)
exports_of(module_id)
```

Absence is explicit. Invalid IDs do not panic at the public boundary. Query
ordering is deterministic by canonical module path and then source location.

## `xl show`

`xl show <module.xl>` prints a deterministic workspace report containing:

- source-backed modules and resolved imports;
- definitions with kind, source range, and type when known;
- references with their resolved definition when known;
- module result types and statically known exports;
- the normalized workspace type graph.

`xl show <module.xl> at <path> <line> <column>` performs a position query and
prints the containing definition or reference and available type.

The first format is intentionally human-readable. A stable JSON DTO is deferred
until the query records have survived this implementation round; LSP consumes
the Rust query API rather than parsing CLI output.

## Diagnostics and failure

This RFC does not change module-load failure behavior. A parse, analysis,
metadata, or execution failure still returns `ModuleError` and no snapshot.
The snapshot of a successful load contains no hidden diagnostics discarded by
the loader.

Capturing multi-diagnostic partial snapshots is part of recoverable semantic
lowering and must not be approximated with rendered error strings here.

## Acceptance criteria

1. loading a multi-module XL/JSON workspace retains one detached snapshot.
2. module and import inventories are deterministic and source-addressable.
3. lexical definitions and references resolve across nested scopes, recursion,
   parameters, patterns, and imports.
4. import definitions identify their resolved user or core module target.
5. top-level binding, declared type, reference, and module-result queries return
   remapped workspace type IDs where available.
6. recursive TypeMetadata appears as a terminating structured workspace graph.
7. Struct module results expose deterministic field/type entries.
8. repeated queries do not execute XL code or mutate runtime state.
9. `xl show` and position mode are backed only by snapshot queries.
10. existing run, check, types, quota, codec, and schema behavior is unchanged.

## Implementation plan

1. Retain detached source/program/analysis observations during module loading.
2. Build deterministic module, import, definition, and reference indexes.
3. Merge existing authoritative module TypeGraphs into a workspace graph.
4. Attach known type roots and module exports to semantic records.
5. Expose read-only location and graph queries from the library.
6. Add `xl show` summary and position inspection.
7. Test multi-module scopes, recursive types, JSON inventory, UTF-8 positions,
   deterministic output, and query purity.

## Deferred work

- invalid or incomplete workspaces and multi-diagnostic snapshots;
- expression-level type facts beyond existing analysis roots;
- open-document overlays, revisions, cancellation, and stale responses;
- JSON serialization of query DTOs;
- core-module definition indexing;
- field-access resolution to generated module result members;
- CST/AST/HIR caching and dependency-granular invalidation;
- LSP transport and protocol position encodings.

## Implementation result

`LoadedModule` now retains a detached `WorkspaceSnapshot`. The loader records
each successfully loaded user XL and JSON module after authoritative metadata
promotion, resolves user and core import targets, and seals all observations
into one deterministic workspace identity space. Core modules are represented
as addressable targets without copying their internal syntax into the user
index.

The new `semantic` module exposes snapshot-local module, definition, reference,
and workspace type IDs plus read-only lookup, position, reference-set,
structured type-node, display, and Struct-export queries. Lexical indexing
covers sequential bindings, predeclared types and definition slots, named
recursion, closure parameters, nested blocks, and pattern bindings. `decl` and
`def` name locations share one definition entity. Prelude names are explicitly
classified as external rather than falsely reported as unresolved.

Per-module authoritative TypeGraphs are copied and recursively remapped into a
detached workspace graph. Recursive identity is preserved during each remap,
qualified names terminate display, and the snapshot retains no runtime value,
heap handle, AST, or `Analysis`. Struct module-result fields are exposed as
typed exports. Type-at-position currently has the deliberately stated
binding/reference/result coverage.

`SourceFile` now provides the inverse UTF-aware line/column-to-byte conversion
needed by position queries. `xl show` prints deterministic module, import,
definition, reference, export, result, and graph records; its `at` mode uses the
same snapshot queries and source conversion.

Tests cover a mixed XL/JSON workspace, resolved import targets, nested closure
scope, recursive TypeMetadata, exports, UTF-8 offsets, deterministic CLI
output, and position type lookup. The complete suite passes with 147 unit tests
and five CLI tests, with one manual parsing baseline ignored. Strict workspace
Clippy, formatting, and diff checks pass.
