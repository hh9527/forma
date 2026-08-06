# RFC 0150: Regex typed decode

- Status: Proposed
- Depends on: RFC 0148, RFC 0149

## Summary

`re.parse(regex)` is a decorator applied to struct type metadata:

```forma
@re.parse(re.compile(r"(?P<name>\w+)=(?P<value>\d+)"))
@struct
type Rec = { name: String, value: Int };
```

The decorator validates the relationship immediately and returns the same
struct metadata wrapped with a `std/regex.parse` attribute containing the
compiled `Regex` value. The resulting metadata remains the witness consumed by
typed decoding:

```forma
re.decode(Rec, "answer=42")
# 'Ok({name: "answer", value: 42})
```

## Correspondence

- every capture must be named and every name must be unique;
- capture names and struct field names must be identical as sets;
- `String` uses capture text directly;
- `Int` uses strict decimal integer parsing;
- an optional capture must map to `Option(T)`;
- a required capture must map to non-optional `T`;
- unsupported, recursive, or composite field types are rejected initially.

The compiled regex is the retained plan. Capture names make the field mapping
deterministic, so the first implementation does not expose a second plan type.
An internal cache may be added later without changing this contract.

## Failure model

Pattern compilation and decorator validation fail during type metadata
construction. Runtime no-match and scalar conversion return
`Result(A, BlameError)`. Errors retain the input as data and the decorated type
metadata as the rule.

## Acceptance criteria

1. Decorator validation rejects every incomplete or ambiguous correspondence.
2. Successful metadata retains a real `Regex` native value.
3. String and integer fields decode into the declared struct shape.
4. Optional captures produce `Option` values and enforce optional fields.
5. Decode failures use the standard `BlameError` representation.
6. Type checking infers the concrete `Result(Rec, BlameError)` result.
