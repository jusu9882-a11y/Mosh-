# Mosh Module System

**Status:** initial interpreter module system implemented.

## Supported syntax

```mosh
import "math_utils.mosh" as math

print(math.square(5))
print(math.answer)
```

The path must be a quoted, non-empty string. The alias is required. Imports are top-level only. `from ... import ...` and dynamic imports are not implemented.

## Runtime behavior

- Every imported file executes in its own interpreter and namespace.
- Top-level variables and functions are accessible through the module alias.
- Unqualified imported names do not leak into the importing file.
- Module functions execute against their own module's global scope.
- Relative paths resolve from the directory of the file performing the import. Absolute paths are accepted.
- The selected path is canonicalized before it is used as a cache key.
- A successfully initialized module is cached and reused for subsequent imports in the same interpreter.
- Direct and indirect cycles are detected and reported with the cycle path.
- Failed module initialization is not added to the successful-module cache.
- Imports from the interactive REPL are rejected because the REPL has no source-file directory.

## Boundaries and limitations

- Module imports are an interpreter feature. `mosh check` explicitly reports them as unsupported by the static checker.
- The initial module system does not provide configured search paths, package installation, `from` imports, or module reloading.
- Module namespace assignment is not supported; module members are read through the alias.
- Imported code runs with the same operating-system permissions as the Mosh process. Modules are not sandboxed.

## Regression coverage

The interpreter test suite covers:
- importing module values and calling exported functions;
- namespace isolation;
- reuse of cached module state across repeated imports;
- indirect circular-import detection.

The CI workflow also executes `examples/modules.mosh`.
