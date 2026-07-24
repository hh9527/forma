# RFC 0047: Conservative semantic completion

- Status: Implemented
- Depends on: RFC 0038, RFC 0039, RFC 0042, RFC 0046

## Summary

XL adds protocol-independent completion queries for statically known module
exports and authoritative Struct fields, then exposes them through
`textDocument/completion` in `xl-lsp`.

Completion is deliberately conservative. The semantic snapshot identifies a
member-access context, resolves its receiver, and returns candidates only when
the receiver is an imported module with known exports or has one unambiguous
Struct type. `Any`, unknown or failed facts, ambiguous unions, incomplete
receivers, and unresolved names produce an empty complete result rather than
guessed candidates.

The query remains asynchronous and receives the same revision and cancellation
context as every other LSP query. It executes no XL code and uses only one
immutable published snapshot.

## Motivation

RFC 0046 completes the asynchronous LSP lifecycle and navigation path but
intentionally omits completion. Adding completion directly in the adapter
would make protocol code inspect source strings, HIR details, import bindings,
and type graph nodes. That would duplicate semantic rules and make completion
disagree with hover and navigation.

RFC 0038 instead requires completion to expose only module exports and model
fields already justified by the authoritative snapshot. The compiler query
layer therefore owns context recognition and candidate selection. The adapter
only converts negotiated positions and maps structured candidates to LSP
items.

## Goals

1. recognize completion at `receiver.` and `receiver.<prefix>` without
   requiring a valid enclosing expression;
2. return known exports for a receiver resolved to an imported module;
3. return fields for a receiver with one authoritative Struct type;
4. follow transparent type references safely through recursive type graphs;
5. preserve explicit empty results for unknown, ambiguous, or incomplete
   semantic states;
6. keep candidate ordering deterministic and independent of editor behavior;
7. expose completion through the RFC 0046 async, cancellation, revision, and
   position-encoding boundaries.

## Non-goals

- global bindings, keywords, snippets, function-call templates, Enum variants,
  Dict keys, tuple indices, or primitive methods;
- fuzzy matching, usage ranking, recent-history ranking, or machine-learned
  ordering;
- auto-imports, additional text edits, import insertion, or completion item
  resolve;
- completion inside strings, comments, import paths, patterns, or declarations;
- union member intersection, speculative narrowing, subtyping, or inference
  beyond facts already stored in the snapshot;
- executing decorators, modules, native functions, or other XL code per
  completion request;
- incremental parsing or maintaining a second parser for incomplete text.

## Semantic query model

The protocol-independent semantic layer gains structured completion types:

```rust
enum CompletionKind {
    ModuleExport,
    StructField,
}

struct CompletionCandidate {
    label: String,
    kind: CompletionKind,
    ty: WorkspaceTypeId,
}

struct CompletionResult {
    replacement: TextRange,
    candidates: Vec<CompletionCandidate>,
}
```

The async entry point is conceptually:

```rust
async fn query_completion_at(
    &self,
    context: &QueryContext,
    location: Location,
) -> Result<Option<CompletionResult>, QueryError>;
```

`None` means the location is not a supported completion context. `Some` with
an empty candidate list means the context is recognized but the current
snapshot cannot justify semantic candidates. The distinction is useful to
tests and future composition, although both map to an empty LSP list in this
RFC.

Candidates contain graph identities and optional deterministic display data,
not heap values, AST references, or LSP types. They belong to the result's
snapshot revision.

## Completion context

Completion is recognized only within a plain member suffix:

```text
receiver.<prefix>
         ^ replacement starts after the dot
```

The cursor must be at the end of `<prefix>`. The prefix is empty or consists
only of identifier-continuation characters accepted by the XL lexer. The
replacement range covers exactly the prefix, never the receiver or dot.

The snapshot uses its rope-backed `DocumentText`, the same XL lexer, and
semantic source ranges to locate the dot and receiver. The initial
implementation may tokenize the complete snapshot document for a request; it
does not flatten the document and does not run a second expression parser.
Trivia, comments, string contents, and tokens unrelated to member syntax cannot
be mistaken for a completion context.

The receiver must correspond to the nearest complete semantic expression or
resolved reference ending before the dot. A damaged enclosing call, block, or
later sibling does not invalidate an otherwise available receiver. A missing,
lexically damaged, or ambiguous receiver produces an empty recognized result.

Candidates are filtered by the typed prefix using exact, case-sensitive
`starts_with` semantics. The server does not fuzzy-rank. Results are ordered by
label, then kind, and contain no duplicates.

## Module export completion

If the receiver resolves to a definition whose `import_target` identifies a
workspace module, completion invokes the existing async `query_exports_of`
path for that module. Each authoritative export becomes a `ModuleExport`
candidate with its existing `WorkspaceTypeId`.

Unavailable modules and modules without an authoritative Struct-shaped export
root return no candidates. Completion never reads or executes the imported
module separately and never offers local implementation details that are not
exports.

Module export completion takes precedence over ordinary Struct completion for
an import binding. This preserves module namespace semantics even when the
module's exported value is itself Struct-shaped.

## Struct field completion

For a non-module receiver, completion obtains the receiver's existing semantic
type fact and asks the type graph for members. `members_of` follows
`WorkspaceTypeNode::Ref` nodes with cycle detection. It returns fields only
when the resolved node is exactly `WorkspaceTypeNode::Struct`.

It returns no fields for:

- `Any`, Pending, primitive, Array, Tuple, Enum, Function, or unknown nodes;
- a missing, Unknown, Conflicted, or Incomputable receiver fact;
- Union, even if every current arm happens to contain a same-named field;
- a reference cycle that reaches no concrete Struct node.

This RFC does not merge fields across unions or derive members from runtime
Dict values. Such behavior would require separate semantics rather than a
presentation choice.

## Incomplete syntax and parsing

`receiver.` is commonly incomplete according to the ordinary expression
grammar. Completion relies on ordinary XL lexer tokens plus recovered semantic
ranges around the cursor. It does not introduce an incremental parser or a
completion-only parser.

The implementation may add a narrow token/syntax query that identifies the
member suffix and receiver range. It must use the same XL lexer tokens and byte
spans as normal parsing. Ad hoc scanning of raw characters in the LSP adapter
is rejected.

## LSP mapping

Initialization advertises a completion provider with `.` as its trigger
character. The server handles `textDocument/completion` through the raw
request-ID dispatcher established by RFC 0046, so cancellation maps to the
same XL token registry and stale results map to `ContentModified`.

Every candidate becomes a plain `CompletionItem`:

- `label` is the exact export or field name;
- `kind` is `MODULE` for module exports and `FIELD` for Struct fields;
- `detail` is the deterministic display form of its `WorkspaceTypeId` when
  available;
- `text_edit` replaces the query's byte replacement range after converting it
  to the negotiated LSP encoding;
- `sort_text` is the label, preserving deterministic lexical order.

The response is a complete `CompletionList` with `isIncomplete = false`.
There are no snippets, commands, commit characters, additional edits, or
resolve data. Client-side filtering may refine the already prefix-filtered
list but cannot introduce semantic candidates.

## Cancellation and consistency

Context recognition, receiver resolution, module export traversal, type
reference traversal, candidate collection, and final response conversion all
belong to one snapshot revision. The query awaits checkpoints before semantic
resolution, during potentially recursive or large candidate traversal, and
before returning.

An explicit cancel returns `RequestCancelled`. A newer document revision
returns `ContentModified`. A result from an older snapshot is never relabelled
as current. Completion does not cause a rebuild and does not answer from
mutable overlay text that is newer than the published snapshot.

## Acceptance criteria

1. initialization advertises completion with `.` as a trigger and does not
   advertise completion-item resolve;
2. `receiver.` and `receiver.<prefix>` produce the correct byte replacement
   range in UTF-8 source coordinates;
3. module completion returns only authoritative exports of the resolved import
   target, with deterministic labels and type identities;
4. Struct completion returns only authoritative fields of the receiver's exact
   Struct type and safely follows named/ref nodes;
5. typed prefixes filter candidates case-sensitively and response ordering is
   deterministic with no duplicates;
6. `Any`, Unknown, Conflicted, Incomputable, Union, unresolved imports,
   malformed receivers, comments, and strings produce no guessed candidates;
7. incomplete syntax after the dot does not require flattening the rope, a
   second parser, or incremental parsing;
8. LSP items use MODULE/FIELD kinds, deterministic type detail, and a text edit
   whose range is correct under UTF-8, UTF-16, and UTF-32 negotiation;
9. completion uses the same request-ID cancellation token and returns
   `RequestCancelled` when stopped at a checkpoint;
10. an edit racing completion returns `ContentModified` and sends no stale
    candidate list;
11. completion executes no XL code, mutates no world, and reads one immutable
    snapshot;
12. existing workspace, CLI, diagnostics, hover, definition, and references
    behavior remains unchanged.

## Implementation plan

1. add lossless member-suffix context recognition over existing token spans;
2. add graph-safe `members_of` and structured completion result types to the
   semantic snapshot;
3. implement async `query_completion_at` with revision and cancellation
   checkpoints;
4. add query tests for imports, Structs, prefixes, recursive refs, incomplete
   syntax, and conservative empty states;
5. advertise and dispatch LSP completion through RFC 0046's raw service;
6. map candidate ranges and kinds under all negotiated encodings;
7. add cancellation, stale-race, capability, and protocol-result tests;
8. run workspace tests, strict Clippy, formatting, and diff checks.

## Implementation result

Implemented in the semantic workspace and asynchronous LSP adapter. Completion
context recognition tokenizes the rope-backed document with the existing XL
lexer, keeps UTF-8 byte replacement ranges in the semantic result, and resolves
only snapshot-backed import exports or exact Struct members. Empty-prefix
module completion remains available when the parser recovers an incomplete
trailing dot; the lexical fallback is deliberately restricted to a unique,
preceding import definition in the same module.

The asynchronous recoverable workspace now retains complete `Analysis` facts
when strict analysis and evaluation succeed. This makes module result and
expression types authoritative in the published snapshot, while invalid or
failed modules continue to expose partial facts and conservative empty
completion results. Query checkpoints cover context resolution, graph/member
traversal, candidate construction, cancellation, and stale-revision checks.

`xl-lsp` advertises `.` completion without resolve support and maps candidates
to deterministic complete lists with explicit encoding-aware text edits.
Tests cover module exports, Struct fields, typed and empty prefixes, recursive
type references, conservative contexts, UTF-16 edits, request cancellation,
and revision races. The final workspace run passed 174 core tests with one
manual benchmark ignored, 9 CLI tests, 19 LSP tests, strict Clippy, formatting,
and whitespace validation.

## Deferred work

- local/global identifier and keyword completion;
- Enum variants and constructor completion;
- callable signatures, argument labels, and snippets;
- union-member intersection and control-flow narrowing;
- auto-imports and completion-item resolve;
- fuzzy ranking, usage statistics, and configurable ordering;
- completion inside import strings or other specialized syntax contexts;
- incremental parsing and semantic caches.

## Rejected alternatives

### Infer completion context in `xl-lsp`

That would duplicate lexer, syntax, receiver-resolution, import, and type rules
inside protocol code. Completion context and candidates are compiler queries;
the adapter only projects them into LSP structures.

### Return every visible binding

Global identifier completion has scope, shadowing, keyword, and ranking
semantics beyond this milestone. This RFC validates known member completion
without presenting a noisy or misleading global list.

### Treat Dict runtime keys as fields

Dict contents are values, not authoritative static members. Discovering them
would require executing or retaining runtime data and would make completion
depend on incidental evaluation results.

### Merge members across Union arms

Even an intersection of same-named fields needs rules for compatible field
types and future narrowing behavior. Returning no candidates is the only
conservative initial contract.

### Offer candidates for `Any` or unknown facts

Unknown precision is not evidence that a field exists. Guessing from nearby
syntax, old revisions, runtime values, or similarly named types would violate
the snapshot's explicit fact states.

### Reparse a flattened prefix on every request

XL already tokenizes rope-backed documents into kinds and byte spans. A second
parser would diverge on malformed input and undo RFC 0045's non-flattening
source boundary.
