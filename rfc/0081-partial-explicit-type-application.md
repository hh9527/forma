# RFC 0081: Partial explicit type application

- Status: Implemented
- Depends on: RFC 0018, RFC 0052, RFC 0070, RFC 0077, RFC 0079, RFC 0080
- Tracking issue: https://github.com/hh9527/forma/issues/1

## Summary

Explicit type application accepts `_` in any positional argument to request
ordinary inference for that scheme parameter:

```forma
pair[Int, _](1, "value")
empty[_]()
```

An explicit TypeMetadata argument remains rigid. `_` creates a fresh inference
obligation that may be solved by value arguments, callbacks, structural
context, or an expected result. It is not `Any`, not `Never`, not a runtime
value, and not a general expression hole.

The source still supplies exactly one argument position per scheme parameter.
This RFC adds partial explicitness, not partial arity or named type arguments.

## Motivation

RFC 0077 supports either complete inference:

```forma
pair(1, "value")
```

or complete explicit application:

```forma
pair[Int, String](1, "value")
```

Real calls often need control over only one position. A result-only parameter
may be intentionally selected while input parameters remain obvious, or one
ambiguous empty constructor may need a hint without repeating every related
type. Requiring all arguments makes explicit application noisy and couples
source to unrelated scheme parameters.

`_` already communicates an intentionally omitted position in Forma's call
section syntax, but RFC 0018 confines its runtime meaning to `\(...)`. Inside
type arguments, the owner is equally unambiguous and the meaning is static:
infer this one parameter.

## Syntax

Type arguments become:

```text
type_application := expression '['
                    type_argument (',' type_argument)* [','] ']';

type_argument := expression | '_';
```

Examples:

```forma
pair[Int, _]
pair[_, String]
pair[_, _]
module.decode[User, _]
```

`_` is accepted only as a direct child of `TypeArguments`. It remains invalid
as an ordinary expression or inside a nested metadata expression:

```forma
pair[Array(_), String] # invalid
let value = _;         # invalid
```

Indexed call-section placeholders such as `_0` are not type arguments. Type
parameters are already positional and each `_` owns exactly one position, so
reuse and permutation have no meaning here.

## AST and source identity

The AST represents each type argument explicitly as either:

```text
Explicit(Expr)
Infer(Location)
```

An inferred argument is not lowered to a synthetic metadata expression. Its
source location remains available for diagnostics, HIR containment, semantic
facts, and cancellation, but it creates no reference, runtime value, or tool
evaluation request.

Lossless CST preserves the existing dedicated `Placeholder` token. Parser
recovery may retain a missing or damaged type argument, but the authoritative
AST never disguises it as `_` or `Any`.

## Instantiation

The callee resolution and exact argument count rules from RFC 0077 remain
unchanged. Given:

```text
pair: for(A, B) Fn(A, B) -> (A, B)
pair[Int, _]
```

the checker constructs one replacement map:

```text
A -> Int
B -> ?B
```

and substitutes both positions into the scheme body:

```text
Fn(Int, ?B) -> (Int, ?B)
```

Every `_` creates a distinct fresh inference variable, even if two parameters
have the same presentation name in malformed recovered input. Repeated
occurrences of one bound parameter in the scheme body receive the same
replacement.

Explicit arguments are evaluated as TypeMetadata under RFC 0077. Inferred
arguments perform no tool-stage evaluation and consume no evaluation fuel or
allocation quota. Traversal still retains ordinary query cancellation
checkpoints.

## Evidence and checking

Placeholder variables use the same directional checker and substitution state
as fully inferred generic calls. They may be solved by:

- ordinary call arguments;
- expected callback shapes;
- structural literals;
- an enclosing annotation or expected result; and
- another occurrence of the same scheme parameter after substitution.

Examples:

```forma
pair[Int, _](1, "value") # B = String

let values: Array(Int) = empty[_](); # A = Int from expected result
```

An explicit argument is rigid and conflicts normally:

```forma
identity[Int]("value") # String is not assignable to Int
```

`Never` supplies no evidence for `_`. Passing `Any` through an explicit dynamic
boundary follows existing erasure rules; the placeholder itself never means
`Any`.

## Completion boundary

A placeholder obligation may remain unresolved through the type application
and its enclosing call so later arguments and expected results can constrain
it. It must resolve before the containing lexical block completes.

```forma
empty[_]() # error without an expected Array item type
```

No unresolved placeholder reaches binding facts, expression facts, hover,
module interfaces, or the final type graph. Underconstrained placeholders
produce a dedicated diagnostic that identifies the argument position and
scheme parameter. RFC 0084 may enrich its labels, but the distinction from a
generic-result failure is part of this RFC.

## Explicit application of inferred schemes

RFC 0079 inferred schemes use the same path:

```forma
let pair = fn(left, right) { (left, right) };
pair[Int, _](1, "value")
```

Semantic parameter identity, not the presentation name `A` or `B`, selects the
replacement. Aliases remain monomorphic and therefore cannot accept explicit
or placeholder type arguments.

## Tooling

Hover on a completed application reports its instantiated monomorphic type.
Explicit metadata argument expressions retain their `TypeOf(T)` facts. `_`
reports the inferred concrete descriptor after completion and has no
definition target.

Navigation through the callee remains unchanged. Formatting, recovery, and
semantic containment treat `_` as one source argument rather than a synthetic
expression tree.

## Runtime behavior

Type application continues to compile exactly its callee. `_` adds no register,
argument, capture, instruction, runtime metadata, or specialization. A call
following partial type application has the same runtime arity and closure
identity as complete application or inference.

## Goals

1. accept `_` in any direct type-argument position;
2. preserve exact positional arity;
3. combine rigid explicit arguments with fresh inference obligations;
4. solve placeholders from ordinary argument and expected-result evidence;
5. distinguish `_` from `Any`, `Never`, and missing syntax;
6. support declared, inferred local, core, and imported schemes;
7. reject unresolved placeholders at a finite lexical boundary;
8. publish only completed monomorphic facts;
9. preserve source identity, navigation, cancellation, and recovery;
10. erase placeholders and all type application at runtime.

## Non-goals

- omitted trailing type arguments;
- named type arguments;
- default type arguments;
- indexed type placeholders;
- general expression holes;
- first-class partial schemes;
- higher-rank or impredicative application;
- constrained parameters, traits, or associated types;
- numeric defaulting;
- runtime type arguments or specialization.

## Implementation plan

1. add a lossless direct type-argument alternative for `Placeholder`;
2. represent explicit and inferred arguments distinctly in the AST;
3. update HIR, free-variable, compiler, interpolation, and annotation traversals;
4. evaluate only explicit TypeMetadata expressions;
5. construct one mixed rigid/fresh replacement map;
6. retain placeholder ownership through the enclosing lexical block;
7. reject unresolved placeholders before fact publication;
8. record completed placeholder and application facts;
9. add parser, local, inferred-scheme, imported, expected-result, repeated-body,
   all-placeholder, unresolved, conflict, `Never`, quota, cancellation, hover,
   and runtime-erasure tests;
10. run full workspace tests and strict static checks.

## Acceptance criteria

1. `pair[Int, _](1, "value")` infers `(Int, String)`;
2. `pair[_, String](1, "value")` infers the same result;
3. `pair[_, _]` creates independent parameter obligations;
4. an expected result solves a result-only placeholder;
5. inferred local and imported schemes support placeholders;
6. repeated bound occurrences share one placeholder solution;
7. rigid explicit arguments still reject conflicting values;
8. `Never` does not solve a placeholder;
9. an evidence-free placeholder fails distinctly from explicit `Any`;
10. missing, extra, nested, and indexed placeholders are rejected;
11. only explicit arguments run TypeMetadata evaluation;
12. hover shows the completed application and placeholder types;
13. no unresolved placeholder reaches a published fact or interface;
14. bytecode and runtime call arity are unchanged;
15. cancellation publishes no provisional substitution;
16. workspace tests and strict static checks pass.

## Deferred work

- omitted trailing arguments and defaults;
- named generic arguments;
- richer placeholder diagnostics under RFC 0084;
- constrained generic parameters;
- higher-rank type application.

## Rejected alternatives

### Treat `_` as `Any`

That would erase the relationship between the scheme parameter, value
arguments, and result. `_` requests inference; `Any` requests a dynamic
boundary.

### Lower `_` to an ordinary expression

There is no runtime or tool-stage value denoted by `_`. A synthetic expression
would create misleading HIR references, evaluation behavior, and semantic
facts.

### Permit fewer positional arguments

An exact source position for every parameter makes intent visible and keeps
arity diagnostics independent of inference. Omitted trailing arguments can be
considered separately if they prove materially clearer.

### Reuse indexed call-section placeholders

Type argument positions already identify distinct scheme parameters. `_0`
would misleadingly suggest that two different parameters can share one
inference identity without a relationship in the declared scheme.

## Implementation result

Implemented in the Forma parser, AST, HIR, module traversal, and rank-1
inference engine. Direct `_` arguments retain their source identity without
becoming metadata expressions or runtime values. The checker installs a fresh
variable in the same substitution map as rigid arguments, records the resolved
descriptor as an expression fact, and diagnoses any remaining obligation at
the placeholder location before semantic facts or module interfaces are
published.

The implementation deliberately performs the unresolved-placeholder check at
the completed program-analysis boundary. Expected types have therefore had a
chance to flow through the containing lexical block, while an unresolved
placeholder still cannot escape into a published result. Calls whose callee is
a partial type application defer the older generic-result diagnostic so the
more specific placeholder diagnostic remains authoritative.

Regression coverage includes direct-only syntax, all rigid/inferred mixes,
expected-result evidence, local inferred schemes, declared and inferred
schemes crossing module interfaces, concrete HIR expression facts, `Never`,
explicit `Any`, conflicts, unresolved source locations, and execution of
erased imported applications. Existing query cancellation checkpoints and the
compiler's callee-only lowering for every `TypeApply` remain unchanged.
