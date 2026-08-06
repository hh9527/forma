# RFC 0149: Native Regex values

- Status: Implemented
- Depends on: RFC 0148

## Summary

`std/regex` declares a public native type and ordinary operations:

```forma
native type Regex = @0;
native compile: Fn(String) -> Regex;
native is_match: Fn(Regex, String) -> Bool;
```

`compile` validates and compiles the pattern immediately. Invalid syntax is a
source-aware evaluation error at the call. A `Regex` value is immutable,
publishable, capturable, and logically equal to another value compiled from
the same pattern.

The implementation uses the Rust `regex` engine. Its compiled program is a
Host payload stored behind the native value; users observe the nominal native
type, not an `Opaque` language type.

## Acceptance criteria

1. `std/regex` has a reserved deterministic native module ID.
2. Only `std/regex` can declare slot `@0` as its `Regex` type.
3. Compiling an invalid expression reports the engine error at the call site.
4. `is_match` accepts only `Regex` and `String` and returns Forma `Bool`.
5. Equal patterns produce logically equal native values across publication.

## Implementation result

Implemented in August 2026. `std/regex` owns reserved native module ID 19 and
slot `@0`. `compile` stores the source pattern with the compiled Rust regex;
logical equality uses the source pattern, so publication and copying do not
change identity. `is_match` consumes the native value directly.
