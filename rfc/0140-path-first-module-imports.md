# RFC 0140: Path-first module imports

- Status: Proposed
- Depends on: RFC 0057, RFC 0139
- Child RFCs: RFC 0141 through RFC 0143

## Summary

Forma replaces name-first imports with one path-first construct:

```forma
import "std/array" as array;
import "std/array" { map, filter as select };
import "core/prelude" *;
import "std/array" as array, { map };
import "std/array" as array, *;
```

The module request is always authored first. This gives parser recovery and LSP
completion the module identity before a user selects a module binding or any
exports.

The old form is removed without a compatibility grammar:

```forma
import array from "std/array";
```

## Import model

An import is a module dependency edge with two orthogonal selectors:

```text
Import {
    target: ModuleRequest,
    module_binding: Option<Identifier>,
    selector: None | Open | Items(Vec<ImportItem>),
}

ImportItem {
    exported: Identifier,
    local: Identifier,
}
```

`as` always maps a source entity to its local name. `:` remains reserved for
type annotations and structural field-to-value or field-to-pattern mappings.

A bare `import "target";` is invalid. Every import must bind the module value,
select exports, open lookup, or combine a module binding with one selector.

## Module binding

`as name` binds the complete module export record. It preserves the current
qualified-access behavior:

```forma
import "std/array" as array;
array.map(values, transform)
```

## Selective import

`{ item, source as local }` creates explicit local bindings backed by the
target module's runtime values and `ModuleInterface` schemes. Missing exports,
duplicate local names, and conflicts with other explicit bindings are reported
at the import item.

Selective imports do not automatically re-export their members.

## Open import

`*` does not create flattened local values. It adds a source-preserving lookup
provider. Local and explicit bindings resolve normally. If an otherwise
unresolved name is exported by exactly one open provider, it resolves there.
If two providers export it, the authored use is ambiguous and reports every
candidate module.

Unused collisions between open providers are accepted so a dependency adding
an unrelated export does not break its users.

Open imports still create ordinary dependency, initialization, cache, cycle,
and semantic-index edges. They do not re-export names.

## Default prelude

After open imports are implemented, the default prelude is represented as a
synthetic open edge to `core/prelude`. That module is not given its own
synthetic edge during bootstrap. Runtime identity and interface projection
remain those established by RFC 0139, but use the general lookup mechanism.

## Child RFC sequence

RFC 0141 introduces path-first module bindings and mechanically migrates all
current source, tests, examples, and current documentation.

RFC 0142 introduces selective items, aliases, interface validation, runtime
projection, and LSP navigation/completion facts.

RFC 0143 introduces open providers, use-site ambiguity, combined forms, and
the synthetic default-prelude edge.

## Shared acceptance criteria

1. Module requests precede selectors in every accepted import form.
2. The removed name-first grammar is rejected.
3. Module, selective, and open forms share one resolver dependency edge.
4. Selective imports preserve generic schemes and runtime closure identity.
5. Open lookup retains provider identity and diagnoses ambiguity only on use.
6. Explicit local bindings take precedence over open providers.
7. Strict execution, recovery, semantic queries, and LSP agree on resolution.
8. `core/prelude` uses the same open-provider model as authored imports.
