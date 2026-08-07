# RFC 0171: Entry module resolution and loading

- Status: Proposed
- Depends on: RFC 0057, RFC 0059, RFC 0170

## Summary

Forma will add the reserved `ModuleId::Entry`, rendered as `@entry`, and an
Engine API that loads an in-memory Forma source as the root entry for one real
main path:

```text
load_synthetic_entry(main_path, entry_source, external_bindings)
```

The resolver builds the ordinary crate/dependency graph from `main_path`, then
compiles `entry_source` as `@entry`. That root alone may import `@main`; no
ordinary module may import either reserved root identity.

## Resolution rules

For `resolve(requester, request)`:

1. `request == "@entry"` always fails;
2. `request == "@main"` succeeds only when `requester == @entry` and resolves
   to the real main Forma file;
3. any other `@entry` request is invalid;
4. direct requests from `@entry` may resolve registered built-ins,
   dependencies, and main-crate `@src` paths without private-boundary checks;
5. imports made by a module reached from `@entry` use that module's own ID and
   ordinary rules, so privilege is not transitive;
6. relative requests from synthetic `@entry` are invalid because it has no
   physical parent directory.

The existing root resolver still rejects a private/native physical file as
the user-selected main root. Synthetic entry construction is the only way to
obtain `ModuleId::Entry`.

## Loading model

The entry source is registered in the shared `SourceDatabase` under the stable
display name `@entry`. Its compilation uses the same parser, HIR, type checker,
compiler, quota account, module cache, and persistent Main world as file-backed
modules.

`@main` is loaded through the ordinary module cache when imported. The
resulting `LoadedModule` represents `@entry`; its dependency list includes the
real main path and all transitive physical dependencies. Entry source is not
written to disk and is not accepted through the normal `load_module` API.

External bindings are scoped to `@entry` only. They are an implementation
bridge for later entry-only modules and are not visible to `@main` or its
dependencies.

## Diagnostics

Parser and checker failures in generated source identify `@entry`. Import and
type diagnostics that originate in `@main` keep the real main source. This RFC
does not yet add cross-source refinement for a failed `ExecFn` assignment;
RFC 0173 owns the final user-facing adapter diagnostic.

## Non-goals

- runtime/options modules or exec behavior;
- transitive resolver privilege;
- user-authored entry roots;
- relative imports from synthetic entry source;
- persistent entry caching across invocations;
- general virtual filesystems or module capability tokens.

## Acceptance criteria

1. `ModuleId::Entry` displays exactly as `@entry`;
2. only `@entry` resolves the exact request `@main`;
3. every request for `@entry` is rejected;
4. `@entry` can directly resolve ordinary built-ins, dependencies, `@src`, and
   otherwise private modules;
5. imported modules do not inherit entry privilege;
6. relative entry imports fail deterministically;
7. an in-memory entry imports and reads exports from the real main module;
8. ordinary `Engine::load_module` behavior is unchanged;
9. entry parse/type/runtime failures retain useful source identities;
10. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add the reserved identity and resolver permission matrix;
2. represent the real main file as an importable resolved module only for
   entry requesters;
3. add a synthetic-root loader sharing the ordinary ModuleLoader;
4. cover direct/non-transitive permissions, loading, dependencies, and
   diagnostics;
5. record implementation evidence.

## Stopping rules

Work returns to discussion if implementation requires writing a temporary
source file, making `@main` generally importable, transitive privilege, or a
parallel compiler path for entry source.
