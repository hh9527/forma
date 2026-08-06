# RFC 0118: Expressive structural patterns

- Status: Proposed
- Depends on: RFC 0054, RFC 0067 through RFC 0079, RFC 0102, RFC 0103

## Summary

Forma develops its existing `match` expression into a statically checked,
structural way to consume the data that the language already constructs. The
phase adds Struct patterns, closed-Enum exhaustiveness and redundancy
diagnostics, and irrefutable destructuring in `let` bindings:

```forma
match result {
    'Ok({ name, age }) => render(name, age),
    'Err(error) => format_error(error),
}

let { bin, args, env } = exec;
```

Forma already supports literal, binding, wildcard, Tuple, Atom, and Tagged
patterns in `match`. RFC 0054 also propagates known Enum payload types into
Tagged payload bindings. This umbrella does not introduce pattern matching; it
completes the most valuable structural and static gaps in that existing model.

RFCs 0119 through 0123 define the shared pattern-checking foundation, Struct
patterns, exhaustiveness, redundancy diagnostics, and irrefutable `let`
destructuring. This RFC becomes Implemented only after those child RFCs are
implemented and its result is amended with acceptance evidence.

## Motivation

Forma's data model is more expressive than its current consumption syntax.
Struct values must be projected one field at a time, while closed Enums can be
matched but receive no diagnostic when a variant is forgotten. Nested
`Result`, `Option`, interpreter, codec, migration, and execution-plan code
therefore contains avoidable plumbing and can silently defer missing cases to
the runtime `NoPatternMatched` error.

Patterns should provide the elimination forms corresponding to Forma's
existing construction forms:

```text
Tuple construction  <-> Tuple pattern
Tagged construction <-> Tagged pattern
Struct construction <-> Struct pattern
closed Enum          <-> exhaustive variant coverage
```

This is a general expression improvement. It benefits ordinary transformations
and every existing application domain without adding effects, Host authority,
traits, or a new data model.

## Surface model

### Struct patterns

A Struct pattern names fields explicitly. Shorthand binds a field under its
own name; the long form applies a nested pattern:

```forma
match user {
    { name, address: { city } } => (name, city),
}
```

Conceptually, `{ name }` is shorthand for `{ name: name }`. Field order is not
significant. A Struct pattern tests and selects declared fields; it does not
convert the value to Dict and does not enumerate unknown fields.

The initial pattern is partial: omitted fields are ignored. No `..rest`
binding is added. This keeps structural selection separate from open-record
typing and unknown-field preservation.

### Exhaustive closed Enums

When the scrutinee has a known closed Enum descriptor, the checker requires
every possible variant to be covered, either directly or by a catch-all:

```forma
match option {
    'None => fallback,
    'Some(value) => use(value),
}
```

Atom patterns cover zero-payload variants. Tagged patterns cover variants with
one payload. A binding or wildcard covers every remaining variant. Nested
payloads matter: `'Some(value)` covers the whole `Some` variant, while
`'Some(1)` does not. The initial checker considers a payload variant covered
only when its payload pattern is proven irrefutable for that payload type. It
does not combine several refutable payload patterns to prove total coverage.

An `Any`, Dyn, unknown, or otherwise open scrutinee has no finite static domain.
It retains runtime matching behavior and requires no false claim of
exhaustiveness. Users must first decode or check dynamic data to obtain a
closed static type.

### Irrefutable `let` patterns

`let` accepts only patterns proven to match every value of the initializer's
static type:

```forma
let (left, right) = pair;
let { name, age } = user;
```

Literal, Atom, Tagged, and partial alternatives are refutable and remain
available only in `match`. Forma does not add an implicit panic, optional
binding, or hidden control-flow edge to `let`.

Plain `let name = value` remains the common case and retains shadowing and
inference behavior.

## Static pattern analysis

The checker gains one shared recursive analysis over a pattern, a scrutinee
type, and an authored location. It is responsible for:

- binding each variable once with its selected static type;
- rejecting patterns incompatible with a known scrutinee type;
- resolving Struct fields without degrading through Dict or Dyn;
- determining whether a pattern is irrefutable for a known type;
- computing conservative whole-variant coverage for closed Enums; and
- reporting unreachable or redundant arms where the conclusion is certain.

The analysis must be conservative. Unknown information suppresses a static
claim; it never invents a narrower type. This phase narrows bindings inside a
pattern and its arm only. It does not add flow-sensitive narrowing to bindings
after a `match` or condition.

Exhaustiveness is a compile-time property of the authored closed type, not a
VM fallback. The runtime retains a defensive no-match outcome for unchecked
or malformed internal input, but well-typed exhaustive Enum matches cannot
reach it.

## Runtime and provenance

Struct patterns lower to kind/field selection operations followed by the
existing recursive pattern machinery. They do not allocate a replacement
Struct or Dict. Bytecode verification checks field identities, registers, and
failure targets using the same trust boundary as Tuple and Tagged patterns.

Selection narrows structural provenance to the selected child, matching RFC
0102. A variable bound by a nested pattern therefore carries the same source
origin as direct field, Tuple-element, or Tagged-payload selection. A failed
pattern test produces no value and does not fabricate provenance.

In best-effort evaluation, a Never scrutinee follows RFC 0103: no arm is
speculated and no pattern binding is created.

## Diagnostics and tooling

Diagnostics point to the smallest useful authored location:

- an unknown or incompatible Struct field points to that field pattern;
- duplicate bindings point to the repeated binding;
- a non-exhaustive Enum match names the missing variants at the `match`;
- an unreachable arm points to its pattern and identifies the prior coverage;
- a refutable `let` pattern explains which shape can fail; and
- an initializer incompatible with a destructuring pattern shows both the
  initializer type and expected structural shape.

Workspace facts, hover, and definition lookup treat pattern bindings like
ordinary lexical bindings and attach their inferred field or payload types.
Cancellation remains checked during pattern analysis and workspace queries.

## Phase sequence

1. **RFC 0119: Typed pattern analysis.** Consolidate recursive compatibility,
   binding, irrefutability, finite-domain coverage, diagnostics, and semantic
   facts without adding syntax.
2. **RFC 0120: Struct patterns.** Add shorthand and nested Struct patterns,
   typed field selection, lowering, VM support, and structural provenance.
3. **RFC 0121: Closed-Enum exhaustiveness.** Require complete coverage for
   statically known Enums and define Atom, irrefutable Tagged payload, and
   catch-all coverage precisely.
4. **RFC 0122: Redundant pattern diagnostics.** Diagnose certainly unreachable
   arms and duplicate finite coverage without claiming general theorem proving.
5. **RFC 0123: Irrefutable `let` destructuring.** Generalize `let` binders to
   proven-irrefutable Tuple and Struct patterns and integrate inference,
   shadowing, workspace facts, and diagnostics.

Each child RFC is proposed and implemented independently. A child may refine
internal representation choices, but it must preserve this umbrella's surface
boundary and stopping rules.

## Goals

1. make Struct, Tuple, Atom, and Tagged values pleasant to consume recursively;
2. make missing closed-Enum variants a static, source-positioned error;
3. infer precise field and payload types for nested bindings;
4. reject certainly redundant match arms;
5. support concise destructuring without introducing partial `let` semantics;
6. preserve source provenance through every structural selection;
7. expose pattern bindings consistently to semantic tooling; and
8. keep the implementation conservative under unknown or dynamic types.

## Non-goals

- guards, or-patterns, range patterns, regex patterns, or view patterns;
- Array rest patterns or variable-length sequence matching;
- `..rest` capture, row polymorphism, open Struct types, or unknown-field
  preservation;
- flow-sensitive narrowing outside the selected match arm;
- matching Dyn directly as if it had a closed static shape;
- refutable `let`, implicit panic, exceptions, or optional binding;
- nominal Enum types, interfaces, traits, or associated types;
- user-defined pattern extractors or active patterns; or
- changing Tagged payload arity or unifying Array and Tuple.

## Shared acceptance criteria

1. existing match syntax and successful behavior remain compatible;
2. Struct patterns recursively bind declared fields at their precise types;
3. omitted Struct fields are ignored without creating an open-record type;
4. known closed Enums require every unit variant and every payload variant to
   have whole-variant coverage;
5. wildcard and plain binding patterns provide explicit catch-all coverage;
6. certainly unreachable arms receive stable source-positioned diagnostics;
7. unknown and dynamic scrutinees do not receive unsound coverage claims;
8. `let` accepts only patterns proven irrefutable for the initializer type;
9. nested pattern bindings retain child provenance and semantic facts;
10. duplicate bindings and incompatible nested patterns are rejected locally;
11. best-effort Never propagation and cancellation remain deterministic; and
12. full core, CLI, LSP, formatting, and strict static checks pass.

## Stopping rules

Work returns to discussion if a child RFC requires:

1. row variables, row unification, or open-record subtyping;
2. a general flow-sensitive type refinement engine;
3. arbitrary predicates or user code during pattern matching;
4. effects, exceptions, implicit failure, or speculative arm evaluation;
5. changing Enum, Tagged, Struct, Tuple, Any, or Dyn value representation;
6. combining refutable nested patterns into a general pattern-matrix
   usefulness proof;
7. making provenance affect equality, hashing, serialization, or types; or
8. weakening malformed-bytecode or runtime invariant handling into ordinary
   pattern failure.

## Alternatives considered

### Add only Struct getter shorthand

Field access sugar helps one expression at a time but does not compose with
Tagged payloads, nested data, binding scopes, exhaustiveness, or tooling. A
single recursive pattern model gives these features one static foundation.

### Make every `let` pattern refutable

An implicit runtime failure makes ordinary binding partial and obscures control
flow. Returning Option or Result changes the meaning of `let`. Explicit
`match` already represents refutable selection; `let` should remain total.

### Introduce row polymorphism with Struct patterns

Partial Struct patterns do not require an open Struct type. They only select
known fields from the initializer's known descriptor. Row polymorphism may be
valuable later, but coupling it to basic destructuring would turn an
expression improvement into a substantially deeper type-system project.

### Require a catch-all for every match

That is safe but discards the strongest value of a closed Enum: the compiler
knows its finite variant set and can tell the author exactly what changed.
Catch-alls remain available for intentionally grouped behavior and open input.
