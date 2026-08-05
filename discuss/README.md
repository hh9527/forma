# Forma design discussions

This directory holds design explorations that are not yet RFCs.

A discussion document may establish motivation, compare semantic models, and
record provisional syntax, but it does not commit Forma to an interface or an
implementation. Once the important alternatives and consequences are
understood, the accepted design can move into a numbered RFC with explicit
goals, non-goals, acceptance criteria, and an implementation plan.

Current discussions:

- `typed-accumulation-channels.md`: caller-selected typed accumulation;
- `type-directed-capability-factories.md`: deriving typed `Eq`/`Hash`-like
  functions from `TypeOf(A)` without trait resolution; and
- `user-space-type-metadata-interpreters.md`: open-recursion interpreter ABI,
  native/Forma parity, fallback, and reflection gaps; and
- `adversarial-validation-gaps-rank1-inference.md`: completed review inventory
  retained as validation history.
