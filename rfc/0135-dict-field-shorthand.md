# RFC 0135: Dict field shorthand

- Status: Proposed
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
