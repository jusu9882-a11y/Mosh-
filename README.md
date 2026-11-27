# MOSH

**Mosh is a general-purpose interpreted programming language implemented in Rust.**

Mosh source files use the `.mosh` extension. The `@` character starts a comment. Mosh currently executes programs directly with a tree-walk interpreter; native compilation is not part of the current product.

## Language direction

- **Execution model:** interpreter only
- **Implementation language:** Rust
- **Source files:** `.mosh`
- **Comments:** `@`
- **Runtime values:** integers, floats, strings, booleans, nil, mutable lists, string-keyed dictionaries, and immutable tuples
- **Current priority:** a complete, dependable interpreter, useful standard library, module system, REPL, diagnostics, and tests

There is no LLVM backend, bytecode VM, native executable output, or compiler command in the current Mosh release direction. Existing lexer, parser, semantic-analysis, and type-checking code is used to support interpretation and diagnostics; it must not be described as native compilation.

## Current interpreter features

- Run a `.mosh` file with `mosh run file.mosh`.
- Interactive REPL with multi-line compound blocks.
- Variables declared with `let` and `mut`.
- Integers, floats, strings, booleans, and nil.
- Arithmetic, comparisons, and boolean operators.
- `if` / `else`, `while`, and `for` loops.
- Named functions and return values.
- Built-ins: `print()`, `input()`, `len()`, `push()`, and `pop()`.
- Mutable and nested lists, positive/negative indexing, and indexed assignment.
- String-keyed dictionaries with lookup, insertion/update, deterministic key iteration, `keys()`, `has_key()`, and `remove()`.
- Immutable tuples, including empty and single-element tuples, indexing, iteration, and `len()`.
- String helpers: `upper()`, `lower()`, `trim()`, `trim_start()`, `trim_end()`, `contains()`, `starts_with()`, `ends_with()`, `replace()`, `split()`, and `join()`.
- Filesystem helpers: `read_text()`, `write_text()`, `append_text()`, `file_exists()`, and `create_dir()`.
- Runtime exceptions with `try:`, `catch error:`, and `raise value`.
- Runtime diagnostics with source line and column, plus a loop iteration safety limit.

**Filesystem security:** filesystem built-ins use the operating-system permissions of the Mosh process. Mosh is not a sandbox; do not run untrusted source files.

## Quick start

Build the interpreter:

```sh
cd compiler
cargo build --bin mosh
```

Run a program:

```sh
cargo run --bin mosh -- run ../examples/collections.mosh
cargo run --bin mosh -- run ../examples/dictionaries.mosh
cargo run --bin mosh -- run ../examples/tuples_strings.mosh
```

Start the REPL:

```sh
cargo run --bin mosh
```

## CLI

- `mosh` or `mosh repl` — start the interactive interpreter.
- `mosh run file.mosh` — execute a source file.
- `mosh check file.mosh` — run the currently supported semantic and type diagnostics; this does not compile or execute the program.
- `mosh lex file.mosh` — inspect tokens.
- `mosh parse file.mosh` — inspect the parsed syntax tree.
- `mosh --version` — print the interpreter version.

## Example

```mosh
@ A first Mosh collection program

mut fruits = ["Mango", "Apple", "Orange"]
print(fruits[0])
print(len(fruits))
push(fruits, "Banana")

for fruit in fruits:
    print(fruit)

mut total = 0
for number in [1, 2, 3, 4]:
    total = total + number
print(total)
```

## Development principles

1. The interpreter is the product; keep it usable as features grow.
2. Implement features end-to-end with regression tests.
3. Keep runtime behavior, diagnostics, and documentation consistent.
4. Clearly distinguish implemented features from planned work.
5. Do not claim a feature is complete until code and tests verify it.

## Repository layout

- `compiler/` — Rust implementation of the interpreter, lexer, parser, and diagnostics.
- `examples/` — runnable Mosh programs.
- `docs/` — language specification and design documentation.

## License

No license has been selected yet. Until one is added, the repository's contents are not granted an open-source license by default.
