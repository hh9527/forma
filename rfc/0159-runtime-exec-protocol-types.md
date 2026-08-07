# RFC 0159: Runtime exec protocol types

- Status: Proposed
- Depends on: RFC 0051, RFC 0062, RFC 0113, RFC 0134, RFC 0142, RFC 0145,
  RFC 0157, RFC 0158

## Summary

Forma will expose the executable Host boundary through an ordinary source-only
built-in module with the authored path used by executable-plan applications:

```forma
import "std/rt-types/exec.forma" {
    ExecFn,
    ExecSettings,
    ExecRequest,
    ExecEnv,
};
```

`ExecFn` denotes:

```forma
Fn(ExecSettings, ExecRequest) -> ExecEnv
```

Importing these types grants no effect. They are runtime protocol metadata;
only the `forma exec` Host gives an export named `exec` operational meaning.

The existing `std/exec` request is removed without a compatibility alias. Its
source declarations move to the new module, which also defines `ExecFn`. The
former reserved module ID 12 becomes an unassigned tombstone and is not reused;
the new canonical module receives reserved ID 21.

## Motivation

RFC 0062 established a typed executable protocol, but application code must
currently repeat its Function shape:

```forma
import "std/rt-types/exec.forma" as exec_types;

export def exec:
    Fn(exec_types.ExecSettings, exec_types.ExecRequest) -> exec_types.ExecEnv =
    fn(settings, request) {
    # ...
};
```

The GCC-wrapper target should state the Host contract directly:

```forma
import "std/rt-types/exec.forma" { ExecFn };
export def exec: ExecFn = wrap_gcc(source);
```

The path also makes the architectural role explicit. This module describes
data exchanged with a runtime mode; it is not a library of effectful
Functions. A program may construct or inspect these values under `run`,
`check`, LSP analysis, or an embedding Host without gaining process access.

## Authoritative identity

The former `std/exec` module used reserved built-in module ID 12 and published
the TypeMetadata graph for:

- `Platform`;
- `ExecSettings`;
- `ExecRequest`;
- `UnpackType`;
- `UnpackOpt`;
- `Install`; and
- `ExecEnv`.

RFC 0159 moves those declarations into the new source-only module at reserved
ID 21 and adds `ExecFn` in the same authoritative source:

```forma
@struct type Platform = { ... };
# ... remaining protocol declarations ...
type ExecFn = Fn(ExecSettings, ExecRequest) -> ExecEnv;

export {
    Platform,
    ExecSettings,
    ExecRequest,
    UnpackType,
    UnpackOpt,
    Install,
    ExecEnv,
    ExecFn,
};
```

There is one canonical path and one metadata definition. `ExecFn` is an
ordinary authored type binding whose value is Function TypeMetadata; it is not
a new parameterized alias mechanism or nominal Function type.

## Module path

The exact built-in request is `std/rt-types/exec.forma`. Resolver authority
comes from the registered built-in list, not from the `std/` prefix or file
suffix. A filesystem package cannot shadow this exact request.

The explicit `.forma` suffix is intentional for this protocol namespace: it
shows that the contract is implemented as inspectable Forma source rather than
as an opaque Host capability. This RFC does not rename other existing
`std/...` modules or establish a general extension rule for all built-ins.

## Host interpretation

The `forma exec` adapter continues to:

1. load a root module under strict bounded evaluation;
2. select its explicit `exec` export;
3. require a Function accepting `ExecSettings` and `ExecRequest`;
4. invoke it with Host-authored input;
5. validate the returned `ExecEnv`; and
6. print canonical JSON under `--dry-run`.

The adapter may use the same structural validation it uses today. Importing
`ExecFn` does not register a callback, mark a module executable, bypass Host
validation, or authorize effects. Conversely, an authored equivalent Function
contract remains acceptable even if the source did not import `ExecFn`.

## Tooling

Check, show, hover, completion, and module-interface publication expose
`ExecFn` as a concrete monomorphic Function scheme. They must not widen it to
`Any`, present it as a native opaque type, or invent a nominal distinction
between the same Function metadata constructed elsewhere.

Selective import and completion use the ordinary explicit export surface. The
new module introduces no implicit prelude names.

## Goals

1. provide the exact `std/rt-types/exec.forma` import used by RFC 0157;
2. give the executable entry Function one reusable `ExecFn` name;
3. keep one authoritative executable protocol metadata definition;
4. keep protocol import separate from Host effect interpretation;
5. expose the complete protocol through ordinary module tooling;
6. reserve the retired ID 12 without retaining a compatibility request.

## Non-goals

- adding real process execution, downloads, unpacking, or filesystem effects;
- requiring a nominal `ExecFn` witness at runtime;
- making `ExecFn` a capability token;
- changing the fields or variants of the RFC 0062 protocol;
- adding general type aliases, parameterized data declarations, or new type
  syntax;
- moving every standard-library type into `std/rt-types`;
- retaining aliases for historical executable protocol import paths;
- inferring a command mode from imported modules.

## Acceptance criteria

1. `std/rt-types/exec.forma` resolves as a reserved source-only built-in;
2. it exports all seven protocol types plus `ExecFn` from one source;
3. `std/exec` no longer resolves and reserved module ID 12 is not reused;
4. `ExecFn` checks exactly as
   `Fn(ExecSettings, ExecRequest) -> ExecEnv`;
5. a root can annotate `export def exec: ExecFn = ...` and complete
   `forma exec --dry-run`;
6. malformed entry results remain rejected by the Host adapter;
7. importing or constructing protocol values under another mode causes no
   external effect;
8. show, hover, completion, and module interfaces expose the authored names
   without `Any` widening;
9. all current non-historical source, tests, and documentation use the new
   canonical path;
10. the new reserved ID is unique and order independent;
11. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. retire the ID 12 registration and reserve it as an unused tombstone;
2. move the embedded protocol source to the new module at reserved ID 21 and
   add `ExecFn`;
3. add module tests for metadata identity, `ExecFn`, interface publication,
   and resolver precedence;
4. migrate all current CLI, library, example, and documentation consumers;
5. update current documentation and record implementation evidence.

## Stopping rules

Work returns to discussion if the implementation requires:

1. effect authorization from a type import;
2. a nominal Function runtime value or new Function calling convention;
3. reusing former stable module ID 12 for the new logical module;
4. retaining two independently evolving protocol definitions;
5. implicit mode-dependent names in ordinary lexical scope; or
6. a general type-alias or parameterized-type feature.
