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
- Numeric and conversion helpers: `abs()`, `sqrt()`, `floor()`, `ceil()`, `round()`, `pow()`, `min()`, `max()`, `sum()`, `int()`, `float()`, `str()`, and `type()`.
- `range()` creates bounded integer sequences with optional start and step arguments.
- Mutable and nested lists, positive/negative indexing, and indexed assignment.
- String-keyed dictionaries with lookup, insertion/update, deterministic key iteration, `keys()`, `has_key()`, and `remove()`.
- Immutable tuples, including empty and single-element tuples, indexing, iteration, and `len()`.
- Collection helpers: `sorted()` returns a sorted copy of a list or tuple, `reverse()` returns a reversed copy (or reversed string), and `values()` returns dictionary values in sorted-key order.
- String helpers: `upper()`, `lower()`, `trim()`, `trim_start()`, `trim_end()`, `contains()`, `starts_with()`, `ends_with()`, `replace()`, `split()`, and `join()`.
- Filesystem helpers: `read_text()`, `write_text()`, `append_text()`, `file_exists()`, and `create_dir()`.
- Path helpers: `path_join()`, `is_file()`, `is_dir()`, `file_size()`, `list_dir()` (sorted names), `current_dir()`, and `absolute_path()`.
- Runtime exceptions with `try:`, `catch error:`, and `raise value`.
- Top-level module imports using `import "module.mosh" as module`, with isolated namespaces, relative paths from the importing file, successful-module caching, and circular-import detection.
- Runtime diagnostics with source line and column, plus a loop iteration safety limit.
- Device capability introspection through `device.<module>.is_supported()`, `is_available()`, `capabilities()`, `status()`, `permission()`, and explicit `request_permission()` calls. Without a registered native host adapter, modules truthfully report unsupported.

**Filesystem and import security:** filesystem built-ins and imported modules use the operating-system permissions of the Mosh process. Mosh is not a sandbox; only run trusted source files and modules.

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
cargo run --bin mosh -- run ../examples/standard_library.mosh
cargo run --bin mosh -- run ../examples/device_capabilities.mosh
```

Start the REPL:

```sh
cargo run --bin mosh
```

## Device namespace

Mosh exposes a capability-first `device` namespace. For example:

```mosh
let camera = device.camera.capabilities()
if device.camera.is_supported():
    print(device.camera.permission())
else:
    print(camera[\"limitations\"])
```

Available introspection methods are `is_supported()`, `is_available()`, `capabilities()`, `status()`, and `permission()`. `request_permission()` is an explicit operation and may trigger a host OS permission prompt only when a real adapter is installed. It returns a permission-state string or raises a structured runtime error. The default CLI/REPL host has no native adapter, so hardware modules report unsupported; this is intentional, not simulated device support. The Linux/Android sysfs backend also exposes real battery telemetry through `device.battery.read()` when a readable battery is present. It reports capacity, charging state, voltage, current, temperature, and technology where the kernel exposes those attributes. Camera capture, Wi-Fi scanning, Bluetooth discovery, and other hardware operations remain unavailable until their native adapters are implemented.

## CLI

- `mosh` or `mosh repl` — start the interactive interpreter.
- `mosh run file.mosh` — execute a source file.
- `mosh check file.mosh` — run the currently supported semantic and type diagnostics; interpreter-only imports are explicitly rejected by this command.
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
