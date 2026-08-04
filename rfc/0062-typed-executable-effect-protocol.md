# RFC 0062: Typed executable effect protocol

- Status: Accepted
- Depends on: RFC 0054, RFC 0058, RFC 0061

## Summary

`forma exec --dry-run` adopts a typed effect protocol defined by the ordinary
built-in module `@bim/std/exec`:

```forma
Fn(exec.ExecSettings, exec.ExecRequest) -> exec.ExecEnv
```

The entry function performs every deterministic decision and returns ordinary
typed Forma data. The CLI validates the result against the fixed ExecEnv ABI
and writes canonical JSON. It still performs no download, installation,
filesystem mutation, or process creation.

## Standard protocol module

`@bim/std/exec` exports these TypeMetadata values:

```forma
@struct type Platform = {
    os: String,
    arch: String,
};

@struct type ExecSettings = {
    platform: Platform,
    install_prefix: String,
};

@struct type ExecRequest = {
    args: Array(String),
    env: Dict(String),
    cwd: String,
};

@enum type UnpackType = {
    TarGzip: 'None,
    Tar: 'None,
};

@struct type UnpackOpt = {
    dest: String,
    ty: UnpackType,
    src: String,
    strip: Int,
    digest: Option(String),
};

@enum type Install = {
    Unpack: UnpackOpt,
};

@struct type ExecEnv = {
    install: Array(Install),
    cwd: Option(String),
    bin: String,
    args: Array(String),
    env: Dict(String),
};
```

These are ordinary `@struct` and `@enum` declarations. User code may inspect,
transform, print, validate, and compose them like any other TypeMetadata. The
VM does not add an Exec instruction or privileged value kind.

## Entry form

An executable module conventionally imports the protocol and makes its result
contract explicit:

```forma
import exec from "@bim/std/exec";

let main: Fn(exec.ExecSettings, exec.ExecRequest) -> exec.ExecEnv =
    fn(settings, request) {
        # Pure plan computation.
    };

main
```

The explicit contract gives ordinary checking, hover, and definition
navigation. The CLI also validates the returned value independently at the
host boundary, so an unannotated or dynamically imprecise entry cannot bypass
the protocol.

## Invocation inputs

The host supplies:

- normalized Rust target names for `platform.os` and `platform.arch`;
- a UTF-8 absolute `install_prefix` under Forma's cache root;
- ordered arguments after CLI `--`;
- the complete UTF-8 environment as `Dict(String)`;
- the UTF-8 absolute current working directory.

Non-UTF-8 input remains an explicit boundary error.

RFC 0058 exposed `cache_prefix` so user code could plan downloads. RFC 0062
removes it from ExecSettings. `Unpack` owns acquisition and caching through its
`src` and optional `digest`; the pure plan chooses only the deterministic final
`dest` under `install_prefix`.

## Install actions

`Install` is an extensible effect enum. This RFC defines only `Unpack`.

`UnpackOpt` means:

- acquire `src` through the future effect layer;
- when `digest` is `Some`, verify the acquired bytes against that digest;
- interpret the archive according to `ty`;
- remove `strip` leading path components while unpacking;
- materialize the result at the concrete `dest`.

RFC 0062 validates and serializes these fields but executes none of those
steps. Digest algorithm vocabulary, URL schemes, cache eviction, archive
safety, atomic installation, and existing-destination behavior are deferred
to the effectful runner RFC.

There is no separate Download action. Acquisition is an internal part of
Unpack because the action already carries the source and integrity rule.

## Command-line rewriting

The entry receives the original request arguments and returns final arguments.
It may use ordinary Array and String functions to add sysroots, library search
paths, reproducible-source mappings, wrappers, or platform-specific defaults.

The host treats `ExecEnv.bin`, `args`, `env`, and `cwd` as concrete values. It
does not expand templates, substitute variables, search PATH, infer install
locations, or reinterpret command-line policy. Dry-run JSON is therefore the
exact future execution plan rather than an intermediate template.

## Boundary validation and JSON form

After invocation, the CLI validates the complete result:

- the outer value has exactly the ExecEnv fields;
- `install` is an Array of known Install Tagged values;
- `Unpack` payloads have the exact UnpackOpt fields and scalar types;
- unit enum values and Options have their canonical Atom/Tagged forms;
- `args` contains only Strings;
- `env` is a Dict whose values are all Strings;
- unknown fields, variants, and malformed nested values fail with a value path.

Canonical JSON uses the same natural representation as Forma's JSON codec:

- `'Unpack(payload)` becomes `{ "Unpack": payload }`;
- `'TarGzip` and `'Tar` become Strings;
- `'None` becomes null and `'Some(value)` becomes `value`;
- Dict keys remain canonically sorted.

Validation completes before stdout is written. Failure therefore emits no
partial plan.

## Closed-world boundary

The module remains a pure closed-world computation. ExecSettings and
ExecRequest are explicit function arguments rather than an ambient module.
The host-side adapter has only three responsibilities:

1. construct immutable invocation inputs;
2. validate and serialize the returned protocol value;
3. in a future RFC, perform the declared effects.

All hashing, platform selection, install destination calculation, environment
construction, and command-line rewriting stay in Forma. The fixed protocol is
an adapter ABI, not hidden evaluation semantics.

## Compatibility

RFC 0058 accepted any JSON-compatible Dict. RFC 0062 intentionally tightens
`forma exec --dry-run` to ExecEnv. Existing generic dry-run fixtures migrate to
the standard protocol. `forma run`, ordinary module results, and JSON codecs
remain unchanged.

Only `--dry-run` exists in this RFC. Effectful `forma exec` remains rejected.

## Non-goals

- downloading, caching, digest verification, or unpacking;
- process creation or environment application;
- additional Install variants;
- URL and digest algorithm vocabularies;
- PATH lookup or shell parsing;
- ambient `@bim/runtime/ctx` state;
- host-side command or path rewriting;
- a short `-n` option.

## Implementation plan

1. Add the declaration-only `@bim/std/exec` core module.
2. Remove `cache_prefix` from host ExecSettings and retain install_prefix.
3. Add exact host validation for ExecEnv, Install, UnpackOpt, Options, and
   homogeneous String Dicts.
4. Serialize validated protocol values to canonical codec-shaped JSON.
5. Replace generic JSON-result CLI fixtures with typed protocol fixtures that
   exercise two Unpack actions and command-line rewriting.
6. Add malformed nested action, environment, Option, variant, and shape tests.
7. Update README examples to return typed ExecEnv values directly.

## Acceptance criteria

1. `@bim/std/exec` exports all protocol TypeMetadata values;
2. a checked entry has the exact function type in hover and `forma show`;
3. host request environment is observed as `Dict<String>`;
4. two Unpack actions survive dry-run in deterministic order;
5. platform-dependent destinations and rewritten user arguments appear exactly
   in canonical JSON;
6. dry-run accepts Tagged Install values and Options without a user codec call;
7. malformed outer shapes, variants, payloads, env values, args, and options
   fail with precise paths before stdout;
8. no cache directory, destination, or other filesystem path is created;
9. repeated invocation with equal inputs produces byte-identical stdout;
10. ordinary effectful exec remains rejected;
11. existing run, check, types, show, LSP, quota, cancellation, codec, and
    schema behavior remains unchanged;
12. formatting, strict Clippy, and the full workspace test suite pass.

## Rejected alternatives

### Require user code to call codec.encode

This changes the entry contract to return `Any`, loses the typed effect
boundary at the final expression, and leaks host serialization mechanics into
every executable module.

### Retain arbitrary JSON Dict results

The host cannot distinguish a complete executable plan from an accidental
shape, and tooling cannot describe the actual adapter contract.

### Separate Download and Unpack actions

Unpack already needs source bytes, integrity information, and a destination.
Exposing acquisition as a second user-visible action duplicates dependencies
and allows inconsistent intermediate paths.

### An ambient context module

Invocation-dependent module identity harms caching and makes the entry harder
to test as an ordinary function. Explicit parameters keep the boundary local.
