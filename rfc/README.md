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
27. Normalized Struct and Enum model metadata.
28. Unified lowercase Struct, Enum, and Union model constructors.
29. Built-in normalized Bool, Option, and Result types.
30. Standard JSON model attribute decorators and payload vocabulary.
31. Attribute-aware bidirectional Struct JSON codecs.
32. Serde-style externally tagged and untagged Enum JSON codecs.
33. JSON Schema generation from the shared codec metadata plan.
34. Recursive TypeMetadata graphs through hidden up-links.
35. Once-only authoritative TypeMetadata promotion and graph analysis.
36. Function-valued JSON skip predicates through reusable native
    continuations.
37. Type-erased native continuation dispatch.
38. Semantic tooling and LSP roadmap.
39. Workspace-wide semantic snapshots and read-only queries.
40. Unified CLI type observation through the workspace snapshot.
41. Resolved HIR identities and expression semantic facts.
42. Recoverable HIR and explicit semantic fact states.
43. Dependency-scoped partial tool evaluation for TypeMetadata.
44. Recoverable workspace module graphs and cross-module fact blocking.
45. Asynchronous workspace revisions and document overlays.
46. Asynchronous LSP adapter and cooperative request cancellation.
47. Conservative semantic completion.
48. Declaration-generic native capabilities.
49. Explicit generic definition contracts.
50. Unified function bindings and contracts.
51. TypeMetadata metatype.
52. Unified bidirectional type checking.
53. Generic core combinators.
54. First-class tagged values.
55. Typed TypeMetadata witnesses.
56. Typed boundary errors and Result composition.
57. Canonical module identities and JSON String boundaries.
58. Executable value adapter.
59. Crate-relative module resolution.
60. Rename XL to Forma.
61. Homogeneous Dict TypeMetadata.
62. Typed executable effect protocol.
