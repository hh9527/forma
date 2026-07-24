# RFC 0046: Asynchronous LSP adapter

- Status: Implemented
- Depends on: RFC 0038, RFC 0045

## Summary

XL adds a separate `xl-lsp` workspace crate and binary. It uses `async-lsp`
with a Tokio current-thread runtime to provide an asynchronous stdio Language
Server Protocol adapter over RFC 0045's workspace, document, revision, and
query APIs.

The adapter owns protocol lifecycle, URI conversion, position-encoding
negotiation, incremental text synchronization, request scheduling, and result
conversion. It preserves each JSON-RPC request ID until it has created an XL
`CancellationToken`; `$/cancelRequest` sets that token so CPU-bound work can
stop at cooperative checkpoints. Dropping or aborting only the outer handler
future is not XL's cancellation mechanism.

The first server publishes diagnostics and supports hover, definition, and
references. Completion and incremental parsing remain deferred.

## Motivation

RFC 0045 deliberately established an executor-independent async query contract
before choosing an LSP stack. The compiler can now capture immutable document
and workspace revisions, accept range edits, rebuild asynchronously, reject
stale publication, and stop long-running analysis through explicit query
checkpoints. The next step is to prove that these boundaries compose with a
real editor protocol without moving LSP types or runtime policy into `xl`.

Framework cancellation alone is insufficient for this design. Both common
Tower-based LSP stacks implement cancellation primarily by aborting and
dropping the request future. That can release protocol resources, but it does
not by itself guarantee that CPU work or separately scheduled analysis sees a
cancellation signal. XL needs an adapter-owned request registry that maps the
wire request ID to the exact token carried through its query pipeline.

## Goals

1. provide an asynchronous stdio LSP server in a separate `xl-lsp` crate;
2. implement initialize, initialized, shutdown, and exit lifecycle semantics;
3. negotiate UTF-8, UTF-16, or UTF-32 positions and default correctly to
   UTF-16;
4. map open, incremental change, full replacement, and close notifications to
   RFC 0045 overlays and document versions;
5. build and publish diagnostics only for the current document revision;
6. provide hover, go to definition, and workspace references from immutable
   semantic snapshots;
7. propagate `$/cancelRequest` into XL's cooperative `CancellationToken`;
8. suppress stale responses and keep scheduling policy outside compiler query
   handlers.

## Non-goals

- completion, signature help, rename, formatting, code actions, semantic
  tokens, inlay hints, or workspace symbols;
- incremental lexing, parsing, HIR, module, or TypeGraph reuse;
- a worker pool, parallel analysis, or a fixed task-per-request policy;
- filesystem watching, package discovery, or multi-root workspaces;
- TCP, WebSocket, or in-process editor transports;
- exposing LSP, Tokio, or `async-lsp` types from the `xl` crate;
- preserving results from an older snapshot while presenting them as current.

## Crate and dependency boundary

The workspace gains a binary crate named `xl-lsp`. Its initial adapter
dependencies are:

```toml
async-lsp = { version = "0.2.4", features = ["stdio", "tokio"] }
tokio = { version = "1", features = ["macros", "rt", "sync"] }
tokio-util = { version = "0.7", features = ["compat"] }
xl = { path = "../xl" }
```

`async-lsp` supplies JSON-RPC framing, peer communication, dynamic request
objects, and the asynchronous main loop. Its `lsp-types 0.95` dependency is
the protocol vocabulary used only inside `xl-lsp`. `tokio-util` adapts Tokio
stdio where required by the futures-IO transport interface.

The initial binary uses a current-thread runtime. This is an execution policy,
not a promise that XL analysis is permanently single-threaded. RFC 0045's
checkpoints yield often enough for the runtime to receive document changes and
cancellation while CPU-bound query futures are polled on that thread. A later
scheduler may move selected futures to workers without changing query handler
signatures or semantics.

`tower-lsp-server` is not selected. Although it is actively maintained and has
convenient native async language-server methods, its typed backend handlers no
longer receive the JSON-RPC request ID. Connecting its built-in abort-based
pending-request map to an explicit XL token would require task-local context,
a parallel raw service layer, or bypassing its principal abstraction.
`async-lsp` exposes `Service<AnyRequest>` at the adapter boundary, including
the request ID, and therefore fits XL's explicit query context with less
competing lifecycle state.

## Server lifecycle

Before `initialize`, the server rejects ordinary requests. Initialization
chooses one position encoding, records the workspace root, and returns
capabilities for incremental text synchronization, hover, definition, and
references. If the client omits `general.positionEncodings`, the selected
encoding is UTF-16. Otherwise the server selects the first mutually supported
encoding according to a documented server preference, initially UTF-8,
UTF-16, then UTF-32.

The adapter accepts document notifications only after initialization. A
successful `shutdown` stops new semantic work and cancels outstanding request
tokens. `exit` terminates the main loop; exit before shutdown remains an
abnormal protocol termination. End-of-file also cancels unpublished work and
closes the server without publishing further messages.

URIs are adapter data. File URIs are converted to normalized local paths and
then to XL canonical document identities. Unsupported URI schemes produce no
semantic result and never become fabricated filesystem paths.

## Document synchronization

The server advertises incremental text synchronization with open and close
notifications. `didOpen` installs a full-text overlay at the supplied version.
`didChange` requires a version newer than the current open version and applies
its content changes in order. A change without a range is a full replacement;
a ranged change is converted from the negotiated protocol encoding against
the text produced by all preceding changes in the same notification.

The complete notification is transactional. Invalid versions, positions,
ranges, surrogate interiors, CRLF interiors, or edit boundaries leave the
overlay unchanged. The server logs the protocol error and awaits a later full
replacement; it does not guess, clamp, or partially apply changes.
`didClose` removes the overlay and exposes disk text in a new revision.

Incremental synchronization does not imply incremental parsing. A successful
change may rebuild the complete workspace from the new rope snapshot.

## Scheduling and snapshots

The adapter contains a small scheduling layer around `Workspace`. It owns
pending rebuilds and requests, decides which futures are polled inline or in a
runtime task, and may coalesce rebuilds made obsolete before publication.
Semantic handlers only create an XL `QueryContext` and await the corresponding
query; they do not call `spawn`, select a worker, or know the runtime.

Each document change advances the workspace revision and schedules analysis.
Only a snapshot whose revision is still current may be published. A newer
change makes older rebuild contexts stale through RFC 0045's revision clock.
Diagnostics and request responses record the snapshot revision from which
they were produced and pass a final current-revision check immediately before
being sent.

The first implementation need not add threads or a pool. Tokio's current-thread
runtime may poll transport, scheduling tasks, and cooperative XL futures. This
keeps the initial scheduling policy small while retaining the async contract
needed for cancellation and future concurrency.

## Cooperative request cancellation

The adapter does not use `async_lsp::concurrency::Concurrency` as its request
cancellation layer. That middleware maps IDs to `AbortHandle`s and drops the
inner future on `$/cancelRequest`; it has no hook that sets XL's token.

Instead, the XL request service receives each `AnyRequest` before typed
parameter dispatch:

```text
request ID
    |
    v
create CancellationToken -> register ID -> create QueryContext -> await query
                                  ^                              |
                                  |                              v
                         $/cancelRequest                  remove on completion
```

The registry owns one token per active client request. A cancel notification
removes the matching entry and calls `cancel`; an unknown or already completed
ID is ignored as required by LSP. Normal completion also removes the entry.
Duplicate active request IDs are rejected as invalid JSON-RPC requests.

Cancellation is translated to JSON-RPC error code `RequestCancelled`
(`-32800`). Revision staleness is translated to `ContentModified` (`-32801`)
when a response is still required. Neither condition becomes an XL diagnostic.
The adapter may additionally drop a completed cancelled future, but token
propagation and checkpoints are the mechanism that stops internal work.

Shutdown, exit, and transport loss cancel every registered token. Document
changes do not masquerade as explicit request cancellation: they advance the
revision clock, and affected queries observe `QueryError::StaleRevision`.

## Protocol features

### Diagnostics

After a successful open, change, or close rebuild, the adapter publishes
diagnostics for affected open XL documents. It converts every primary range
through the negotiated encoding. Secondary labels in another source become
related information when they have a representable file URI. Diagnostics from
stale snapshots are never published.

An empty current diagnostic set is published when needed to clear an earlier
set. The first implementation uses push diagnostics; diagnostic pull is
deferred.

### Hover

Hover uses the current published snapshot and the byte position converted by
`DocumentText`. It returns the stable display form of a binding and its
computed TypeMetadata when authoritative information exists. Missing,
unknown, or conflicted semantic facts do not fabricate precision. The hover
range is returned when the queried fact has a source range.

### Definition

Definition converts the requested position to a byte offset, invokes the
snapshot definition query, and maps the resulting canonical source location
to a file URI and negotiated range. An unrepresentable or absent target
returns no location.

### References

References first resolves the definition at the requested position, then
queries references across the loaded workspace. It honors the client's
`includeDeclaration` flag, maps each result independently, and returns a
deterministically ordered list. Traversal receives the request's query context
and can stop at checkpoints.

## Error and logging policy

Protocol errors are returned with the corresponding JSON-RPC error code.
Malformed notifications cannot receive responses and are reported on stderr
or through the LSP logging channel without writing non-protocol text to
stdout. Recoverable XL analysis failures remain diagnostics in the current
snapshot. Internal adapter failures return `InternalError` without exposing
VM or host implementation details.

## Acceptance criteria

1. `xl-lsp` builds as a separate binary and no LSP or Tokio dependency appears
   in `xl`'s public dependency graph;
2. an in-memory stdio-equivalent integration test completes initialize,
   initialized, shutdown, and exit in protocol order;
3. initialization negotiates UTF-8, UTF-16, and UTF-32 and defaults to UTF-16
   when the client advertises no encodings;
4. server capabilities advertise incremental open/change/close sync, hover,
   definition, and references, but not completion;
5. open, ordered ranged changes, full replacement, and close update exactly
   one canonical overlay using monotonically increasing document versions;
6. invalid or out-of-order changes are rejected transactionally without
   changing the overlay revision;
7. Unicode request and response positions round-trip correctly in every
   negotiated encoding, including emoji and CRLF boundaries;
8. current diagnostics are published after edits and an empty publication
   clears diagnostics that disappeared;
9. hover, definition, and references are answered from one immutable current
   snapshot and never execute separate semantic rules in the adapter;
10. `$/cancelRequest` sets the exact XL token associated with the request ID,
    long-running CPU work observes it at a checkpoint, and the response is
    `RequestCancelled`;
11. an edit racing an older request or rebuild prevents stale publication and
    produces no result labelled as current;
12. duplicate request IDs, unknown cancellation IDs, shutdown with pending
    work, and transport EOF have deterministic tested behavior;
13. query handlers contain no executor-specific spawn or worker-pool policy;
14. existing compiler, CLI, and workspace tests remain unchanged and pass.

## Implementation plan

1. add the `xl-lsp` crate, dependencies, current-thread Tokio entry point, and
   `async-lsp` stdio main loop;
2. implement lifecycle state, capability negotiation, URI conversion, and
   protocol-to-XL error mapping;
3. implement transactional open/change/close notification dispatch using
   `DocumentText` position conversion and workspace overlays;
4. add the adapter scheduler for current-revision rebuilds and diagnostic
   publication;
5. implement the raw request-ID cancellation registry and typed request
   dispatch without `async-lsp`'s abort-only cancellation middleware;
6. map diagnostics, hover, definition, and references to LSP structures;
7. add in-memory transport tests for lifecycle, encodings, edits, stale
   suppression, cooperative cancellation, and feature results;
8. run workspace tests, strict Clippy, formatting, and diff checks.

## Deferred work

- conservative completion from module exports and authoritative Struct fields;
- request prioritization, bounded concurrency, worker threads, and pools;
- rebuild debounce and measured edit coalescing policy;
- pull diagnostics and diagnostic refresh;
- filesystem watching and multi-root workspace discovery;
- incremental compiler caches and parser reuse;
- editor packaging, installation, and client-specific configuration.

## Rejected alternatives

### Use a synchronous transport around async queries

The protocol must continue receiving changes and cancellation while analysis
is pending. A synchronous event loop would recreate the lifecycle mismatch
that RFC 0045 removed from compiler queries.

### Use only framework future abortion for cancellation

Dropping an outer future is not a cooperative signal to CPU work and does not
cover work already handed to another scheduler. XL explicitly maps request IDs
to tokens checked inside parsing, analysis, and tooling execution.

### Use `tower-lsp-server`

Its active ecosystem and typed native async trait are useful, but its standard
backend handler boundary omits the request ID needed to construct XL's
per-request query context. Recovering that context would add a second raw layer
or implicit task-local state. `async-lsp` exposes the lower service boundary
directly and keeps the adapter's ownership explicit.

### Use `async-lsp::Concurrency` unchanged

Its cancellation map stores abort handles rather than XL tokens. The adapter
needs the same request ID to control both protocol completion and internal
cooperative cancellation, so it owns that registry and scheduling policy.

### Put LSP handlers in `xl`

That would leak protocol positions, URIs, framework versions, and runtime
lifecycle into the compiler boundary. A separate crate enforces the intended
one-way dependency.

### Support only UTF-16

UTF-16 is the compatibility default, but clients such as Helix and Neovim can
use UTF-8 directly. RFC 0045 already provides all three correct projections,
so the adapter negotiates them rather than narrowing the core model.

### Add completion to the first adapter

Transport, versioning, diagnostics, navigation, and cancellation already form
a complete architectural test. Completion has additional context and ranking
semantics and remains the next tooling RFC.

## Implementation result

Implemented as the separate `xl-lsp` crate and binary using `async-lsp 0.2.4`
and a Tokio current-thread runtime. The binary selects its workspace root from
the first workspace folder, `rootUri`, legacy `rootPath`, or finally its launch
directory. LSP and runtime dependencies remain outside the `xl` crate.

The adapter implements its own `Service<AnyRequest>` dispatch boundary so the
wire request ID remains available when it creates an XL `CancellationToken`.
Active IDs map directly to tokens; `$/cancelRequest`, shutdown, exit, and
transport termination cancel those tokens. Query cancellation and stale
revisions map to `RequestCancelled` and `ContentModified` respectively. The
adapter does not use `async-lsp::Concurrency` and semantic handlers contain no
spawn policy.

Initialization negotiates UTF-8, UTF-16, or UTF-32 and defaults to UTF-16. The
server advertises incremental open/change/close synchronization plus hover,
definition, and references. Ordered changes are converted against the text
produced by preceding edits and committed through the transactional RFC 0045
workspace API. Unsupported URIs and malformed positions do not fabricate
paths or clamp offsets.

Successful document notifications schedule cooperative workspace rebuilds on
the adapter runtime. Revision checks prevent stale snapshot publication and a
final check precedes push diagnostics. Diagnostics are published only for open
documents with their current version, empty sets clear previous results, and
secondary source labels become related information. Hover, definition, and
references resolve through immutable snapshot queries and convert all ranges
through the negotiated document encoding.

Adapter tests cover encoding selection, initialization capabilities and root
selection, ordered UTF-16 edits, exact request-ID token cancellation, and a
framed in-memory initialize/initialized/shutdown/exit exchange through the
real asynchronous main loop. Workspace tests, strict Clippy, formatting, and
diff checks pass with the implementation.
