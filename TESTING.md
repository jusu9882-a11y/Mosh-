# Mosh 0.1 compiler preview — tester guide

## What this preview is

This is an early Linux x86_64 compiler preview for the implemented primitive-language subset. It is suitable for checking the current lexer, parser, name/type analysis, MIR path, and native executable generation. It is **not** a complete Mosh language release.

## Requirements

- Linux x86_64
- Clang installed and available as `clang` on `PATH`
- For building from source: stable Rust toolchain

The packaged `moshc` executable is built by GitHub Actions on Ubuntu. Other operating systems and CPU architectures are not verified.

## Run the examples

Extract the artifact, open a terminal in the extracted directory, then run:

```sh
chmod +x compiler/target/release/moshc
MOSHC=./compiler/target/release/moshc

$MOSHC --version
$MOSHC --help

$MOSHC check examples/hello.mosh
$MOSHC check examples/control_flow.mosh
$MOSHC check examples/fibonacci.mosh
$MOSHC check examples/primitive_values.mosh

$MOSHC build examples/hello.mosh -o /tmp/mosh-hello
/tmp/mosh-hello

$MOSHC build examples/control_flow.mosh -o /tmp/mosh-control-flow
/tmp/mosh-control-flow

$MOSHC build examples/fibonacci.mosh -o /tmp/mosh-fibonacci
/tmp/mosh-fibonacci

$MOSHC build examples/primitive_values.mosh -o /tmp/mosh-primitives
/tmp/mosh-primitives
```

Expected program output, in order:

- `Hello, Mosh!`
- `sum=15`
- `55`
- `Mosh 3.5 3`

A successful `check` confirms parsing, name resolution, and the current static type checks. A successful `build` additionally exercises MIR lowering/validation, LLVM IR generation, and Clang linking.

## Current supported subset

- `.mosh` source files; `@` line comments
- `Int`, `Float`, `Bool`, and `String` values
- `Unit` as a function return type only
- Named and recursive function calls
- Arithmetic, comparisons, and integer bitwise operations
- Block-scoped local variables, shadowing, mutable locals, and assignments
- `if/else`, `while`, returns, and the built-in `print`

## Known limitations — do not treat these as supported

- Ownership and borrow checking
- Arrays, collections, structs, enums, generics, indexing, and member access
- `for` loops and nested functions
- Global variables and executable top-level statements
- Raw pointers, manual allocation, custom allocators, explicit memory layout, atomics, volatile operations, C interop, and inline assembly
- A complete runtime or standard library
- Windows, macOS, Android, or non-x86_64 native targets

The compiler is expected to reject unsupported constructs rather than silently pretending to compile them. A rejection of one of these listed unsupported features is not a failed test of this preview.

## Report a problem

When reporting a failure, include:
1. The exact command you ran.
2. The complete terminal output.
3. The operating system and CPU architecture.
4. The smallest `.mosh` source file that reproduces the issue.

Please distinguish a compiler crash, incorrect output, wrong diagnostic, and an unsupported-feature rejection.
