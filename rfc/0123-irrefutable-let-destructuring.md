# RFC 0123: Irrefutable `let` destructuring

- Status: Proposed
- Depends on: RFC 0120, RFC 0121, RFC 0122

## Summary

Forma lets a local `let` bind a Tuple or Struct pattern when that pattern is
proven irrefutable for the initializer's static type:

```forma
let (left, right) = pair;
let { name, address: { city } } = user;
render(name, city, left, right)
```

Plain `let name = value` retains its existing AST, inference, generalization,
and runtime path. Destructuring `let` is elaborated into a one-arm match whose
arm is marked as requiring irrefutability and whose body is the remainder of
the lexical block.

## Elaboration

Conceptually:

```forma
let { name } = user;
rest
```

becomes:

```forma
match user {
    { name } => { rest },
}
```

with an internal `irrefutable_required` marker on the arm. This is parser
elaboration, not user-observable match syntax. It guarantees that the
initializer is evaluated once, pattern bindings scope over exactly the
remaining block, shadowing follows existing match-arm rules, and compiler/HIR
selection uses the same operations as ordinary patterns.

Several destructuring lets nest in source order. Ordinary bindings before and
after a destructuring let remain in their corresponding outer or inner block,
so dependency and evaluation order do not change.

## Static semantics

The shared analysis must report the pattern irrefutable for the resolved
initializer type. Tuple arity must match exactly. Every Struct field must
exist, and every nested pattern must itself be irrefutable. Binding and
wildcard children are irrefutable; literal, Atom, and Tagged children are
refutable unless their exact singleton type already makes failure impossible.

The surface grammar initially admits only Tuple and Struct top-level patterns,
which keeps the intent obvious. Refutable nested patterns receive an error at
the smallest failing pattern rather than an implicit panic, Option, Result, or
hidden control-flow edge.

Any, Dyn, unresolved, and incompatible initializer shapes cannot prove a
structural pattern irrefutable and are rejected. Users first annotate, decode,
or match such values explicitly.

## Tooling and runtime

Pattern bindings are ordinary HIR pattern definitions with their selected
types, references, and source locations. Hover, definition lookup, completion,
and rename consumers therefore see the same facts as match-arm bindings.

Lowering reuses ordinary match pattern operations, including provenance-
preserving Tuple/field selection. The VM retains its defensive no-match path,
but a well-typed destructuring let cannot reach it. No new bytecode or runtime
value is introduced.

## Grammar

```text
let_pattern_binding:
  'let' (tuple_pattern | struct_pattern) '=' expression ';'
```

An annotation directly on a destructuring binder is deferred. Authors can
annotate the initializer through an ordinary preceding binding when needed.
Plain identifier lets retain their existing optional annotation.

## Acceptance criteria

1. Tuple and Struct destructuring lets parse, nest, and preserve source order;
2. the initializer is evaluated exactly once;
3. precise selected types flow to every pattern binding and semantic fact;
4. exact Tuple arity and known Struct fields are required;
5. nested refutable patterns are rejected at their authored location;
6. Any, Dyn, and unresolved shapes cannot justify structural destructuring;
7. shadowing and references follow existing sequential `let` scope;
8. selected values retain child provenance;
9. plain identifier `let` behavior and local closure generalization remain
   unchanged;
10. no new VM value, bytecode operation, exception, or implicit failure; and
11. full core, CLI, LSP, formatting, and strict static checks pass.

## Non-goals

- refutable `let`, `if let`, `let else`, or optional binding;
- Tagged or literal top-level destructuring binders;
- binder-level annotations in the initial syntax;
- polymorphic generalization of closures nested inside a destructured value;
- Array rest patterns or open-record capture; or
- exposing the internal elaboration marker in syntax or semantic APIs.

## Implementation result

Pending.
