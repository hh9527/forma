# RFC 0145: Explicit export surface

- Status: Implemented
- Depends on: RFC 0144

## Summary

Forma adds top-level export declarations:

```forma
export let value = make_value();
export def transform = fn(value) { value };
export @struct type User = {
    name: String,
};

let internal = 1;
export { internal };
export { transform as map };
```

Exports may appear more than once. They form one ordered public-name table and
do not affect the identity or lexical name of the referenced binding.

## Grammar

```text
export_statement :=
    "export" exportable_binding
  | "export" export_items ";"

exportable_binding :=
    let_binding
  | def_binding
  | type_binding

export_items :=
    "{" export_item ("," export_item)* [","] "}"

export_item := Identifier ["as" Identifier]
```

`export` precedes the complete binding, including decorators:

```forma
export @struct type User = { name: String };
```

This keeps `export` visibly responsible for the module boundary while existing
decorators continue to apply only to the type binding.

`decl`, `native`, `native type`, and `import` are not directly exportable
binding forms. Their completed values can be exported by a later export list.

## Binding and export data

An exported binding lowers to its existing ordinary binding plus one export
entry. An export list lowers only to export entries:

```text
Export {
    local: Identifier,
    public: Identifier,
    location: Location,
}
```

The ordinary binding participates in name resolution, inference, evaluation,
and recursion exactly as before. Export entries do not create lexical bindings
and are ignored by expression compilation.

The local identifier in an export list must already be visible at that source
position. An exported binding is visible to its generated export entry, so
`export def f = ...;` is valid. Forward export lists are rejected.

## Placement and conflicts

Exports are allowed only in a module's outermost body. An export parsed inside
a function, block, branch, or match expression receives a dedicated placement
diagnostic and contributes no public entry.

Every public name is unique across the module. These are errors:

```forma
export let value = 1;
export { value };

let left = 1;
let right = 2;
export { left as item };
export { right as item };
```

The second declaration is primary and the first public-name location is a
secondary diagnostic. Re-exporting one local binding under distinct public
names is allowed.

## Transitional module modes

The parser accepts an optional final expression at module scope. Semantic
validation assigns one exclusive mode:

- legacy: no export entries and one authored final expression;
- explicit: one or more export entries and no authored final expression.

No exports plus no final expression is an empty-module diagnostic. Explicit
exports plus a final expression is a mixed-mode diagnostic. Blocks and function
bodies still require their result expressions.

This RFC records explicit export data but does not yet change runtime module
results. RFC 0146 consumes the table and defines host selection.

## Acceptance criteria

1. Exported `let`, `def`, decorated `type`, lists, aliases, trailing commas, and
   multiple statements parse losslessly.
2. Exported bindings retain their existing binding kinds and source locations.
3. Export entries create no lexical definitions or runtime instructions.
4. Export lists reject unknown and forward local names.
5. Duplicate public names identify both declarations.
6. Nested exports have a dedicated placement diagnostic.
7. Legacy and explicit module modes are exclusive and blocks still require a
   result expression.
8. Recovery retains valid bindings and export entries around malformed siblings.

## Implementation result

The lexer, lossless CST, typed syntax views, and AST lowering now recognize the
complete export family. Exported bindings lower to their unchanged ordinary
binding plus an interface-only marker; export lists lower only to markers.
HIR, type-expression traversal, and bytecode compilation explicitly skip those
markers as lexical definitions and executable work.

Module lowering accepts no final expression when exports exist and records
whether a result was authored. It rejects mixed mode, duplicate public names,
and nested exports. Type analysis validates local references in source order
after resolved external and open-provider names are available, retaining the
ability to re-export an imported binding without permitting a forward local
export.

Tests cover lossless decorated and aliased forms, multiple export locations,
marker lowering, optional module results, and duplicate, mixed, and nested
diagnostics. The complete core suite continues to require results in ordinary
blocks.
