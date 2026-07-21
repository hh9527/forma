# RFC 0005: Unified Source, Lossless Parsing, and Provenance

- Status: Accepted
- Implementation: Pending

## Summary

This RFC replaces the MVP's separate, fail-fast XL and JSON parsers with a
shared source and diagnostic model. Logos lexers and Lelwel-generated parsers
produce lossless concrete syntax trees for both formats. The compiler, CLI,
and a future LSP consume the same parse results and semantic lowering.

Every semantic node that can cause a user-visible error retains a source span.
Data values remain independent of source locations; a separate provenance tree
tracks where imported data originated. This permits a validation diagnostic to
identify both the invalid data and the type or rule that rejected it.

## Source model

All parsed input is registered in a `SourceDatabase`. A source has a stable
`SourceId`, a display name, UTF-8 text, and a lazily or eagerly constructed line
index. Filesystem paths are metadata rather than source identity: stdin and
in-memory sources use the same representation as files.

```text
Span = SourceId + TextRange
TextRange = start byte offset .. end byte offset
```

Ranges are half-open UTF-8 byte ranges. Syntax and semantic structures do not
store line and column numbers. Diagnostics convert byte offsets through the
source database when rendered. A future LSP adapter additionally converts them
to UTF-16 positions.

The source database owns source text independently of syntax trees. This lets
Lelwel's owned `CstData` be retained without a self-referential Rust structure.

## Parse contract

XL and JSON each have their own Logos token definition and Lelwel grammar, but
they expose the same conceptual result:

```text
Parse<T> {
    syntax: T,
    diagnostics: Vec<Diagnostic>,
}
```

Parsing is resilient. Invalid input produces as much CST as recovery permits,
including error nodes and multiple diagnostics. A successful parse has no
error-severity diagnostics. Compiler and CLI entry points reject erroneous
parses before semantic evaluation; editor tooling may continue to inspect and
lower recoverable subtrees.

Tokens retain their exact source ranges. Whitespace and comments are emitted as
trivia and preserved in the CST. The CST is therefore sufficient to reconstruct
the source text byte-for-byte. Semantic AST/HIR is produced only by lowering
the CST; no second parser is permitted for compilation or editor analysis.

Full-file lexing, parsing, and lowering occur after each edit. Incremental
reparsing is not part of this design.

## XL syntax lowering

The existing XL surface syntax and semantics remain unchanged. The Lelwel
grammar becomes the authoritative grammar, replacing the hand-written lexer
and Pratt parser. Precedence, associativity, pipeline elaboration, block rules,
patterns, imports, and literals must preserve their RFC 0002 and RFC 0004
behavior.

Lowered XL nodes carry spans. A node's span covers the complete construct,
including delimiters but excluding unrelated leading and trailing trivia.
Names, fields, literals, and operators retain narrower spans where diagnostics
need to identify a particular token.

Lexical and syntactic diagnostics use ranges rather than a single location.
Malformed literals are represented in the CST and reported during lexing or
lowering without silently substituting a semantic value.

## JSON syntax lowering

JSON is parsed by its own Logos lexer and Lelwel grammar rather than directly
into runtime values. Its CST is lossless and uses the same source and diagnostic
types as XL. Strict MVP behavior remains in force:

- object keys must be strings and must be unique;
- integers must fit in `i64`;
- floats must be finite;
- string escapes and Unicode surrogate pairs must be valid;
- trailing non-trivia input is an error.

These constraints may be diagnosed during lexing, parsing, or lowering, but all
diagnostics carry precise source ranges. JSON lowering produces both an XL
runtime value and provenance for that value.

## Provenance

Runtime `Value` does not contain a span. Source location is observational
metadata and must not affect value equality, dictionary shape interning,
bytecode constants, or VM execution.

Instead, data lowering creates a provenance tree parallel to the value. Each
entry identifies the span of a value; dictionary entries may additionally
identify the key span. Child provenance follows array indexes and dictionary
keys, so transformations and validators can address a precise source value.

For this RFC, provenance is guaranteed for values directly lowered from JSON
and for semantic XL expressions. Arbitrary provenance propagation through
function evaluation is deferred. Validation diagnostics must retain distinct
labels for:

- the primary data span that failed validation, when available;
- the type declaration or validator expression that imposed the requirement,
  when available.

Absence of provenance is supported for programmatically constructed or external
host values and must not prevent validation.

## Diagnostics

A diagnostic contains a severity, a message, a primary labeled span, and zero
or more secondary labeled spans. It may also carry notes. Rendering is a client
concern: CLI rendering uses source names and human line/column positions, while
future LSP rendering uses protocol ranges.

The old `FrontendError` and `JsonError` types may remain as compatibility
wrappers at public API boundaries during migration, but parsing and lowering
internally use the unified diagnostic representation.

## Tooling boundary

This RFC supplies the syntax substrate needed by an LSP but does not implement
an LSP server, document synchronization, completion, rename, formatting, or
incremental semantic queries. Those features will consume `SourceDatabase`,
lossless CST, lowering, and diagnostics in later RFCs.

Lelwel's owned arena-based CST is the canonical first-generation syntax tree.
Rowan is not introduced. It may be reconsidered if formatter, refactoring, tree
pointer, or measured local-reparse requirements justify another syntax-tree
representation.

## Rejected alternatives

### Keep the hand-written parsers

The MVP parsers discard trivia, fail on the first error, and directly construct
semantic values. Extending them independently for editor recovery would create
two grammar implementations and make drift likely.

### Tokora

Tokora offers strong control over combinator parsing, recovery, and optional
Rowan events. XL currently benefits more from Lelwel's declarative grammar and
generated recovery sets. Tokora remains an option if concrete XL grammar cases
show that Lelwel cannot provide adequate recovery or CST structure.

### Rowan immediately

Rowan supplies persistent green/red trees, not a parser or automatic incremental
reparse. Lelwel's owned `CstData` meets the current storage and traversal needs;
adding Rowan now would create a second CST representation without a demonstrated
consumer.

### Store spans inside runtime values

This would make provenance part of the VM representation and complicate value
identity, sharing, and generated data. A parallel provenance tree keeps runtime
semantics pure while preserving source information for tools.

## Implementation plan

1. Add source identifiers, text ranges, line indexing, labeled diagnostics, and
   CLI rendering.
2. Replace the XL lexer/parser with Logos and a Lelwel grammar that builds a
   lossless CST, then lower it to the existing semantic AST.
3. Replace direct JSON decoding with a lossless JSON CST and a lowering pass.
4. Attach spans to semantic XL syntax and introduce JSON value provenance.
5. Thread sources and diagnostics through module loading, checking, validation,
   and CLI commands without changing successful program behavior.
6. Document the parsing architecture and measure representative full-file parse
   latency to establish a baseline rather than an optimization target.

## Acceptance criteria

1. XL and JSON lexers use Logos and parsers are generated by Lelwel.
2. Each parser returns a lossless CST whose token text reconstructs the original
   UTF-8 source, including whitespace and comments where the format permits.
3. Normal compilation, CLI checking, and future-tooling entry points share the
   same parser and CST-to-semantic lowering path.
4. Valid programs and JSON accepted by the MVP retain their values, inferred
   types, bytecode behavior, and CLI output.
5. Recoverable malformed XL and JSON inputs produce a CST and more than one
   diagnostic when independent errors are present.
6. Every diagnostic range resolves through the source database to correct
   human line and column positions, including non-ASCII text before the range.
7. JSON duplicate keys, invalid numbers, and invalid escapes identify the
   offending source range.
8. JSON values retain path-addressable provenance without changing `Value`
   equality or VM representation.
9. A validation failure can display the invalid JSON value's location and the
   applicable XL type or rule location when both are available.
10. Representative full-file parsing is covered by a repeatable benchmark or
    ignored timing test, with no incremental parsing implementation.
11. Existing tests plus focused CST, recovery, span, provenance, and diagnostic
    tests pass; formatting and strict Clippy checks pass.

