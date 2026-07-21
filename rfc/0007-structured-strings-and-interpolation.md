# RFC 0007: Structured Strings and Interpolation

- Status: Accepted
- Implementation: Complete

## Summary

XL and JSON strings become structured, lossless CST constructs instead of one
opaque token. Logos recognizes source slices without allocating or decoding;
Lelwel assembles quotes, text, escapes, and, in XL expressions, interpolation.
Semantic lowering performs the first allocation needed for decoded text.

XL adds interpolation with `\{ expression }`. Interpolated expressions accept
only `String`, `Int`, and `Atom` values. Other statically known types are tool
errors; `Any` is checked at runtime.

## Lexical model

Tokenization has normal and string modes. Both modes use Logos and return only
token kinds and byte ranges into the original source. Token callbacks do not
allocate, decode escapes, or build token payloads.

An XL string has this lexical shape:

```text
DoubleQuote (StringText | EscapeSequence | Interpolation)* DoubleQuote
Interpolation = InterpolationStart expression RightBrace
InterpolationStart = "\{"
```

`StringText` consumes a maximal consecutive source slice. `EscapeSequence`
retains its original spelling and exact range. Empty strings contain no empty
token: `""` is two adjacent `DoubleQuote` tokens. Invalid escapes and missing
closing quotes produce located diagnostics and recovery tokens.

When `InterpolationStart` is recognized, tokenization returns to normal mode.
It tracks nested braces until the interpolation's matching `RightBrace`, then
resumes the surrounding string mode. Strings nested inside interpolation use
the same mechanism.

JSON uses the same structured boundary but never recognizes interpolation. Its
escape set remains the JSON escape set, including UTF-16 surrogate-pair
decoding during lowering. XL and JSON share no assumption that their accepted
escapes are identical.

Byte literals remain opaque in this RFC. Their representation will be revisited
when byte escape and interpolation semantics are designed.

## CST model

Lelwel is authoritative for the following structure:

```text
string_literal:
  DoubleQuote string_part* DoubleQuote;

string_part:
  StringText
| EscapeSequence
| interpolation;

interpolation:
  InterpolationStart expression RightBrace;
```

JSON has an equivalent `string_literal` without `interpolation`. Quotes and all
parts remain in the CST, so source reconstruction stays byte-for-byte lossless.

String literals used as import paths, dictionary keys, and patterns must be
plain: interpolation in those positions is rejected during semantic lowering.
This keeps module discovery, field shapes, and pattern constants closed-world
and deterministic.

## AST model

Plain strings continue to lower to:

```rust
ExprKind::String(String)
```

An interpolated expression lowers to a dedicated semantic form:

```rust
ExprKind::InterpolatedString(Vec<StringPart>)

type StringPart = Located<StringPartKind>;

enum StringPartKind {
    Text(String),
    Expression(Expr),
}
```

Adjacent lexical text and escape parts may be decoded and coalesced into one
AST text part. Every retained part has a mandatory `Location`; expression parts
use the interpolation expression's location. The containing expression covers
both quotes.

The AST is not immediately rewritten into calls. A future HIR lowering may
expand it and attach `Origin::Source` or `Origin::Synthetic` to generated
operations.

## Semantics

Parts are evaluated left to right and concatenated without separators. Values
are converted as follows:

```text
String -> its contents
Int    -> canonical base-10 representation
Atom   -> its name without the source apostrophe
```

No other value has interpolation semantics. In particular, `Float`, `Bytes`,
`Array`, `Tuple`, `Dict`, and `Func` are rejected. This operation is private to
interpolation and does not define a general-purpose `to_string` protocol.

Static analysis rejects a part whose inferred type contains a definitely
unsupported type. `Any` is permitted and emits a runtime check. A union is
accepted only when every non-`Any` variant is one of `String`, `Int`, or an Atom
type. Runtime failure reports the interpolated expression's source location
when debug-origin plumbing becomes available; this RFC preserves the current
VM error envelope until the LIR/debug-map RFC.

## Runtime contract

Bytecode gains explicit interpolation operations rather than exposing a core
function to source code. The compiler evaluates each expression part, converts
it using the restricted rule, and concatenates all parts. Runtime `Value`
remains location-free.

The instruction boundary must avoid constructing intermediate strings for
every pairwise concatenation. One instruction may consume an ordered register
list and allocate the final string once after validating and sizing all parts.
This is an initial opcode contract local to the feature, not the general LIR
design previously deferred.

## Rejected alternatives

### One opaque string token

It hides escape and interpolation boundaries from recovery, diagnostics,
formatting, semantic highlighting, and later source-preserving transformations.

### Decode or allocate in Logos callbacks

Lexing only classifies slices. Allocation in callbacks would make the lossless
token path own semantic data and duplicate work performed by AST lowering.

### A zero-width empty token

Empty matches threaten lexer progress and add no information. Repetition with
zero parts already represents an empty string.

### Lower interpolation directly to `concat` and `to_string`

Those source-level protocols do not yet exist. Early desugaring also loses the
semantic construct before origin-aware HIR is available.

### Stringify every runtime value

Debug display is not stable language semantics. The deliberately narrow
`String | Int | Atom` set can expand only through a later RFC.

## Implementation plan

1. Split XL and JSON tokenization into zero-allocation normal/string modes.
2. Replace opaque string tokens in both Lelwel grammars with structured string
   productions while retaining lossless recovery.
3. Decode structured CST parts during lowering and preserve existing plain XL
   and JSON string behavior.
4. Add located interpolated-string AST parts and reject interpolation in plain
   string contexts.
5. Add static interpolation checks and one VM instruction that validates,
   sizes, and constructs the result.
6. Update every AST consumer and add lexer, CST, parser, analysis, VM, module,
   diagnostic, and CLI coverage.

## Acceptance criteria

1. Logos callbacks perform no string allocation or escape decoding.
2. XL and JSON CSTs expose quotes, maximal text slices, and individual escapes
   and still reconstruct the original source byte-for-byte.
3. Empty, escaped, malformed, and unterminated strings have precise ranges and
   recover without zero-width lexer tokens.
4. Existing plain strings, JSON Unicode decoding, imports, dictionary keys,
   patterns, and provenance retain their behavior and locations.
5. `"hi, \{name}"` produces a located interpolated AST and evaluates parts left
   to right.
6. String, Int, and Atom interpolation produce their specified text.
7. Known unsupported types fail analysis at the interpolation expression;
   unsupported `Any` values fail at runtime.
8. Interpolation in import paths, dictionary keys, and patterns is rejected.
9. Runtime values remain source-location-free and string construction avoids
   pairwise intermediate allocations.
10. Workspace tests, formatting, strict Clippy, and diff checks pass.

## Implementation result

Implemented for XL and JSON. Their lexers now use separate Logos normal and
string modes, producing only token kinds and source ranges. Lelwel CSTs retain
opening and closing quotes, maximal text tokens, individual escape tokens, and
XL interpolation nodes while remaining byte-for-byte reconstructable. Empty
strings require no zero-width token, JSON Unicode behavior is preserved, and
byte literals remain opaque as specified.

XL lowering preserves plain strings as `ExprKind::String` and produces located
`StringPartKind::Text` and `StringPartKind::Expression` parts for interpolation.
Imports, dictionary keys, and string patterns reject interpolation. Static
analysis accepts only `String`, `Int`, `Atom`, and `Any` parts; unsupported known
types receive a source diagnostic, while unsupported dynamic values fail in
the VM.

`InterpolateString` validates all source registers, calculates the final UTF-8
length, allocates the result once, and appends String contents, decimal Ints,
and Atom names in evaluation order. Runtime `Value` remains location-free.
Lexer ranges, lossless CST structure, lowering locations, malformed strings,
nested interpolation, static and dynamic failures, VM behavior, existing
module/JSON behavior, and the CLI path are covered by tests.
