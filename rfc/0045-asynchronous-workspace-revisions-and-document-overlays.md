# RFC 0045: Asynchronous workspace revisions and document overlays

- Status: Implemented
- Depends on: RFC 0038, RFC 0039, RFC 0044

## Summary

XL introduces revisioned open-document overlays, immutable workspace
snapshots, and an asynchronous query contract before adding an LSP transport.
An open document is stored as a copy-on-write UTF-8 rope backed by `crop` with
its UTF-16 metric enabled. XL byte ranges remain the authoritative internal
location representation.

Every potentially long-running workspace operation is asynchronous and
receives a `QueryContext` carrying its revision and cancellation state. Query
handlers describe work but do not choose whether that work is polled inline,
spawned on one analysis worker, or distributed across a thread pool. CPU-bound
analysis cooperatively checks cancellation and stale revisions at stable work
boundaries.

This RFC does not introduce LSP protocol types, an async runtime, a transport,
incremental parsing, or dependency-granular semantic caching.

## Motivation

RFC 0044 can recover a complete static module graph, but it reads every module
from disk and has no lifetime boundary suitable for an editor. An editor may
send several document changes while an earlier workspace build is still
running, cancel an individual request, and issue a query against the newest
published result. XL must not publish a mixture of those revisions or continue
expensive metadata evaluation after its result is known to be obsolete.

A synchronous query surface wrapped only at the protocol boundary would make
the current execution strategy part of the compiler API. It would also make a
cancelled outer future misleading: dropping that future does not stop
CPU-bound parsing, graph construction, or tool evaluation already in progress.
Cancellation must be visible inside the query pipeline.

The text representation has related requirements from several independent
consumers:

- the lexer, CST, HIR, and diagnostics use UTF-8 byte spans;
- LSP clients use negotiated UTF-8, UTF-16, or UTF-32 line columns;
- workspace snapshots require cheap immutable sharing across revisions;
- Logos must produce unowned tokens and global byte spans from chunked text;
- terminal diagnostics require display-cell alignment distinct from every
  protocol encoding.

These requirements call for one encapsulated document abstraction rather than
an LSP-specific document helper or a second location representation.

## Goals

1. represent open files as versioned in-memory overlays over canonical module
   identities;
2. publish immutable snapshots tagged with one monotonically increasing
   workspace revision;
3. make workspace build and query entry points asynchronous throughout their
   potentially long-running call chains;
4. propagate explicit cancellation and stale-revision checks into parsing,
   module traversal, semantic analysis, partial evaluation, and VM work;
5. preserve UTF-8 byte ranges as the core location model while supporting
   correct UTF-8, UTF-16, and UTF-32 line-column conversion;
6. accept incremental text edits without requiring incremental lexing,
   parsing, or semantic reuse;
7. keep scheduling, protocol, and runtime choices outside query handlers and
   compiler data structures.

## Non-goals

- LSP initialization, JSON-RPC transport, or protocol request handlers;
- choosing Tokio or another async executor for the compiler core;
- concurrent or parallel analysis;
- incremental CST, HIR, module, or TypeGraph reuse;
- preserving semantic IDs across revisions;
- filesystem watching or multi-root workspace discovery;
- a general text-editor buffer API;
- grapheme-aware editing, cursor movement, or terminal rendering;
- changing strict `check`, `run`, `types`, or module publication semantics.

## Document model

The public workspace model owns document snapshots through XL types. The
underlying rope implementation remains private:

```rust
struct DocumentSnapshot {
    identity: DocumentId,
    version: DocumentVersion,
    text: DocumentText,
}

struct DocumentText {
    rope: crop::Rope,
}
```

`DocumentId` identifies the canonical source independently of an LSP URI.
`DocumentVersion` is the version supplied by the overlay owner and is distinct
from the workspace-wide `Revision`.

`DocumentText` exposes byte-oriented operations, line lookup, position
conversion, slices, and ordered chunks. It does not expose `crop::Rope` in a
public signature. A new revision clones the rope and applies edits to the
clone; old snapshots continue to share unchanged tree nodes and remain
immutable.

The initial dependency is:

```toml
crop = { version = "0.4.3", features = ["utf16-metric"] }
```

`lsp-document`, `line-index`, and `ropey` are not introduced. `lsp-document`
couples the document to an older `lsp-types` and UTF-16-only protocol model.
`line-index` duplicates line and UTF-16 metadata already maintained by the
rope. `ropey` is mature but its stable editing API is primarily character
indexed, while XL edits and locations are byte indexed.

## Coordinates and display

XL retains `TextRange` as a half-open range of UTF-8 byte offsets. A document
converts at its boundary among explicit encodings:

```rust
enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

struct TextPosition {
    line: u32,
    character: u32,
}
```

Lines and protocol characters are zero based. Conversion rejects positions
outside the document, positions inside a UTF-8 scalar, and UTF-16 positions
inside a surrogate pair. It does not silently clamp malformed client input.

UTF-8 characters count bytes within the line. UTF-16 characters count code
units. UTF-32 characters count Unicode scalar values. The rope's line and
UTF-16 metrics provide logarithmic navigation; UTF-32 may scan the selected
line until measurement justifies another tree metric.

CLI source headers continue to use a documented human source column, while
underlines and excerpts align by terminal display cells. Tabs, wide characters,
combining characters, and rendered control characters are presentation
concerns for `codespan-reporting` or a later renderer. Terminal display columns
are not stored in `TextRange` and are not reused as LSP positions.

## Text edits

An edit replaces one byte range with UTF-8 text:

```rust
struct TextEdit {
    range: TextRange,
    replacement: String,
}
```

Protocol adapters first convert their encoded line-column range against the
current document version. A sequence of edits is applied in order; each range
is interpreted against the text produced by preceding edits in that sequence.
A full replacement is represented explicitly rather than by a fabricated
range.

Every edit must land on UTF-8 scalar boundaries. An invalid version, range, or
boundary rejects the complete change transaction and leaves the current
overlay unchanged. A later LSP adapter may request a full resynchronization,
but that policy is outside this RFC.

Accepting range edits does not imply incremental parsing. The first
implementation lexes and parses the complete current document after a change.

## Chunked lexing and source access

Logos-generated XL tokens are unowned enums accompanied by byte spans. The CST
also retains spans rather than borrowed token text. This permits full lexing of
a rope without flattening the complete document.

The lexer gains an XL-owned source bridge. It feeds contiguous rope chunks or
temporary boundary windows to the existing Logos lexers and relocates their
spans into document-global byte offsets. A token that may continue beyond a
chunk is withheld until enough subsequent text establishes its end. Only a
token crossing a chunk boundary requires temporary contiguous storage.

The bridge preserves the existing root, string, and interpolation contexts.
Chunk boundaries must not change tokenization. In particular identifiers,
numbers, comments, whitespace, byte strings, escapes, normal strings, and
interpolations may all cross a boundary.

Typed syntax and lowering read source text through byte slices rather than
indexing a global `&str`. A single-chunk slice is borrowed; a fragmented slice
may be iterated or materialized only for the token or node that needs owned
text. No full-document flat copy is required.

This bridge is not incremental lexing. Each build may scan every chunk and
produce a fresh token stream.

## Workspace revisions and overlays

The workspace owns canonical disk inputs and open overlays separately:

```rust
struct Workspace {
    revision: Revision,
    overlays: DocumentOverlaySet,
    published: Option<Arc<WorkspaceSnapshot>>,
}
```

Opening, changing, or closing a document creates a new workspace revision.
An open overlay shadows disk text for the same canonical module identity.
Closing it reveals the disk source in a new revision. Import resolution never
treats the overlay as a second module.

The workspace captures one immutable set of document snapshots before a build
begins. Disk reads discovered later in that build belong to the same build
input and cannot be replaced with documents from a newer overlay revision.

The first implementation rebuilds the complete recoverable workspace graph.
It publishes the result atomically only if its revision is still current and
the operation has not been cancelled. A failed or partial current build may
publish its recoverable current snapshot. A stale build never publishes.

`WorkspaceSnapshot` records its `Revision`. Snapshot-local source, definition,
reference, expression, module, diagnostic, and type identities cannot be mixed
with another revision.

## Asynchronous query contract

Potentially long-running workspace entry points are asynchronous from the
first implementation:

```rust
async fn rebuild(
    &self,
    context: &QueryContext,
) -> Result<Arc<WorkspaceSnapshot>, QueryError>;

async fn hover(
    &self,
    context: &QueryContext,
    request: HoverRequest,
) -> Result<Option<Hover>, QueryError>;
```

The asynchronous contract extends through module discovery, recovery,
semantic analysis, metadata partial evaluation, and any other stage whose work
may be unbounded with respect to a small syntax node. Small pure helpers with a
clear bounded cost remain ordinary synchronous functions.

An async query handler never calls `spawn`, selects a worker, or assumes a
particular executor. The scheduling layer may initially poll all analysis on
one worker. It may later use `spawn_blocking` or a thread pool without changing
query signatures or semantics.

Core APIs use standard Rust futures and XL-owned context types. Tokio,
transport request IDs, and LSP types do not appear in compiler, VM, semantic,
or workspace public APIs.

## Cancellation and checkpoints

Every async operation receives a clonable cancellation state and its captured
revision:

```rust
struct QueryContext {
    revision: Revision,
    cancellation: CancellationToken,
}

impl QueryContext {
    async fn checkpoint(&self) -> Result<(), QueryError>;
}
```

`checkpoint().await` verifies explicit cancellation and whether the operation's
revision is stale. It may cooperatively yield according to an execution budget
so that a single CPU-bound future cannot indefinitely prevent the scheduling
layer from receiving and recording cancellation.

Checkpoints occur at stable work boundaries, including:

- documents, modules, imports, declarations, and substantial HIR nodes;
- dependency graph traversal and diagnostic aggregation batches;
- partial-evaluation scheduling and recursive metadata traversal;
- VM instruction or fuel intervals during tooling execution;
- before publishing a snapshot or returning a revision-sensitive result.

Cancellation is an expected control-flow result, not a diagnostic and not an
XL runtime error. Dropping a future alone is not the implementation of
cancellation. Work that observes cancellation must abandon unpublished Work
World state and return without updating the published snapshot.

Explicit request cancellation and implicit revision staleness share the same
checkpoint path but remain distinguishable in `QueryError` for tests and
adapter policy.

## Query consistency

A query either targets a specified immutable snapshot or requests the latest
published snapshot. It never observes mutable overlays directly. Results carry
the revision from which they were produced.

If latest text is newer than the published semantic snapshot, an adapter may
wait for the matching build, answer from an explicitly identified older
snapshot where the feature permits it, or return no result. It must not label
an old result as belonging to the latest document version.

Queries over an already published immutable snapshot remain read-only and do
not execute XL code. Their async shape preserves one query contract and allows
cancellation of large result traversals such as workspace references.

## Resource and failure model

Existing fuel and quota limits remain authoritative. Cancellation is an
additional exit condition, not a replacement for deterministic resource
limits. A cancellation checkpoint does not publish tool-stage values or
partially mutate Main World.

Document edit errors, cancellation, stale revisions, workspace build failures,
and semantic uncertainty are distinct:

- an invalid edit leaves the overlay unchanged;
- cancellation and staleness return `QueryError` without diagnostics;
- recoverable current-source failures appear in a current partial snapshot;
- `Unknown` and `Conflicted` remain semantic fact states inside that snapshot.

## Dependency and crate boundary

`crop` belongs to the protocol-independent workspace/source layer and remains
encapsulated behind XL document types. No LSP dependency is added by this RFC.

The next RFC will add a separate LSP adapter crate or binary with an
asynchronous transport stack. That adapter will map negotiated position
encodings and protocol cancellation onto this RFC's types. Its runtime and
scheduling policy will not become dependencies of the `xl` compiler crate.

## Acceptance criteria

1. open overlays shadow disk modules without changing canonical import
   identity;
2. open, ordered change, full replacement, and close each create a new
   workspace revision;
3. old document and workspace snapshots remain unchanged after later edits;
4. edits use validated UTF-8 byte ranges and reject invalid transactions
   atomically;
5. UTF-8, UTF-16, and UTF-32 positions round-trip through byte offsets for
   ASCII, CJK, BMP non-ASCII, emoji, combining characters, CRLF, and empty
   trailing lines;
6. UTF-16 positions inside surrogate pairs and byte positions inside UTF-8
   scalars are rejected;
7. complete rope lexing produces the same tokens, diagnostics, and global byte
   spans as contiguous lexing for tokens split at every tested chunk boundary;
8. syntax and lowering obtain token text without a full-document flat copy;
9. workspace build and potentially long query entry points are async and carry
   a `QueryContext` through their long-running stages;
10. explicit cancellation stops a build at a checkpoint and publishes no
    snapshot;
11. a build made stale by a newer edit stops or is rejected before publication;
12. a current recoverable partial build may publish atomically with exactly
    one revision;
13. query handlers contain no executor-specific spawn policy;
14. strict CLI and runtime behavior remains unchanged;
15. no LSP, Tokio, `line-index`, `lsp-document`, or `ropey` dependency is added.

## Implementation plan

1. add private `DocumentText` and public document/revision/position types using
   `crop` with `utf16-metric`;
2. implement validated ordered edits, immutable clone-on-write snapshots, and
   encoded position conversion;
3. abstract syntax and lowering source slices away from a global `&str`;
4. add the rope-to-Logos chunk/window bridge and equivalence tests;
5. add overlay state keyed by canonical module identity and capture immutable
   build inputs per revision;
6. tag `WorkspaceSnapshot` and query results with `Revision`;
7. introduce `QueryContext`, cancellation state, query errors, and async
   workspace/query entry points;
8. place cooperative checkpoints through workspace traversal and tooling VM
   execution without putting scheduling policy in handlers;
9. test rapid revisions, cancellation, stale publication, Unicode positions,
   snapshot isolation, and unchanged strict commands.

## Deferred work

- concrete asynchronous LSP transport and runtime selection;
- LSP diagnostics, hover, definition, and references handlers;
- incremental token, CST, HIR, module, or semantic reuse;
- concurrent query execution and worker-pool sizing;
- edit coalescing and backpressure policy;
- persistent snapshots across process restarts;
- terminal grapheme truncation and configurable ambiguous-width policy;
- filesystem watching and package/multi-root workspace semantics.

## Rejected alternatives

### Keep synchronous compiler queries behind async LSP handlers

An async wrapper cannot interrupt synchronous CPU work and makes scheduling
behavior part of the adapter. XL instead gives long-running query stages an
async contract and a cooperative cancellation context.

### Let every query handler spawn its own task

This couples semantic code to a runtime and prevents the workspace scheduler
from controlling ordering, coalescing, and resource limits. Handlers describe
work; the scheduling layer chooses where futures are polled.

### Use `String` plus a line index

This is simple for full synchronization but copies the complete document for
ordinary range edits and does not cheaply preserve old document revisions. A
persistent byte-oriented rope better matches the snapshot model.

### Use `lsp-document`

Its types and UTF-16 assumptions belong to an older protocol-specific adapter,
not XL's negotiated-encoding document core. It also performs whole-string
replacement for edits.

### Flatten the rope before every parse

XL tokens and CST nodes retain kinds and byte spans rather than borrowed token
text. A chunk/window bridge can lex the full rope while allocating only for
cross-chunk tokens or later fragmented slices.

### Require incremental parsing with the rope

Persistent text and incremental protocol edits are useful independently of
syntax reuse. Full lexing and parsing establish a correctness baseline before
cache invalidation is designed.

### Store UTF-16 or display columns in core spans

Protocol code units and terminal cells are projections of source text, not
stable source identities. Storing them would duplicate derived state and make
edits invalidate multiple location systems.

## Implementation result

Implemented with `crop 0.4.3` and its `utf16-metric` feature. Every
`SourceFile`, including disk, core, JSON, test, and overlay sources, now owns
the same private `DocumentText` rope representation. `SourceDatabase` no
longer stores a parallel flat string or line-start index. Byte slices borrow a
single rope chunk when possible and materialize only the requested fragmented
span.

`DocumentText` provides validated byte edits and explicit UTF-8, UTF-16, and
UTF-32 line-column conversion. Protocol conversion rejects UTF-8 and surrogate
interiors and treats CRLF interiors as non-protocol positions. The existing
one-based scalar CLI source position remains a separate projection so a
diagnostic located on line-ending bytes still renders safely. Terminal display
cells remain presentation work for the existing diagnostic layer.

Both XL and JSON Logos front ends now consume ordered rope chunks through a
window bridge. The bridge retains an unmatched string/interpolation region and
a conservative root-token suffix, relocates committed spans and diagnostics to
global byte offsets, and never retains borrowed token payloads. Generated
parsers gained internal token-stream constructors; CST validation and AST/JSON
lowering use rope byte slices. Tests compare contiguous and chunked tokenization
at every character split and across actual multi-leaf Crop documents.

`Workspace` owns canonical overlay paths, document versions, a monotonic
`RevisionClock`, and one atomically published `Arc<WorkspaceSnapshot>`. Ordered
change transactions clone and edit the COW rope before replacing the current
document. Open overlays shadow disk sources in the recoverable module graph.
Valid overlay dependencies are evaluated in an isolated tooling world and
supply their real exported values to dependent partial analysis; failed
overlays provide no fabricated capability.

`QueryContext` carries a revision clock and clonable cancellation token.
Checkpoint futures check cancellation and staleness before and after yielding.
Recoverable graph traversal is async, including recursive module loads, and
awaits checkpoints at module and partial-analysis boundaries without spawning
or choosing an executor. The synchronous CLI recovery entry point polls the
same future as a compatibility adapter.

Workspace snapshots record their revision. Async diagnostics, definition,
reference, references, type, and export queries verify that their context and
snapshot revisions match. A rebuild captures COW overlay snapshots, rejects a
cancelled or stale result before publication, and publishes under the workspace
state lock only after a final revision check.

Tool-stage `QuotaAccount`s optionally carry the query context. Existing VM
fuel, call, and back-edge checkpoints then terminate cancelled or stale tooling
execution early. Ordinary runtime accounts have no query probe and preserve
their existing behavior and quotas.

Tests cover COW and transactional edits, negotiated Unicode coordinates,
surrogate and CRLF boundaries, chunk-equivalent XL and JSON lexing, stale and
explicitly cancelled builds, atomic publication, overlay shadowing, and real
cross-module overlay capabilities. The workspace tests, CLI tests, strict
Clippy, formatting, and diff checks pass. No LSP transport, async runtime,
`line-index`, `lsp-document`, `ropey`, incremental parser, or executor-specific
spawn policy was introduced.
