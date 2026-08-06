# RFC 0132: Array spread

- Status: Implemented
- Depends on: RFC 0060

## Summary

Array literals accept `...expression` items:

```forma
let args = ["gcc", ...common_args, source];
```

The operand must have type `Array(T)` and contributes its elements at that
position. Ordinary and spread items are evaluated exactly once from left to
right. The resulting item type is inferred from both ordinary elements and the
element types of spread operands.

`...` is deliberately limited to collection-literal item positions. It is not
a general unary operator or function-call argument convention. `..` remains
available for a future range design.

## Implementation result

Implemented in the lexer, collection grammar, AST traversal, type inference,
compiler, bytecode, and VM. Literals without spread retain the existing direct
array construction path. A literal containing spread is compiled into ordered
array fragments followed by one internal concatenation operation.

Tests cover multiple and empty fragments, source-order flattening, nested array
elements, and rejection of non-Array operands.
