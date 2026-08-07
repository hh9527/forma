# RFC 0176: Pending module lifecycle

- Status: Implemented
- Depends on: RFC 0175

## Summary

Introduce an unforgeable pending main handle and an explicit one-way
initialization transition. Loading a selected entry parses only the main root's
immediate options and invokes the entry's exported function with:

```forma
native type ModuleHandle = @1;

@struct type OptionAction = { key: String, value: Any };
@struct type Module = {
    options: Array(OptionAction),
    module: ModuleHandle,
};
```

The main import graph remains unopened until trusted entry code calls:

```forma
native type InstantiatedModule = @2;
native def initialize_module:
    Fn(ModuleHandle) -> Result(InstantiatedModule, BlameError);
```

## Ownership

`ModuleHandle` owns an invocation-local state object rather than referring to
an ambient current entry. Host observations and later injection APIs receive
the handle explicitly. This permits concurrent Engine sessions, nested Hosts,
and isolated tests without thread-local or process-global registries.

The state contains the normalized main root, Engine configuration, native
module registry, debug sink, ordered options, pending virtual modules, and a
lifecycle cell. It does not contain a preloaded main graph.

Opaque handle equality is identity equality. Handles and instantiated modules
cannot be serialized, constructed, decoded, reflected into payload data, or
accepted under a different native type ID.

## Lifecycle

```text
Pending --initialize--> Initializing --success--> Ready
                              |
                              +--failure--> Failed
```

- the first initialization consumes the pending transition;
- a successful repeat returns the same ready instance;
- a failed repeat returns the same stable failure;
- recursive initialization while `Initializing` reports a cycle;
- mutation APIs accept only `Pending`;
- state synchronization never holds a lock while compiling or running Forma.

Initialization snapshots pending virtual modules, releases the lifecycle lock,
loads main through the ordinary module pipeline, executes its initialization,
publishes exports, and atomically stores either the ready instance or failure.

## Entry invocation

The Host compiles/evaluates the selected entry without `@main`, selects its
explicit `entry` export, and invokes it with the `Module` value. Entry options
are closed immediate values copied from the preliminary root parse. No other
main binding or import is visible before initialization.

The initial API may expose `initialize_module` as a Rust-level operation while
the opaque ABI is installed in the same change. RFC 0177 supplies the complete
entry runtime module and RFC 0179 switches exec to the function protocol.

## Diagnostics and quotas

Preliminary option parse errors point to the main source. Initialization uses
the Engine's ordinary module quota and the entry invocation shares the session
quota. Loader/runtime failures retain all main sources and gain the authored
entry call location when reraised through the native boundary.

The pending handle does not grant a second independent quota by repeated
initialization.

## Acceptance criteria

1. preparing a pending module does not resolve a main import;
2. ordered repeated main options are available before initialization;
3. handles are unforgeable and compare by identity;
4. initialization loads and executes main exactly once;
5. success and failure repeats are stable;
6. recursive initialization is rejected;
7. strict main diagnostics retain authored sources;
8. no ambient or global current-entry state is introduced;
9. existing direct module loading remains unchanged;
10. full workspace tests and warning-denied Clippy pass.

## Non-goals

- virtual module mutation, defined by RFC 0177;
- typed export projection, defined by RFC 0178;
- changing ordinary import eagerness;
- exposing pending handles to ordinary main code by default.

## Implementation result

`Engine::prepare_module[_with_arguments]` parses root options without opening
imports and returns an identity-opaque `PendingModule`. Initialization caches
`Pending`, `Initializing`, `Ready`, and `Failed` outcomes and never holds its
lifecycle lock while loading or evaluating main.

The entry argument is the handle itself rather than the draft `Module` record.
`module_options(handle)` and the other entry runtime operations expose the
record's intended fields explicitly. Tests cover deferred imports, ordered
options, stable repetitions, and handle-local injected state.
