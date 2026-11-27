# Mosh Module System — Implementation Design

**Status:** design baseline for the interpreter-first module milestone  
**Implementation status:** not implemented by this document

This document defines the architecture to use when implementing imports. It deliberately does not claim that `import` or `from ... import ...` currently works.

## Goals

- Load reusable `.mosh` source files from the filesystem.
- Give each module its own namespace rather than merging all declarations into the caller.
- Support relative imports from the importing file.
- Cache successfully initialized modules so repeated imports execute a module once.
- Detect import cycles and report the full import chain.
- Keep interpreter execution separate from the static checker and MIR until those stages gain explicit module support.

## Initial syntax proposal

The first implementation should support:

```mosh
import "math_utils.mosh" as math

print(math.square(5))
```

A later compatible step may add:

```mosh
from "math_utils.mosh" import square
```

The path is a string literal. The alias is an identifier. Imports are top-level statements in the first release; nested/dynamic imports are deferred. The parser and lexer must reject malformed paths and aliases with normal source diagnostics.

## Namespace model

A module must own:
- its canonical source path;
- its exported module namespace;
- its initialization state;
- its source-level diagnostics context.

The namespace must expose top-level declarations only. A module must not leak its local variables or functions into the importing module's scope. Member access such as `math.square` resolves against the module namespace. Missing members produce an error naming both the module and requested member.

Function values must be callable through module member expressions. The current interpreter stores functions separately from variable bindings, so this requires a deliberate callable representation or a module-aware call resolver; treating a function as an ordinary variable without updating call semantics is incorrect.

## Resolution order

For an import path:
1. If the path is absolute, resolve that exact path.
2. Otherwise resolve relative to the directory of the importing source file.
3. If not found, search configured Mosh module paths in deterministic order.
4. Canonicalize the selected path before using it as the cache key.

The initial implementation should not silently search the process working directory as an undocumented fallback. A future CLI option/environment variable may configure additional module paths; its name and precedence should be documented before implementation.

## Cache and cycle handling

Use canonical paths as module identities. Maintain:
- a cache of fully initialized modules;
- an in-progress stack/set for modules currently being evaluated.

On import:
- return the cached namespace if initialization completed;
- if the canonical path is already in progress, report a circular-import error with the chain;
- otherwise mark it in progress, parse and execute it in an isolated module environment, cache the completed namespace, and clear the in-progress marker on both success and failure.

A failed module initialization must not leave a partially initialized module in the success cache. The error must retain the imported file's path and source location.

## Runtime and security

Imports execute code with the same operating-system permissions as the Mosh process. They are not a sandbox. The interpreter's filesystem helpers and imported modules must be documented as trusted-code execution only.

## Compiler-stage boundaries

The first module milestone is interpreter-only. Until separately implemented:
- `mosh check` must report imports as unsupported rather than silently ignoring them;
- MIR lowering must report imports as unsupported;
- the README must distinguish interpreter module support from static-analysis/MIR support.

A later compiler milestone will add module graph construction, cross-module symbol resolution, type checking, dependency ordering, and cycle policy to semantic analysis and MIR.

## Required tests before calling the feature complete

1. Import a module and call an exported function.
2. Import a module and read an exported immutable value.
3. Confirm imported declarations do not leak into the caller's unqualified scope.
4. Confirm two modules can use the same local name without collision.
5. Confirm repeated imports initialize a module once.
6. Resolve a nested relative import relative to the importing module, not the process working directory.
7. Confirm a missing file reports the attempted path and import location.
8. Confirm a missing member reports the module and member names.
9. Detect direct and indirect circular imports and include the cycle chain.
10. Confirm a failed import is not left cached as successfully initialized.
11. Confirm imports inside functions are rejected in the initial top-level-only syntax.
12. Confirm the REPL has a documented base directory policy before enabling imports there.
13. Confirm `mosh check` and `mosh mir` clearly reject unsupported import statements.
14. Run formatting, all Rust targets, every documented example, and the GitHub Actions workflow.

## Implementation sequence

1. Add source-file identity to file execution while keeping the REPL's source identity explicit.
2. Add an import AST node and top-level parser support.
3. Introduce module namespaces and a callable function representation that works with member access.
4. Implement deterministic path resolution and isolated module evaluation.
5. Add module cache and circular-import tracking with cleanup on all error paths.
6. Add semantic/type-checker/MIR unsupported diagnostics for the interim stage.
7. Add tests, examples, CLI documentation, and only then enable the feature in the README.

The implementation must be developed and tested as one coherent feature. A file-reading builtin that executes imported source in the caller's global scope is not an acceptable substitute for modules.
