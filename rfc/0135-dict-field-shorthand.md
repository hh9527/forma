# RFC 0135: Dict field shorthand

- Status: Implemented
- Depends on: RFC 0002, RFC 0133

## Summary

Dict literals accept an identifier without a colon as shorthand for a field
whose key and value use that identifier:

```forma
let name = "forma";
let version = 1;
{ name, version }
```

is equivalent to:

```forma
{ name: name, version: version }
```

Shorthand fields may be mixed with explicit fields and spread entries:

```forma
{ name, enabled: 'True, ...extra }
```

They participate in the existing left-to-right evaluation and duplicate-field
rules exactly as their expanded form does.

## Scope

Only an unadorned identifier in a Dict expression is shorthand. String keys,
field access, calls, and other expressions still require an explicit key and
colon. A decorator also requires an explicit field because it transforms the
field value and needs an authored value expression.

This RFC does not add shorthand to patterns, imports, parameters, type fields,
or any other binding form. Those constructs have different binding and
exhaustiveness semantics and should be considered independently.

## Lowering

The lossless CST retains the authored shorthand. AST construction lowers it to
the existing named `DictField` representation, using the identifier location
for both the key and the synthesized variable expression. HIR indexing, type
inference, compilation, evaluation order, duplicate detection, source
provenance, and diagnostics then follow the ordinary explicit-field path.

No runtime representation or bytecode change is required.

## Braced-expression disambiguation

`{ name }` is parsed as a Dict literal. Blocks in syntax-directed positions,
including function, `if`, `match`, and `let else` bodies, remain blocks. This
resolves the otherwise unavoidable ambiguity in favor of the new data form;
an unconstrained braced expression containing only an identifier no longer
acts as a redundant standalone block.

## Acceptance criteria

- `{ name }` produces the same value and inferred type as `{ name: name }`.
- Multiple shorthand fields and mixtures with explicit fields work.
- Shorthand fields compose with Dict spread under the RFC 0133 ordering rules.
- Duplicate names are diagnosed consistently across shorthand and explicit
  fields.
- An unresolved shorthand identifier reports the normal unknown-binding
  diagnostic at the authored identifier.
- Decorated shorthand and non-identifier bare entries remain syntax errors.
- Built-in Forma modules use shorthand for their export records.

## Implementation result

Implemented in the lossless grammar and AST lowering. A one-token lookahead
distinguishes shorthand from explicit fields, while braced-expression
selection recognizes identifier-only Dicts. The synthesized variable retains
the authored identifier location and all later stages reuse the existing named
field path.

The embedded standard modules now use shorthand for value and Type metadata
exports. Tests cover lossless CST reconstruction, inferred and evaluated
values, mixtures with explicit fields and spread, duplicate and unresolved
names, and rejection of decorated or non-identifier shorthand entries.
