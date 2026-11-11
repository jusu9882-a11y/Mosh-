# MOSH

**A general-purpose compiled programming language.**

Mosh aims to combine readable, productive syntax with native-code performance ambitions and explicit low-level control.

## Foundational architecture

- **Compiler implementation:** Rust
- **Type system:** Static typing with type inference
- **Memory model:** Ownership and borrowing, with explicit low-level facilities
- **Code generation:** LLVM
- **Compilation model:** Ahead-of-time (AOT)
- **Programming scope:** General-purpose, from application software to systems and embedded programming

## Project status

Mosh is in its language-design and compiler-foundation stage. The specification is being developed before the implementation is treated as stable. Features described as proposed are not yet implemented.

## Repository layout

- `docs/SPECIFICATION.md` — initial language specification and open design questions
- `compiler/` — Rust compiler workspace (initial skeleton)

## Development principles

1. Specify language behavior before implementing it.
2. Test every compiler stage.
3. Make safe-code guarantees precise and auditable.
4. Make unsafe operations explicit and document their obligations.
5. Never claim a feature works until it is implemented and tested.

## License

No license has been selected yet. Until one is added, the repository's contents are not granted an open-source license by default.
