# RFC 0036: Function-valued JSON skip predicates

- Status: Proposed
- Depends on: RFC 0015, RFC 0024, RFC 0030, RFC 0031

## Summary

`core:json.skip_serializing_if` accepts either an existing policy Atom or an
ordinary unary XL Func:

```xl
fn is_zero(value) { value == 0 }

@struct type Metrics = {
    @json.skip_serializing_if(is_zero)
    retries: Int,
};
```

During Struct encoding the predicate is called with the canonical XL field
value. `'True` omits the field and `'False` encodes it normally. The callback
runs through the ordinary VM call boundary and therefore shares fuel,
allocation, stack, call-depth, source-location, and trace behavior with
`core:array.filter` callbacks.

## Motivation

RFC 0030 limited skip configuration to `'None`, `'False`, and `'Empty` because
the JSON codec was synchronous. RFC 0015 subsequently established suspended
native execution: a native operation can return `VmAction::Call`, retain its
state in a native continuation, and resume after an XL or native callback.
Function predicates now require a codec state machine, not a new execution
model.

The feature also verifies that native continuations are a reusable VM boundary
rather than an Array-specific mechanism. Default factories and custom codecs
can build on the same representation later without entering this RFC's scope.

## Surface contract

The exported factory is described as:

```xl
native skip_serializing_if: fn(Any) -> fn(Any, Any) -> Any;
```

Its configuration argument must be one of:

- `'None`, `'False`, or `'Empty`, retaining RFC 0030 behavior;
- a Func with arity one.

Other values and functions of another arity fail at the configured factory
call. The produced decorator remains an ordinary two-argument Func and stores
the original policy or predicate under the flat attribute key
`core:json.skip_serializing_if`.

The predicate receives the canonical field value before child encoding or
flattening. It must return exactly `'True` or `'False`. Any other result is a
codec type error attributed to the skip rule. A callback runtime failure keeps
its ordinary callback frame and appends the JSON encode continuation frame.

Skip remains encode-only. Decode, required-field rules, defaults, validation,
and JSON Schema requiredness are unchanged.

## Native continuation boundary

The VM generalizes the existing Array-only return target:

```rust
enum NativeContinuation {
    Array(ArrayContinuation),
    JsonEncode(JsonEncodeContinuation),
}

enum ReturnTarget {
    Root,
    Register(Register),
    Native(Box<NativeContinuation>),
}
```

Every native continuation exposes its parent return target and trace frame for
logical-depth accounting and error traces. The VM dispatcher resumes the
matching state machine when a callback returns.

`JsonEncodeContinuation` owns the remaining Struct encode work, already
emitted fields, the predicate result destination, the original codec call
origin, and its parent return target. It contains only XL runtime values and
owned planning data; it borrows neither Work nor Main heap across suspension.

Built-in Atom policies retain the synchronous fast path. A function predicate
creates one ordinary VM call per visited configured field. A skipped field does
not run its child codec. Flattened fields apply their predicate to the complete
canonical nested Dict before flattening, matching current policy behavior.

## Quota and determinism

The callback call consumes ordinary call fuel. Work performed by the callback
uses the same active quota account as the enclosing encode. Codec output
allocation continues to be charged when the completed `CodecNode` is
materialized. No fresh VM, stack, heap, or quota account is created.

Struct field order remains deterministic. Predicate callbacks run in that
order, including when earlier predicates omit fields. A callback can use
`core:debug`; its observations therefore follow field order.

## Diagnostics

- invalid configuration points to the factory argument;
- wrong predicate arity reports expected one argument;
- a non-Boolean predicate result points to the skip attribute rule;
- callback failures retain the callback's source frame and the codec call
  frame;
- collisions and child codec failures retain RFC 0031 data/rule blame.

## Deferred work

- function-valued `default` factories;
- asynchronous decode transformations;
- user-defined field and type codecs;
- static predicate signature refinement beyond runtime arity/result checks.

## Acceptance criteria

1. Existing Atom policies retain their exact behavior.
2. A unary XL closure can omit and retain different Struct fields.
3. Native and bytecode predicates both work.
4. Captured predicate values survive TypeMetadata promotion into Main World.
5. Non-Func configurations, wrong arity, and non-Boolean results are rejected.
6. Predicate calls share the enclosing encode quota and consume call fuel.
7. Callback errors include callback and JSON codec continuation frames.
8. Predicate calls are deterministic and skipped children are not encoded.
9. Decode and JSON Schema behavior remain unchanged.

## Implementation plan

1. Generalize Array's native continuation return target and dispatcher.
2. Accept unary Func values in skip attribute configuration and codec planning.
3. Split Struct encoding into resumable field steps.
4. Resume after predicates, validate Boolean results, and continue encoding.
5. Add promotion, quota, trace, invalid-result, flatten, and compatibility tests.

## Implementation result

Pending.
