# RFC 0098: Parameter-wise interpreter adapters

- Status: Proposed
- Depends on: RFC 0096, RFC 0097

## Summary

Forma generalizes the semantic `interpreter` expression to derive an erased
operand ABI and ordinary adapter from each parameter of its explicit expected
scheme:

```forma
def capability:
    for(A, B) Fn(TypeOf(A), TypeOf(B)) ->
        Fn(String, A, Bool, B, A) -> Result(String, BlameError) =
    interpreter(erased);
```

The operand is checked as:

```text
Fn(String, Dyn, Bool, Dyn, Dyn) -> Result(String, BlameError)
```

Direct `A`/`B` inputs are packed with their unique witnesses. Inputs independent
of interpreted parameters retain their exact type and value.

## Witness plan

For a scheme with quantified parameters `T0 ... Tn`, the outer Function must
contain exactly one parameter `TypeOf(Tj)` for every interpreted `Tj`. Each outer
parameter therefore establishes:

```text
Tj -> (witness parameter index, TypeOf(Tj))
```

Witness order is not semantically significant. Duplicate witnesses and a
directly used type parameter without a witness are rejected. A witnessed type
parameter may occur zero times in the inner Function; this is a valid explicit
metadata-only capability rather than an implicit or ambiguous quantifier.

This phase requires every quantified parameter to be witnessed. It does not
mix unrelated generic quantification into an interpreter factory.

## Parameter classification

For each inner parameter `Pi`, semantic analysis uses the evaluated descriptor:

1. if `Pi` is exactly `Bound(Tj)`, classify it as `Pack(witness(Tj))` and place
   `Dyn` at the same erased ABI position;
2. if `Pi` contains no interpreted bound parameter, classify it as
   `PassThrough(Pi)` and preserve its exact descriptor; or
3. otherwise reject it as a nested interpreted parameter.

Repeated `Bound(Tj)` positions each receive a pack operation using the same
witness. Pass-through values are never converted to `Any` or `Dyn`.

The result descriptor must contain no interpreted bound parameter. The rule is
structural and includes Array, Dict, TypeOf, Tagged, Tuple, Struct, Enum, Union,
and Function positions.

## Derived operand ABI

Given the classifications, the compiler derives exactly one Function type:

```text
Fn(erased(P0), ..., erased(Pm)) -> R
```

and checks the authored operand against it. The expected ABI is compiler-owned;
source code neither annotates erasure positions nor supplies an adapter plan.
Diagnostics display this derived Function and point to the authored operand.

The outer and inner adapter closures preserve source parameter order. The
adapter calls the operand once with the same number and order of values, adding
only invariant-preserving packs at `Pack` positions.

## Context and elaboration

`interpreter` remains valid only as the direct initializer of an explicitly
contracted generic `def`. Semantic validation derives the authoritative witness
and parameter plan from evaluated TypeDescriptors. The compiler-only AST
elaboration introduced by RFC 0097 is built from the same authored contract and
must agree with that plan before it can be compiled.

This agreement check is required by the current direct-AST compiler
architecture. A later typed-HIR may store the plan directly; that migration
does not change language semantics.

## Diagnostics

Dedicated diagnostics distinguish:

- outer contract is not `Fn(TypeOf(...), ...) -> Fn(...) -> R`;
- quantified parameter has no witness;
- quantified parameter has more than one witness;
- inner parameter contains an interpreted type below its root;
- result contains an interpreted type; and
- operand is incompatible with the derived erased ABI.

Messages use source type-parameter names and one-based parameter positions.
Generated adapter names are never displayed.

## Goals

1. support arbitrary direct interpreted input arity and position;
2. support multiple explicit witnesses and repeated uses;
3. preserve independent parameters exactly;
4. derive one statically checked erased ABI;
5. retain ordinary closure, call, and Dyn-pack execution; and
6. keep the adapter operation-neutral.

## Non-goals

- nested `F(A)` parameters or descriptor derivation;
- callback bridging;
- results containing interpreted parameters;
- unrelated unwitnessed generic parameters;
- additional returned closure layers;
- implicit witnesses, traits, capability lookup, or specialization; or
- new VM instructions or dynamic casts.

## Acceptance criteria

1. unary direct input derives `Fn(Dyn) -> R`;
2. direct and closed inputs may be interleaved in either order;
3. multiple interpreted parameters use their matching witnesses;
4. repeated direct positions reuse one witness;
5. a metadata-only witnessed parameter is accepted;
6. pass-through parameters retain exact static types in operand checking;
7. missing/duplicate witnesses, nested inputs, and interpreted results fail with
   dedicated diagnostics;
8. operand arity, parameter, and result mismatches name the derived ABI;
9. existing RFC 0093 equality adapters remain source and behavior compatible;
10. tooling exposes authored schemes and operands only;
11. bytecode and VM add no interpreter-specific operation; and
12. full workspace tests and strict Clippy pass.

## Implementation plan

1. model witness and inner-parameter plans in semantic validation;
2. generalize hygienic adapter elaboration by witness/parameter position;
3. derive and enforce the operand Function descriptor;
4. add positive tests for unary, mixed, repeated, multiple, and metadata-only
   shapes;
5. add negative tests for every rejected boundary and diagnostic; and
6. run the quality gate and record the implementation result.

