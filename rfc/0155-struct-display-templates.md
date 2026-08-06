# RFC 0155: Struct Display templates

- Status: Implemented
- Parent: RFC 0154
- Depends on: RFC 0153

## Summary

`std/fmt` exports `display` and the `display_by` type decorator. The first
template grammar contains literal text, `{field}` substitutions, and `{{` or
`}}` escaped braces.

Every referenced field must exist and have a Display capability. A template
may omit or repeat fields. Field substitution recursively applies the compiled
Display plan for that field's complete type metadata.

The compiled template is private native data retained under the standard
`std/fmt.display` TypeDesc attribute. It is an implementation plan, not a new
public language value.

## Deferred work

- `Float`, `Bytes`, collections, enums, and optional values;
- width, alignment, fill, and numeric formatting;
- custom function providers;
- `Debug` and diagnostic structural rendering.

## Implementation result

Implemented in August 2026 with built-in `String` and `Int` rendering, checked
struct templates, escaped braces, and nested named struct composition.
