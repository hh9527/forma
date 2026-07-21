# RFC 0008: Typed Syntax Views and Tolerant Lexical Errors

- Status: Accepted
- Implementation: Complete

## Summary

XL adds a typed, read-only AST facade over Lelwel's lossless and resilient CST.
Typed syntax nodes are lightweight queries, not a second allocated tree. A
missing child or token is represented by `None`; a separate syntax-validation
pass records missing-slot diagnostics. Existing owned `Located<T>` nodes remain
the complete semantic AST used by analysis and compilation.

Lexically recognizable errors remain valid CST tokens whenever their local
shape is known. In particular, unsupported and malformed string escapes are
not converted to the parser-skipped generic `Error` token.

This RFC establishes the syntax substrate needed by an editor. It does not add
an LSP server, incremental identity, tolerant type inference, LIR, or runtime
debug maps.

## Layers

The frontend has four explicit layers:

```text
Logos tokens
    -> Lelwel lossless resilient CST
    -> typed syntax views and syntax validation
    -> complete owned semantic AST
```

The CST is authoritative for source structure and exists for every input. It
contains trivia, lexical-error tokens, and Lelwel `Rule::Error` subtrees.

Typed syntax views wrap `(&CstData, NodeRef)` and expose grammatical slots:

```rust
pub trait AstNode<'tree>: Sized {
    fn cast(tree: &'tree CstData, node: NodeRef) -> Option<Self>;
    fn syntax(&self) -> SyntaxNode<'tree>;
}

pub struct LetBinding<'tree> {
    syntax: SyntaxNode<'tree>,
}

impl LetBinding<'_> {
    pub fn name(&self) -> Option<SyntaxToken>;
    pub fn annotation(&self) -> Option<Expr>;
    pub fn value(&self) -> Option<Expr>;
}
```

Queries are pure. Repeating a query has no diagnostic side effect and returns
the same source-backed result. Child selection follows grammar slots and child
order, not source-offset heuristics.

The owned semantic AST from RFC 0006 is unchanged. It represents executable
semantics, contains no missing/error variants, and is produced only after
syntax validation succeeds.

## Missing syntax

Lelwel represents consumed unexpected input with `Rule::Error`. A missing
expected token is not inserted into the CST; an invoked but empty rule may have
a zero-width span at the recovery position.

Typed queries therefore return `None` for absent syntax. They do not construct
fictional identifier, operator, delimiter, or expression tokens. Validation
uses the containing node and nearby grammatical delimiters to choose a
zero-width insertion location.

```rust
pub struct SyntaxIssue {
    pub location: Location,
    pub kind: SyntaxIssueKind,
}

pub enum SyntaxIssueKind {
    Missing { expected: ExpectedSyntax },
}
```

The first validator covers required program bodies, binding names and values,
named-function parameters/bodies on nodes retained by recovery, and required
expression children that are needed for useful binding and expression queries.
Lelwel's own diagnostics remain authoritative for unexpected-token recovery.
Syntax validation must avoid duplicating an equivalent parser diagnostic at the
same recovery point. A missing name after `fn` can be grammatically
indistinguishable from a valid closure and is not reclassified heuristically.

## Tolerant lexical errors

The generic lexer `Error` token remains reserved for text whose local syntactic
role cannot be preserved. It is skipped by Lelwel.

Recognizable malformed constructs instead receive dedicated public token kinds
accepted by the grammar. XL strings distinguish:

```text
EscapeSequence
UnknownEscapeSequence
UnterminatedEscapeSequence
```

JSON additionally distinguishes malformed Unicode escapes. A tolerant token
retains its complete source range and causes a lexer diagnostic, but it is
still passed to Lelwel as a string part. For example:

```text
"a\(b"

DoubleQuote
StringText("a")
UnknownEscapeSequence("\(")
StringText("b")
DoubleQuote
```

The CST remains lossless and the string literal remains queryable. Strict XL
and JSON semantic lowering still fail because the parse contains an error
diagnostic.

Malformed JSON `\u` sequences should be grouped as one intended escape where a
bounded local match is possible, rather than fragmented into a short escape
and unrelated text.

## Syntax validation

Validation is a deterministic walk over typed syntax views:

```text
query returns Some -> validate the child
query returns None -> record one missing-slot issue
Rule::Error        -> rely on the parser diagnostic for consumed input
unknown token      -> rely on the lexer diagnostic
```

The validator produces structured issues first and converts them to the common
`Diagnostic` model at the source boundary. It does not mutate the CST and does
not make getters stateful.

Parser, lexer, and syntax-validator diagnostics are merged in source order.
Exact duplicates with the same source range and message are removed. Recovery
continues after an issue, so later valid bindings remain queryable.

## Strict semantic boundary

Existing public `parse`, `check`, `run`, module loading, and JSON decoding remain
strict. Any error-severity lexer, parser, or syntax-validation diagnostic
prevents complete semantic lowering.

Tooling may retain:

```rust
pub struct SyntaxParse {
    pub cst: CstData,
    pub diagnostics: Vec<Diagnostic>,
}
```

and construct typed views on demand. No self-referential typed root is stored
inside the parse result.

## Rejected alternatives

### Add `Error` to every semantic AST enum

This leaks syntax recovery into the compiler and makes every semantic consumer
handle states that have no runtime meaning. Typed CST views already model
absence naturally.

### Allocate a second recoverable AST

It duplicates CST structure and introduces synchronization and location
problems. Typed views are small wrappers over the authoritative CST.

### Emit generic lexer `Error` for an unknown escape

Lelwel skips generic errors, which breaks an otherwise recognizable string
subtree. A dedicated token preserves both structure and diagnostics.

### Produce diagnostics from query getters

Getter side effects make repeated queries and traversal order observable.
Validation is a separate pass.

### Insert fictional missing tokens

The source contains no such token. `None` plus a zero-width structured issue is
enough, and avoids confusing source-preserving tools.

## Implementation plan

1. Add common syntax-node/token wrappers and an `AstNode` casting trait for XL.
2. Add typed program, body, binding, identifier, and expression queries over
   the existing Lelwel CST.
3. Add a pure syntax validator for required grammatical slots and merge its
   diagnostics with lexer/parser diagnostics.
4. Split XL and JSON string error recognition into valid public token kinds
   accepted by their Lelwel grammars.
5. Keep strict semantic lowering gated on the merged diagnostics.
6. Add malformed-source query, lossless reconstruction, diagnostic range,
   no-duplicate, and arbitrary-input robustness tests.

## Acceptance criteria

1. Typed syntax nodes are pointer-sized/lightweight views over `CstData` and
   `NodeRef`; no parallel recoverable tree is allocated.
2. Query getters are pure and return `Option<T>` for missing children/tokens.
3. `let x = ; let y = 2; y` retains queryable `x`, `y`, and final `y` structure
   while reporting the missing value at a zero-width location.
4. Missing binding names and required slots on retained binding/function nodes
   produce structured syntax issues without invented tokens.
5. Lelwel `Rule::Error` subtrees remain visible and lossless.
6. Unknown XL escapes and unknown/malformed JSON escapes are valid CST tokens
   with precise diagnostics and do not break their containing string node.
7. Existing complete semantic AST enums receive no syntax-error variants.
8. Strict parse, module, JSON, check, run, and CLI behavior remains unchanged.
9. Typed queries work on every CST produced for arbitrary UTF-8 input without
   panicking.
10. Workspace tests, formatting, strict Clippy, and diff checks pass.

## Implementation result

Implemented an XL typed-syntax facade over Lelwel `CstData`. `SyntaxNode`,
`SyntaxToken`, and the `AstNode` trait support typed program, body, binding,
expression, and string-literal views without allocating a parallel tree.
Required getters return `Option`, preserve grammar-slot order, and remain pure
across repeated queries.

The syntax validator reports missing names, values, paths, parameters, bodies,
and result expressions with zero-width locations. Existing parser diagnostics
at the same recovery offset take precedence, preventing duplicate user-facing
errors. Later valid bindings and result expressions remain queryable after an
earlier missing value, and `Rule::Error` subtrees stay visible and lossless.

XL strings now preserve unsupported and unterminated escapes as dedicated CST
tokens. JSON likewise preserves unknown escapes, malformed Unicode escapes,
and unterminated escapes. Each produces a precise lexer diagnostic while the
containing string remains structurally valid for Lelwel and typed queries.
Strict semantic AST, JSON decoding, module loading, compilation, and runtime
entry points remain gated on the merged diagnostics and retain their previous
behavior.
