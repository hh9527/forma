# RFC 0012: Layered Heaps and Export Promotion

- Status: Accepted
- Implementation: Complete

## Implementation result

The implementation uses owner-tagged runtime handles, one read-only world heap,
and one writable arena per execution. Bytecode and native calls share a
`RuntimeValue` register stack; public `Value` conversion occurs only at API and
serialization boundaries. Promotion uses a failure-atomic pending copy with
shared forwarding for all roots and shallow-shares `Arc<FuncByteCode>` while
rebuilding heap-relative links.

Module initialization publishes its reachable export as a `Ready` root in the
shared world. Import instructions use external value slots in the linking table
and are resolved directly to those persistent roots. They do not reconstruct
imports through owned public values. The dependency-diamond test verifies that
`a -> c` and `b -> c` retain the exact same persistent root and that initializing
`b` does not grow the world when its export is only `c`.

The current ordinary loader keeps the world append-only for its lifetime. It
does not perform module tree shaking or reclamation. Session results are read
through their mixed world/arena view and exported directly without publication.

### Sealed execution amendment

The implementation does not assign process-global identities to local heaps.
A local heap is confined to one `ExecutionArena`; only the persistent world is
shared. Runtime references distinguish the `Local` and `Persistent` storage
domains, but local references do not carry an arena identity.

The publication API consumes a sealed execution and returns `PersistentValue`
roots. It does not accept caller-composed raw roots or an arbitrary source and
target heap pair. `ExecutionArena` keeps its local heap and result private, and
offers only two terminal operations:

```text
publish(world)   -> PersistentValue
export(world)    -> public Value
```

Module cache entries and bytecode external-link resolvers store only
`PersistentValue`. Consequently a local reference cannot enter a module cache,
cross into another execution, or be supplied as an imported value. Persistent
objects are validated to contain no local references before publication is
committed.

## Summary

This RFC defines the internal value-lifetime boundary used by XL executions.
Each execution reads an immutable persistent world and allocates new compound
values in a private arena. Successful module initialization publishes only the
local graph reachable from its export root. Publication copy-collects that
graph into persistent storage, re-interning text and shapes in the destination.
Failed executions publish nothing and discard their arena as a unit.

This is a runtime implementation strategy, not a language promise that XL has
no garbage collector or that publication always performs a physical copy. The
language exposes immutable values and stable results, never heap identities,
object addresses, intern identifiers, collection timing, or reclamation timing.

## Motivation

XL modules are pure, evaluate once, and frequently perform expensive work to
produce a much smaller exported result. A capability module may process a large
data module in a binding module and export only a specialized validator. Keeping
every initialization temporary alive places unnecessary pressure on a general
tracing collector; retaining arbitrary `Arc` graphs also obscures which values
belong to an execution and which may safely outlive it.

RFC 0011 already gives each execution a private stack and quota account. A
private allocation arena completes that ownership boundary:

```text
background: persistent, immutable, shared
current:    private to one execution, append-only
```

The model also prepares a later snapshot optimizer to trace code, data, and
type metadata uniformly from selected entry roots. Snapshot optimization,
late-bound data configuration, and ahead-of-time partial evaluation are not
part of this RFC.

## Normative language semantics

The following properties are language-visible and independent of heap design:

- XL values are immutable.
- A module is initialized at most once in one world construction.
- A successful module export remains valid while its world remains valid.
- A failed module initialization publishes no module value.
- XL has no observable object address or reference identity.
- XL has no user-visible finalizers, destructors, or weak references.
- Value equality depends on semantic contents, not storage or interning.
- Resource quota failures remain deterministic and source-aware.

The language does not specify allocation regions, handle widths, interning,
copying, tracing, compaction, or collection schedules.

## Runtime architecture

The current implementation uses one append-only persistent heap per `VmState`
and one execution arena per active evaluation:

```rust
struct VmState {
    world: WorldHeap,
    modules: ModuleRegistry,
}

struct Execution<'vm> {
    background: &'vm WorldHeap,
    current: ExecutionArena,
    stack: ValueStack,
    quota: QuotaAccount,
}
```

Existing persistent objects are read-only to an execution. The publication
operation is the only component permitted to append a validated result graph to
the world and install its root in the module registry. XL and native callbacks
cannot mutate either heap directly.

An execution resolves a mixed graph through both heaps. Its stack and current
objects may contain local and background references. New objects, text, and
shapes are allocated only in `current`; a background lookup miss never appends
to the world:

```text
current    -> current       allowed
current    -> background    allowed
background -> background    allowed
background -> current       forbidden
```

The VM and native `CallContext` use this combined read view and a current-only
allocator. They receive no operation that mutates `background`.

The MVP loader remains sequential. Parallel module evaluation and atomic
multi-writer publication are deferred; the ownership and publication contract
must not preclude them.

## Locations and values

There are three internal location mechanisms:

```text
RegisterId        locates a slot on the current XL stack
LocalHandle       locates an object in the current execution arena
PersistentHandle  locates an object in the world's persistent heap
```

`RegisterId` is not a value and cannot be stored in an XL object. Stack slots
contain execution values:

```rust
enum ExecutionValue {
    Int(i64),
    Float(f64),
    Atom(ExecutionInternRef),
    String(ExecutionString),
    Local(LocalHandle),
    Background(PersistentHandle),
}
```

Persistent objects contain only persistent values:

```rust
enum PersistentValue {
    Int(i64),
    Float(f64),
    Atom(PersistentInternId),
    String(PersistentString),
    Heap(PersistentHandle),
}
```

Rust types or equivalent checked constructors must make
`PersistentObject -> LocalHandle` unrepresentable. Module registry entries and
other long-lived roots accept only persistent values.

These types are runtime-private. The embedding API exposes owned roots and
borrowed value views, not raw handles or intern IDs.

## Heap objects

Compound runtime values are heap objects:

```text
String payload when not interned
Bytes
Array
Tuple
Dict
Closure
Prototype
```

An execution object may contain immediate values, local references, and
background references. A persistent object may contain immediate values and
persistent references only. Copying a background value into a register copies
its tagged value, not its persistent object graph.

Closures retain their prototype and immutable upvalues. A bytecode prototype
separates heap-independent code from heap-specific links:

```rust
struct ByteCodeProto<L> {
    code: Arc<FuncByteCode>,
    links: LinkingTable<L>,
}
```

`FuncByteCode` is an immutable, position-independent code blob. It contains
opcodes, registers, jumps, arities, register counts, debug origins, and host
diagnostic metadata. It contains no XL `Value`, object handle, intern ID, shape
ID, or nested linked prototype. Instructions refer to typed link-table slots:

```rust
enum Instruction {
    LoadConst { dst: Register, value: ValueLinkId },
    GetField { dst: Register, dict: Register, field: TextLinkId },
    MakeClosure {
        dst: Register,
        prototype: ProtoLinkId,
        captures: Box<[Register]>,
    },
    // heap-independent operations
}
```

Every item whose runtime representation may allocate, intern, or depend on a
heap belongs to `LinkingTable`:

```text
String and Bytes constants
Atom constants
Array, Tuple, and Dict constants
Dict field text and Shapes
type metadata and other XL values
nested bytecode or native prototypes
```

Immediate numeric constants may also use value links for a uniform constant-
load instruction. Typed link IDs let assembly/link validation prove that an
instruction cannot read a text slot as a prototype or a value slot as a shape.
Execution treats a sealed linking table as immutable.

The compiler first produces heap-independent code plus owned link
specifications:

```rust
struct UnlinkedProto {
    code: Arc<FuncByteCode>,
    links: Box<[LinkSpec]>,
}

enum LinkSpec {
    Value(FrozenValue),
    Text(Box<str>),
    Shape(Box<[Box<str>]>),
    Prototype(Box<UnlinkedProto>),
}
```

Linking injects these specifications into an execution heap and produces a
local `ByteCodeProto`. Copying that prototype shallow-clones
`Arc<FuncByteCode>` and rebuilds every linking-table entry in the target heap
using the same value/text/shape/prototype forwarding context as ordinary root
copying. Prototype forwarding is recorded before links are scanned, preserving
shared nested prototypes and future recursive cycles.

A native prototype follows the same boundary: its trusted static callback
descriptor is shallow-copied while all bound XL values live in links or closure
upvalues and are copied normally. Consequently an exported closure is a
complete tracing root even though immutable instruction bytes are shared.

## Per-heap interning

Interning belongs to a heap lifetime; there is no immortal process-global text
table. Both the execution arena and persistent heap own text and shape
interners. An intern ID is meaningful only in its owner.

Atoms always wrap an interned text reference while retaining the distinct XL
type `Atom`. Strings whose content satisfies a deterministic implementation
threshold may use the same text entries while retaining the XL type `String`:

```text
Atom("Ok")   != String("Ok")
```

The representation choice for a String is not observable. It depends only on
its contents and fixed engine rules, never cache state or initialization order.

Execution lookup first checks the persistent interner. Existing text may be
referenced persistently without creating a local entry. Otherwise eligible text
is interned in the execution arena. Long strings remain ordinary heap objects.
Session-local text therefore disappears with the session and cannot grow the
world interner implicitly.

Dict field names are interned text. Shapes contain sorted intern references and
are interned per heap. Shape equality and Dict field ordering remain content-
based and deterministic.

## Root copying and export promotion

The primitive operation copies one or more roots into a target heap:

```rust
fn copy_roots(
    to: &mut Heap,
    from: &HeapView,
    roots: impl IntoIterator<Item = ValueRef>,
) -> Result<Vec<OwnedRoot>, CopyError>;
```

`HeapView` resolves the source execution's current and background references.
Returned roots and their complete reachable graphs are self-contained in `to`:
no source-local handle, intern ID, or shape ID remains. A register wrapper may
select a source stack slot and write the returned root to a target stack, but
stacks are root carriers rather than part of the collector.

Multiple roots copied in one operation share forwarding tables. When a
reference already belongs to `to`, it is retained without rescanning; a
reference owned by any other heap is copied. Module publication is the common
special case where the source background is the target world, so background
references are retained and only current objects require copying. Snapshot
compaction copies roots into a different empty image heap, so all reachable
source-world objects are copied and renumbered.

Export promotion is `copy_roots` used at a module publication boundary. It
creates a pending, unpublished persistent delta:

```rust
struct PromotionContext {
    objects: Map<LocalHandle, PendingHandle>,
    text: Map<LocalInternId, PendingInternId>,
    shapes: Map<LocalShapeId, PendingShapeId>,
    prototypes: Map<SourceProtoRef, PendingProtoRef>,
}
```

Root copying follows these rules for normal module promotion:

```text
immediate value       copy unchanged
target-world reference retain unchanged and do not rescan
local object          copy once through the object forwarding table
local intern          resolve bytes and re-intern in the destination
local shape           promote its field text, then re-intern the shape
linked prototype      shallow-clone code and recursively rebuild its links
```

The collector reserves a destination object and records its forwarding entry
before scanning children. This preserves shared subgraphs and supports future
recursive closures or other cycles.

Destination intern IDs need not equal source IDs. Existing destination text or
shapes are reused by content. Copy output must contain no reference or
auxiliary ID owned by a source heap, except references already owned by the
target.

The collector writes a `PendingHeapDelta`, not the visible world. After the
complete graph and invariants have been validated, the runtime atomically
attaches the delta and publishes the persistent root. Any failure discards both
the pending delta and execution arena without modifying the world.

Promotion allocation is runtime maintenance and is not charged a second time
to the execution's allocation quota. Hosts may impose a separate physical
world-size limit. RFC 0011 continues to charge logical payload construction in
the current arena.

## Module initialization

Module initialization is coordinated by canonical module identity:

```rust
enum ModuleState {
    Uninitialized,
    Initializing,
    Ready(PersistentValue),
    Failed(ModuleError),
}
```

For the dependency diamond:

```text
root -> a -> c
     -> b -> c
```

`c` is evaluated and published once. Both `a` and `b` receive the same
persistent `c` root as background data; neither copies it during its own
promotion. The world is one logical append-only heap, so a module-heap tree or
DAG is not exposed by the runtime.

Every `Ready` entry is a root in the same shared `WorldHeap`. The dependency
API returns only its `PersistentValue`; it never exposes the dependency's old
execution arena, stack, or local handles. Consequently `a` and `b` observe the
same `c` root, and all data reachable from it is known to be persistent.

The MVP retains every initialized module export until its `VmState` is dropped:

```text
world roots = all ModuleCache::Ready values
```

There is no module unloading, module-level reclamation, or main-root tree
shaking in the ordinary engine. A later snapshot optimizer may select only
`main` and other configured entries as roots copied into an image heap. That
optimization does not change ordinary initialize-once or retention semantics.

The current source loader may continue to reject import cycles. Recursive
bindings and future cyclic module semantics require a separate RFC.

The root module is initialized and published like any other module. An exported
`main` closure is ordinary persistent module data. Until record-shaped module
exports and entry selection are implemented, the current compiled root entry
may remain a compatibility execution boundary, but dependency modules must use
the registry's initialize-once publication path.

## Sessions

A runtime session reads the completed world and allocates into a fresh arena.
Its ordinary result is normally encoded as JSON, YAML, TOML, JSONL, or text
while its stack and mixed heap view remain alive, then discarded with the
arena. Serialization traverses local and background references read-only and
does not copy the result to the module world. If an embedding needs an owned
result beyond that lifetime, it explicitly copies the root into a separate
result heap; sessions never append implicitly to the module world.

The MVP may keep the session arena until return rather than collecting dead
temporaries online. Allocation quota bounds such growth. A future local,
generational, or copying collector may reclaim within long-lived sessions
without changing language semantics or public APIs.

## Quotas and failures

Runtime construction continues to charge RFC 0011 logical allocation bytes,
regardless of whether a short String is interned or stored as an object.
Interning and shape cache hits do not alter charges.

Arena allocation failure, invalid handles, collector invariant failure, or a
host persistent-world limit produces a structured runtime/module error. Module
publication errors identify the responsible module. No partially published
root or persistent interner entry may become visible.

## Public API

Raw `LocalHandle`, `PersistentHandle`, intern IDs, heap objects, and promotion
tables are private. Public APIs use conceptual wrappers:

```text
ValueRef<'execution>  borrowed view valid for one execution/world borrow
OwnedValue            root that owns or retains its persistent storage
LoadedModule          owns/retains its world and persistent export root
```

Formatting, equality, validation, and native register access resolve values
through these views. Public code cannot construct a dangling handle or inspect
storage identity.

## Rejected alternatives

### Expose a no-GC language guarantee

The arena and promotion collector make an online global collector unnecessary
for the current workload, but long-lived processes may benefit from one later.
Collection strategy is not observable language semantics.

### Use process-global immortal interning

Short session strings and future dynamic atoms could grow an uncollectable
table and bypass execution allocation boundaries. Intern tables follow heap
lifetimes and participate in promotion.

### Freeze the entire execution arena

This retains high-cost initialization intermediates that do not contribute to
the export. Export-root collection is the principal benefit of the design.

### Deep-copy persistent dependencies during publication

It duplicates diamond dependencies and breaks initialize-once sharing.
Persistent references remain references to the same world objects.

### Require an execution heap to be self-contained

Copying a large persistent data module into every binding module would defeat
the capability/data/binding pattern this design is intended to support. Mixed
execution graphs are allowed; only values crossing a publication or owned-
result boundary must become self-contained in their target heap.

### Maintain one sealed heap per module

That creates a heap dependency DAG, complicates lookup and final compaction,
and is unnecessary without independent module unloading. The MVP uses one
logical append-only world heap.

### Mutate the visible world during tracing

A failure could leak intern entries or unreachable objects. Publication first
builds an unpublished delta and commits only a complete validated graph.

### Expose handles through the embedding API

Handles are meaningful only with their owning heap and constrain future moving
or memory-mapped implementations. Owned roots and borrowed views preserve
implementation freedom.

### Store constants or field strings in `FuncByteCode`

Such operands would make an apparently shared code blob retain source-heap
values or require instruction rewriting during every copy. Heap-dependent
operands belong in the linking table; `FuncByteCode` remains shallow-clonable.

## Deferred work

- snapshot file format and memory-mapped runtime images;
- late-bound data-module packaging configuration;
- partial evaluation, residual code, and global tree shaking;
- parallel module initialization and concurrent publication;
- online collection for long-lived sessions or process heaps;
- module unloading and independent persistent sub-heaps;
- configurable persistent-world physical memory limits;
- dynamic String-to-Atom conversion and explicit atom policy;
- recursive modules and source-visible module export records.

## Implementation plan

1. Introduce private local/persistent value, object, text, shape, and handle
   representations plus checked resolver APIs.
2. Centralize compound value allocation in an execution arena; stack and
   native code retain register-only access through value views.
3. Add per-heap deterministic text and shape interning. Represent atoms as
   typed intern references and eligible short strings through the same entries.
4. Split the current bytecode function into heap-independent `FuncByteCode`,
   unlinked `LinkSpec`, and heap-specific sealed `LinkingTable`; move constants,
   field text, shapes, and nested prototypes behind typed link IDs.
5. Implement multi-root heap copying with object/text/shape/prototype
   forwarding, target-owner reuse, code-blob shallow cloning, and an unpublished
   pending delta; expose module promotion as a specialized caller.
6. Make persistent publication reject every local reference before attaching
   the delta.
7. Introduce initialize-once module registry states, retain every `Ready` value
   as a shared world root, and route dependency XL modules through persistent
   publication; preserve current root-entry compatibility.
8. Keep public owned values and borrowed views independent of raw handles.
9. Add collector, diamond dependency, failure atomicity, quota, equality,
   interning, closure-capture, and source-diagnostic tests.

## Acceptance criteria

1. Every runtime compound allocation is owned by an execution arena or the
   persistent world; arbitrary runtime `Arc` value graphs are eliminated.
2. Stack and current-heap values may mix local and background references while
   all background access remains read-only and native callbacks remain
   register-only.
3. Persistent objects and registry roots cannot contain local handles, intern
   IDs, or shape IDs.
4. `copy_roots` produces target-self-contained roots, shares one forwarding
   context across all roots, and copies every foreign reference while retaining
   references already owned by the target.
5. Shared subgraphs and cycles are preserved through forwarding entries.
6. String and Atom promotion re-interns text in the target while preserving
   their distinct XL types and content equality.
7. Dict promotion re-interns field text and shapes with deterministic ordering.
8. `FuncByteCode` contains no heap-relative data; copying a linked prototype
   preserves the same `Arc<FuncByteCode>`, rebuilds all links in the target, and
   preserves shared or recursive prototype graphs through forwarding.
9. String, Bytes, Atom, aggregate constants, field text, shapes, type metadata,
   and nested prototypes are all represented through typed linking-table slots.
10. A failed promotion changes neither persistent objects nor persistent
   interner/shape counts.
11. The dependency diamond initializes `c` once and gives `a` and `b` the same
   persistent export from `ModuleCache`; neither can observe a private c heap.
12. Initialization temporaries unreachable from the module export are absent
    from the persistent world.
13. Session results can be serialized directly through their mixed heap view;
    local allocations and intern entries are then discarded without growing
    the module world unless explicit result copying is requested.
14. Allocation quota behavior remains based on RFC 0011 logical payload sizes
    and does not vary with interner cache hits.
15. Every initialized module export remains rooted by the ordinary engine until
    its `VmState` is dropped; module-level tree shaking is not performed.
16. Public APIs expose owned roots and borrowed views, never raw handles.
17. Existing XL results and diagnostics are unchanged when resources suffice.
18. Workspace tests, strict Clippy, formatting, and diff checks pass.
