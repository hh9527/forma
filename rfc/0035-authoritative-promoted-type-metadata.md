# RFC 0035: Authoritative promoted TypeMetadata

- Status: Partially implemented
- Depends on: RFC 0012, RFC 0024, RFC 0034

## Summary

Each XL module constructs its retained TypeMetadata graph exactly once in a
dedicated work world. After every reachable type up-link is Ready, the complete
root set is promoted through one forwarding context into Main World. Static
analysis, LSP queries, validation, codecs, schema generation, and final module
bytecode all refer to this authoritative promoted graph.

The tool stage no longer exports recursive metadata through legacy tree-shaped
`Value`, does not replace forward edges with `Any`, and does not independently
re-evaluate type definitions.

## Motivation

RFC 0034 established correct runtime recursive metadata but retained a
conservative tool-stage shadow. Constructing a second graph for analysis causes
identity drift and requires an artificial `Any` boundary. The language's closed
world and phase discipline permit a stronger pipeline:

```text
source + imported persistent roots
            |
            v
     metadata bootstrap analysis
            |
            v
       MetadataInit bytecode
            |
            v
       sealed work-world graph
            |
       promotion (one context)
            v
  authoritative Main World roots
       |          |          |
       v          v          v
 graph analysis  program    generators
```

## Metadata dependency closure

`MetadataInit` contains every retained type binding and the transitive binding
closure needed to compute it, including ordinary pure decorator factories and
helper functions. Imports are linked to already-ready persistent module roots.
The final module result and bindings unrelated to metadata construction are not
executed by this phase.

The closure is determined from the HIR binding graph rather than source-order
text slicing. A metadata dependency may itself depend on another metadata
dependency. Cycles are legal only through type-name up-links or function closure
captures; eager value cycles retain RFC 0013 errors.

XL currently has no user-visible effect capability. `core:debug` is a
value-preserving evaluation observer and is permitted in MetadataInit. Its
observation belongs to metadata construction and therefore occurs exactly once;
phase splitting must not repeat it in final module or session execution.

## Root-set promotion

MetadataInit returns a deterministic Dict from source type name to its private
up-link or resolved metadata root. All roots are copied together:

```rust
promote_roots(
    target: &mut MainWorld,
    source: &WorkWorld,
    roots: &[(TypeName, RichValue)],
) -> PromotedTypeRoots
```

One forwarding map is shared by the complete root set. Shared subgraphs remain
shared, self and mutual cycles are relinked entirely into Main World, and no
Main handle points back into the discarded work world. Only reachable metadata
is retained. A Pending link aborts module loading before publication.

## Final program linking

Promoted type roots receive stable module-local link keys. The final compiler
treats corresponding `type` bindings as preinitialized external constants:

- it emits no `MakeUpLink`, constructor calls, or `InitializeUpLink` for them;
- ordinary references resolve the persistent metadata root;
- recursive child edges remain hidden Main World up-links;
- LinkTable rebinding relocates the persistent handles without changing the
  heap-independent bytecode blob.

This is an evaluation optimization with an observable invariant: a type RHS and
its decorators execute once per module initialization, never once per session.

## Graph analysis

Analysis owns an immutable graph view indexed by `TypeId`:

```rust
struct TypeGraph {
    nodes: Vec<TypeNode>,
    names: BTreeMap<String, TypeId>,
}

enum TypeNode {
    Ref(TypeId),
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(String),
    Array(TypeId),
    Tuple(Vec<TypeId>),
    Struct(BTreeMap<String, TypeId>),
    Enum(BTreeMap<String, Option<TypeId>>),
    Union(Vec<TypeId>),
    Function { parameters: Vec<TypeId>, result: TypeId },
}
```

The graph is derived from promoted runtime metadata with a handle-to-TypeId
memo. Up-links preserve declaration identity; ordinary acyclic nodes may be
interned structurally later but are not required to be. Assignability tracks
visited `(expected, actual)` TypeId pairs. Display and LSP traversal track active
nodes and print a declared name or a recursion marker on a back edge.

The existing owned `TypeDescriptor` is only a private, finite intermediate used
while bootstrapping analysis. Public Analysis results use `TypeGraph` and
`TypeId` directly; XL does not maintain a second public tree-shaped type API.

## Module lifecycle

For every non-data XL module:

1. imports are loaded and their persistent roots linked;
2. bootstrap analysis identifies and checks the metadata dependency closure;
3. MetadataInit executes under the module quota in a fresh work world;
4. all metadata up-links are sealed;
5. the complete named root set is promoted atomically to Main World;
6. TypeGraph and refined Analysis are derived from promoted roots;
7. final bytecode is compiled and linked against those roots;
8. ordinary module initialization or session execution proceeds without
   re-evaluating type definitions.

MetadataInit fuel and allocation count against the module quota. The final
module phase receives the remaining account rather than a fresh allowance.

## Failure atomicity

Metadata construction failure is a fatal module-load error. No roots enter the
module registry, and no rollback of the append-only Main heap is required. As
with existing module publication, unreachable failed allocations are harmless;
the registry is the authority for visibility.

Diagnostics retain the type binding, decorator, imported rule, and failing
operation origins. Promotion errors name the module and type root whose graph
violated an invariant.

## Deferred work

- cross-module structural type interning;
- snapshot serialization of TypeGraph indices;
- parallel MetadataInit after dependency scheduling;
- user-configurable metadata-phase quota;
- eliminating compatibility TypeDescriptor projections.

## Acceptance criteria

1. self and mutually recursive type roots are promoted together and preserve
   identity in Main World.
2. forward references retain full graph precision without an `Any` edge.
3. type decorators execute exactly once during module loading.
4. final module/session execution does not reconstruct TypeMetadata.
5. codec and schema consumers observe the same persistent root identities.
6. assignability and type display terminate on recursive graphs.
7. metadata-only dependency helpers execute only in MetadataInit, while a
   helper also reachable from ordinary module results remains in final code.
8. metadata construction and final initialization share one module quota.
9. a failed or Pending graph is absent from the module registry.

## Implementation plan

1. Add HIR binding-dependency closure extraction for retained type roots.
2. Compile a named-root MetadataInit function from that closure.
3. Execute, seal, and promote all roots with one forwarding context.
4. Add persistent type-root link keys and skip type RHS in final bytecode.
5. Derive TypeGraph from promoted heap values with identity memoization.
6. Move assignability, display, and LSP-facing analysis to TypeId traversal.
7. Add once-only decoration, identity, quota, and failure-atomicity tests.

## Implementation result

The authoritative runtime graph and publication path are implemented. The compiler extracts a
transitive binding closure rooted at all module type bindings and emits a
MetadataInit function whose result is a deterministic name-to-TypeMetadata
Dict. The loader executes this function once under the existing module quota,
publishes the Dict root in one operation, and extracts named persistent roots
from the resulting Main World object. Because publication starts from one Dict,
one forwarding context preserves sharing and all recursive identities.

The final compiler receives the promoted type-name set. It replaces each type
RHS with a LinkTable-backed external constant under a stable `type:<name>` key;
no type up-link allocation, decorator call, or metadata constructor remains in
session execution. A forward-reference test now succeeds even when its source
places a codec call before the referenced type, because MetadataInit has sealed
the complete graph before final bytecode can run. The debug integration test
executes two sessions and confirms that the type RHS is observed once at module
load while ordinary session debug work repeats.

After publication, Analysis derives an immutable `TypeGraph` directly from the
persistent roots. Persistent object and up-link handles share one identity memo,
so a named root and an edge that links back to it receive the same `TypeId`.
Public declared and binding type maps and the result type contain `TypeId`s;
display and assignability traverse the graph with active-node and visited-pair
sets. Recursive display uses the declared name on a back edge. The old
`TypeDescriptor` is no longer part of the public API.

Bootstrap analysis remains necessary to compile MetadataInit. For modules with
type bindings its debug sink is intentionally discarded so this conservative
pass is not observably mistaken for authoritative construction. It runs under a
separate compiler-analysis account bounded by the same configured limits, so it
cannot consume fuel or allocation from the semantic module-initialization
quota. It may use `Any` internally while checking a forward edge, but that
shadow is replaced by the persistent graph in the returned Analysis.

The dependency closure is also compared with ordinary runtime reachability.
Top-level helpers and imports reachable only from metadata roots are removed
from the final AST before bytecode generation. A helper that is also reachable
from the module result or another ordinary binding is retained. Because `dbg`
is an observer rather than an effect, a metadata-only observation occurs once
during MetadataInit, while a retained helper observes each ordinary execution.

The following RFC 0035 items therefore remain open:

- eliminating duplicated bootstrap CPU work by moving all checks to the
  promoted graph; the duplicate shadow is already unobservable and does not
  consume module quota.
