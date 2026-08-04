# RFC 0069: Deterministic output plans

- Status: Implemented
- Depends on: RFC 0058, RFC 0062, RFC 0063, RFC 0068

## Summary

Forma gains a pure output boundary for generated text. A build entry is an
ordinary function:

```forma
Fn() -> build.OutputPlan
```

`forma build --dry-run` invokes it, validates the complete result, and prints
canonical JSON. It does not create directories or write files.

```forma
import build from "@bim/std/build";

type OutputPlan = build.OutputPlan;

let main: Fn() -> OutputPlan = fn() {
    {
        files: [
            'TextFile({path: "generated/app.conf", content: render()}),
        ],
    }
};

main
```

## Protocol

`@bim/std/build` exports declaration-only TypeMetadata:

```forma
@struct type TextFile = {
    path: String,
    content: String,
};

@enum type Artifact = {
    TextFile: TextFile,
};

@struct type OutputPlan = {
    files: Array(Artifact),
};
```

The single-variant Enum is intentional. Later RFCs may add binary files,
directories, symlinks, deletion, permissions, or source maps only after their
canonical representation and safety contract are defined.

## Output paths

Every `TextFile.path` is a normalized logical relative path using `/` as its
separator. The host rejects:

- empty and absolute paths;
- empty, `.` or `..` components;
- backslashes;
- duplicate paths within one plan.

The first implementation does not resolve these paths against the filesystem.
Validation establishes the future write boundary without observing or mutating
the external world.

Artifacts retain their declared Array order. Dict fields and JSON object keys
use their existing canonical order. Equal modules therefore produce
byte-identical dry-run output.

## Text layout foundations

`@bim/std/string` adds ordinary deterministic functions:

```forma
lines: Fn(String) -> Array(String)
join_lines: Fn(Array(String)) -> String
indent: Fn(String, Int) -> String
ensure_trailing_newline: Fn(String) -> String
trim_margin: Fn(String, String) -> String
```

`lines` recognizes LF and removes a preceding CR from each line; it retains a
final empty line after a trailing newline. `join_lines` inserts LF.

`indent` prefixes every non-empty physical line with `width` ASCII spaces and
rejects negative widths. It retains existing line endings. `ensure_trailing_newline`
adds one LF only when the input does not already end in LF.

`trim_margin(source, marker)` examines every physical line. When its first
non-space/tab content begins with the non-empty marker, leading whitespace and
that marker are removed. Other lines remain byte-for-byte unchanged. The
marker is explicit so the library does not reserve template syntax.

These operations return Strings. This RFC adds neither a `Doc` runtime type nor
stateful layout escapes.

## CLI boundary

`forma build --dry-run <module.forma>`:

1. loads and evaluates the closed module graph;
2. requires the result to be a zero-argument function;
3. invokes it with the ordinary session quota;
4. validates `OutputPlan`, every Artifact, and every nested field;
5. validates paths and conflicts before producing stdout;
6. prints one canonical JSON value followed by a newline.

An unannotated entry cannot bypass host validation. An annotated entry gains
ordinary type checking, hover, and navigation through `@bim/std/build`.

## Non-goals

- writing, deleting, chmod, linking, or creating directories;
- reading existing outputs or producing diffs;
- binary content and its JSON encoding;
- ambient build context, workspace root, environment, or command arguments;
- automatic escaping for shell, HTML, YAML, or source languages;
- template syntax, a `Doc` algebra, or implicit indentation propagation;
- a short dry-run option.

## Acceptance criteria

1. `@bim/std/build` exports the complete protocol metadata;
2. `forma build --dry-run` invokes `Fn() -> OutputPlan` and emits canonical JSON;
3. malformed shapes and values fail with precise value paths and no stdout;
4. unsafe and duplicate output paths are rejected;
5. dry-run creates no output path or parent directory;
6. repeated equal invocations produce byte-identical output;
7. String layout functions have explicit contracts and charge allocation quota;
8. concat, raw Strings, codecs, exec, module loading, and LSP remain unchanged;
9. full tests, formatting, strict Clippy, and whitespace checks pass.

## Rejected alternatives

### Add a template language

Forma expressions, raw Strings, and concat already provide computation and
interpolation. A second condition/loop/scope language would duplicate the
language core. Layout remains ordinary String functions until real use proves
a structured `Doc` value necessary.

### Write files immediately

Filesystem semantics require decisions about roots, atomic replacement,
permissions, symlinks, stale outputs, conflicts, and recovery. Dry-run first
makes the pure plan inspectable and stabilizes the adapter ABI before effects.

### Include BinaryFile immediately

Bytes need a canonical JSON representation, integrity vocabulary, and host
size policy. A premature encoding would become part of the public protocol.

## Implementation result

The declaration-only `@bim/std/build` module publishes `TextFile`, `Artifact`,
and `OutputPlan` metadata. The CLI build adapter invokes a zero-argument entry,
validates exact shapes, Tags, Strings, normalized paths, and duplicates, then
serializes canonical JSON without touching output paths.

The String native family now implements line splitting/joining, non-empty-line
indentation, trailing-newline normalization, and explicit margin removal under
the same allocation quotas and typed native contracts as existing String
operations. End-to-end CLI tests generate two files, prove deterministic
output, prove no write occurs, and reject escape and conflict plans.
