# RFC 0144: Explicit module exports

- Status: Implemented
- Depends on: RFC 0057, RFC 0140
- Child RFCs: RFC 0145 through RFC 0147

## Summary

Forma replaces implicit final-value exports with explicit named exports.
Modules declare exports throughout their top-level body:

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

`@main` uses the same explicit export model. Its only module-level distinction
is that no other module can import it. A host command selects a named export
according to its running mode instead of receiving one universal final value.

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

## Entry module and host modes

Every module, including `@main`, uses explicit named exports and evaluates to a
synthesized export record. `@main` is special only in resolution: it is the
host-selected entry and cannot be imported by another module.

Host commands define which export they require. For example:

```forma
# data evaluation entry
let result = build_data();
export { result as output };
```

```forma
# executable-plan entry
export def exec = fn(settings, request) {
    make_exec(settings, request)
};
```

`forma run` can select `output`, while `forma exec` can select and invoke
`exec` under its established entry contract. A main module may expose both, so
one source unit can support more than one host mode. Commands such as `check`
and semantic inspection need not require either export.

These names and contracts belong to host-command protocols, not to module
evaluation. The language has no default export and does not treat one public
name as universally privileged.

## Transition

The implementation may temporarily accept two exclusive forms for modules:

1. legacy mode: no export declarations and one final expression, interpreted
   as an export record for an imported module or as the host result for
   `@main`;
2. explicit mode: one or more export declarations and no final expression.

A module cannot mix explicit exports with a final result. During the transition,
the current host behavior may continue to consume a legacy `@main` result;
after migration each host mode selects its named export. This prevents two
possible interfaces from disagreeing within one module.

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
- importing `@main`;
- a universal or default entry export.

Re-exporting an imported binding through `export { name }` is sufficient for
the initial model. Broader re-export syntax can be justified independently.

## Child RFC sequence

RFC 0145 defines export syntax, AST data, name binding, aliases, duplicate and
placement diagnostics, decorator interaction, and the exclusive transitional
module modes.

RFC 0146 synthesizes runtime export records and `ModuleInterface` entries,
preserves persistent roots and generic schemes, and aligns strict execution,
recovery, semantic indexing, LSP queries, and host selection of named `@main`
exports.

RFC 0147 migrates built-in and repository-owned modules and entry points,
removes legacy final-expression module results, updates current documentation,
and records the final implementation result here.

## Shared acceptance criteria

1. A module may export at multiple top-level source locations.
2. Binding exports and export lists preserve local runtime identity and exact
   static schemes.
3. Public aliases do not rename or duplicate their local bindings.
4. Duplicate, unknown, nested, and mixed-mode exports have precise diagnostics.
5. Private bindings remain absent from the runtime record and module interface.
6. Selective and open imports observe only the explicit export table.
7. `@main` cannot be imported and uses the same explicit export table as every
   other module.
8. Host modes select documented named exports and report a missing or invalid
   entry contract precisely.
9. Modules no longer require an authored final export `Dict` after migration.
10. Strict loading, recoverable analysis, semantic navigation, completion, and
   hover agree on each exported symbol.

## Implementation result

Implemented through RFCs 0145-0147 in August 2026. Forma modules now publish a
synthesized immutable record containing only their explicit named exports.
Selective and open imports consume that interface, and `@main` follows the same
model while remaining non-importable.

The transition is complete: production module loading and recovery no longer
accept an authored final result. `forma run`, `forma exec`, and `forma build`
select `output`, `exec`, and `build` respectively. Arbitrary final-expression
evaluation survives only in the separately named low-level expression harness
used by compiler, VM, and type-system tests.
