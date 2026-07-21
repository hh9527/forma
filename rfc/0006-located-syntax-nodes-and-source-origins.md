# RFC 0006: Located Syntax Nodes and Source Origins

- Status: Accepted
- Implementation: Pending

## Summary

This RFC replaces ad hoc AST spans and `Expr::Spanned` wrappers with one
mandatory, generic location model. Every semantic syntax node lowered from a
source file is a `Located<T>`. Source identity and byte ranges are separate
compact values, and source-derived locations remain distinct from the origins
of synthetic nodes introduced by later compiler stages.

The model is designed to survive future AST-to-HIR-to-LIR lowering. This RFC
does not introduce HIR, LIR, new opcodes, or runtime provenance propagation.

## Source coordinates

Source coordinates use the following representations:

```rust
#[repr(transparent)]
pub struct SourceId(u32);

pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

pub struct Location {
    pub source: SourceId,
    pub range: TextRange,
}
```

`TextRange` is a half-open UTF-8 byte range. It must satisfy `start <= end`.
Line, Unicode-scalar column, and future LSP UTF-16 coordinates are derived from
`SourceDatabase`; they are never stored in AST or diagnostics.

A source may contain at most `u32::MAX` bytes. Registration rejects a larger
source. Lelwel's `Range<usize>` is converted through a checked constructor at
the CST-lowering boundary. Unchecked integer casts are not permitted.

`SourceId` values are meaningful only within their owning `SourceDatabase`.
Module loading and tooling therefore retain one shared database for all related
sources.

## Located source nodes

The generic source-node wrapper is:

```rust
pub struct Located<T> {
    pub value: T,
    pub location: Location,
}
```

Locations are mandatory. `Option<Location>` is not used for parsed AST nodes.
Recovered missing syntax uses a zero-width range at the recovery position;
erroneous syntax uses the range consumed by its error node.

The semantic AST follows the recursive kind pattern:

```rust
pub type Expr = Located<ExprKind>;
pub type Pattern = Located<PatternKind>;
pub type Binding = Located<BindingKind>;
pub type Block = Located<BlockKind>;
pub type Program = Located<ProgramKind>;
```

`ExprKind` recursively contains `Expr`, not `ExprKind`. The same rule applies to
other recursive syntax. Consumers match on `node.value`; there is no transparent
`Spanned` enum variant and no `unspanned()` operation.

Semantic tokens that need narrower diagnostics are also located. This includes
binding names, parameters, dictionary field names, field-access names, unary
and binary operators, and import paths. A containing expression still covers
the complete construct.

Parentheses that do not create a semantic tuple may be erased from `ExprKind`.
The retained expression keeps its own location; CST remains the authority for
delimiter-aware tooling.

## Source origins

Later lowering stages may create nodes that do not correspond one-to-one with
source syntax. They use an explicit origin model:

```rust
pub enum Origin {
    Source(Location),
    Synthetic {
        derived_from: Option<Location>,
    },
}

pub struct WithOrigin<T> {
    pub value: T,
    pub origin: Origin,
}
```

`Located<T>` remains the AST representation. `WithOrigin<T>` is reserved for
HIR, LIR, bytecode debug metadata, or other stages that actually synthesize
nodes. The two wrappers are not interchangeable aliases.

Pipeline elaboration currently occurs while constructing the AST. Its produced
call uses the complete pipeline expression location. A later HIR RFC may retain
the pipeline AST form and move elaboration to an origin-aware lowering pass.

## Diagnostics and compiler APIs

Diagnostic labels contain `Location`. Provenance maps paths to `Location`.
Neither stores line and column values.

All compiler and tool-stage entry points that render compatibility errors must
have access to the corresponding `SourceDatabase` or `SourceFile`. The fallback
calculation `column = byte_offset + 1` is removed. APIs without source text may
return structured byte locations but may not invent human coordinates.

The compatibility `FrontendError` may remain temporarily, but its line and
column fields must be produced by `SourceFile::position`.

## Runtime and provenance boundary

Runtime `Value` remains location-free. Value equality, Dict shape interning,
constant sharing, and VM execution do not depend on source locations.

JSON/data provenance remains a side table, now mapping value paths to
`Location`. A value's provenance may include several locations over its
lifetime; it is not represented by `Located<Value>`.

The future LIR and opcode-boundary RFC must carry `Origin` through a side table
or instruction metadata so runtime failures can map program counters back to
source. The runtime contract itself is deferred.

## Error recovery boundary

This RFC preserves the current behavior where syntax diagnostics prevent AST
lowering. It defines zero-width locations so a future recoverable AST can model
missing nodes, but it does not add `Missing` or `Error` AST kinds. Recoverable
AST/HIR requires a separate design for partial semantic nodes and cascading
diagnostic suppression.

## Rejected alternatives

### Optional locations on every node

This makes a missing location silently legal throughout the compiler. Parsed
syntax always has a source position, including zero-width recovery positions.
Synthetic nodes are represented by `Origin` instead.

### Keep `Expr::Spanned`

A transparent enum variant forces every consumer to recurse through it and can
be accidentally stripped before a diagnostic is created. `Located<ExprKind>`
makes location access uniform and leaves `ExprKind` exhaustiveness focused on
language semantics.

### Include locations in runtime values

Syntax location and data provenance have different cardinality and semantics.
Embedding either in `Value` would contaminate runtime identity and sharing.

### Introduce LIR in this RFC

Located AST is a prerequisite for an origin-aware LIR, but the opcode contract,
control-flow representation, validation, and bytecode debug map form a separate
design surface. They will be specified by the next RFC.

## Implementation plan

1. Add checked `SourceId`, `TextRange`, `Location`, `Located<T>`, `Origin`, and
   `WithOrigin<T>` primitives.
2. Convert diagnostics and provenance from the old combined span to `Location`.
3. Refactor all semantic AST nodes and diagnostic-relevant tokens to the
   `Located<T>`/kind representation.
4. Update CST lowering to perform one checked `usize`-to-`u32` conversion path.
5. Update analysis, compiler, module loading, tests, and public exports.
6. Remove `Expr::Spanned`, `Pattern::Spanned`, `unspanned()`, optional AST spans,
   and byte-offset-as-column fallbacks.

## Acceptance criteria

1. `SourceId` and `TextRange` are compact `u32` values and source-size/range
   conversions are checked.
2. Program, Block, Binding, Expr, Pattern, MatchArm, and narrow semantic tokens
   use mandatory `Located<T>` values.
3. No AST enum contains a transparent span wrapper or exposes `unspanned()`.
4. Diagnostics and JSON provenance use `Location` without changing runtime
   `Value` representation or equality.
5. Compiler and tool-stage errors with non-ASCII or multiline prefixes resolve
   through `SourceFile::position`; no byte-offset column fallback remains.
6. XL and JSON CST lowering preserve the exact locations tested in RFC 0005.
7. `Origin` and `WithOrigin<T>` support source and synthetic origins but are not
   prematurely embedded in AST or runtime values.
8. Existing language, module, validation, recovery, and CLI behavior remains
   unchanged.
9. Focused layout, checked-conversion, nested-location, Unicode diagnostic, and
   provenance tests pass with formatting and strict Clippy checks.

