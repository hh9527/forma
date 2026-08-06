# RFC 0141: Path-first module binding

- Status: Proposed
- Depends on: RFC 0140

## Summary

The module-record import form becomes:

```forma
import "std/array" as array;
```

and the old name-first form is removed. `as` is a reserved keyword and always
introduces the local name of the source entity to its left.

This RFC changes syntax order only. The AST continues to lower this form to one
module binding, and resolver, caching, interface, runtime, recovery, and LSP
behavior remain unchanged.

## Completion order

After `import "`, tooling knows it should complete a module request. Once the
string closes, `as` makes the remaining slot a local identifier. Export-aware
completion is introduced by RFC 0142.

## Acceptance criteria

1. `import "target" as name;` binds the target module record.
2. `import name from "target";` is rejected.
3. `from` returns to the ordinary identifier vocabulary.
4. `as` is reserved for import aliases.
5. All active repository source, examples, tests, and current documentation
   use the path-first form.
6. Module resolution identity and execution behavior do not change.
