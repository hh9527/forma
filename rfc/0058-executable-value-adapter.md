# RFC 0058: Executable value adapter

- Status: Implemented
- Depends on: RFC 0004, RFC 0005, RFC 0011, RFC 0054

## Summary

XL adds a small `exec` CLI command:

```text
xl exec script.xl
```

It evaluates the module exactly like `xl run`, interprets the resulting value
as an `ExecSpec`, computes deterministic local positions for declared install
locators, and starts the requested process without a shell.

This is a built-in application adapter, not a language, VM, type-system, or
module-system capability. Conceptually:

```text
xl exec script.xl = xl run script.xl | exec-helper
```

The first implementation does not download, unpack, or create artifacts. It
only computes the positions a future artifact provider would populate and
passes those positions to the child process.

## Source form

Since `#` is ordinary XL comment trivia, an executable module may begin with:

```xl
#!/usr/bin/env -S xl exec
# deps.json inside
{
    install: ["http://url/to/python/tarball"],
    command: "python3",
    args: ["--version"],
}
```

The shebang has no special AST or runtime meaning. The operating system selects
`xl exec`; XL parses the line as a comment.

## ExecSpec

The accepted runtime shape is deliberately small:

```xl
type ExecSpec = {
    install: Array(String),
    command: String,
    args: Array(String),
};
```

All fields are required. Unknown fields are rejected so misspellings do not
silently change execution. The adapter validates the ordinary runtime value
after module evaluation; `ExecSpec` is not a privileged built-in XL type.

The command is passed directly to the host process API. Arguments are passed as
an array. No shell parsing, interpolation, redirection, globbing, or pipeline
syntax is provided.

## Simulated installs

Each install String is an opaque locator. The first implementation derives a
stable lowercase hexadecimal FNV-1a digest from its UTF-8 bytes and computes:

```text
<cache-root>/xl/exec/artifacts/<digest>
```

The cache root is selected in this order:

1. `XL_CACHE_HOME`, when set;
2. `XDG_CACHE_HOME`, when set;
3. `$HOME/.cache`;
4. the host temporary directory as a final fallback.

No network request, filesystem creation, or existence check occurs. Repeated
equal locators produce equal positions; distinct locators are not promised
collision resistance in this simulation.

The child receives:

```text
XL_EXEC_INSTALL_COUNT=<N>
XL_EXEC_INSTALL_0=<first position>
...
XL_EXEC_INSTALL_<N-1>=<last position>
```

Order and duplicates are preserved. Existing variables with these names are
overwritten for the child only. The simulated positions are not added to
`PATH`; the command continues to use ordinary host process resolution.

## Execution and errors

Module loading and evaluation retain the same quotas, diagnostics, imports, and
debug behavior as `xl run`. Validation happens before process creation.

Failure to load, evaluate, validate, or spawn is an `xl exec` error. A child
that exits successfully makes `xl exec` succeed. A non-zero exit or signal is
reported as an error; exact exit-code forwarding is deferred until the CLI has
a general command-outcome abstraction.

The child inherits stdin, stdout, stderr, the current working directory, and
the host environment, except for the install variables above.

## Non-goals

- downloading, unpacking, installing, or verifying artifacts;
- digests supplied by users or content-addressed storage;
- modifying `PATH` or resolving commands inside artifacts;
- lockfiles, registries, dependency solving, or platform selection;
- sandboxing or capability control;
- environment fields, working-directory fields, or shell commands;
- making process execution available inside ordinary XL evaluation.

## Implementation plan

1. share module evaluation between `run` and `exec`;
2. validate the resulting `Value` as the exact `ExecSpec` shape;
3. compute deterministic simulated artifact positions;
4. launch the command with indexed install environment variables;
5. add CLI tests for shebang execution, position stability, shape errors, and
   child failure.

## Acceptance criteria

1. `xl exec` evaluates imports and expressions exactly as `xl run` does;
2. a first-line `#!/usr/bin/env -S xl exec` is accepted;
3. malformed or non-`ExecSpec` values fail before spawning;
4. command and args are passed without a shell;
5. install positions are deterministic, ordered, and visible to the child;
6. execution performs no download and creates no simulated artifact path;
7. child spawn and non-zero status failures are reported;
8. existing `run`, `check`, `types`, `show`, LSP, quota, and cancellation tests
   remain valid;
9. workspace tests, formatting, clippy, and strict checks pass.

## Rejected alternatives

### Add process effects to XL

The module must remain a closed-world computation producing data. Execution is
an explicit CLI consumer of that data.

### Encode this as a new module format

An executable specification is an ordinary XL module. The shebang selects a
consumer and does not alter parsing or module identity.

### Pretend artifacts are installed by adding them to PATH

The first implementation has no artifact contents or known layout. Indexed
environment variables expose computed positions without implying installation
has occurred.

## Implementation result

Implemented in July 2026.

- `xl run` and `xl exec` share the same module evaluation helper and therefore
  retain identical loading, quota, debug, and runtime behavior.
- The CLI validates the evaluated legacy `Value` as an exact three-field
  `ExecSpec`; field and array-element mismatches fail before process creation.
- Locator bytes are mapped with stable 64-bit FNV-1a to the documented cache
  layout. The adapter only constructs `PathBuf` values and performs no artifact
  filesystem or network operation.
- The child is launched through `std::process::Command` with direct arguments,
  inherited standard streams and environment, and indexed
  `XL_EXEC_INSTALL_*` variables. Stale indexed variables inherited by `xl` are
  removed from the child environment.
- CLI integration tests cover shebang evaluation, repeated path stability,
  child visibility, absence of the simulated path, exact-shape rejection, and
  non-zero child status. The process-observation tests are Unix-only because
  they use the standard `env` and `false` utilities; the adapter implementation
  itself uses portable Rust process APIs.

No download, unpack, cache creation, `PATH` modification, or language-level
effect was added.
