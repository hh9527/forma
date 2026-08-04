# RFC 0068: String literals and concat expressions

- Status: Implemented
- Amends: RFC 0007

## Summary

Forma separates inert String literals from evaluated text concatenation:

```forma
"ordinary String"
r##"raw "String" with # characters"##
`hello \{name}`
```

Double-quoted and raw forms always produce an immediate `String`. Backticks
produce a concat expression made from located text and expression parts. The
separation makes evaluation visible in source and leaves a stable structural
boundary for a later document renderer without changing ordinary Strings.

## Escaped Strings

Double-quoted Strings support the following escapes:

```text
\0  \n  \r  \t  \"  \\  \xNN  \u{H...}
```

`\xNN` must denote ASCII. `\u{H...}` accepts one through six hexadecimal
digits and must denote a Unicode scalar value.

A backslash immediately followed by LF or CRLF removes that newline and the
maximal following run of ASCII source whitespace (`space`, tab, CR, and LF):

```forma
"first \
    second"
```

The value is `"first second"`. A backslash followed by same-line whitespace is
not continuation and remains an invalid escape. Continuation performs no
general dedent or trimming.

## Raw Strings

Raw Strings use matched hash delimiters:

```forma
r"text"
r#"text containing "quotes""#
r##"text containing "#"##
```

The opener is `r`, zero through 255 `#` characters, and `"`. The closer is `"`
followed by exactly the same number of `#` characters. Content is retained
verbatim: escapes, interpolation markers, newlines, and indentation have no
special meaning. Unterminated delimiters and more than 255 hashes are errors.

## Concat expressions

Backticks contain text parts, the same escapes and continuation behavior as
escaped Strings, and expression parts introduced by `\{`:

```forma
`platform=\{settings.os}-\{settings.arch}`
```

Concat remains a String-producing expression in this RFC. Its AST retains the
part boundaries and source locations, so compilation can validate each
inserted value and a later `Doc` layer can reuse the same frontend structure.
Backticks in text are written as `\``. Double quotes need no escaping inside a
concat expression.

Plain String-only positions, including import requests, Dict String keys,
patterns, and `@@manifest`, accept escaped or raw Strings and never execute
concat expressions.

## Compatibility

RFC 0007 used `\{...}` inside double-quoted Strings. This RFC replaces that
surface syntax rather than retaining an ambiguous compatibility form. Current
source and user-facing examples migrate interpolation to backticks. Historical
RFC text remains an account of the design at that time.

## Acceptance criteria

1. double-quoted Strings never contain interpolation nodes;
2. raw Strings preserve content exactly and enforce the 255-hash limit;
3. backtick concat expressions retain located text and expression parts;
4. Rust-style scalar, ASCII, and continuation escapes decode as specified;
5. malformed escapes and delimiters produce precise diagnostics;
6. all three forms remain lossless in the CST and across rope chunk boundaries;
7. String-only syntax positions cannot evaluate concat expressions;
8. existing runtime interpolation type checks remain unchanged.

## Implementation result

The Logos frontend now has distinct escaped-String and concat contexts plus a
scanned raw-String token. The crop bridge commits only complete root-level
tokens and is tested at every UTF-8 split boundary for all three forms.

Lowering maps escaped and raw literals directly to `ExprKind::String` and maps
backticks to the existing located part representation consumed by concat
bytecode. Escape decoding validates ASCII bytes, Unicode scalar values, raw
delimiter matching, and explicit multiline continuation before compilation.
