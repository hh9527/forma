# RFC 0116: Host native module imports

- Status: Implemented
- Depends on: RFC 0059, RFC 0114, RFC 0115

## Summary

Forma modules may import a native module that the embedding Engine registered
before construction:

```forma
import secrets from "@bim/acme/secrets";
```

The resolved ModuleId is the exact canonical logical name and has no physical
path. Resolution recognizes the namespace shape; authority comes only from the
Engine's frozen registry. An unregistered name is unavailable and cannot be
satisfied by a file, dependency, or manifest.

## Resolution

Registered Host modules use the existing `@bim/` request rules:

1. the request must contain a non-empty path after the prefix;
2. it resolves to `ModuleId::Builtin(exact_name)` with Forma format and no
   physical path;
3. strict loading looks up the exact name in the frozen native registry;
4. a miss reports `unknown built-in module` at the import site;
5. relative and `@src/` imports remain unavailable from native modules.

Host names cannot shadow current or future Forma modules because registration
reserves the complete `@bim/std` and `@bim/core` subtrees. Core and Host
modules otherwise share one built-in namespace and one import path model.

## Workspace projection

Strict loading and recoverable synchronous/asynchronous workspace analysis use
the same frozen module values and interfaces. A successful import contributes
the exact ModuleId and its interface to semantic resolution and completion.
Unknown imports produce one sourced unavailable built-in-module fact and do
not block independent definitions.

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
8. files and manifests cannot satisfy an `@bim/...` request;
9. no registry mutation or dynamic loading path is introduced; and
10. full workspace tests and strict Clippy pass.

## Implementation plan

1. reuse canonical `@bim/...` module requests;
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

## Implementation result

Implemented Host modules as ordinary registered `@bim/...` built-ins, with no
second namespace. The existing resolver produces the exact pathless Builtin
ModuleId; strict and recoverable loaders authorize it by exact lookup in the
Engine's frozen module map. Unknown names receive a sourced `unknown built-in
module` diagnostic and cannot fall through to files or dependencies.

Integration tests register `@bim/acme/runtime`, call its native Function,
observe its opaque type name, type-check its interface, and complete its exact
exports. Synchronous and asynchronous recovery both project the module as a
known built-in. A separately built Engine cannot resolve it. Unknown built-ins
remain unavailable while independent type facts stay known. Module completion
now falls back to the known import-interface type for synthetic built-ins that
have no source/result node, improving core and Host modules uniformly.
