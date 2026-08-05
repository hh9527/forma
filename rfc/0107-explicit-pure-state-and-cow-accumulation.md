# RFC 0107: Explicit pure state and persistent accumulation

- Status: Implemented
- Depends on: RFC 0053, RFC 0061, RFC 0089 through RFC 0099, RFC 0102

## Summary

Forma develops structural hashing and user-space diagnostic collection without
first adding an accumulation effect, movable bindings, linear types, or Host
resources. State remains an ordinary explicit function input and output:

```forma
def my_hash:
    Fn(Foo, HashState) -> Result(HashState, BlameError)
=
    fn(value, state) {
        let state = hash.update_string(state, value.foo);
        let state = hash.update_int(state, value.bar);
        'Ok(state)
    };
```

Runtime values use immutable language semantics and persistent storage. The VM
may add copy-on-write only where it has real uniqueness evidence; COW is an
optional implementation optimization, never a phase requirement or a
language-level uniqueness promise.

The phase delivers two concrete applications:

1. a module-owned opaque HashState and a user-space TypeDesc/Dyn structural hash
   interpreter; and
2. ordinary Array-based diagnostic records threaded explicitly through
   validation functions and higher-order combinators.

This is an umbrella RFC. RFCs 0108 through 0112 define persistent collection updates,
HashState, the hash byte protocol, the reference hash interpreter, and explicit
diagnostic collection before this RFC becomes Implemented.

## Motivation

Hashing and diagnostic production both appear accumulator-shaped. An earlier
direction considered call-site capture, dynamic accumulation channels, Port
IDs, `$state` movable bindings, hidden state returns, and a general
Unsharable/Marker/AutoMarker foundation. Each can express the examples, but
each adds semantics substantially broader than the demonstrated need.

The essential application data flow is already expressible with ordinary pure
functions:

```text
HashState -> HashState

Array(DiagnosticRecord) -> Array(DiagnosticRecord)
```

Repeated `let` shadowing makes the current state visually local, while
`array.fold` and `array.try_fold` carry it through recursion. Persistent copies
preserve ordinary value semantics without requiring a static ownership system;
runtime sharing may reduce copying where the representation supports it.

This phase tests that simpler model with real user-space interpreters before
Forma commits to a language-level accumulation abstraction.

## Implementation result

RFCs 0108 through 0112 complete both reference applications. Persistent Array
push supplies the ordinary collection update; module-owned native types carry
immutable opaque library state; `@bim/std/hash` defines a deterministic
stateful byte protocol; the reference structural hash interpreter explicitly
threads HashState; and the diagnostic example explicitly threads
Array(BlameError) through nested validation.

Both applications preserve old aliases and expose state in their Function
types. The diagnostic example proves that sourced records remain ordinary data
until a caller explicitly projects them through a Host-visible failure. No
accumulation effect, movable binding, hidden state result, Port, or Host
resource table was needed. This is evidence for retaining explicit pure state
as the baseline; it does not rule out a later accumulation abstraction backed
by a separate demonstrated use case.

## Semantic baseline

All state is an ordinary Forma value. Functions declare every state parameter
and every state result. Calls have no hidden arguments, hidden return values,
dynamic handlers, or caller-side rebinding:

```forma
let state = hash.new();
let state = my_hash(value, state)?;
let digest = hash.finish(state);
```

Aliasing preserves immutable snapshots:

```forma
let original = hash.new();
let saved = original;
let updated = hash.update_string(original, "forma");

# saved still denotes the empty hash state
```

The runtime may mutate backing storage only when it proves that doing so cannot
change any live alias. Otherwise it copies. The current handle heap does not
prove unique reachability merely because an inner Arc has one strong reference:
multiple runtime values may still share the same Handle. RFC 0108 therefore
does not claim an Array COW fast path.

`Result` placement remains explicit API policy. These two contracts are
different and the language does not choose between them:

```text
Fn(A, S) -> Result(S, E)          # failure returns no updated state
Fn(A, S) -> Tuple(Result(A, E), S) # failure may retain accumulated state
```

## Module-owned opaque HashState

HashState is declared and owned by `@bim/std/hash`; it is not a dedicated
language primitive. Its representation is unavailable to Forma code. It is not
a Host resource and has no external identity or lifetime:

```text
HashState = heap-owned opaque HashContext
```

RFC 0109 provides one narrow generic carrier for immutable module-owned opaque
values. Adding another such library type does not extend the VM's closed value
or type enums. HashState may cross ordinary Function, Result, Tuple, Array, closure, heap
promotion, and module boundaries like any other value. Implementations may
share an immutable context across heap copies, but update must produce an
independent logical context and old aliases remain valid. The initial
fixed-size SHA-256 context may simply be copied on update.

HashState cannot be constructed structurally, reflected into fields, encoded
by ordinary codecs, or persisted as external data. TypeDesc reports a stable
opaque nominal leaf so user-space interpreters can either handle it explicitly
or return BlameError. A Host resource table is reserved for future values with
external identity, invalidation, or release behavior and is not introduced by
this phase.

## Hash protocol

`@bim/std/hash` exposes ordinary pure Functions, conceptually:

```forma
new: Fn() -> HashState
update_bytes: Fn(HashState, Bytes) -> HashState
update_string: Fn(HashState, String) -> HashState
update_int: Fn(HashState, Int) -> HashState
finish: Fn(HashState) -> Bytes
```

The byte protocol is deterministic and domain-separated. Composite hashing
must distinguish kind, boundaries, field names, collection length, tag, and
payload presence; it must not depend on heap identity, source provenance,
physical module paths, hash-map iteration, platform endianness, or debug
formatting. Dict fields use canonical field order.

The algorithm and protocol version are explicit standard-library contracts.
`finish` does not consume or invalidate its input at the language level.
Structural `==` compares HashState's logical algorithm/version/context state,
never backing-storage or heap identity.

## Reference structural hash

A reference user-space interpreter uses the existing TypeDesc graph, Dyn
observers, recursion through explicit refs, and ordinary state passing:

```forma
def hash_dyn:
    Fn(TypeDesc, Dyn, HashState) -> Result(HashState, BlameError)
=
    fn(desc, value, state) {
        # inspect desc/value, update state, and return the new state
        ...
    };

def my_hash:
    for(A) Fn(TypeOf(A))
        -> Fn(A, HashState) -> Result(HashState, BlameError)
=
    interpreter!(hash_dyn);
```

Functions and unsupported opaque leaves remain explicit errors. Recursive
TypeDesc refs use the existing `$ref`/resolve model; HashState itself is not
recursively inspected. The reference implementation is evidence for the
user-space interpreter boundary, not the native `==` or a future native Hash
capability factory.

## Explicit diagnostic collection

Diagnostic records are ordinary data. Producing a record does not publish a
Host diagnostic:

```forma
@struct type DiagnosticRecord = {
    data: Any,
    message: String,
    rule: Any,
};
```

Validation code explicitly threads `Array(DiagnosticRecord)`:

```forma
def check:
    Fn(User, Array(DiagnosticRecord))
        -> Tuple(User, Array(DiagnosticRecord))
=
    fn(user, diagnostics) {
        let diagnostics = if user.age < 0 {
            array.push(diagnostics, blame!(user.age, "age must be non-negative"))
        } else {
            diagnostics
        };
        (user, diagnostics)
    };
```

Array push and combination preserve child provenance. A new collection root is
Generated at the operation site, while existing records and the appended
record retain their own data/rule anchors. A caller or Host explicitly decides
whether to format, reject, or publish the finished records.

## Persistent storage and quotas

Persistent updates are never exempt from resource accounting. Operations
charge deterministic logical work and deterministic logical output allocation.
A future storage-sharing fast path does not receive extra quota, and a copied
path does not change semantic success/failure merely because its physical
representation differs. Storage choices must not change fuel, diagnostic
ordering, serialized output, equality, or source provenance.

Heap copy and promotion preserve sharing where supported but never publish a
mutable alias. Failed WorkWorlds discard newly allocated persistent state normally.
No resource-table transaction log is needed.

## Phase sequence

1. RFC 0108: add provenance-preserving persistent Array append, including
   alias, quota, heap-copy, and callback-boundary tests, without claiming a COW
   fast path in the current handle heap;
2. RFC 0109: add a narrow module-owned opaque value boundary, nominal type
   declaration, and generic TypeDesc leaf;
3. RFC 0110: add `@bim/std/hash`, deterministic domain-separated update
   operations, finish, vectors, and quota contracts;
4. RFC 0111: implement a reference user-space structural hash interpreter over
   TypeDesc and Dyn with explicit HashState threading; and
5. RFC 0112: implement explicit Array(DiagnosticRecord) collection, recursive
   validation examples, provenance checks, and Host-boundary guidance.

Each child receives a proposal commit and a separate implementation/result
commit. This umbrella stays Proposed until both reference applications pass
the full workspace quality gate.

## Goals

1. validate explicit pure state threading for hashing and diagnostics;
2. keep update costs bounded and leave room for unobservable runtime sharing;
3. add a useful deterministic standard hash API;
4. prove user-space structural hashing over existing reflection boundaries;
5. collect multiple source-aware diagnostic records as ordinary data;
6. preserve provenance, quota, cancellation, and heap-publication invariants;
7. keep Function signatures and Result/state ordering fully explicit; and
8. gather evidence before considering accumulation syntax.

## Non-goals

- accumulation channels, `yield`, `accumulate!`, or dynamic handlers;
- `$state` bindings, Move checking, linear/affine types, or borrow semantics;
- Unsharable, Marker, AutoMarker, trait, interface, or effect rows;
- hidden Function parameters, hidden returns, or automatic caller rebinding;
- Host resource tables, Port IDs, generations, invalidation, or finalizers;
- dynamic native loading or a general FFI ABI beyond immutable opaque payloads;
- making diagnostic records emit Host diagnostics as a side effect;
- hashing Functions, source provenance, physical paths, or runtime identities;
- cryptographic signing, password hashing, or keyed MAC APIs; or
- claiming that COW proves static uniqueness.

## Shared acceptance criteria

1. state parameters and results are visible in every public Function type;
2. aliases observe immutable snapshots before and after updates;
3. aliases and persistent updates produce stable values and provenance;
4. storage sharing cannot be detected through equality, codec, debug, or quotas;
5. HashState is nominal and opaque but remains an ordinary heap value;
6. HashState needs no Host resource table or lifetime protocol;
7. the hash byte protocol is deterministic, versioned, and domain-separated;
8. structural hash handles supported TypeDesc/Dyn forms recursively;
9. unsupported Function and opaque domains return sourced BlameError;
10. diagnostic Array append retains each record's data/rule provenance;
11. diagnostic collection remains ordinary computation until explicitly
    consumed by a caller or Host;
12. recursive and higher-order examples use ordinary fold/try_fold state
    threading without hidden propagation;
13. failures, cancellation, and heap publication preserve existing atomicity;
14. strict execution and best-effort analysis behavior remain unchanged; and
15. full Forma, CLI, LSP, formatting, and warning-denied Clippy checks pass.

## Stopping rules

Work returns to discussion if a child requires:

1. observable mutation of an aliased Forma value;
2. static uniqueness, general Move analysis, or marker propagation;
3. hidden state parameters or returns to make examples usable;
4. a Host resource table merely to represent HashState;
5. a general `dyn Any` native payload ABI before a second concrete need;
6. provenance loss when appending or threading diagnostic records;
7. platform-dependent or heap-identity-dependent hash output;
8. publishing diagnostics as an evaluator side effect; or
9. weakening quota, cancellation, or WorkWorld atomicity.

## Relationship to the accumulation discussion

`discuss/typed-accumulation-channels.md` remains historical design evidence,
not the implementation plan for this phase. RFC 0107 deliberately tests the
weaker hypothesis that explicit state plus COW is sufficient. A later proposal
may revisit accumulation only after the completed Hash and Diagnose examples
show concrete, repeated ergonomic or compositional failures that ordinary
Functions and combinators cannot address cleanly.
