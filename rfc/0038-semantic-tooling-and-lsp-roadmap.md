# RFC 0038: Semantic tooling and LSP roadmap

- Status: Accepted roadmap
- Depends on: RFC 0005, RFC 0006, RFC 0008, RFC 0021, RFC 0035

## Summary

XL will expose its compiler and tool-stage results through an immutable
semantic snapshot and query layer before implementing the Language Server
Protocol itself. The command line, tests, and an eventual LSP server consume
the same queries:

```text
source documents + module graph + Main World metadata
                         |
                         v
                 WorkspaceSnapshot
                    /     |     \
                   v      v      v
              `xl show`  tests   LSP adapter
```

The snapshot connects source positions to syntax nodes, definitions,
references, module exports, diagnostics, and the authoritative `TypeGraph`
derived from promoted TypeMetadata. It does not create a second type language,
re-evaluate metadata for each query, or expose VM heap handles as a tooling
API.

This is an umbrella RFC. It defines direction, boundaries, sequencing, and the
acceptance bar for later executable RFCs. It has no direct implementation.

## Motivation

XL now has most of the semantic inputs required by an editor:

- one lossless CST and lexer path for normal parsing and editing;
- byte-accurate source spans, UTF-aware position conversion, and structured
  multi-label diagnostics;
- located semantic nodes and rich runtime values;
- a closed module graph with static data modules;
- once-only promoted TypeMetadata and an immutable recursive `TypeGraph`;
- definition and metadata dependency information in compiler analysis.

These parts have been validated through execution, codecs, schema generation,
and diagnostics, but they are not yet presented as one position-oriented
tooling model. Building protocol handlers directly over compiler internals
would couple LSP lifetime, UTF-16 positions, partial documents, heap identities,
and type display to unrelated implementation details.

The next architectural test is whether XL can answer ordinary source questions
from the same computed metadata that drives runtime validation and generation:

- What does the name at this position refer to?
- Where is it defined and where is it used?
- What type metadata did this expression or binding compute?
- Which fields, variants, and attributes are available here?
- Which source and rule locations explain this diagnostic?

## Goals

The tooling line will provide:

1. a stable snapshot and query boundary independent of LSP transport;
2. deterministic structured output through an `xl show` family of commands;
3. useful syntax and semantic results while a document is incomplete;
4. workspace-aware definitions, references, imports, and diagnostics;
5. a thin LSP adapter for diagnostics, hover, navigation, references, and
   conservative completion;
6. graph-safe display and inspection of recursive computed TypeMetadata;
7. explicit revision and cancellation boundaries for editor use.

## Non-goals

This roadmap does not introduce:

- a second parser, type checker, evaluator, or metadata representation;
- full bidirectional type inference, traits, HKT, or subtyping;
- arbitrary evaluation of expressions selected by an editor client;
- code quotation or a public representation of HIR as XL data;
- formatting, rename, code actions, semantic tokens, or inlay hints in the
  first LSP milestone;
- mandatory incremental parsing or incremental compilation;
- a stable external compiler plugin ABI.

The first implementation rebuilds the complete workspace snapshot from current
sources. Correct revision isolation and deterministic results are required
before any incremental reuse is considered.

## Semantic snapshot

A workspace snapshot is an immutable view of one workspace revision. Its exact
Rust representation remains private, but conceptually contains:

```rust
struct WorkspaceSnapshot {
    revision: Revision,
    sources: SourceSnapshot,
    module_graph: ModuleGraph,
    syntax: WorkspaceSyntaxIndex,
    hir: WorkspaceHir,
    bindings: WorkspaceBindingIndex,
    types: TypeGraph,
    diagnostics: Vec<Diagnostic>,
}
```

The first snapshot has one workspace-wide identity space for syntax, HIR,
definitions, and types. Modules remain explicit names and dependency nodes, but
do not own independently reusable semantic snapshots or TypeGraphs. In
particular, metadata computed in one module may execute decorators imported
from another module, so its semantic result cannot be treated as a function of
the local source alone.

All query results belong to one workspace revision. IDs may be compact indices,
but are meaningful only with that snapshot and must not be retained across a
rebuild. Public result objects use source locations and serializable semantic
data rather than references into compiler arenas, VM stacks, Work Worlds, or
Main World heaps.

The snapshot owns or retains every source needed to render its results.
`SourceId` remains an engine identity, not a file URI or an LSP document
version. A source record carries its canonical module identity, display path,
text, and revision information separately.

Snapshot construction may run the bounded tool stage needed to obtain
authoritative TypeMetadata. Queries themselves are read-only and never execute
XL code. Repeated hover, completion, or reference requests therefore cannot
consume fuel, allocate in Main World, invoke `dbg`, or change diagnostics.

## Query model

The initial protocol-independent query surface should cover:

```text
diagnostics(source?)
syntax_at(location)
definition_at(location)
references_of(definition)
type_at(location)
exports_of(module)
members_of(type)
```

Queries return explicit absence when no trustworthy answer exists. `Any`, a
missing result, and an analysis failure are distinct:

- `Any` is a valid static result with intentionally lost precision;
- missing means that no semantic entity is available at the requested syntax;
- failure is represented by diagnostics attached to the snapshot.

The query layer must not infer semantics from formatted type strings. Type
answers identify nodes in a snapshot-owned graph and may additionally provide
a deterministic display form. Recursive references terminate through graph
identity and prefer declared names where available.

## Source positions

XL continues to use UTF-8 byte ranges internally. Every external position is
converted through the source database:

- CLI output may use UTF-8 line and character positions;
- LSP positions use the encoding negotiated with the client, initially UTF-16
  where required;
- no handler computes a column from a byte offset directly;
- end positions and zero-width missing slots are handled consistently.

Diagnostics preserve primary and secondary labels. The LSP adapter may publish
one diagnostic with related information, but it must not discard rule-side
locations merely because they are in another source file.

## Tolerant editing model

Lossless CST construction and lexical error tokens remain available for every
document revision. Semantic lowering becomes best-effort rather than
all-or-nothing:

- missing grammar slots stay absent and produce diagnostics;
- unknown tokens remain represented in the CST;
- valid declarations and expressions outside a damaged region may be indexed;
- unresolved names and unavailable types do not fabricate bindings or precise
  metadata;
- recovery nodes are compiler-internal and are never runtime XL values.

This does not require treating every syntax error as a normal language AST
node. Typed CST queries expose optional slots; lowering creates semantic nodes
only where their required shape exists and records a local failure otherwise.

A snapshot distinguishes syntax availability from semantic completeness. A
client can therefore navigate a valid import or definition while another
function in the same file is incomplete.

## Modules and workspace lifetime

The workspace layer maps open document overlays and files on disk into the same
static module graph used by normal loading. An in-memory open document shadows
its on-disk text for one revision without changing import identity.

Any source change rebuilds the complete workspace snapshot. This deliberately
defers CST, AST, HIR, module, and TypeGraph reuse until measurement justifies a
cache model. It also handles XL metadata dependencies conservatively: changing
an imported decorator may significantly change a dependent module's computed
types even when that module's source is unchanged.

The builder must not publish a snapshot containing a mixture of semantic
results from different revisions.

Data modules participate in source diagnostics and provenance, but JSON object
keys and values do not become XL definitions. Their positions may still be
returned as data locations in codec and validation diagnostics.

Tool-stage evaluation remains closed and quota-bound. Cancellation may abandon
an unpublished Work World and snapshot build. Main World publication remains
atomic at the existing module boundary. A failed module may still produce a
current partial snapshot containing CST, local bindings, and diagnostics, but
its authoritative type graph is absent. It must not borrow semantic facts from
the last successful revision and present them as current.

## LSP boundary

The LSP server is a transport and lifecycle adapter. It is responsible for:

- document versions and open-text overlays;
- request cancellation and stale-response suppression;
- URI and negotiated position-encoding conversion;
- mapping semantic query results into LSP structures;
- publishing diagnostics for the current snapshot only.

It does not inspect VM values, traverse compiler ASTs independently, re-run
metadata functions per request, or implement separate name/type rules.

The first LSP feature set is deliberately small:

1. syntax, name, type, module, and tool-stage diagnostics;
2. hover for definitions, bindings, and computed TypeMetadata;
3. go to definition;
4. references within the loaded workspace;
5. completion for statically known module exports and Struct fields where the
   authoritative graph provides them.

Completion is conservative. It may return no semantic candidates for `Any`, an
incomplete receiver, or an ambiguous union. It must not execute user code to
discover candidates.

## `xl show`

Before LSP transport is introduced, the query boundary is exercised through
deterministic CLI output. The exact command spelling belongs to its child RFC,
but it should support machine-readable inspection of at least:

- diagnostics;
- definitions and references;
- the type at a source location;
- module exports;
- normalized recursive type graphs.

This output is both a user-facing inspection tool and a golden-test surface.
JSON records use stable documented shapes, source paths and ranges, and
snapshot-local graph IDs. Human-readable output is a presentation over those
records, not a separate semantic path.

## Proposed RFC sequence

This umbrella RFC is followed by small executable RFCs. Numbers and exact
grouping may change after each implementation result.

### A. Semantic snapshot and `xl show`

Define immutable snapshots for valid module graphs, a position-oriented query
API, structured serialization, and CLI inspection. Reuse the authoritative
`TypeGraph`; do not address damaged semantic syntax yet.

### B. Recoverable semantic lowering

Replace the current whole-file semantic fail-fast boundary with typed-CST slot
queries, local lowering failures, partial binding indexes, and explicit
completeness. Preserve all syntax diagnostics.

### C. Workspace overlays and revision lifetime

Introduce document revisions, in-memory overlays, cancellation, and atomic
publication of eager workspace snapshots. Dependency-granular invalidation is
deferred.

### D. Minimal LSP adapter

Implement diagnostics, hover, definition, and references solely through the
query API. Add protocol position conversion and stale-result tests.

### E. Conservative completion

Complete module exports, normalized Struct fields, Enum variants, decorator
names, and other candidates justified by the authoritative semantic graph.
Define behavior for `Any`, unions, partial syntax, and pipeline positions.

### F. Tooling quality and performance

Measure snapshot latency and memory before adding incremental CST reuse,
dependency-granular semantic caching, richer type displays, semantic tokens,
rename, code actions, inlay hints, or formatting.

## Architectural invariants

Every child RFC must preserve these constraints:

1. normal compilation, CLI inspection, and LSP use the same lexer, CST, lowering,
   module resolution, and semantic analysis;
2. promoted TypeMetadata and its derived `TypeGraph` are authoritative;
3. a semantic query is read-only and cannot execute XL code;
4. every result is tied to one source and snapshot revision;
5. partial analysis reports uncertainty rather than inventing precision;
6. diagnostics retain cross-source primary and secondary labels;
7. recursive metadata is traversed by identity with guaranteed termination;
8. protocol-specific types do not leak into compiler and VM layers;
9. correctness does not depend on incremental implementation;
10. tool-stage execution remains deterministic, quota-bound, and atomically
    published.

## Roadmap completion criteria

This roadmap is considered validated when:

1. a recursive decorated model can be inspected through `xl show` and hover,
   and both expose the same named recursive graph used by codec and schema;
2. definition and reference navigation crosses XL module boundaries without a
   second name-resolution path;
3. malformed source still provides syntax diagnostics and navigation for an
   unaffected declaration;
4. a JSON validation failure is published with its data location and model-rule
   related location;
5. rapid document revisions never publish diagnostics or hover from a stale
   snapshot;
6. repeated queries perform no XL evaluation and produce deterministic results;
7. completion exposes known model fields and module exports without guessing
   through `Any`;
8. CLI golden tests and LSP integration tests exercise the same query objects.

## Deferred work

- incremental parsing and fine-grained query memoization;
- module snapshots, dependency fingerprints, and cross-revision ID reuse;
- cached CST, AST, HIR, name-resolution, and TypeGraph subgraphs;
- metadata-dependency-aware invalidation;
- persistent snapshot serialization;
- package-manager and multi-root workspace semantics;
- remote modules and dynamic import discovery;
- rename safety across generated or decorated names;
- formatter ownership of trivia and comments;
- custom LSP methods for schema or codec previews;
- exposing HIR or quoted XL logic as first-class language data.

## Rejected alternatives

### Implement LSP handlers directly over compiler internals

This gives an early demo but creates separate traversal and display behavior
for every handler. A protocol-independent query layer provides a testable
semantic boundary and keeps LSP replaceable.

### Serialize runtime TypeMetadata for every query

Runtime metadata may contain hidden links, functions, rich origins, and heap
identity. Re-exporting it is expensive and leaks VM representation. The derived
snapshot-owned `TypeGraph` is the tooling view.

### Require a valid module before returning any result

That is acceptable for batch compilation but makes editing unusable. Syntax
availability and semantic completeness must be represented separately.

### Design incremental compilation first

Without a stable snapshot and query contract, caches would preserve accidental
compiler structure. Eager immutable snapshots establish correctness and provide
a baseline against which incremental work can be measured.

### Add a separate lightweight editor type checker

It would answer quickly but undermine XL's central claim that computed
TypeMetadata is authoritative across tooling and runtime. Conservative absence
is preferable to a second semantic system.
