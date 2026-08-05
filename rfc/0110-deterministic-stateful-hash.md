# RFC 0110: Deterministic stateful hash standard library

- Status: Proposed
- Depends on: RFC 0107, RFC 0109

## Summary

`@bim/std/hash` defines its own opaque `HashState` and exposes pure persistent
SHA-256 state transitions:

```forma
@opaque type HashState;
native new: Fn() -> HashState;
native update_bytes: Fn(HashState, Bytes) -> HashState;
native update_string: Fn(HashState, String) -> HashState;
native update_int: Fn(HashState, Int) -> HashState;
native finish: Fn(HashState) -> Bytes;
```

The existing `sha256: Fn(String) -> String` remains compatible. HashState is a
standard-library type backed by RFC 0109's generic opaque carrier; neither the
language type enum nor VM dispatch contains a HashState case.

## Immutable state

Every update clones the fixed-size logical SHA-256 context, appends one framed
value, and returns a new opaque value. The input and all aliases remain valid.
`finish` clones/finalizes the context and does not consume or invalidate it.

The Host payload type is checked against the declaration's qualified identity
`@bim/std/hash#HashState`. A mismatch is a normal native type error.

## Byte protocol

Every state begins with these exact bytes:

```text
66 6f 72 6d 61 2e 68 61 73 68 00 01   # "forma.hash", NUL, version 1
```

Updates append:

```text
Bytes:  01 || u64_be(byte_length) || bytes
String: 02 || u64_be(UTF-8 byte_length) || UTF-8 bytes
Int:    03 || i64_be(two's-complement value)
```

This framing distinguishes kinds and concatenation boundaries. It is
independent of platform endianness, heap identity, source provenance, physical
module paths, locale, and debug formatting. Composite interpreters add their
own deterministic markers and lengths through these primitives.

## Quotas and provenance

Operations charge deterministic logical output allocation for one opaque state
or 32 digest bytes. Update work is bounded by the framed input length. Storage
sharing does not alter quota success. Results are Generated at the authored
call; input provenance does not become the new root provenance.

## Non-goals

- cryptographic algorithm selection or negotiation;
- keyed hashing, MACs, signatures, or random seeds;
- implicit structural hashing;
- consuming/linear state or resource handles; or
- serialization of HashState.

## Acceptance criteria

1. all five functions use the declared module-owned HashState contract;
2. repeated updates match published byte-protocol vectors;
3. aliases remain immutable snapshots and equal logical states compare equal;
4. update kind and boundary framing cannot collide trivially;
5. finish returns exactly 32 bytes and may be called repeatedly;
6. quota and provenance behavior is deterministic;
7. existing one-shot sha256 behavior remains compatible;
8. no HashState-specific runtime/type/VM enum variant is added; and
9. full workspace tests and strict Clippy pass.

## Implementation plan

1. complete the internal incremental SHA-256 context;
2. expose Bytes observation/construction through the synchronous native ABI;
3. declare HashState and register ordinary native callbacks in the hash module;
4. implement the versioned framing protocol and focused vectors;
5. test aliasing, equality, type rejection, quotas, and debug opacity; and
6. record the implementation result.
