# XL RFCs

XL is developed as a sequence of small, executable design proposals. Each RFC
must be committed before its implementation begins. Its implementation is then
tested and committed before the next RFC is written.

An RFC contains:

- motivation and scope;
- user-visible and internal semantics;
- rejected or deferred alternatives;
- implementation plan;
- executable acceptance criteria.

The MVP is planned as the following sequence. Later RFCs may narrow their scope
in response to implementation results, but may not silently change accepted
semantics from an earlier RFC.

1. Runtime values and bytecode VM.
2. Expression language, functions, pattern matching, and pipelines.
3. Type metadata, tool-stage evaluation, checking, and validation.
4. Modules, JSON data modules, external JSON input, and the MVP CLI.
5. Unified sources, lossless XL and JSON parsing, spans, and provenance.
6. Located syntax nodes, compact source ranges, and synthetic origins.
7. Structured lossless strings and restricted expression interpolation.
8. Typed CST views, missing-slot validation, and tolerant lexical errors.
9. Register LIR, VM call context, unified closures, and debug origins.
10. Evaluation fuel for calls and control-flow back edges.
11. Unified execution quotas for module initialization and runtime sessions.
12. Layered heaps, per-heap interning, and export-root promotion.
13. Single-assignment definition slots, recursive definitions, and focused
    function contracts.
14. Contiguous call windows and proper tail calls.
15. Core Array functions and VM-managed native continuations.
16. Core Dict enumeration, construction, and shallow merge functions.
17. Uniform reverse-application pipeline semantics.
18. Explicit placeholder application and call sections.
19. Structured debug observation through an explicit core module and host sink.
20. Derived structural codecs, an explicit Result boundary, and strict JSON
    output.
21. Rich runtime values with compact inline source locations.
22. First-class rich TypeMetadata and contract blame.
23. Two-tier Main/Work execution worlds.
24. Declarative native bindings in XL source.
25. Contextual functional decorators.
26. Flat attributed values and transparent TypeMetadata wrappers.
