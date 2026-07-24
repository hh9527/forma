# RFC 0037: Type-erased native continuations

- Status: Implemented
- Depends on: RFC 0015, RFC 0036

## Summary

The VM replaces its closed `NativeContinuation` enum with a crate-private,
type-erased continuation interface:

```rust
trait NativeContinuation: Debug {
    fn return_target(&self) -> &ReturnTarget;
    fn trace_frame(&self) -> &RuntimeFrame;

    fn resume(
        self: Box<Self>,
        value: RichValue,
        current: &mut Heap,
        background: &Heap,
        account: &mut QuotaAccount,
    ) -> Result<VmAction, RuntimeError>;
}

enum ReturnTarget {
    Root,
    Register(Register),
    Native(Box<dyn NativeContinuation>),
}
```

Array and JSON encode continuations implement the trait directly. Adding a new
native state machine no longer requires adding an enum variant or editing the
central return, depth, trace, and resume dispatcher.

This RFC changes no XL syntax, value, quota, error, callback, or execution
semantics.

## Motivation

RFC 0015 introduced the first suspended native operation and represented its
state directly in `ReturnTarget`. RFC 0036 generalized that representation with
an enum for Array and JSON encode continuations. The enum retains exhaustive
checking, but every new native state machine requires coordinated edits to the
central dispatcher:

- add a variant;
- expose its parent return target;
- expose its trace frame;
- add its resume branch.

Function defaults, validators, and user-defined codecs are expected to reuse
the same VM boundary. Their implementations should own their dispatch behavior
locally rather than extend a central list.

## Object-safe ownership boundary

Resume consumes a suspended computation. `self: Box<Self>` is object-safe and
expresses this ownership transfer without cloning, downcasting, `Any`, or an
unsafe erased-state pointer. A continuation may move its owned schema, input,
callback, accumulator, output, decisions, and parent return target into the
next `VmAction`.

The trait remains crate-private. It is not an extension API or stable ABI.
Continuation implementations are ordinary Rust types compiled with the VM.

`Debug` is retained for internal diagnostics. `Send` and `Sync` are not
required: a continuation belongs to one active VM execution and never crosses
threads independently of its heap and stack.

## Shared invariants

Every continuation must:

- return its direct parent `ReturnTarget` by reference;
- return the frame appended for callback fuel, depth, and runtime failures;
- resume using the current Work heap, Main background, and active quota account;
- borrow neither heap nor VM stack across suspension;
- preserve deterministic callback ordering;
- move, rather than duplicate, its parent return target when it completes.

The VM remains responsible for:

- charging call fuel before entering a callback;
- including nested native continuations in logical call depth;
- appending continuation frames to callback failures;
- dispatching bytecode and native callback prototypes;
- writing ordinary register return values.

## Rejected alternatives

### Keep the enum

The enum is appropriate for a permanently closed set and offers exhaustive
matching. XL already has multiple planned callback-driven native operations, so
the repeated central edits are expected to grow without adding safety to the
individual state machines.

### Function pointer plus `Box<dyn Any>`

A manually erased state and resume function also avoids the enum, but requires
downcasting and separates state from its behavior. The object-safe trait keeps
that relationship checked by Rust.

### Native bytecode frames

Representing native state machines as a second bytecode or explicit PC/register
frame could improve inspection and snapshotting. It is substantially broader
than the present dispatch problem and remains unnecessary while continuations
are internal runtime implementations.

## Acceptance criteria

1. `ReturnTarget` stores `Box<dyn NativeContinuation>` and has no continuation
   kind enum.
2. Array and JSON encode continuations implement the same object-safe trait.
3. Resume consumes the boxed continuation without clone or downcast.
4. Nested native depth and trace traversal use only trait methods.
5. Existing Array callback behavior, quota accounting, and errors are unchanged.
6. Existing function-valued JSON skip behavior, promotion, quota, and errors are
   unchanged.
7. No public API or XL-visible behavior changes.

## Implementation plan

1. Introduce the crate-private object-safe trait and change `ReturnTarget`.
2. Move Array resume, parent, and trace behavior into its trait implementation.
3. Move JSON encode resume, parent, and trace behavior into its implementation.
4. Remove the enum dispatcher and direct variant construction.
5. Run the full execution, quota, trace, Array, and JSON codec test suites.

## Implementation result

`ReturnTarget::Native` now owns a `Box<dyn NativeContinuation>`. The previous
closed continuation enum and its central dispatch matches have been removed.
Array and JSON encode continuations implement the same crate-private,
object-safe trait locally; resumption consumes `Box<Self>`, while call-depth
and trace traversal use the trait's parent-target and frame accessors.

The state machines, callback order, quota account, error propagation, and
XL-visible behavior are unchanged. Verification completed with 146 unit tests
passing (plus one ignored manual parsing baseline), all four CLI tests passing,
strict workspace Clippy, formatting checks, and `git diff --check`.
