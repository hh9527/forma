# RFC 0065: TOML static data modules

- Status: Accepted
- Depends on: RFC 0022, RFC 0056, RFC 0057, RFC 0061

## Summary

Forma supports TOML 1.0 files as immutable static data modules:

```forma
import config from "./config.toml";
```

TOML is parsed by Forma's own lossless lexer, CST, and semantic lowerer. It
uses the same resolved-module cache, source database, provenance paths, codec
boundary, workspace snapshot, and diagnostics as JSON modules. No external
TOML or serde implementation is introduced.

TOML's four date/time categories lower to validated Tagged String values:

```forma
'OffsetDateTime("1979-05-27T07:32:00Z")
'LocalDateTime("1979-05-27T07:32:00")
'LocalDate("1979-05-27")
'LocalTime("07:32:00")
```

`@bim/std/toml` exports the corresponding declaration-only `DateTime` Enum
metadata so user schemas can refer to the complete set without repeating it.

## Motivation

Static JSON already proves that external data can enter Forma's closed module
graph without privileged configuration semantics. TOML adds a line-oriented,
non-flat syntax: dotted keys, tables, arrays of tables, multiline Strings, and
typed temporal scalars all converge into the same immutable value model.

Using Forma's frontend machinery is necessary for exact source attribution.
A generic TOML value parser would lose the individual key and value locations
needed when `codec.decode(Type, config)` reports both data blame and rule blame.

## Syntax and values

The parser accepts TOML 1.0 basic and literal Strings, including multiline
forms; decimal and base-prefixed integers; finite and non-finite floats;
Booleans; arrays; inline tables; bare and quoted keys; dotted keys; tables; and
arrays of tables.

Lowering maps values as follows:

| TOML | Forma |
| --- | --- |
| integer | `Int` |
| float | `Float` |
| Boolean | `Bool` |
| String | `String` |
| array | `Array` |
| table / inline table | immutable `Dict` |
| offset date-time | `'OffsetDateTime(String)` |
| local date-time | `'LocalDateTime(String)` |
| local date | `'LocalDate(String)` |
| local time | `'LocalTime(String)` |

TOML heterogeneous arrays remain ordinary heterogeneous runtime Arrays and
therefore infer conservatively when imported through an `Any` boundary.

## Temporal values

Date/time parsing validates calendar dates, leap years, clock ranges, offsets,
and TOML syntax before constructing a value. It is not pattern recognition
followed by an unchecked String conversion.

Payloads use a canonical spelling:

- a date/time separator is `T`;
- `t` and `z` normalize to `T` and `Z`;
- a zero numeric offset normalizes to `Z`;
- fractional second digits, including trailing zeroes, are retained.

The lossless CST retains the original spelling for diagnostics and future
formatting. Tagged values preserve the distinction between an unquoted TOML
temporal scalar and an ordinary quoted String without adding a VM primitive or
prematurely defining calendar arithmetic. A later pure library can expose
components, ordering, epoch conversion, and formatting.

## Table construction

Dotted keys create intermediate tables. Explicit table headers may complete an
implicitly created table exactly once. An array-of-tables header appends a new
table and subsequent assignments target that element.

The lowerer rejects:

- duplicate keys;
- redefining a scalar, array, inline table, or explicit table;
- extending an inline table outside its literal;
- traversing a non-table component;
- conflicting table and array-of-tables declarations;
- duplicate keys within one inline table.

Diagnostics label the conflicting definition and the first definition when it
is available. Final Dict fields use canonical key ordering, independent of
source order.

## Strings and numbers

Basic Strings implement TOML escapes and Unicode scalar escapes. Multiline
basic Strings implement the initial-newline trim and backslash newline folding.
Literal Strings do not process escapes. Control characters rejected by TOML do
not enter Forma Strings.

Integer underscores and radix prefixes are removed before checked `i64`
conversion. Floats lower to IEEE 754 `f64`; TOML `inf`, `+inf`, `-inf`, `nan`,
`+nan`, and `-nan` are retained as Float values even though the JSON codec may
later reject non-finite values at its own boundary.

## Modules, caching, and tooling

`.toml` selects `ModuleFormat::Toml`; exact manifest format overrides continue
to obey RFC 0057. A resolved TOML module is parsed and lowered once per module
identity and promoted into the same persistent world as JSON data.

Workspace recovery records TOML modules as a distinct module kind. Invalid
TOML remains navigable source with diagnostics but supplies no guessed value.
Imports, CLI checks, `forma show`, LSP diagnostics, and codec validation all
observe the same published snapshot.

## Non-goals

- TOML mutation or preservation of source field order in runtime Dicts;
- automatic decoding to a user type during import;
- implicit conversion of temporal values to ordinary Strings;
- date arithmetic, time zones, locale formatting, or epoch conversion;
- reading TOML manifests instead of `forma-deps.json` in this RFC;
- YAML parsing or a shared JSON/TOML grammar.

## Acceptance criteria

1. `.toml` resolves and loads without an external TOML dependency;
2. the CST reconstructs comments, whitespace, quoting, and multiline input;
3. every TOML scalar category lowers as specified;
4. temporal values are validated, categorized, and canonically spelled;
5. dotted keys, tables, inline tables, and arrays of tables produce the correct
   immutable nested value;
6. duplicate and conflicting definitions label precise source ranges;
7. value and key provenance reaches codec `BlameError` diagnostics;
8. aliases of the same resolved module reuse one cached value;
9. workspace and LSP snapshots distinguish known and invalid TOML modules;
10. JSON and Forma module behavior remains unchanged;
11. full workspace tests, strict Clippy, formatting, and whitespace checks pass.

## Rejected alternatives

### Use the `toml` crate

It provides a decoded value efficiently but does not define Forma's lossless
CST, source provenance, recovery behavior, or diagnostic contract. Wrapping it
would leave the language boundary split between two parsers.

### Lower temporal values to String

That permanently erases the distinction TOML already made between a quoted
String and a validated temporal scalar. Users could no longer require one or
reliably encode back to TOML.

### Add temporal VM primitives

TOML import does not justify committing the VM to calendar, precision, and
timezone semantics. Tagged String preserves category and precision while
leaving richer operations to ordinary libraries.

### Lower temporal values immediately to records

Records make component access convenient but prematurely choose a shared time
model and allocate a much larger value for every scalar. A pure parser function
can produce records later without changing the import representation.
