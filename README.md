# MOSH

**A general-purpose compiled programming language.**

Mosh is designed around readable syntax, static typing, native compilation, ownership-based memory management, and explicit low-level control.

## Foundational architecture

- **Compiler implementation:** Rust
- **Type system:** Static typing with type inference
- **Safe memory model:** Ownership and borrowing (design requirement; checker not implemented yet)
- **Code generation:** LLVM IR with Clang as the initial native toolchain
- **Compilation model:** Ahead-of-time (AOT)
- **Source extension:** `.mosh`
- **Line comments:** `@`

## Current implementation

The repository now contains a working compiler pipeline for an initial primitive subset:

1. Lexer and indentation-aware parser.
2. Name resolution and lexical scope checks.
3. Primitive static type checking and inference.
4. MIR lowering for functions, locals, expressions, `if/else`, `while`, and returns.
5. MIR structural, control-flow dominance, binding, and inferable type validation.
6. Textual LLVM IR generation and native executable linking through Clang.
7. End-to-end tests that build and execute generated native programs.

The current native backend supports the primitive types `Int`, `Float`, `Bool`, `String`, and `Unit` in the supported contexts; named and recursive function calls; arithmetic and comparisons; bitwise integer operations; block-scoped local variables (including shadowed names) and assignments; conditional and while-loop control flow; and the built-in `print`.

The backend currently maps `Int` to signed 64-bit integers and `Float` to LLVM `double`. Integer division and remainder trap on a zero divisor; signed division also traps on the minimum-integer divided by negative one case. These are implementation choices for the first backend, not yet a final language-wide representation contract.

## Build the compiler

Requirements: stable Rust and Clang available on `PATH`.

From the repository root:

```sh
cd compiler
cargo build --release
./target/release/moshc build ../examples/hello.mosh -o hello
./hello
```

The `build` command runs parsing, semantic analysis, type checking, MIR lowering and validation, LLVM IR generation, and Clang native linking. Use `moshc --help` to see the available development and compiler commands.

## Current limitations

This is not yet the complete Mosh language or a complete systems compiler. In particular:

- `for`-loop lowering and iterable/collection types are not implemented.
- Nested functions, member access, indexing, structures, enumerations, arrays, and generics are not implemented in the initial compiler pipeline.
- The native backend does not yet support uninitialized local storage or module-global variable storage.
- Ownership and borrow checking are not implemented.
- Raw pointers, manual allocation, custom allocators, explicit layouts, atomics, volatile operations, C interoperability, and inline assembly are not implemented.
- A complete Mosh runtime and standard library do not exist yet.
- Native executable generation is currently tested on Linux with Clang; other targets are not claimed as supported.

The backend uses the platform C library for the initial `print` and string-comparison operations. This is not a finished Mosh runtime.

## Development principles

1. Specify language behavior before implementing it.
2. Test every compiler stage and execute generated programs.
3. Make safe-code guarantees precise and auditable.
4. Make unsafe operations explicit and document their obligations.
5. Never claim a feature works until it is implemented and tested.

## Repository layout

- `docs/SPECIFICATION.md` — language design baseline and open questions
- `docs/LEXER.md` — lexical rules and lexer limits
- `docs/PARSER.md` — grammar, AST scope, and parser limitations
- `compiler/` — Rust compiler implementation
- `examples/` — small Mosh programs

## License

No license has been selected yet. Until one is added, the repository's contents are not granted an open-source license by default.
