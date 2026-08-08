# RFC 0192: Recursive type metadata across module boundaries

- Status: Implemented
- Depends on: RFC 0090

## Summary

Allow a module that exports recursive `TypeDesc` metadata to retain an ordinary
typed module interface. Importers may bind the module, selectively import its
types and functions, use imported recursive types in annotations, and observe
the authoritative recursive descriptor through the existing public `'Ref` and
`type_desc.resolve` API.

This RFC completes the existing UpLink lifecycle. It does not introduce cyclic
ordinary values or a second TypeDesc representation.

## Existing gap

Recursive declarations are already constructed and published correctly:

```text
metadata initializer
    -> MakeUpLink
    -> construct the descriptor graph
    -> InitializeUpLink
    -> publish into the persistent main heap
```

RFC 0090 also exposes recursive edges safely: `type_desc.kind` maps an internal
initialized UpLink to `'Ref`, and `type_desc.resolve` returns its logical target.

The remaining failure occurs when the module loader additionally tries to turn
the complete module export into the legacy tree-shaped Rust `Value`. A recursive
descriptor makes the module record cyclic, so export fails. The loader retained
the correct persistent root but replaced the tool-stage value with `'None` and
marked the complete import opaque. Consequently module binding, selective
imports, imported annotations, and ordinary typed helper functions became
unavailable even though program-stage linking already had the real graph.

## Tool-stage projection

When a complete module record cannot cross the legacy boundary, the loader
constructs a finite tool-stage projection from its checked `ModuleInterface`:

- an export that can be exported independently keeps its ordinary value;
- an exported type whose root is cyclic uses the same conservative `Any`
  metadata placeholder used while a local recursive component is predeclared;
- another implementation value, including a closure that captures recursive
  metadata, retains its finite structure while each captured UpLink back-edge
  is projected to the same `Any` metadata placeholder;
- a genuinely recursive ordinary value is not mistaken for metadata and keeps
  the existing inert legacy placeholder behavior;
- every program-stage external constant still links the real persistent root.

The projection exists only at the tool-stage legacy boundary and is not exposed
as a second TypeDesc ABI. Tool-stage closures may execute with this finite
projection for analysis and best-effort diagnostics; program-stage execution
always links the real module root. The projection gives the analyzer the same
conservative precision already used while a local recursive component is
predeclared. After metadata publication, the authoritative `TypeGraph` and
runtime descriptor retain exact TypeId and UpLink identity. This RFC
deliberately does not claim that legacy `TypeDescriptor` schemes acquire
recursive static precision; callable contracts around recursive values retain
their existing conservative `Any` back-edges.

## Semantics

```text
module A tool stage
    finite interface projection

module A program stage
    authoritative persistent graph with initialized UpLinks

module B tool stage
    interface schemes + finite projection

module B program stage
    external links to A's authoritative persistent roots
```

`kind`, `children`, `resolve`, codecs, schema generation, and user-space
interpreters observe the authoritative graph. JSON and the Host legacy `Value`
boundary continue to reject internal UpLinks.

## Non-goals

- cyclic Array, Dict, Tagged, or user-constructed runtime values;
- serializing TypeDesc or raw UpLink handles;
- globally stable reference identity;
- increasing the current static precision of recursive back-edges;
- general lazy modules or module import cycles; or
- replacing the persistent heap publication model.

## Acceptance criteria

1. a module can export mutually recursive Struct/Enum metadata and helper
   functions in one explicit export record;
2. another module can bind that module and use an exported recursive type in an
   annotation;
3. selective and open imports work for the same module;
4. helper functions execute against finite values of the recursive type;
5. `type_desc.kind/children/resolve` still expose a finite public `'Ref` graph
   across the module boundary;
6. codec and JSON Schema behavior for imported recursive metadata remains
   unchanged;
7. ordinary JSON encoding still rejects the internal recursive link;
8. existing acyclic imports and the full workspace test suite do not regress.

## Implementation result

The module loader and recoverable workspace evaluator now publish a Telora
module result before projecting it to legacy `Value`. If full projection
detects an UpLink, they build a deterministic module-shaped tool value from the
checked interface and independently exportable field roots. Cyclic exported
types use an `Any` metadata placeholder. Closures and other composite exports
remain available by replacing only captured UpLink edges with that placeholder;
program bytecode continues to link the corresponding persistent field root.

The graph decoder now retains generic `Bound` nodes and native type constructors
validate metadata as a graph instead of recursively flattening it into a tree.
This prevents recursive imported metadata from overflowing the Rust stack while
preserving the public kinds specified by RFC 0090.

Regression coverage uses a mutually recursive `Expr`/`Binary` model with
constructors, a recursive evaluator, whole-module access, selective import, and
public TypeDesc traversal. The imported annotation compiles, evaluation uses
the real recursive metadata graph, raw JSON encoding remains rejected, and the
intelligent-reporting example now uses a genuinely recursive SQL `Expr` AST.
Its successful SQL lowering and four independent best-effort diagnostics both
remain covered by tests.
