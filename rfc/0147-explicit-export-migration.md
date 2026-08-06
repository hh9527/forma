# RFC 0147: Explicit export migration

- Status: Proposed
- Depends on: RFC 0144, RFC 0145, RFC 0146

## Summary

Forma migrates repository-owned modules and host entries to explicit exports,
then removes legacy final-expression results from resolver-visible modules.

Built-in modules, ordinary source dependencies, and `@main` all require at
least one explicit export. Host commands select their named protocol entry as
specified by RFC 0146.

## Production module rule

A source loaded through `Engine`, `ModuleResolver`, workspace recovery, or an
import edge is a module and must contain explicit exports. These are rejected:

```forma
let value = 1;
{ value }
```

```forma
compute_output()
```

They become:

```forma
export let value = 1;
```

```forma
let value = compute_output();
export { value as output };
```

No implicit field name or default export is inferred from a final expression.

## Expression harness

Compiler, VM, and type-system tests still need to evaluate arbitrary expressions
without manufacturing a host protocol. A separately named low-level expression
harness may accept bindings followed by one final expression.

That harness:

- does not participate in module resolution;
- cannot be imported;
- publishes no `ModuleInterface`;
- is not used by CLI, LSP, workspace, or production `Engine` paths;
- is documented as an implementation/testing API rather than Forma module
  syntax.

This boundary prevents test convenience from silently preserving the removed
module convention.

## Migration

Repository-owned migration includes:

- every embedded `core/` and `std/` source;
- executable examples and codec examples;
- README and VISION examples describing current source;
- CLI integration fixtures and host-mode entry points;
- semantic and LSP fixtures intended to model modules.

Historical RFCs and discussion records are not mechanically rewritten.
Focused parser, compiler, and VM snippets may remain expression-harness inputs.

Built-in modules replace their final record with one or more export lists. A
native or split declaration is exported only after its completed binding is
available. Public field names and runtime identity must remain unchanged.

## Diagnostics and recovery

Strict production loading reports `module requires at least one explicit
export` at the module body. An authored final expression additionally reports
that module results were removed and suggests a named export.

Recovery retains bindings and diagnostics but marks a legacy module unavailable
to importers. Independent modules and facts continue normally.

Host commands diagnose missing `output`, `exec`, or `build` only after the
module satisfies the explicit-export rule.

## Acceptance criteria

1. Every embedded built-in module uses explicit exports with an unchanged
   public record and interface.
2. Repository examples and current README/VISION source use explicit exports.
3. Production root and imported Forma modules reject legacy final expressions.
4. Workspace recovery marks legacy modules unavailable with one primary cause.
5. `run`, `exec`, and `build` use only named explicit entries.
6. The expression harness is isolated from resolver and production Engine
   paths and is named accordingly.
7. Historical RFC and discussion text remains unchanged.
8. RFC 0144 records the completed child sequence and final implementation
   result.

