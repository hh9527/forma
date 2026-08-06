# RFC 0142: Selective module imports

- Status: Proposed
- Depends on: RFC 0141

## Summary

Forma can bind selected module exports directly:

```forma
import "std/array" { map, filter as select };
```

Each item names an authored export and an optional local alias. `item` is
shorthand for `item as item`.

## Projection

A selective binding retains both names in the AST. Module loading resolves its
target once and projects three corresponding artifacts:

- the runtime export value;
- its persistent heap root;
- its exported `TypeScheme` from `ModuleInterface`.

The local binding therefore has the same generic behavior and opaque runtime
identity as qualified field access through the module record. It is not
implemented as an inferred alias or a copied native closure.

## Validation

Missing runtime fields or interface exports are import diagnostics at the
authored item. A value/interface mismatch is a module publication error.
Duplicate local aliases and conflicts with other explicit top-level bindings
are rejected under the existing single-assignment rules.

Static data modules currently publish no named `ModuleInterface` exports and
therefore cannot be selectively imported. They remain available through a
module binding.

## Scope

This RFC implements the selector without a simultaneous module binding. The
combined `as module, {...}` form follows in RFC 0143 after selectors and module
bindings can coexist in one parsed import edge.

Selective imports do not re-export their members.

## Acceptance criteria

1. `{ item }` binds one export under its own name.
2. `{ item as local }` binds it under `local` only.
3. Generic functions instantiate normally through selective bindings.
4. Function values retain identity with qualified module access.
5. Missing exports and duplicate local names have item-local diagnostics.
6. Resolver caching loads a target only once for multiple selected items.
7. Strict and recoverable analysis publish the selected schemes and semantic
   import targets.
