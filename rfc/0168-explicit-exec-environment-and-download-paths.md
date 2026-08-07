# RFC 0168: Explicit exec environment and download paths

- Status: Implemented
- Depends on: RFC 0062, RFC 0159, RFC 0163, RFC 0166

## Summary

The executable-plan protocol will distinguish captured Host input from the
environment policy returned for the target process. `ExecEnv.env` becomes:

```forma
@struct type ExecEnvironment = {
    clear: Bool,
    update: Dict(Option(String)),
};
```

`clear` selects whether the executor starts from an empty or inherited
environment. Each `update` entry either sets a variable with `'Some(value)` or
removes it with `'None`.

Every `Unpack` action will also contain the fully computed archive cache path:

```forma
@struct type UnpackOpt = {
    file: String,
    dest: String,
    ty: UnpackType,
    src: String,
    strip: Int,
    digest: Option(String),
};
```

`ExecSettings` supplies separate `download_prefix` and `install_prefix`
values. Forma code computes both `file` and `dest`; an external executor does
not derive cache identities or reinterpret the plan.

## Motivation

`option "exec.capture-envs"` controls which Host variables Forma may observe
through `ExecRequest.env`. Reusing that Dict as `ExecEnv.env` incorrectly
makes captured inputs implicit target-process outputs. A wrapper may need
TARGET to select a sysroot without exposing TARGET to the compiler, or may
construct an entirely clean deterministic environment.

Likewise, an `Unpack` action containing only `src` and `dest` leaves the
effectful executor responsible for inventing a download cache path. Cache
addressing is deterministic pure policy and belongs in the Forma plan.

## Protocol

The source-owned protocol becomes conceptually:

```forma
@struct type ExecSettings = {
    platform: Platform,
    download_prefix: String,
    install_prefix: String,
};

@struct type ExecEnvironment = {
    clear: Bool,
    update: Dict(Option(String)),
};

@struct type UnpackOpt = {
    file: String,
    dest: String,
    ty: UnpackType,
    src: String,
    strip: Int,
    digest: Option(String),
};

@struct type ExecEnv = {
    install: Array(Install),
    cwd: Option(String),
    bin: String,
    args: Array(String),
    env: ExecEnvironment,
};
```

`ExecEnvironment {clear: 'False, update: {}}` preserves the inherited process
environment unchanged. With `clear: 'True`, the executor clears it first.
After that choice, `'Some(value)` sets or overwrites a name and `'None`
ensures that a name is absent.

This RFC intentionally uses `update`, not `upset` or `upsert`: the operation
includes deletion and is not limited to insertion/update semantics.

## Addressing rule

The GCC-wrapper fixture computes:

```forma
file = `\{settings.download_prefix}/\{sha256(package.src)}`
```

The URL is the download identity for this phase. `digest` remains an
independent integrity assertion. `dest` continues to use the install identity
derived from the package name and complete install action inputs. The Host
provides physical prefixes but does not calculate either final path.

The dry-run adapter validates and renders these exact values. A future
effectful executor must download `src` to `file`, verify `digest` when present,
and unpack `file` into `dest`; it must not silently substitute another cache
address.

## Compatibility

There is no compatibility shape. Current source, fixtures, and tests migrate
atomically. An `ExecEnv` containing the old `Dict(String)` environment or an
`Unpack` without `file` is rejected by the typed boundary and Host adapter.

## Non-goals

- downloading, verifying, unpacking, or executing a process;
- defining cache locking, partial-file, retry, or garbage-collection policy;
- content-addressing from downloaded bytes;
- automatically forwarding captured environment variables;
- platform-specific environment key normalization;
- adding mutable environment or install builders.

## Acceptance criteria

1. `ExecSettings` exposes distinct download and install prefixes;
2. `ExecEnvironment` is exported from `std/rt-types/exec.forma`;
3. `ExecEnv.env` accepts only `{clear, update}` with typed optional values;
4. empty update with `clear: 'False` represents no environment changes;
5. every `Unpack` requires a concrete `file` path;
6. the GCC wrapper derives `file` from download prefix and `sha256(src)`;
7. captured TARGET remains available as input but is not implicitly copied to
   the target process environment;
8. canonical dry-run JSON renders clear/update plus each archive file path;
9. malformed old protocol shapes are rejected;
10. repeated dry-runs remain byte-identical and create no cache files;
11. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. revise the authoritative source protocol and its metadata tests;
2. provide both prefixes in the CLI-authored `ExecSettings`;
3. make the dry-run adapter validate and render the new exact shapes;
4. update the GCC fixture to calculate archive paths and an explicit unchanged
   target environment;
5. update end-to-end expectations and record implementation evidence.

## Stopping rules

Work returns to discussion if completion requires performing effects, letting
the Host derive an omitted path, automatically forwarding captured input, or
introducing mutable/effectful plan construction.

## Implementation result

Implemented in August 2026. The authoritative source protocol now exports
`ExecEnvironment`, gives `ExecSettings` separate download/install prefixes,
requires `UnpackOpt.file`, and uses the explicit environment policy from
`ExecEnv`. The CLI adapter supplies both prefixes and validates/renders the
new shapes without deriving missing values.

The GCC-wrapper fixture hashes each package URL under `download_prefix`, keeps
its independent install identity under `install_prefix`, and returns
`{clear: 'False, update: {}}`. Captured TARGET selects the sysroot but is not
forwarded. CLI coverage also demonstrates explicit forwarding through
`'Some`, renders deletion as `null`, and rejects old environment and Unpack
shapes. Repeated dry-runs remain deterministic and do not create the cache.
