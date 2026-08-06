# RFC 0146: Synthesized export records

- Status: Implemented
- Depends on: RFC 0144, RFC 0145

## Summary

Explicit export entries synthesize the immutable runtime record and static
`ModuleInterface` consumed by imports:

```forma
let implementation = fn(value) { value };
export { implementation as map };
```

is semantically lowered to the existing module result shape:

```forma
{
    map: implementation,
}
```

The synthesized expression is compiler-owned. It has no authored final-value
semantics and is not exposed as source syntax.

## Runtime and persistent values

Each public field reads the referenced local binding once at module completion.
Forma bindings are immutable, so the record retains the same runtime identity
as the local value. Aliases change only the record field name.

The existing compiler and module loader publish the synthesized record as one
persistent root. Field roots are therefore available to selective and open
imports without a second copy, wrapper, or evaluation path.

Private bindings may still be retained internally when required by exported
closures or type metadata, but they are absent from the record shape.

## Static interface

For every export entry, `ModuleInterface.exports` maps its public name to the
exact scheme of the referenced local binding. Inferred and declared generics
remain schemes and instantiate independently at import use sites.

An export whose local binding has no publishable scheme is a source-located
interface error. Public aliases preserve the scheme body and parameters.

Type exports use the same path. Their runtime field is the existing metadata
value, their persistent root preserves recursive links, and their interface
entry is the `TypeOf(T)` witness.

## Recovery and semantic queries

Recovery synthesizes an export record from every valid recovered export marker,
even when an unrelated sibling is malformed. Unknown or conflicting entries
remain unavailable facts and do not erase independent public entries.

Semantic indexing records each export's public and local locations. Definition,
references, hover, completion, and module-member completion use the public
table. Export markers do not appear as executable lexical definitions.

Strict execution and recovery must publish the same record shape whenever both
complete successfully.

## Host selection

Executing `@main` first produces its synthesized export record. Host commands
then select a protocol-specific field:

- `forma run` requires `output` and prints that value;
- `forma exec` requires `exec`, validates it against the established
  `Fn(ExecSettings, ExecRequest) -> ExecEnv` boundary, and invokes it;
- `forma build` requires `build`, invokes it with no arguments, and validates
  the existing output-plan boundary;
- `check`, `types`, `show`, and LSP operations require no host entry export.

A missing field reports the required public name and host mode. A present value
with the wrong callable or result shape uses the existing runtime and boundary
diagnostics. A main module may export any combination of these entries.

Legacy modules retain their current final-value host behavior only until RFC
0147 migrates repository entry points and removes the transition.

## Acceptance criteria

1. Explicit exports evaluate to one canonical `Dict` containing only public
   names.
2. Public aliases and qualified, selective, and open imports preserve runtime
   identity.
3. Exported generic definitions retain exact independently instantiated schemes.
4. Exported recursive type metadata retains persistent links.
5. Recovery publishes independent valid exports and source-located failures.
6. Semantic and LSP queries expose public names without treating markers as
   lexical definitions.
7. `run`, `exec`, and `build` select `output`, `exec`, and `build` respectively
   for explicit `@main` modules.
8. Missing and invalid host entries have mode-specific diagnostics.
9. Legacy final-value modules continue to operate during this RFC only.

## Implementation result

Strict and recovered parsing synthesize an internal `Dict` expression from
export markers. The existing inference, compiler, persistent publication, and
module-loading paths therefore produce one canonical export record without a
second runtime representation. Public aliases read the same local register and
field root, preserving function and metadata identity.

Interface publication retains existing generic schemes and now supplies a
resolved zero-parameter scheme for explicitly exported monomorphic bindings.
This fallback is deliberately limited to explicit exports so legacy module
inference remains unchanged during migration. Workspace export projection and
module completion consume the synthesized result shape.

`LoadedModule` records whether its source used explicit exports. The CLI selects
`output`, `exec`, or `build` only for that mode and preserves legacy final-value
behavior until RFC 0147. Tests cover private omission, aliases, generic and
monomorphic selective/open imports, recovery export projection, forward-export
failure, mode-specific missing entries, and explicit run, exec, and build
entry points.
