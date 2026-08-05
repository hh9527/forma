# RFC 0097: Semantic `interpreter` expression

- Status: Implemented
- Depends on: RFC 0093, RFC 0096

## Summary

Forma preserves `interpreter(operand)` as a distinct AST expression through
name resolution and type analysis. The node retains the authored operand for
references, diagnostics, and tooling, while carrying a compiler-only ordinary
expression elaboration used after semantic validation.

This RFC moves the RFC 0093 contract audit out of parser shape matching and
into contextual type analysis. It deliberately preserves RFC 0093's accepted
shape and runtime behavior; parameter-wise generalization follows in RFC 0098.

## Motivation

RFC 0093 parses `interpreter` and immediately replaces it with generated
closures. It also validates the surrounding definition contract with syntactic
AST matching in the parser. Later phases therefore cannot distinguish authored
syntax from generated code, and diagnostics risk exposing generated structure.

RFC 0096 needs semantic type descriptors to classify witnesses and parameter
positions. Extending parser matching would duplicate type semantics and make
the parser responsible for ABI derivation. The language construct must remain
visible until its expected scheme is known.

## Representation

The AST adds an `Interpreter` expression containing:

1. the authored operand expression; and
2. a compiler-owned elaboration into ordinary closures, calls, and Dyn packs.

Forma currently has no separate typed-HIR lowering stage: both analysis and
compilation consume the AST. Keeping both views in one node is a bounded bridge,
not a new public dual semantics. The views have strict ownership:

- HIR resolution, references, source navigation, and user diagnostics traverse
  the authored operand;
- contextual typing validates the interpreter node and its elaboration;
- bytecode compilation consumes only the validated elaboration; and
- source formatting and CST recovery remain based on authored syntax.

Generated identifiers remain unspellable and carry the interpreter source
location. They are never indexed as authored definitions or references.

## Contextual validation

In this RFC the accepted context remains exactly:

```text
for(A) Fn(TypeOf(A)) -> Fn(A, A) -> R
```

where `R` does not contain `A`. The interpreter must be the direct initializer
of a `def` with an explicit contract. Type analysis validates this invariant
from the evaluated definition scheme, checks the elaboration against that
scheme, and checks the authored operand as:

```text
Fn(Dyn, Dyn) -> R
```

The parser only recognizes structure and constructs the AST node. It does not
decide whether a contract is a valid interpreter scheme.

Use outside an explicitly contracted definition remains rejected. The
diagnostic points at `interpreter` and describes the required contextual shape;
operand type errors point at the operand and name its expected erased ABI.

## Analysis and compilation

Strict and partial analysis both traverse the authored operand so dependency,
reference, cancellation, and stale-publication behavior remains complete.
Expression type facts record the authored scheme at the interpreter location
and the erased Function type at the operand location.

Compilation is permitted only with successful strict analysis. It compiles the
ordinary elaboration already associated with the node and adds no bytecode
operation. Metadata evaluation follows the same rule. A compiler encounter
without corresponding successful analysis is an internal invariant violation,
not a dynamic fallback.

## Diagnostics and tooling

The migration must preserve:

- the `interpreter` source span as the primary invalid-context span;
- the operand's real references and hover information;
- the authored generic definition scheme in module interfaces and hover;
- no generated closure parameters in HIR, diagnostics, or navigation; and
- cancellation checks in ordinary analysis and compilation paths.

Recoverable parsing retains an interpreter node whenever the operand is
recoverable. An incomplete operand remains ordinary missing/invalid syntax and
does not trigger compiler elaboration.

## Goals

1. retain `interpreter` as authored semantic structure;
2. move contract validation out of parser syntax matching;
3. give the operand an explicit compiler-derived expected ABI;
4. keep generated adapter details out of HIR and tooling;
5. preserve RFC 0093 behavior and runtime representation; and
6. establish the implementation point generalized by RFC 0098.

## Non-goals

- accepting any shape beyond RFC 0093;
- adding a general typed-HIR or compiler IR redesign;
- exposing elaborations through public AST or tooling APIs;
- changing Dyn, TypeDesc, witness inference, or runtime behavior;
- supporting interpreter expressions without explicit definition schemes; or
- adding an interpreter opcode, callable, registry, or fallback.

## Acceptance criteria

1. parser output retains a distinct Interpreter node and authored operand;
2. parser construction no longer performs the RFC 0093 contract audit;
3. semantic analysis accepts the existing equality shape and rejects malformed
   contexts with source-level interpreter diagnostics;
4. operand arity and result mismatches report against the authored operand;
5. HIR indexes operand references but no generated identifier;
6. expression facts and module interfaces expose authored types;
7. existing explicit and inferred equality interpreter calls still execute;
8. recoverable syntax, cancellation, quota, and stale publication do not regress;
9. bytecode and VM gain no interpreter-specific operation; and
10. full Forma, CLI, LSP, formatting, and strict Clippy checks pass.

## Implementation plan

1. add the Interpreter AST variant and parser construction;
2. teach all AST traversals to select the correct authored or elaborated view;
3. validate the contextual scheme and erased operand in type analysis;
4. compile only the validated ordinary elaboration;
5. add parser, HIR, type, execution, recovery, and diagnostic regressions; and
6. run the full quality gate and record the implementation result.

## Implementation result

Implemented a distinct `ExprKind::Interpreter` retaining both the authored
operand and a compiler-only ordinary elaboration. The parser now recognizes
and preserves the construct without auditing its generic contract. HIR indexes
only the authored operand, while capture/runtime-name collection and bytecode
compilation consume the elaboration; generated parameters and the hidden Dyn
pack binding therefore do not appear as authored references.

Strict type analysis evaluates definition contracts and validates the RFC 0093
scheme before unresolved operand diagnostics, so missing and malformed contexts
retain dedicated interpreter errors. Bidirectional inference checks the ordinary
elaboration against the authored expected Function, preserving existing operand
arity/result diagnostics and execution. Partial expression recording and source
validation understand the new node without introducing an opcode or runtime
callable.

Regression coverage proves AST retention, authored-only HIR references, valid
explicit and inferred calls, erased operand mismatch, and invalid contextual
shapes. Full Forma tests pass with 290 passed and 1 ignored; all 13 CLI tests
pass. Workspace LSP tests and strict Clippy also pass.
