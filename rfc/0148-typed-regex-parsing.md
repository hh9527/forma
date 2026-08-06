# RFC 0148: Typed regular-expression parsing

- Status: Implemented
- Depends on: RFC 0056, RFC 0123, RFC 0134
- Child RFCs: RFC 0149 through RFC 0150

## Summary

Forma adds type-first regular-expression parsing. A struct remains the
authoritative result contract, while a compiled regular expression supplies a
validated textual representation:

```forma
import "std/regex" as re;

@re.parse(re.compile(r"(?P<name>\w+)=(?P<value>\d+)"))
@struct
type Rec = {
    name: String,
    value: Int,
};

re.decode(Rec, "answer=42")
```

`Regex` is a public native type declared by `std/regex`. Its Host payload uses
the VM native-value storage, but that representation is not part of the
language model. `compile` is an ordinary public constructor.

## Direction

The type defines field names, requiredness, and decoded value types. The
regular expression does not synthesize a module or a type. Applying `re.parse`
at type-construction time compiles and validates the relation between named
captures and struct fields, then retains the `Regex` value in type metadata.

This requires neither `mod!` nor generated static names. Such facilities need
independent motivation.

## Initial scope

The first implementation supports named captures mapped to `String`, `Int`,
and `Option` of those scalar types. It rejects duplicate capture names,
unnamed captures, missing or extra captures, unsupported field types, and a
mismatch between optional captures and optional fields.

`re.decode` is typed as:

```forma
for(A) Fn(TypeOf(A), String) -> Result(A, BlameError)
```

It requires metadata previously validated by `re.parse`. A match failure,
scalar conversion failure, or invalid contract returns a sourced
`BlameError`.

## Child sequence

RFC 0149 introduces the `std/regex` module, native type `Regex`, deterministic
compilation, matching, equality, and publication behavior.

RFC 0150 defines the struct/capture correspondence, decorator metadata,
typed decoding, diagnostics, and source attribution.

## Acceptance criteria

1. `Regex` is a public native type and `re.compile` is its ordinary constructor.
2. Invalid patterns fail at the call site, including type-construction calls.
3. `re.parse` validates the complete capture/field relation before returning
   transformed type metadata.
4. `re.decode` returns the statically witnessed struct type.
5. Compiled regex values survive module publication and compare by logical
   pattern identity.
6. No regex operation changes the module graph or static namespace.

## Implementation result

Implemented in August 2026 through RFCs 0149 and 0150. `std/regex` provides a
public native `Regex` value, while type decorators validate and retain compiled
patterns without changing the module graph. The initial scalar surface is
`String`, `Int`, and their `Option` forms.
