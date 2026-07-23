# RFC 0021: Rich Runtime Values and Inline Source Locations

- Status: Implemented

## Summary

XL adopts a Nickel-style rich runtime value boundary. Every value flowing
through VM registers or stored on a heap edge carries an optional compact
source location:

```rust
struct RichValue {
    value: RuntimeValue,
    loc: Option<Loc>,
}

struct Loc {
    source: SourceId,
    start: u32,
    end: u32,
}

struct SourceId(NonZeroU32);
```

`SourceId` is 1-based. Zero is unavailable to valid IDs and is used by Rust as
the niche for `Option<Loc>::None`. On supported Rust targets, `Loc` and
`Option<Loc>` are both 12 bytes and `RichValue` remains `Copy`.

Locations are observational metadata. Equality, hashing, shape interning,
function identity, JSON representation, debug representation, and XL-visible
semantics compare or inspect only `RuntimeValue` unless an operation is
explicitly producing a diagnostic.

## Motivation

JSON provenance currently lives in a module-loader side table keyed by value
paths. That works for immediate validation, but a value loses the connection
after extraction, collection transformation, closure capture, codec
normalization, or construction of a new container. RFC 0020 therefore reports
stable codec paths but cannot attach the original JSON source span.

XL is a data transformation language. Data locations should survive the same
ordinary immutable transformations as data values. Carrying a compact location
on each runtime edge is simpler than rebuilding paths after arbitrary
computation and gives validation, codecs, debug tooling, and future editor
features one common source boundary.

## Compact source model

`SourceId` becomes a transparent wrapper around `NonZeroU32`. A
`SourceDatabase` assigns IDs as `files.len() + 1` and subtracts one for vector
lookup. Exhaustion and source text larger than `u32::MAX` remain checked
errors.

The public compact location is:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Loc {
    source: SourceId,
    start: u32,
    end: u32,
}
```

Construction enforces `start <= end`. It exposes:

```rust
fn source(self) -> SourceId;
fn start(self) -> u32;
fn end(self) -> u32;
fn range(self) -> Range<usize>;
```

`range` performs the lossless conversion required on supported 32- and 64-bit
targets. `SourceFile::slice(Loc)` checks the source ID at the database boundary
and uses `str::get`, so malformed UTF-8 boundaries never panic.

The existing `Location` name remains a compatibility alias for `Loc` during
the migration. `TextRange` remains available for lexer/CST APIs, but runtime
values store the flat three-word `Loc` rather than a nested range object.

Layout assertions are internal performance regression tests, not a stable FFI
or serialized ABI promise:

```text
size_of::<SourceId>()     == 4
size_of::<Loc>()          == 12
size_of::<Option<Loc>>()  == 12
size_of::<RichValue>()    == 32 on the current 64-bit target
```

## Runtime representation

`RuntimeValue` remains the compact tagged payload containing immediate scalars
or heap handles. `RichValue` is the unit of movement and storage:

```text
VM register                 RichValue
Array element               RichValue
Tuple element               RichValue
Dict field value            RichValue
closure upvalue             RichValue
definition up-link payload  RichValue
bytecode value link         RichValue
module persistent root      RichValue
```

Strings, Atoms, Shapes, and prototypes continue to use intern IDs and handles.
Their storage objects do not duplicate source locations; each edge referencing
the payload owns the observation location. The same heap object may therefore
be referenced by two RichValues with different locations.

Legacy public `Value` import creates `RichValue { loc: None }` unless the API
explicitly supplies a location. Export drops runtime locations because legacy
`Value` has no provenance field. This is an explicit compatibility boundary.

Promotion copies RichValues and their locations while rebuilding local heap
handles. Source IDs are not relocated: one loaded module world, its persistent
heap, its local execution heaps, and its retained `SourceDatabase` share one
append-only source-ID namespace.

## Location propagation

The initial propagation rules are deliberately local and deterministic.

- Move, binding, capture, identity calls, `dbg`, and successful `unwrap`
  preserve the complete RichValue.
- Field and tuple access return the location stored on the selected edge.
- Literal loads and scalar/comparison results use the current instruction's
  source origin when available.
- Array, Tuple, and Dict construction give the container root the construction
  expression location and preserve each input element/field RichValue.
- Arithmetic and interpolation results use the operator/expression location.
- Function calls do not overwrite the returned RichValue location.
- Native collection functions preserve selected input/callback results; newly
  constructed collection roots use the native call origin.
- JSON lowering assigns every scalar and container its exact CST range.
- Host values and synthetic values without a meaningful source use `None`.

For codec normalization, rebuilt roots and wrappers inherit the corresponding
input location. A present field payload retains its JSON value location.
Missing Option fields use the containing input Dict location because no source
token exists. Type metadata is itself rich data, so codec diagnostics can use
the input RichValue location as the primary label and the relevant Type
metadata edge location as the rule-side secondary label.

This RFC carries one optional location, not a provenance DAG. Multi-parent
derived histories and user-visible source reflection remain deferred.

## Instruction origins and value locations

Bytecode debug origins remain separate. They identify the XL expression whose
instruction failed. RichValue locations identify the data that reached the
instruction. Runtime errors may therefore render both:

```text
primary: invalid data value location
secondary: codec/type/call expression location
```

Adding RichValue does not remove opcode origins, call traces, or rule-side
locations.

## JSON provenance migration

JSON lowering initially continues producing the existing path-addressable
`Provenance` table for public compatibility. Module loading additionally
imports JSON through a located path that places each provenance location on the
corresponding RichValue edge. Codec and validation runtime paths consume inline
locations and no longer require the loader side table to find the failing
value.

Once all public consumers can use rich values, the redundant loader-side table
may be deprecated in a later RFC. It is not removed here.

## Diagnostics

Runtime errors gain an optional data location. Rendering prefers the data
location as the primary label and retains the bytecode/debug origin as a
secondary label when the two differ. Errors without a rich input preserve
their current instruction-origin behavior.

Codec `Err` remains an ordinary tagged value with its deterministic path and
String payload for this RFC. `result.unwrap` has access to the Err RichValue
location and upgrades it into a located runtime diagnostic. A structured
diagnostic Result payload is still deferred.

## Rejected alternatives

### Store Location inside each RuntimeValue variant

It duplicates fields across variants, complicates matching, and mixes payload
identity with observational metadata. A uniform wrapper preserves a compact
payload and one propagation boundary.

### Keep only the JSON provenance side table

Paths cease to identify a value after arbitrary XL transformations. Every
consumer would need operation-specific path remapping.

### Store TraceId instead of Loc

A trace arena can represent derived DAGs and reduce edge size, but adds arena
lifetime, relocation, and synchronization complexity. A three-word inline Loc
solves the current diagnostic problem directly. Trace IDs remain a possible
future representation behind the RichValue boundary.

### Attach only container-root locations

It cannot identify a nested scalar such as `$.users[4].name`. Locations belong
on collection edges as well as roots.

## Deferred work

- provenance DAGs and multi-parent derived histories;
- public source-reflection functions and `dbg!`;
- removal of the compatibility JSON Provenance side table;
- serialization or FFI ABI for RichValue layouts;
- cross-Engine rich-value transfer with source-database remapping;
- structured diagnostic values inside Result;
- policy annotations for choosing source versus derived rule locations.

## Implementation plan

1. Make SourceId 1-based and non-zero; flatten Location into Loc and add
   checked range/slice APIs plus layout tests.
2. Introduce Copy RichValue and migrate VM registers, heap edges, closures,
   up-links, bytecode links, persistent roots, and promotion.
3. Preserve legacy Value import/export as explicit location-dropping edges.
4. Attach instruction source origins to newly produced values according to the
   propagation table.
5. Import JSON provenance onto every runtime edge while retaining the public
   side table.
6. Teach validation, codecs, Result unwrap, and runtime diagnostics to prefer
   inline data locations and retain rule origins.
7. Add layout, propagation, JSON nesting, collection, closure, promotion,
   codec, and double-label diagnostic tests.

## Acceptance criteria

1. SourceId is 1-based NonZeroU32 and database lookup remains correct.
2. Loc construction is checked, range conversion is convenient, and source
   slicing cannot panic on invalid boundaries.
3. Option<Loc> has no size overhead over Loc on supported CI targets, and
   RichValue remains Copy.
4. Every VM register and heap value edge uses RichValue; no mixed raw-value
   path silently drops locations.
5. XL equality, function identity, interning, JSON, and debug output ignore
   locations unless producing a diagnostic.
6. Moves, captures, accessors, native identity functions, calls, promotion,
   and collection operations obey the propagation rules.
7. Nested JSON scalars retain exact CST locations through module loading and
   codec normalization.
8. A codec/type failure can render the data location as primary and the XL
   rule location as secondary.
9. Legacy Value APIs remain compatible and explicitly use unknown locations.
10. Existing language, quota, module, snapshot-boundary, and CLI tests remain
    behaviorally compatible.

## Implementation result

Implemented with `SourceId` as a one-based `NonZeroU32`, flat three-word
`Loc`, and a `Copy` `RichValue` wrapper. On the supported 64-bit target,
`Option<Loc>` remains 12 bytes and `RichValue` is 32 bytes. VM registers,
native windows, collection edges, closures, up-links, linked bytecode values,
and persistent roots all carry the wrapper. Promotion relocates payloads while
preserving locations; legacy `Value` import and export remain explicit
location-adding and location-dropping boundaries.

JSON module loading now imports the existing path provenance onto every rich
runtime edge before publishing the root. The compatibility side table remains
available to callers. Codec normalization preserves locations through rebuilt
collections and reports a nested mismatch at the exact offending JSON value.
`result.unwrap` promotes the located `Err` payload into a runtime diagnostic;
rendering uses the data location as primary and the opcode origin as secondary.

Opcode/debug origins remain separate from data provenance. The current codec
secondary label identifies the codec or unwrap operation expression rather
than the precise field inside the schema metadata. Carrying rule-edge
locations through decoded Type metadata is deferred with structured diagnostic
Result payloads.
