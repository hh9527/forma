# RFC 0024: Declarative native bindings

- Status: Accepted for implementation
- Depends on: RFC 0006, RFC 0009, RFC 0013, RFC 0022, RFC 0023

## Summary

XL modules may declare host-provided functions as ordinary named bindings:

```xl
native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any);
```

The declaration, contract, stable binding identity, symbol name, and source
location belong to XL syntax and analysis. A host registry supplies only the
runtime implementation. Once linked, XL observes an ordinary `Func` and calls
it through the existing function ABI.

## Motivation

Core modules are currently complete Dict values manufactured by Rust. The
analyzer sees their resulting values but has no source declaration describing
which bindings are native, where their contracts came from, or which symbol a
future HIR/type evaluator should inspect. Tooling must not infer this interface
from Rust constructors.

Declarative native bindings make the host boundary explicit data before HIR is
datafied. They also separate two lifecycles:

- `decl`/`def` creates an XL-initialized single-assignment UpLink;
- `native` creates a host link that must be resolved before module evaluation.

## Syntax

```text
native_binding := "native" Identifier ":" contract ";"
```

Native bindings are allowed only at module top level. An explicit contract is
required in this RFC so core interfaces never silently degrade to `Any`.

The declaration does not automatically export the binding. A module returns
the desired interface normally:

```xl
native length: fn(Array(Any)) -> Int;
native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any);

{ length, map }
```

## Binding and analysis

`native` is an independent binding kind. It introduces an immutable name at
the declaration point, cannot be shadowed or paired with `def`, and retains the
contract expression and declaration location. Duplicate bindings use the same
diagnostic rules as other module bindings.

The parser and semantic AST retain enough information for a later datafied HIR
binding table to represent:

```text
binding identity
name
definition = Native(module identity, symbol name)
contract expression
source location
```

This RFC does not yet expose the table as XL runtime data. It establishes an
authoritative declaration rather than requiring later analysis to reconstruct
one from a native closure value.

## Linking

Compilation emits the same external-value link used for imported stable roots.
The module loader resolves every native declaration through a registry keyed by
module identity and symbol name. A missing symbol is a module-link error whose
primary label is the native declaration.

The registry entry contains a `NativeFunction`. Arity must agree with the
declared function contract when the contract has statically known arity.
Contract evaluation and ordinary call-site checking continue to use the
existing metadata system.

No `CallNative` opcode is added. The linked value is an ordinary native closure
and all calls retain the existing register ABI, quota charging, debug origins,
and tail-call behavior.

## Core modules

Core module interfaces move into embedded XL declaration sources. Rust keeps a
registry of implementations, not a separately hand-shaped exported Dict. The
declaration source is parsed and analyzed through the normal frontend so tools
can observe exactly the same contract data as compilation.

The first implementation may embed these sources in the binary. Filesystem
packaging and precompiled core artifacts are deferred.

## Diagnostics

The implementation diagnoses:

- native outside module top level;
- missing or malformed contract;
- duplicate or conflicting binding;
- missing registry symbol;
- known arity disagreement between declaration and implementation.

Link failures identify the declaration source and symbol. Runtime failures
inside a correctly linked function retain existing call-expression origins.

## Rejected alternatives

### `decl native map`

This incorrectly suggests an XL-visible uninitialized cell later filled by
`def`. Native resolution is a distinct link-time lifecycle.

### Rust-built core module Dicts as analysis input

They expose runtime values but erase declaration identity and source contracts,
forcing analysis to guess which native closure represents which interface.

### A new native-call opcode

It would make native functions observably different and duplicate the stable
function ABI established by RFC 0009.

## Deferred work

- datafied HIR and binding tables;
- native abstract/type semantics associated with declarations;
- optional contracts defaulting to `Any`;
- user-supplied native extensions and package manifests;
- filesystem or snapshot packaging of core declaration modules.

## Implementation plan

1. Add the `native` token, CST node, AST binding, and recovery tests.
2. Retain native contracts and locations in binding analysis.
3. Compile native names as external stable-value links without UpLinks.
4. Add a module-scoped native registry with missing-symbol and arity checks.
5. Express core module interfaces as embedded XL declaration sources.
6. Verify ordinary calls, contracts, diagnostics, quotas, and existing modules.

## Acceptance criteria

1. `native name: contract;` is a lossless, located, top-level binding.
2. Analysis can distinguish Native from Let, Decl/Def, Import, and Type.
3. Native contracts participate in ordinary checking and remain queryable.
4. Linking resolves a module symbol to an ordinary `Func` stable root.
5. Missing symbols and known arity mismatches point to the declaration.
6. Core module shape and behavior come from XL declaration sources.
7. No new call opcode or runtime-visible native reference kind is introduced.
8. Existing source locations, quotas, recursive definitions, and CLI behavior
   remain compatible.
