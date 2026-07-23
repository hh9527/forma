# RFC 0034: Recursive TypeMetadata through hidden up-links

- Status: Implemented
- Depends on: RFC 0013, RFC 0027, RFC 0033

## Summary

Type bindings may recursively and forward-reference other type bindings in the
same block. The runtime represents these metadata graph edges with the hidden
up-link mechanism already used by recursive function construction:

```xl
@struct
type Node = {
    value: Int,
    children: Array(Node),
};
```

No explicit `decl` or `def` is required. A type binding is already a declaration
that constructs metadata exactly once.

## Distinction from recursive functions

A recursive function captures an up-link while constructing its closure and
resolves the link only when its body executes. A recursive type embeds an
up-link directly as an edge in the TypeMetadata graph while constructing its
right-hand side.

Ordinary value reads retain RFC 0013 behavior. In particular, this remains an
invalid eager read:

```xl
decl value: Int;
def value = value + 1;
```

Only compilation of a type RHS preserves references to same-block type slots.
XL code cannot construct, inspect, match, compare, serialize, or initialize an
up-link directly.

## Construction

Before evaluating a block, the compiler identifies retained type bindings and
allocates one private up-link for each name. Each type RHS is evaluated once in
metadata-construction mode and initializes its corresponding slot. References
to these type names embed the slot rather than issuing `ReadUpLink`. All slots
must be Ready before the block completes.

Model constructors accept a hidden up-link wherever TypeMetadata is accepted.
They do not resolve a pending link during construction. Other arguments retain
their ordinary eager validation. Publication copies Ready links and their
reachable graph using the existing cycle-preserving promotion algorithm;
pending links cannot publish.

## Consumers

Metadata consumers resolve hidden links internally:

- validation and codec execution follow links according to the finite input
  value being checked or transformed;
- planning recognizes link identity rather than structurally expanding the
  graph forever;
- diagnostics use the nearest concrete field/variant rule and may use the type
  binding location for a link-level failure;
- ordinary JSON serialization rejects a leaked up-link as an internal error.

The first implementation may retain lazy link nodes in its codec plan instead
of converting the complete plan representation to an arena. This is semantic,
not observable: following the same Ready link always reaches the same immutable
metadata.

## JSON Schema

Schema generation assigns deterministic definition names to reachable up-link
identities and emits recursive uses as `$ref`:

```json
{
  "$ref": "#/$defs/Node",
  "$defs": {
    "Node": {
      "type": "object",
      "properties": {
        "value": {"type": "integer"},
        "children": {
          "type": "array",
          "items": {"$ref": "#/$defs/Node"}
        }
      }
    }
  }
}
```

Names prefer the source type binding name when available and otherwise use a
stable traversal name such as `Type0`. Collisions receive deterministic numeric
suffixes. `$defs` order is deterministic.

## Static analysis boundary

The current `TypeDescriptor` and legacy tool-stage `Value` are tree-shaped. The
tool environment therefore predeclares all type names with `Any` metadata before
evaluating type RHS expressions. A recursive or forward edge is represented as
`Any` for static checking, while already completed acyclic references retain
their precision. Runtime TypeMetadata remains authoritative and fully recursive.

Replacing `TypeDescriptor` with a graph representation and preserving recursive
precision in LSP output is deferred. This degradation must not alter runtime
codec, validation, or schema behavior.

## Resource boundaries

Construction remains covered by module allocation/fuel/stack quota. Codec
execution consumes resources according to actual input depth. Schema traversal
tracks active and completed link identities, terminates on cycles, and charges
the generated `$defs` and `$ref` values normally.

## Deferred work

- graph-shaped static TypeDescriptor and recursive LSP display;
- nominal names retained directly on metadata nodes;
- plan arenas and cross-call plan caching;
- recursive aliases that contain no productive Struct, Enum, Array, Tuple, or
  Union node;
- user-visible reflection over reference identity.

## Acceptance criteria

1. a self-recursive Struct type constructs and publishes successfully.
2. mutually recursive type bindings construct independently of source order.
3. ordinary eager `decl/def` reads remain invalid.
4. model constructors accept hidden type links but reject them in non-type data.
5. codec decode and encode traverse recursive Struct/Array data correctly.
6. invalid recursive data retains data and nearest rule locations.
7. `json.schema` terminates and emits deterministic `$defs`/`$ref` output.
8. pending links cannot publish and leaked links cannot stringify.
9. module and execution quotas remain enforced.

## Implementation plan

1. Predeclare type names in tool analysis with conservative `Any` shadows.
2. Preallocate retained type up-links and compile type RHS references as graph
   edges before one-time initialization.
3. Admit hidden links at TypeMetadata constructor boundaries.
4. Add lazy link resolution to codec planning and transformation.
5. Generate `$defs`/`$ref` with identity memoization.
6. Add self/mutual recursion, diagnostics, publication, and quota tests.

## Implementation result

Implemented by preallocating hidden up-links for retained type bindings before
their runtime RHS expressions are compiled. During a type RHS only, references
to those names preserve the slot value; normal expressions continue to emit
`ReadUpLink`. Each RHS initializes its own slot exactly once and the existing
block seal and heap publication machinery reject pending links.

The tool stage predeclares all module type names with `Any` metadata. Sequential
acyclic references regain precision as definitions replace their shadows;
self-recursive and forward edges remain conservative. Runtime metadata is not
degraded. Native and model TypeMetadata constructors admit private link edges,
while ordinary native values retain the public `ValueKind` boundary.

Codec nodes retain lazy `UpLink` handles and resolve them only while traversing
actual input. Struct planning transparently resolves ordinary type aliases one
level while preserving recursive child edges. JSON Schema generation memoizes
link handles, assigns deterministic traversal names (`Type0`, `Type1`, ...),
and emits `$defs`/`$ref`; source names are unavailable in current normalized
metadata and remain deferred. When the root was obtained through an ordinary
resolved binding, it is emitted inline and its recursive backlink receives the
definition, which is valid but may duplicate the root shape once.

Metadata construction and consumption now have an explicit readiness boundary.
Decorators and model constructors treat a pending link as an opaque valid type
edge: they preserve wrappers and attributes but defer target-dependent checks
such as whether `flatten` eventually names a Struct. Before codec or schema
planning, the VM traverses the complete metadata graph by link identity and
requires every reachable link to be Ready. An early consumer invocation reports
`UninitializedDefinition` rather than returning a data-level codec `Err`;
encode, decode, and schema traversal may consequently treat readiness as an
execution invariant.

The end-to-end module fixture exports self-recursive `Node` and mutually
recursive `Left`/`Right` metadata through the persistent world, decodes and
encodes finite recursive values, and generates terminating schemas containing
multiple references. A located invalid JSON leaf retains both its deep data
path and rule location. Direct JSON stringification of the same recursive
metadata rejects the hidden link, confirming that it has not become an XL data
type. A decorated forward reference verifies that decoration preserves the
pending edge, and a deliberately interleaved codec call verifies the readiness
boundary. Existing eager `decl/def` failure tests remain unchanged.
