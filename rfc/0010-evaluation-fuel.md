# RFC 0010: Evaluation Fuel for Dynamic Control Flow

- Status: Accepted
- Implementation: Pending

## Summary

This RFC replaces XL's per-instruction budget with evaluation fuel charged only
when execution can expand its dynamic path: function calls and taken control-
flow back edges. Linear instructions, forward branches, returns, and program
entry do not consume fuel.

Evaluation fuel is a termination and dynamic-evaluation bound. It is not a
virtual CPU price and does not attempt to charge for bytes copied, collection
items traversed, allocation size, parsing, or native implementation details.
Call depth and XL stack slots remain independently bounded by RFC 0009.

## Motivation

The initial VM charged every dispatched instruction. That model is simple, but
it makes harmless compiler lowering choices observable through resource
behavior: instruction fusion, register moves, debug-preserving operations, and
future opcode encoding all change the amount of work a program is permitted to
perform.

In finite bytecode, a path containing no calls and no edge to an already
executed or current PC must terminate. Calls and control-flow back edges are the
operations that can make the dynamic path unbounded. Charging those operations
directly gives XL a stable evaluation model without maintaining a virtual CPU
tariff for every opcode and native data operation.

## Terminology

This RFC defines direction by numeric program counters, avoiding ambiguity in
the words "forward" and "backward":

```text
linear successor:  target == pc + 1
forward edge:      target > pc
back edge:         target <= pc
```

An edge to the current instruction is a back edge. It must consume fuel because
an instruction that jumps to itself can otherwise execute forever.

## Fuel rules

The VM applies exactly these charges:

```text
program entry                         0
ordinary instruction with PC + 1      0
untaken conditional branch            0
taken branch to target > pc            0
taken branch to target <= pc           1
bytecode function call                 1
native function call                   1
future tail call                       1
return                                 0
```

Both unconditional and conditional jumps use the same target rule. A
conditional edge is charged only when it is taken. A call is charged before
arity validation or frame/native-window creation once the callee has been
resolved as a function. Bytecode and native calls consume the same amount and a
native call is not charged a second time for dispatch.

Fuel is shared by the complete execution, including nested bytecode calls,
native calls, and tool-stage evaluation. It is not reset per frame or module.

## Exhaustion

Fuel is an unsigned count supplied at the execution boundary. A charge follows
this operation:

```text
if remaining == 0:
    fail with FuelExhausted at the charging instruction
else:
    remaining -= 1
```

Consequently, `fuel = 0` can execute finite straight-line bytecode and forward
control flow, but fails at the first call or taken back edge. Failure retains
the charging instruction's debug origin and normal frame trace.

The runtime error kind is named `FuelExhausted`, and diagnostics say
"evaluation fuel exhausted". Public API parameter names use
`evaluation_fuel`. Compatibility is positional for existing Rust callers; the
semantic change is intentional.

## Native functions

A native call consumes one unit, regardless of the callback's internal Rust
implementation. `CallContext` does not expose a general `charge(n)` method.
Native functions therefore cannot make implementation optimizations observable
through fuel accounting.

A native callback that re-enters XL pays for the resulting call normally.
Native callbacks must terminate on their own and may only operate within the
runtime's structural and input boundaries. Expensive or superlinear native
operations may define dedicated semantic limits, but those limits are not
evaluation fuel.

In particular, validation of a large already-admitted data value consumes one
call unit rather than one unit per node. Bounding JSON/YAML/TOML bytes, parsed
nodes, collection lengths, and similar external-data dimensions belongs to a
future input-limits RFC. Parsing remains outside VM fuel.

## Structural limits

Fuel is independent from RFC 0009's execution limits:

```text
evaluation fuel     bounds calls and repeated control flow
maximum call depth  bounds frame growth
maximum stack slots bounds XL value-stack memory
```

Exhausting one limit produces its own runtime error kind. A large frame cannot
consume fuel in place of satisfying the stack-slot limit, and tail calls must
consume fuel even if they reuse a frame.

## Determinism

Fuel consumption depends only on the executed bytecode control-flow path and
call boundaries. It does not depend on:

- value allocation strategy;
- string or collection size;
- shape interning or cache hits;
- native algorithm choice;
- debug metadata;
- register moves introduced by lowering;
- opcode dispatch or encoding implementation.

Given the same bytecode and inputs, execution consumes the same fuel.

## Rejected alternatives

### Charge every dispatched instruction

This couples evaluation limits to compiler lowering and opcode fusion. It also
suggests a CPU bound that recursive equality, string construction, and native
callbacks do not actually obey.

### Charge every branch

Forward branches only select or skip finite code and cannot by themselves
create unbounded evaluation. Charging them makes conditionals and pattern
lowering consume fuel for no termination benefit.

### Charge returns

The corresponding call has already been charged. Charging return duplicates
the cost and makes an equivalent tail call differ by an arbitrary extra unit.

### Charge native work by elements or bytes

This requires a virtual price schedule for every built-in and makes fuel depend
on implementation details. Data-size and algorithm-specific limits are clearer
as separate boundaries.

### Use wall-clock deadlines

Wall time is nondeterministic and unsuitable for reproducible tool-stage
evaluation. A host may additionally impose a deadline, but it is not language
fuel.

## Deferred work

- external-data byte, node, depth, and collection-size limits;
- host deadlines and cancellation;
- tail-call instructions and frame reuse;
- coroutine scheduling and per-process fuel policy;
- package/build-wide resource aggregation;
- dedicated limits for future expensive native algorithms.

## Implementation plan

1. Rename instruction-budget terminology in VM, compiler, type, module, CLI,
   tests, and documentation to evaluation fuel.
2. Remove fuel consumption from ordinary opcode dispatch.
3. Add one centralized `consume_fuel` operation at bytecode/native calls and
   taken edges whose target is less than or equal to the current PC.
4. Rename the runtime exhaustion kind and message while preserving source
   origin and trace behavior.
5. Update tool-stage and module execution to pass one shared fuel count through
   nested evaluation.
6. Add exact boundary tests for zero fuel, calls, forward jumps, untaken and
   taken conditional edges, self-jumps, nested calls, and runtime diagnostics.

## Acceptance criteria

1. A finite straight-line function succeeds with zero fuel.
2. Forward unconditional and taken conditional jumps consume no fuel.
3. Untaken conditional back edges consume no fuel.
4. A taken edge with `target <= pc` consumes exactly one unit.
5. Bytecode and native calls each consume exactly one unit and returns consume
   none.
6. Zero fuel fails at the first call or taken back edge with `FuelExhausted` at
   that instruction's source origin.
7. Nested calls share a single fuel count; it is not reset per frame.
8. Call-depth and stack-slot errors remain independent of fuel.
9. Tool-stage evaluation and module execution use the same fuel semantics.
10. No general native per-item charging API is introduced.
11. Existing XL language results are unchanged when sufficient fuel is
    provided.
12. Workspace tests, strict Clippy, formatting, and diff checks pass.
