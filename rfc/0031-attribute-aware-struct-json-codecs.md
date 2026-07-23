# RFC 0031: Attribute-aware Struct JSON codecs

- Status: Proposed
- Depends on: RFC 0020, RFC 0026, RFC 0030

## Summary

The existing derived codec consumes standard JSON attributes on normalized
Struct metadata for both deserialization and serialization:

```xl
import json from "core:json";

@json.rename_all('CamelCase)
@struct
type User = {
    user_id: Int,

    @json.rename("display")
    display_name: String,

    @json.default('None)
    @json.skip_serializing_if('None)
    nickname: Option(String),

    @json.flatten
    address: Address,
};
```

`core:codec.decode` maps JSON-facing keys and shapes into canonical XL Struct
values. `core:codec.encode` maps canonical XL values back into strict JSON-domain
Dicts. Both directions are deterministic and use the same metadata plan.

## Naming

`core:json.rename` on a field selects its exact external key. Otherwise the
nearest enclosing Struct's `core:json.rename_all` policy applies. RFC 0031
supports `'CamelCase`:

```text
user_id      -> userId
display_name -> displayName
already      -> already
```

Explicit rename takes precedence over rename_all. XL Struct field names remain
unchanged after decode and are the required input names for encode.

Two non-flattened fields may not resolve to the same external key. Planning
reports the later field's rename payload when explicit, otherwise its field
rule. Unknown external keys remain errors.

## Flatten

A field with `core:json.flatten: 'True` must have Struct TypeMetadata after
attribute transparency. During decode, its nested fields consume keys from the
same external object and the resulting nested XL Dict is stored under the
field's internal name. During encode, the nested encoded Dict is merged into
the parent JSON object.

Flatten is recursive. Every external key must be consumed by exactly one leaf
field. Collisions between ordinary fields, two flattened structures, or nested
and parent names are errors attributed to the conflicting field rule.

`rename` and `default` are rejected on a flattened field because the field has
no own external key. `skip_serializing_if` is allowed and skips the complete
flattened subtree during encode. A flattened field remains required during
decode in the sense that all required leaves of its nested Struct must be
present; nested defaults and Options apply normally.

## Default

If a non-flattened external key is absent and `core:json.default` exists, decode
inserts the attribute payload as the canonical XL field value. The payload must
satisfy the field TypeMetadata; otherwise decode returns a structured failure
whose rule is the default payload.

Default takes precedence over Option's implicit missing-to-`'None` behavior.
It has no direct effect during encode.

RFC 0031 defaults are values, not factories. They retain their metadata source
location and require no unmetered callback.

## Skip serialization

`core:json.skip_serializing_if` affects encode only:

- `'None` skips when the canonical field value is the unit Atom `'None`;
- `'False` skips when the value is the Atom `'False`;
- `'Empty` skips an empty String, Array, or Dict.

The field remains accepted and validated during decode. A value not belonging
to the policy's domain simply is not skipped; its ordinary field codec then
validates it.

## Field planning and strictness

Codec planning retains each wrapper's opaque attributes and rich rule values.
Struct transformation recursively builds a deterministic field plan containing
internal name, external name, flatten mode, default, skip policy, and child
schema. Decode tracks consumed external keys; encode tracks emitted keys.

Inputs remain strict:

- decode rejects every unconsumed external key;
- encode rejects every unknown internal XL field;
- output name collisions are errors rather than last-write-wins merges.

## Diagnostics

Data failures retain the offending JSON or XL rich value as the primary rule
payload, as in RFC 0022. Configuration and collision failures select the most
specific attribute payload (`rename`, `flatten`, `default`, or skip policy) as
the rule value. `result.unwrap` can therefore render data and model source
labels without compiler-specific attribute knowledge.

Paths use external names while traversing JSON input/output and internal names
when diagnosing malformed canonical XL Struct values.

## Quota

Codec output accounting includes renamed key bytes, nested flattened output,
defaults materialized into decoded Dicts, and the existing Result envelope.
Planning performs bounded traversal of already-loaded metadata and uses no
unaccounted XL heap allocation.

## Deferred work

- arbitrary predicate and default factory callbacks;
- flattening map-like Dict values;
- aliases and deny/allow unknown field policies;
- deserialize-only or serialize-only renames;
- additional rename_all policies;
- borrowing or zero-copy decoded fields.

## Acceptance criteria

1. rename works in decode and encode while preserving internal field names.
2. CamelCase rename_all works bidirectionally and explicit rename wins.
3. flatten recursively decodes and encodes nested Struct values.
4. unknown inputs and all external-name collisions are rejected.
5. default supplies a validated canonical value only when the key is absent.
6. Option missing behavior remains compatible and default takes precedence.
7. all three skip policies omit matching values only during encode.
8. flatten conflicts and invalid attribute placement produce structured errors.
9. failures retain data and the most specific attribute or field rule location.
10. output allocation accounting includes transformed keys and values.
11. Structs without JSON attributes retain existing codec behavior.

## Implementation plan

1. Retain flat attribute maps on runtime CodecType nodes.
2. Parse and validate Struct/field JSON configuration into deterministic plans.
3. Replace name-equal Struct traversal with recursive consumed/emitted-key
   traversal supporting flatten.
4. Add bidirectional behavior, collision, diagnostics, and quota tests.

## Implementation result

Pending.
