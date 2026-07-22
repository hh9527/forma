# RFC 0001: Runtime Values and Bytecode VM

- Status: Accepted for MVP
- Implementation: Complete

## Summary

This RFC defines XL's initial runtime value model and a small register-based
bytecode VM. It deliberately accepts hand-built bytecode as its only input;
surface parsing and compilation belong to RFC 0002.

## Motivation

The dynamic runtime is the semantic foundation shared by program-stage and
tool-stage evaluation. It must be deterministic, small enough to understand,
and capable of representing immutable structured data without depending on the
future type checker.

Establishing the VM before the surface language prevents parser conveniences
from accidentally defining runtime semantics.

## Runtime values

The MVP runtime exposes these value categories:

```text
Int(i64)
Float(f64)
String
Bytes
Dict
Array
Atom
Tuple
Func
```

Values are immutable at the language boundary. The Rust implementation may use
reference counting and private mutation that cannot be observed by XL code.

### Integers and floats

Integer arithmetic is checked. Overflow and division by zero produce runtime
errors in every build profile. Float operations follow Rust `f64`/IEEE 754
behavior. Arithmetic does not implicitly mix integers and floats.

### Atoms

The following built-in atoms have stable runtime identities:

```text
'None, 'Some, 'Ok, 'Err, 'True, 'False
```

Other atoms contain an immutable symbolic name. VM control-flow instructions
accept only `'True` and `'False` as conditions.

### Arrays and tuples

Arrays and tuples contain immutable ordered values. They are distinct runtime
categories even though both may initially use shared slices internally.
Tagged sum values are ordinary tuples whose first element is an atom.

### Dicts and shapes

A Dict is represented conceptually as:

```text
Dict {
    shape: Shape,
    values: [Value],
}
```

`Shape.fields` is a sorted, duplicate-free array of UTF-8 strings. Sorting uses
Rust string ordering, which is lexicographic UTF-8 byte ordering. Values align
with the canonical field order. Source expression order cannot be observed.

The VM interns equal shapes, but shape identity is not observable. Field lookup
uses binary search in the MVP. Dict equality compares canonical fields and
values.

### Functions

A `Func` initially references a bytecode function. Calls and captured lexical
environments are specified by RFC 0002. Function equality is not defined.

## Bytecode model

Each bytecode function owns:

- a name used for diagnostics;
- a constant pool;
- an instruction sequence;
- a declared register count.

Instructions address registers by a compact integer index. RFC 0001 includes:

```text
LoadConst, Move
Add, Subtract, Multiply, Divide, Negate
Equal, LessThan
MakeArray, MakeTuple, MakeDict, GetField
Jump, JumpIfFalse
Return
```

Bytecode construction validates constant and register indexes before execution
where practical. Malformed bytecode must return an error rather than panic.

`Equal` compares scalar values by value and Array, Tuple, and Dict recursively
by structure. Numeric equality does not coerce Int and Float.

Functions use opaque identity equality. Reusing one closure value compares
equal; evaluating another closure construction compares unequal even when its
prototype and captures have the same contents. Function identity has no
numeric or textual representation and cannot be ordered. Runtime storage
changes, including local-to-persistent publication, must preserve it. A Func
inside a structured value participates in recursive equality by this same
identity rule.

The runtime may use equal handles as a fast path, but handle numbers and heap
ownership are not themselves language-visible identity.

## Execution budget

The VM receives an instruction budget. Every dispatched instruction consumes
one unit. Exhaustion returns a distinct error. This mechanism will bound
tool-stage metadata computation in RFC 0003 and is equally available to program
execution.

## Errors

Runtime errors are Rust values containing:

- an error kind suitable for programmatic tests;
- a human-readable message;
- the bytecode function name and instruction offset when available.

The MVP does not expose stack traces because calls are deferred to RFC 0002.

## Deferred work

- source spans and source-level stack traces;
- calls, closures, and tail calls;
- garbage collection beyond Rust-owned immutable graphs;
- persistent tree updates for large Dicts and Arrays;
- bytecode serialization and verification as an untrusted input format;
- specialized tagged-value instructions;
- observable effects and native extension APIs.

## Implementation plan

Create a workspace crate named `xl` with public modules for values, bytecode,
and the VM. Keep the crate dependency-free. Direct Rust tests construct
bytecode functions and execute them.

## Acceptance criteria

1. All native value categories can be constructed and formatted.
2. Dict fields are canonical regardless of insertion order, and equal shapes
   are shared within a VM.
3. Checked integer arithmetic reports overflow and division by zero.
4. Branching rejects conditions other than the two built-in Boolean atoms.
5. Arrays, tuples, tagged tuples, Dict construction, and field access execute
   through bytecode.
6. Instruction budget exhaustion is deterministic.
7. Malformed register, constant, and jump indexes return errors without panic.
8. `cargo test --workspace` passes.

## Implementation result

Implemented in the `xl` crate. The acceptance suite passes under
`cargo test --workspace`, and the implementation is warning-free under
`cargo clippy --workspace --all-targets -- -D warnings`.
