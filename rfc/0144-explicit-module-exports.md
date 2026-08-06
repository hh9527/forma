# RFC 0144: Explicit module exports

- Status: Proposed
- Depends on: RFC 0057, RFC 0140
- Child RFCs: RFC 0145 through RFC 0147

## Summary

Forma separates a module's public interface from its final evaluation result.
Importable modules declare exports explicitly and may do so throughout their
top-level body:

```forma
let internal = load_default();
let public_value = normalize(internal);

export def transform = fn(value) {
    normalize(value)
};

export type Config = struct('None, {
    name: String,
});

export { public_value };
```

The runtime representation remains one immutable export `Dict`. The compiler
synthesizes that record from export declarations instead of inferring the
module interface from an authored final `Dict` expression.

`@main` remains an executable entry module whose final expression is delivered
to the host. It does not have an importable public interface and cannot contain
exports.

## Motivation

The current convention uses the final value for two jobs:

```forma
let private_helper = ...;
def public_fn = ...;
type PublicType = ...;

{
    public_fn,
    PublicType,
}
```

That representation is uniform but makes the public interface an incidental
property of one expression. It also requires a distant list to be kept in sync
with definitions and makes source-local diagnostics, navigation, documentation,
and compatibility analysis less direct.

Explicit exports preserve the closed-world evaluation model while making the
lightweight connection between modules visible in source.

## Surface model

The export family contains two forms:

```forma
export let value = expression;
export def function = expression;
export type Model = expression;

export { existing };
export { existing as public_name };
export { first, second as renamed };
```

An exported binding both declares its ordinary top-level binding and adds that
binding to the module export table. An export list refers to bindings already
visible at that source position. Forma does not introduce forward export lists.

Export lists use `as` for the same source-to-local naming relation as imports.
The exported name is the public name; the referenced binding remains unchanged.
Exporting a value never wraps, copies, or reevaluates it.

The first child RFC decides the exact interaction between `export` and type
decorators. Native and declaration slots need no dedicated export modifier:
system modules and split declarations can export the completed binding with an
export list.

## Export table

Exports are collected in source order for diagnostics and semantic queries.
Their runtime record uses the ordinary canonical `Dict` field order.

Every public name must be unique across all export declarations. Repeating an
export is an error even when both entries refer to the same binding. Private
and public names occupy different roles, so this is valid:

```forma
let implementation = ...;
export { implementation as run };
```

Each export retains:

- its public name and source location;
- the local binding identity and runtime value;
- the binding's exact `TypeScheme`;
- type metadata and persistent root information needed across module worlds.

`export type T` therefore publishes the existing runtime `TypeOf(T)` metadata
value and its static witness. It does not create a separate type-only namespace.

Open and selective imports consume this explicit table. Imported, including
open-imported, names are not re-exported unless an export declaration names
them explicitly.

## Module kinds

The resolver determines whether a source is the entry module or an importable
module:

- `@main` requires a final expression and rejects every export declaration;
- an importable module uses explicit exports and has no authored final result;
- evaluation of an importable module produces the synthesized export record.

This keeps the language expression-oriented at its host boundary without
making a library interface pretend to be an application result.

## Transition

The implementation may temporarily accept two exclusive forms for importable
modules:

1. legacy mode: no export declarations and one final export-record expression;
2. explicit mode: one or more export declarations and no final expression.

A module cannot mix explicit exports with a final result. This makes the mode
visible and prevents disagreement between two possible interfaces.

After built-in modules, examples, tests, and current documentation migrate,
RFC 0147 removes legacy mode. Historical RFC text is not rewritten.

## Non-goals

This RFC does not introduce:

- default exports;
- wildcard re-exports;
- an `export ... from` shortcut;
- mutable or live export bindings;
- dynamic export names;
- separate value and type namespaces;
- importing `@main`.

Re-exporting an imported binding through `export { name }` is sufficient for
the initial model. Broader re-export syntax can be justified independently.

## Child RFC sequence

RFC 0145 defines export syntax, AST data, name binding, aliases, duplicate and
placement diagnostics, decorator interaction, and the exclusive transitional
module modes.

RFC 0146 synthesizes runtime export records and `ModuleInterface` entries,
preserves persistent roots and generic schemes, and aligns strict execution,
recovery, semantic indexing, and LSP queries.

RFC 0147 migrates built-in and repository-owned modules, removes legacy final
record exports from importable modules, updates current documentation, and
records the final implementation result here.

## Shared acceptance criteria

1. A module may export at multiple top-level source locations.
2. Binding exports and export lists preserve local runtime identity and exact
   static schemes.
3. Public aliases do not rename or duplicate their local bindings.
4. Duplicate, unknown, nested, and mixed-mode exports have precise diagnostics.
5. Private bindings remain absent from the runtime record and module interface.
6. Selective and open imports observe only the explicit export table.
7. `@main` retains its final host result and cannot be imported or exported.
8. Importable modules no longer require an authored final export `Dict` after
   migration.
9. Strict loading, recoverable analysis, semantic navigation, completion, and
   hover agree on each exported symbol.

