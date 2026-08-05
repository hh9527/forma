# RFC 0090: Public `TypeDesc` graph

- Status: Proposed
- Depends on: RFC 0089
- Tracking issue: https://github.com/hh9527/forma/issues/4

## Summary

Forma exposes its canonical runtime type metadata as a finite, safely
observable graph. `TypeDesc` is the erased descriptor type and is represented
by the existing `Type` metatype; `TypeOf(A)` remains the precise subtype-like
witness accepted wherever `Type` is expected.

Recursive metadata edges are observed explicitly as `'Ref` nodes and resolved
through a trusted operation. VM `UpLink` handles never become ordinary public
data. Function descriptors are leaves in this first data-interpreter view.

The initial API is provided by `@bim/std/type-desc`:

```forma
def TypeDesc: Type = Type;

native kind: Fn(TypeDesc) -> TypeDescKind;
native resolve: Fn(TypeDesc) -> Result(TypeDesc, BlameError);
```

`TypeDescKind` includes the public canonical kinds plus `'Ref`. `resolve`
succeeds only for `'Ref` and returns its initialized target descriptor.

## Representation

`TypeDesc` is not a new heap object and does not duplicate TypeMetadata. The
existing canonical metadata value remains authoritative. The distinction is
semantic:

```text
TypeOf(A)  precise witness connecting metadata to static A
TypeDesc   erased Type view used by an interpreter
```

Ordinary Forma code can construct `Type` metadata only through existing
validated type constructors and declarations. No operation converts an
arbitrary `TypeDesc` into `TypeOf(A)`.

Internally recursive declarations use VM up-links while metadata is assembled.
The observer maps an initialized up-link to `'Ref`; it does not expose the
handle, storage generation, or target address. Reference identity is therefore
not global and is not part of this RFC.

## Public kinds

The first view distinguishes:

```text
Any Never Type TypeOf Int Float String Bytes Atom
Array Dict Tagged Tuple Struct Enum Union Function
WithAttributes Bound Ref
```

Kind inspection does not implicitly strip `WithAttributes` or resolve `Ref`.
An interpreter chooses when to normalize attributes and when to follow a
recursive edge.

`Function` is a terminal public kind. Existing compiler and codec internals
retain parameter and result metadata, but this module does not expose those
edges. Function signature reflection can be proposed independently if a real
non-data interpreter needs it.

## Error behavior

`kind` validates that an ordinary value is canonical TypeMetadata. For a hidden
initialized recursive link it returns `'Ref`. Malformed metadata and
uninitialized links fail as native contract errors because they cannot be
created by a well-typed completed Forma program.

`resolve` returns `Err(BlameError)` when called on a non-reference descriptor.
An initialized reference returns its logical target. It never returns the raw
link or allows mutation.

## Goals

1. reuse canonical TypeMetadata rather than introduce a duplicate graph;
2. distinguish precise `TypeOf(A)` witnesses from erased descriptor use;
3. make recursive edges explicit to user interpreters;
4. keep reference representation and identity private;
5. make Function a deliberate leaf; and
6. preserve deterministic, finite metadata observation.

## Non-goals

- globally stable type or reference identity;
- user construction or mutation of references;
- automatic recursive expansion;
- Function parameter or result reflection;
- value observation, `Dyn`, or typed interpreter lifting;
- metadata-to-static-type conversion; or
- a second runtime TypeMetadata representation.

## Acceptance criteria

1. `TypeOf(Int)` is accepted by `kind` and reports `'Int`;
2. attributed metadata reports `'WithAttributes` without implicit stripping;
3. recursive data metadata exposes a finite `'Ref` edge;
4. `resolve` follows that edge to a canonical descriptor;
5. `resolve(Int)` returns a structured error;
6. Function reports `'Function` and has no child traversal API;
7. raw up-links cannot be printed, serialized, compared, or constructed through
   the public module; and
8. existing codec and type-construction behavior does not regress.

## Implementation plan

1. add `@bim/std/type-desc` with the erased alias and kind declarations;
2. add a dedicated native operation family for graph observation;
3. recognize hidden up-links before ordinary TypeMetadata decoding;
4. map canonical metadata kind fields to `TypeDescKind`;
5. resolve initialized links without exporting handles;
6. add primitive, attributed, recursive, malformed, and Function tests; and
7. run the full quality gate and record the implementation result.
