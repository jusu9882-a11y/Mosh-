# MOSH

**Mosh is an interpreter-first general-purpose programming language, implemented in Rust.**

Mosh is being developed incrementally: start with a practical interpreted language, then add stronger static analysis, a bytecode virtual machine, and eventually native compilation and low-level systems capabilities. The `.mosh` source extension and `@` comment marker are part of the language contract.

## Current direction

- **Implementation language:** Rust
- **Initial execution engine:** Tree-walk interpreter
- **Source files:** `.mosh`
- **Comments:** `@`
- **Current values:** integers, floats, strings, booleans, nil, mutable lists, string-keyed dictionaries, and immutable tuples
- **Later execution engines:** bytecode VM, then optional LLVM native backend
- **Long-term type and memory model:** static typing, ownership and borrowing, with explicit unsafe low-level facilities

The interpreter is the first usable execution path. Existing semantic analysis, type checking, and MIR work are retained; they are not yet a complete replacement for interpreter execution.

## Current implementation status

### Interpreter

- Execute a `.mosh` file with `mosh run file.mosh`.
- Start an interactive REPL with `mosh` or `mosh repl`, including multi-line compound blocks submitted with a blank line.
- Variables: `let` and `mut`.
- Integers, floats, strings, booleans, and nil.
- Arithmetic, comparisons, and boolean operators.
- `if` / `else`, `while`, and `for` loops.
- Named functions and return values.
- Built-ins: `print()`, `input()`, `len()`, `push()`, and `pop()`.
- List literals, nested lists, positive/negative indexing, indexed assignment, and list/string iteration.
- Dictionary literals with string keys, lookup, insertion/update, deterministic key iteration, and `keys()`, `has_key()`, and `remove()` built-ins.
- Tuple literals, including empty and single-element tuples, indexing, negative indexing, iteration, and `len()`.
- String helpers: `upper()`, `lower()`, `trim()`, `trim_start()`, `trim_end()`, `contains()`, `starts_with()`, `ends_with()`, `replace()`, `split()`, and `join()`.
- Real filesystem helpers: `read_text()`, `write_text()`, `append_text()`, `file_exists()`, and `create_dir()`; paths are relative to the process working directory unless absolute.
- **Filesystem security:** these functions access the host filesystem using the permissions of the Mosh process. The interpreter is not sandboxed; do not run untrusted `.mosh` programs.
- Interpreter exceptions: `try:` / `catch error:` handles runtime failures, while `raise value` raises an explicit runtime error. The caught `error` binding is a string containing the error message and source location. Errors propagate through function calls until caught.
- Runtime diagnostics with source line and column.
- A loop iteration safety limit.

### Compiler foundation

The Rust lexer, parser, semantic analyzer, primitive type checker, and MIR infrastructure remain in the repository. The current MIR backend does not yet lower list literals or the full interpreter feature set. Interpreter execution and MIR/native compilation are distinct development tracks.

## Quick start

Build the interpreter from the compiler directory:

```sh
cd compiler
cargo build --bin mosh
```

Run a source file:

```sh
cargo run --bin mosh -- run ../examples/collections.mosh
cargo run --bin mosh -- run ../examples/dictionaries.mosh
cargo run --bin mosh -- run ../examples/tuples_strings.mosh
```

Start the interactive shell:

```sh
cargo run --bin mosh
```

Other compiler-inspection commands currently available include `mosh lex`, `mosh parse`, `mosh check`, and `mosh mir`. The checker and MIR backend support a smaller subset than the interpreter.

## Language example

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

1. Make each feature work end-to-end before announcing it.
2. Keep the interpreter executable and its regression tests green as the language grows.
3. Preserve the existing lexer/parser and compiler work where practical.
4. Document differences between interpreter support and compiler/MIR support.
5. Add static types, ownership, bytecode, LLVM, and low-level facilities in deliberate stages.
6. Never claim a feature is implemented until code and tests verify it.

## Repository layout

- `compiler/` — Rust interpreter and compiler foundation
- `docs/SPECIFICATION.md` — language architecture and design contract
- `docs/LEXER.md` — lexical rules
- `docs/PARSER.md` — grammar and parser behavior

## License

No license has been selected yet. Until one is added, the repository's contents are not granted an open-source license by default.
