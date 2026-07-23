# RFC 0032: Serde-style Enum JSON codecs

- Status: Proposed
- Depends on: RFC 0029, RFC 0030, RFC 0031

## Summary

Derived JSON codecs consume normalized Enum metadata without changing XL's
canonical tagged runtime values. The default representation is externally
tagged. A new `json.untagged` decorator selects an untagged representation.
Variant names participate in the existing `rename` and `rename_all`
vocabulary.

```xl
import json from "core:json";

@json.rename_all('CamelCase)
@enum
type Event = {
    Idle: 'None,
    UserJoined: User,
    @json.rename("fatal") FatalError: String,
};

@json.untagged
@enum
type StringOrUser = {
    Text: String,
    User: User,
};
```

## Canonical XL values

Enum values keep the RFC 0029 representation:

```text
'Idle
('UserJoined, user)
('FatalError, message)
```

JSON representation attributes never change validation, matching, equality,
or the in-memory tag. Decode produces canonical internal tags; encode consumes
only canonical internal tags.

## Externally tagged representation

Unit variants encode as their external String name:

```text
'Idle <-> "idle"
```

Payload variants encode as a single-entry JSON object:

```text
('UserJoined, user) <-> {"userJoined": <encoded user>}
```

Decode rejects objects with zero or multiple fields, unknown external names,
payloads supplied to unit variants, and missing payloads. Encode rejects
unknown internal tags and malformed tagged tuples.

## Naming

`core:json.rename` on a variant chooses its exact external name. Otherwise the
Enum's `core:json.rename_all: 'CamelCase` policy applies. Explicit rename wins.
Resolved names must be unique. The canonical runtime tag remains the variant's
internal metadata key.

## Untagged representation

`core:json.untagged: 'True` on an Enum encodes a payload variant as only its
payload and decodes by applying each payload codec in deterministic metadata
order. Exactly one variant must accept the input. Zero matches report the
collected variant failures; multiple matches report an ambiguity rather than
selecting an order-dependent winner.

Unit variants are rejected in untagged Enums because they have no payload that
can distinguish them. Variant `rename` and Enum `rename_all` are also rejected
as semantically inert in untagged mode.

## Diagnostics and quota

Failures retain the input as data and the most specific Enum, variant, or
attribute rich value as rule. Paths use external names while processing JSON
and internal tags while processing XL values. Output accounting includes
external tag strings, singleton Dicts, canonical tuples, and the Result
envelope.

## Deferred work

- internally and adjacently tagged representations;
- variant aliases;
- per-direction variant skipping;
- explicit priority for intentionally overlapping untagged variants;
- structural disjointness analysis before runtime.

## Acceptance criteria

1. unit and payload variants round-trip through external tagging.
2. rename and CamelCase naming work bidirectionally without changing XL tags.
3. duplicate external names are rejected.
4. untagged payload variants round-trip by structural matching.
5. zero-match and ambiguous untagged inputs are structured failures.
6. malformed canonical XL Enum values and unknown tags are rejected.
7. failures preserve useful data and rule locations.
8. codec output obeys allocation quota accounting.
9. Struct fields containing Enum metadata compose with RFC 0031 behavior.

## Implementation plan

1. Add the ordinary `json.untagged` decorator and canonical attribute key.
2. Retain variant attributes while decoding normalized Enum metadata.
3. Plan deterministic external names and representation policy.
4. Implement external and untagged transformations in both directions.
5. Add nested, invalid, ambiguous, diagnostic, and quota tests.

## Implementation result

Pending.
