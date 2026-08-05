# RFC 0116: Host native module imports

- Status: Proposed
- Depends on: RFC 0059, RFC 0114, RFC 0115

## Summary

Forma modules may import a native module that the embedding Engine registered
before construction:

```forma
import secrets from "@host/acme/secrets";
```

The resolved ModuleId is the exact canonical logical name and has no physical
path. Resolution recognizes the namespace shape; authority comes only from the
Engine's frozen registry. An unregistered name is unavailable and cannot be
satisfied by a file, dependency, or manifest.

## Resolution

`@host/` requests follow these rules:

1. the request must contain a non-empty path after the prefix;
2. it resolves to `ModuleId::Builtin(exact_name)` with Forma format and no
   physical path;
3. strict loading looks up the exact name in the frozen native registry;
4. a miss reports `unknown Host native module` at the import site;
5. relative and `@src/` imports remain unavailable from native modules.

The existing `@bim/` behavior is unchanged. Host names cannot shadow core
names because registration rejects the core namespace.

## Workspace projection

Strict loading and recoverable synchronous/asynchronous workspace analysis use
the same frozen module values and interfaces. A successful import contributes
the exact ModuleId and its interface to semantic resolution and completion.
Unknown imports produce one sourced unavailable-module fact with
WorkspaceModuleKind::Host and do not block independent definitions.

Building another Engine from another builder may produce another registry;
snapshots never consult a global mutable registry. Existing LoadedModule and
WorkspaceSnapshot values remain tied to the Engine registry used during their
construction.

## Acceptance criteria

1. strict execution imports and calls a registered Host native Function;
2. imported native opaque values retain `(Host module ID, local slot)` identity;
3. exact registered exports participate in type checking and completion;
4. unknown Host imports receive a focused diagnostic at the import request;
5. recovery continues independent bindings after an unknown Host import;
6. sync and async recovery observe the same frozen registry;
7. two Engines do not leak Host registrations into one another;
8. files and manifests cannot satisfy an `@host/...` request;
9. no registry mutation or dynamic loading path is introduced; and
10. full workspace tests and strict Clippy pass.

## Implementation plan

1. recognize canonical `@host/...` module requests;
2. generalize strict native-module lookup and diagnostics;
3. project Host imports through recoverable workspace analysis;
4. add execution, opaque identity, completion, unknown, async, and isolation
   tests;
5. mark RFC 0114 Implemented and record final evidence.

## Non-goals

- imports between native declaration sources;
- Host module relative paths or submodule discovery;
- manifest-declared native capabilities;
- runtime registration, unloading, or replacement;
- FuncId or native-call bytecode instructions; or
- persistence of automatically allocated IDs.

