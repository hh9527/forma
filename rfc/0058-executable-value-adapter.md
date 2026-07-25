# RFC 0058: Executable value adapter

- Status: Implemented
- Depends on: RFC 0004, RFC 0005, RFC 0011, RFC 0054

## Amendment

The initial implementation interpreted a fixed three-field `ExecSpec`, derived
artifact paths in the host with FNV-1a, passed them through environment
variables, and launched a process. Further design established a cleaner
boundary: every deterministic decision belongs in XL, while the host only
supplies immutable invocation inputs and eventually performs declared effects.

This amendment replaces the initial protocol. The only command implemented in
this phase is:

```text
xl exec --dry-run <module.xl> [-- <arguments>...]
```

There is no short option and ordinary `xl exec` is not yet available.

## Summary

An executable XL module evaluates to an ordinary pure function:

```xl
Fn(ExecSettings, ExecRequest) -> Exec
```

The host evaluates the module, constructs two immutable XL values, calls the
function under the normal session quota, requires a JSON-compatible Dict
result, and writes canonical compact JSON to stdout. It performs no download,
installation, filesystem mutation, or process creation.

Conceptually:

```text
host invocation inputs
  -> ExecSettings + ExecRequest
  -> pure XL function
  -> concrete JSON-compatible Exec
  -> stdout
```

## Source form

An executable module may be invoked directly through a shebang:

```xl
#!/usr/bin/env -S xl exec --dry-run

fn(settings, request) {
    {
        install: [],
        command: "python3",
        args: request.args,
        env: request.env,
        cwd: request.cwd,
    }
}
```

The shebang remains ordinary `#` comment trivia.

## Inputs

The initial input shapes are:

```xl
type ExecSettings = {
    platform: {os: String, arch: String},
    cache_prefix: String,
    install_prefix: String,
};

type ExecRequest = {
    args: Array(String),
    env: Dict,
    cwd: String,
};
```

`args` contains values after the optional CLI `--`, in order. `env` is the
complete UTF-8 host environment snapshot. A non-UTF-8 name or value is an
explicit boundary error rather than silently omitted data. `cwd` is the
absolute current working directory.

The cache root is selected from `XL_CACHE_HOME`, then `XDG_CACHE_HOME`, then
`$HOME/.cache`, and finally the host temporary directory. The supplied prefixes
are:

```text
cache_prefix   = <cache-root>/xl/exec/downloads
install_prefix = <cache-root>/xl/exec/installs
```

These are input Strings only. The host does not create them.

Environment, arguments, cwd, platform, and prefixes vary per invocation, but
the module itself does not: explicit function parameters preserve ordinary
module identity, once-only initialization, tooling, and testability. No
ambient `core:ctx` module is introduced.

## Pure hashing

XL adds one generally useful pure core capability:

```xl
import hash from "core:hash";
hash.sha256: Fn(String) -> String
```

It returns the lowercase 64-character hexadecimal SHA-256 digest of the input's
UTF-8 bytes. Download cache positions can therefore be computed as:

```xl
"\{settings.cache_prefix}/\{hash.sha256(url)}"
```

Install positions can hash a name plus a canonical representation of actions.
Defining the action vocabulary and a first-class canonical JSON function is
deferred; a program may already construct its own stable String key.

## Exec result

`Exec` is deliberately application-owned in this phase. The host requires only
that the result is a Dict recursively containing JSON-compatible values:

- Int and finite Float;
- String;
- Array and Dict;
- built-in `'None`, `'True`, and `'False`, encoded as JSON null/booleans.

Bytes, named Atoms, Tagged values, Tuples, functions, and non-finite Floats are
rejected with a boundary error. Dict fields are already canonically sorted, so
compact JSON output is deterministic.

The host does not interpret, replace, or validate `install`, `command`, `args`,
`env`, paths, or action records. They are already concrete policy results of
the XL function.

## Host closure invocation

The implementation adds a general host API for invoking an XL closure exported
by a `LoadedModule`. It executes against the module's frozen main world and
external roots, imports ordinary Value arguments, preserves captures, charges
the supplied session quota, and exports the result as a Value.

Native functions and non-functions are rejected as executable entry points.
This API is pure VM execution and grants no new effect to XL code.

## CLI and errors

Accepted forms are:

```text
xl exec --dry-run tool.xl
xl exec --dry-run tool.xl -- input.c -o input.o
```

All other `exec` forms fail with usage. Loading, module evaluation, input
construction, entry invocation, result shape, and JSON compatibility are
validated before stdout is written. Debug observations retain their existing
stderr behavior.

## Non-goals

- ordinary effectful `xl exec`;
- downloading, unpacking, installing, or verifying artifacts;
- process creation, `PATH` modification, or command resolution;
- an ambient or dynamically resolved `core:ctx` module;
- a fixed host-owned Exec schema;
- lockfiles, registries, dependency solving, or platform acquisition;
- sandboxing or capability control;
- non-UTF-8 invocation context;
- a short `-n` option.

## Implementation plan

1. add quota-aware host invocation of an exported XL closure;
2. add pure `core:hash.sha256`;
3. parse the single `exec --dry-run` CLI form;
4. construct `ExecSettings` and `ExecRequest` as ordinary Values;
5. invoke the entry and encode a JSON-compatible Dict canonically;
6. remove the superseded host FNV, install environment, and process launcher;
7. test context visibility, argument order, hashing, captures, deterministic
   output, JSON rejection, usage rejection, and absence of side effects.

## Acceptance criteria

1. executable modules return `Fn(ExecSettings, ExecRequest) -> Exec`;
2. module initialization remains independent of invocation context;
3. `exec --dry-run` exposes platform, prefixes, all UTF-8 environment, cwd, and
   ordered user arguments through explicit function parameters;
4. exported XL closures retain captures and use the normal session quota;
5. `core:hash.sha256` matches standard SHA-256 vectors;
6. the result is a JSON-compatible Dict encoded as deterministic compact JSON;
7. dry-run performs no download, installation, path creation, or process spawn;
8. ordinary `exec`, malformed options, non-functions, native functions,
   malformed results, and non-JSON values fail explicitly;
9. existing `run`, `check`, `types`, `show`, LSP, quota, and cancellation
   behavior remains valid;
10. workspace tests, formatting, clippy, and strict checks pass.

## Rejected alternatives

### Dynamic `core:ctx`

Ambient invocation state makes evaluation of the same module ID depend on the
command that loaded it. Explicit parameters preserve module caching, make the
entry independently testable, and avoid a special module unavailable to normal
tooling.

### Host-side path and argument materialization

Path hashing, platform selection, environment construction, and argument
rewriting are deterministic policy. Keeping them in XL makes dry-run the exact
plan rather than an intermediate representation requiring hidden replacement.

### A separate `dry-exec` command

Dry-run is a mode of the future `exec` effect boundary. The long option keeps
the relationship explicit without committing to short-option semantics.

## Initial implementation result (superseded)

The first implementation shipped the fixed `ExecSpec` and simulated FNV install
environment described in commit `599eef2`. It performed no downloads, but its
host-side materialization model is superseded by this amendment and is removed
by the amended implementation.

## Amended implementation result

Commit `5037221` implements the explicit pure-function boundary. `LoadedModule`
and `Engine` now expose quota-aware invocation of exported XL bytecode closures,
including captured values, against the module's frozen main world. Non-functions
and native functions are rejected at this host boundary.

The `core:hash` module exposes `sha256: Fn(String) -> String` and produces the
lowercase SHA-256 digest of UTF-8 input. Its implementation is internal, has no
external package dependency, charges the 64-byte output allocation, and passes
the standard empty-input and `abc` vectors.

The CLI accepts only `xl exec --dry-run <module> [-- <arguments>...]`. It builds
the specified settings and request Dicts, invokes the module result under the
session quota, requires a JSON-compatible Dict, and prints canonical compact
JSON. The superseded FNV paths, install environment variables, and process
launcher have been removed. Tests cover shebang parsing, closure captures,
ordered arguments, environment and platform inputs, deterministic SHA-based
paths, repeatable output, invalid entries and results, rejected CLI forms, and
the absence of cache-directory side effects.

The amended implementation passes the full workspace test suite, formatting,
strict Clippy checks, and whitespace validation.
