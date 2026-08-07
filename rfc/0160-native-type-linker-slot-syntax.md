# RFC 0160: Native type linker slot syntax

- Status: Proposed
- Depends on: RFC 0116, RFC 0134

## Summary

Native type declarations attach their module-local linker slot directly to the
declared name:

```forma
native type State @3;
```

The previous spelling is removed without a compatibility form:

```forma
native type State = @3;
```

## Motivation

`@3` is not a Forma expression that computes a type. It is a declaration
attribute consumed while a privileged module is linked to its Host registry.
Writing it after `=` incorrectly presents the slot as a value-level right-hand
side and makes a native type look like an ordinary type binding.

Putting the slot between the name and terminator makes the declaration model
explicit and leaves room for one consistent distinction:

- ordinary type declarations define metadata from a Forma expression;
- native type declarations name metadata supplied by the Host at a stable
  module-local slot.

## Syntax

The grammar is:

```text
native-type-binding := "native" "type" identifier "@" integer ";"
```

The slot must fit in `u32`. Slot uniqueness remains scoped to one registered
native module, and declaration order does not affect identity.

## Semantics

This RFC changes only surface syntax. Existing authority checks, registry
lookup, duplicate-slot diagnostics, exported TypeMetadata, and runtime opaque
value checks are unchanged.

In particular, this RFC does not yet expose kernel primary types such as `Int`
through native declarations. A later RFC may allow a slot to link either an
opaque native type or an existing kernel TypeMetadata value. That requires a
registry contract beyond this syntax correction.

## Goals

1. represent the slot as declaration metadata rather than an expression;
2. retain stable module-local native type identity;
3. reject the misleading historical spelling;
4. keep parser, CST, diagnostics, and embedded modules aligned.

## Non-goals

- assigning slots to ordinary `@struct` or `@enum` declarations;
- changing native module authority;
- defining kernel primary type slots;
- introducing a general annotation syntax;
- retaining source compatibility.

## Acceptance criteria

1. `native type State @3;` parses and lowers to native type slot 3;
2. `native type State = @3;` is rejected;
3. duplicate and overflowing slots retain precise diagnostics;
4. all current embedded modules and non-historical fixtures use the new form;
5. formatting, workspace tests, and warning-denied Clippy pass.

## Implementation plan

1. update the Forma grammar and generated parser;
2. migrate embedded native type declarations and test fixtures;
3. add positive and negative syntax coverage;
4. record implementation results and mark this RFC Implemented.
