# RFC 0154: Type-directed Display

- Status: Implemented
- Depends on: RFC 0123, RFC 0151
- Child RFC: RFC 0155

## Summary

Forma adds a type-directed, user-facing text representation:

```forma
import "std/fmt" as fmt;

@fmt.display_by("{host}:{port}")
@struct type Endpoint = { host: String, port: Int };

fmt.display(Endpoint, endpoint)
```

The public entry point is:

```forma
for(A) Fn(TypeOf(A), A) -> String
```

`Display` is distinct from diagnostic `Debug`. This sequence does not define
`Show`, debug formatting, alignment, numeric format specifiers, or a general
template engine.

## Direction

As with `std/string.parse`, the type object selects a provider. Built-in types
have direct implementations; decorated structs retain a compiled provider in
their TypeDesc attributes. Nested named struct fields follow ordinary TypeDesc
links and recursively use their own Display capability.

Missing or malformed capability metadata is a programming-contract error.
Unlike parsing external input, Display has no data-level failure result once a
typed value and a valid capability have been supplied.

## Acceptance criteria

1. `String`, `Int`, and `Float` have built-in Display implementations.
2. A struct can install a validated template with `fmt.display_by`.
3. Unknown fields and malformed templates fail during type construction.
4. Nested decorated struct fields compose recursively.
5. Templates are compiled once rather than reparsed for every value.

## Implementation result

Implemented in August 2026 through RFC 0155.
