# RFC 0162: Scoped module option actions

- Status: Proposed
- Depends on: RFC 0059, RFC 0134, RFC 0147, RFC 0157

## Summary

Forma replaces the root-only `@@manifest { ... };` directive with ordered,
scope-qualified option actions:

```forma
option "crate.dependency" {
    name: "models",
    source: 'Path({path: "../models"}),
};

option "module.documentation" {category: "tooling"};
```

The general form is:

```text
option string-literal immediate-value ";"
```

Options are top-level module metadata. They may occur between any top-level
declarations, may repeat the same key, retain source order, and have no binding,
evaluation, type-inference, or export relationship with surrounding code.

## Action model

Each parsed module publishes an ordered sequence:

```text
OptionAction = {key: String, value: Immediate, location: Location}
```

An option is an action, not an assignment. Repeated keys do not overwrite one
another and there is no language-wide last-wins rule. Each registered consumer
defines whether order affects its result. Consumers should be order-independent
where their domain permits it, but the parser and tooling always preserve
authored order and source locations.

Keys use lower-case dotted segments. The first segment identifies scope:

- `crate.*` is accepted only from `@main` and may be consumed before import
  resolution;
- `module.*` may occur in any Forma module and affects only that module;
- `exec.*` is accepted only from `@main` and is consumed by the executable Host.

The prefix does not itself grant authority. Consumers register complete keys,
their scope, phase, ordering policy, and value schema. Unknown options remain
available to tooling but cannot acquire resolver or Host effects merely by
choosing a privileged-looking prefix.

## Placement and immediacy

Options may be interleaved with imports, definitions, and exports because they
are collected independently from the executable program:

```forma
import "std/array" as array;
option "module.documentation" {category: "collection"};
export def values = fn() { [] };
option "module.documentation" {stability: "experimental"};
```

They are not expressions and are forbidden inside Functions and ordinary
blocks. Values extend the closed immediate subset previously accepted by
`@@manifest`: finite numbers, Strings, Atoms, zero- or one-payload Tagged
constructors, Arrays, and undecorated Dicts composed recursively from those
values. A Tagged constructor is data syntax, not an arbitrary call. Ordinary
calls, variable references, interpolation, and computed fields remain
forbidden.

## Initial resolver actions

This RFC registers two pre-resolution actions:

```forma
option "crate.dependency" {
    name: "models",
    source: 'Path({path: "../models"}),
};

option "crate.format" {
    module: "models/data",
    format: "json",
};
```

`crate.dependency` is repeatable and order-independent. Dependency names must
be unique. The initial `Path` provider preserves the current local development
capability; pinned remote providers remain a child of RFC 0157.

`crate.format` is repeatable and order-independent. Module keys must be unique.
It retains exact format matching and cannot bypass reserved module suffix
validation.

An external `forma-deps.json` is decoded into the same ordered semantic action
model; its JSON provider objects normalize to the corresponding Tagged option
data.
A root cannot combine external actions with embedded `crate.*` actions. This
keeps development and single-file publication representations equivalent
without defining precedence or merge behavior.

## Removal of `@@manifest`

The `@@manifest` grammar, AST field, lowering, diagnostics, and resolver path
are removed without compatibility syntax. Historical RFC text remains intact.

Production modules continue to require explicit exports. Legacy tests that
treat a trailing module expression as an export are removed; the isolated
expression-evaluation harness remains available for compiler and VM tests but
is not a resolver-visible module form.

## Goals

1. provide one extensible static metadata mechanism for every Forma module;
2. preserve repeated actions and authored order without implicit overwrite;
3. make scope and consumer ownership visible in option keys;
4. keep resolver-affecting actions restricted to `@main` and pre-resolution;
5. replace embedded manifests with ordinary option actions;
6. retain precise source diagnostics for each action;
7. remove legacy trailing-expression export coverage from production modules.

## Non-goals

- allowing option values to reference bindings or imports;
- allowing options inside runtime blocks;
- making every unknown option operational;
- allowing dependency modules to extend the root dependency graph;
- implementing Git, GitHub, or network acquisition;
- defining merge or last-wins semantics;
- removing the isolated expression test harness.

## Acceptance criteria

1. repeated options with one key parse losslessly and retain source order;
2. options may occur anywhere among top-level declarations;
3. non-immediate values and block-local options are rejected;
4. options create no binding, type fact, runtime instruction, or export;
5. `crate.dependency` and `crate.format` are accepted only in `@main`;
6. embedded path dependencies and exact format overrides resolve through the
   new actions;
7. external `forma-deps.json` reaches the same normalized action consumer;
8. external configuration and embedded `crate.*` actions cannot coexist;
9. `@@manifest` is rejected and has no active implementation path;
10. production tests use explicit exports rather than trailing results;
11. formatting, workspace tests, and warning-denied Clippy pass.

## Implementation plan

1. add lossless option syntax and an AST `OptionAction` sequence;
2. validate immediate values and key shape during lowering;
3. normalize embedded and external resolver options into one consumer;
4. remove `@@manifest` and migrate active fixtures;
5. remove legacy production trailing-result export coverage;
6. expose option metadata to workspace/tooling snapshots;
7. record implementation results and mark this RFC Implemented.
