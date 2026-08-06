# RFC 0143: Open module imports

- Status: Implemented
- Depends on: RFC 0140, RFC 0141, RFC 0142

## Summary

Forma completes path-first imports with open lookup and combined selectors:

```forma
import "core/prelude" *;
import "std/array" as array, *;
import "std/array" as array, { map, filter as select };
```

`*` contributes a lookup provider. It does not copy every export into the
module's authored bindings, and it does not re-export those names.

## Syntax

An import has one required selector and an optional module binding:

```text
import_binding :=
    "import" string_literal import_selector ";"

import_selector :=
    "as" Identifier ["," ("*" | import_items)]
  | "*"
  | import_items
```

The following forms are accepted:

```forma
import "a/b" *;
import "a/b" as b;
import "a/b" { item, source as local };
import "a/b" as b, *;
import "a/b" as b, { item, source as local };
```

`import "a/b";` and `import "a/b" * as b;` remain invalid. `as b` binds the
module value; `*` only affects unqualified lookup.

## Lookup

For an unqualified name, resolution proceeds in this order:

1. a local or explicit imported binding;
2. a unique open provider exporting the name;
3. the ordinary unknown-binding diagnostic.

An explicit binding shadows every open provider with the same name. Two or
more open providers may export the same name without an error while that name
is unused. If the name is used and no explicit binding shadows it, resolution
fails at the use and lists all candidate module ids.

The implementation retains the provider module id, export scheme, runtime
value, persistent root where applicable, and import location until lookup is
resolved. This preserves generic schemes, runtime identity, provenance, and
semantic navigation.

## Dependency and export behavior

Every open or combined import creates one module dependency edge. Loading,
cycle detection, caching, authority checks, and invalidation therefore behave
the same as for module and selective imports.

Open names are inputs to the importing module only. They are absent from its
export record and interface unless the module defines an explicit exported
binding for them.

## Default prelude

The implicit default prelude is represented as a synthetic open provider for
`core/prelude`. It has the lowest authored priority because all explicit and
local bindings shadow open providers uniformly. `core/prelude` itself receives
no synthetic self-provider while it is bootstrapped.

The bootstrap projection used to construct the prelude remains an internal
implementation detail. Once `core/prelude` is available, strict execution,
recoverable analysis, and semantic queries consume it through the same open
lookup path as an authored `import "core/prelude" *;`.

## Diagnostics and recovery

Missing modules are diagnosed at the import path. Ambiguous open names are
diagnosed at each authored use, with secondary information identifying the
provider imports. Recovery leaves that name unavailable and continues with
unrelated bindings.

Strict execution, recoverable workspace analysis, and LSP queries must agree
on shadowing and ambiguity. Completion may present candidates from all open
providers, qualified by provider module id.

## Acceptance criteria

1. Open-only and both combined forms parse and execute.
2. Bare imports and `* as name` are rejected.
3. Explicit/local bindings shadow open providers.
4. One open provider supplies its value and exact exported type scheme.
5. Unused collisions are accepted; used collisions identify every provider.
6. Open imports do not re-export names.
7. Combined selectors create only one dependency edge per target.
8. The default prelude uses the open-provider lookup model without bootstrap
   self-reference.
9. Strict, recoverable, semantic, and LSP behavior is covered by tests.

## Implementation result

The parser lowers open selectors to dependency-only `OpenImport` AST edges;
combined forms additionally lower their explicit module or item bindings.
Strict and recoverable loaders retain candidates by export name and provider
module id, deduplicate repeated access to one provider, apply explicit-binding
precedence, and diagnose only referenced collisions.

Unique candidates enter the existing external-value and interface pipeline,
so runtime identity and exported generic schemes are preserved without adding
authored bindings or exports. `core/prelude` is seeded through the same
candidate path after bootstrap and is omitted while that module initializes.

The semantic workspace records open dependency edges and resolves their unique
names as external references. Tests cover combined selectors, runtime identity,
explicit shadowing, unused and used collisions, recovery diagnostics, and the
rejected bare and `* as name` forms.
