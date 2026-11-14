# MOSH Language Specification

- **Version:** 0.1 (initial draft)
- **Status:** Design baseline; initial name-resolution and scope rules in §4.1 are fixed
- **Category:** General-purpose compiled programming language

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** describe intended requirements at this draft stage. A requirement is not implemented merely because it appears here.

## 1. Vision

Mosh is an independent programming language intended to provide:

- readable, productive source code;
- ahead-of-time compilation to native machine code;
- static typing with type inference;
- ownership-based resource management;
- direct low-level memory and hardware facilities;
- a path from application programming to systems and embedded programming.

Mosh is not a Python wrapper, a database language, or a web framework.

## 2. Foundational architecture decisions

| Area | Decision |
|---|---|
| Compiler implementation language | Rust |
| Type system | Static, with inference |
| Safe memory model | Ownership and borrowing |
| Low-level access | Raw pointers and explicit unsafe operations |
| Code generation | LLVM |
| Initial compilation model | Ahead-of-time |
| Initial executable model | Native target executables |

These decisions establish direction. Detailed semantics remain open until specified and tested.

## 3. Design principles

1. **Readable syntax:** common programs SHOULD be concise and easy to understand.
2. **Native compilation:** the initial compiler SHOULD produce native executables through LLVM.
3. **Static correctness checks:** types, names, and other statically decidable rules MUST be checked before code generation.
4. **Ownership:** safe code MUST follow defined ownership and borrowing rules.
5. **Low-level capability:** the language MUST provide a path to raw pointers, manual allocation, explicit layouts, and platform interfaces.
6. **Explicit unsafe boundary:** operations that can invalidate memory-safety guarantees MUST be identifiable and governed by documented programmer obligations.
7. **No mandatory garbage collector:** deterministic ownership-based cleanup is the default design.
8. **Testability:** each compiler stage and language guarantee MUST have tests.
9. **Honest guarantees:** Mosh MUST NOT claim that arbitrary unsafe programs are memory-safe.
10. **Portability:** target-specific behavior MUST be isolated and documented.

## 4. Compiler architecture

The intended compiler pipeline is:

1. Source loading and diagnostics.
2. Lexical analysis (lexer).
3. Parsing into an abstract syntax tree (AST).
4. Name and module resolution.
5. Semantic analysis.
6. Static type checking and type inference.
7. Ownership and borrow checking for safe code.
8. Lowering into Mosh intermediate representation (MIR).
9. Mosh-level optimization and validation.
10. LLVM IR generation.
11. LLVM optimization and native code generation.
12. Linking into an executable or library.

The compiler is implemented in Rust. LLVM is the initial backend, not the definition of Mosh's language semantics or runtime.


### 4.1 Fixed initial name-resolution and scope rules

The following rules are normative for the initial language and MUST NOT be changed implicitly:

1. A module has a global scope. Top-level function names are registered before function bodies are analyzed. A function MAY be called before its textual definition, and functions MAY call each other recursively.
2. A variable MUST be declared before it is used. An initializer is analyzed before the newly declared variable enters scope, so a declaration cannot refer to itself through its own name.
3. Function parameters are local to that function. A function body has its own scope.
4. Each `if`, `else`, and `while` body introduces a nested block scope. Names declared in that block are not visible after the block ends.
5. A `for` iterable is evaluated before the loop variable is introduced. The loop variable and declarations in the loop body are visible only within the loop body scope.
6. A name MAY be shadowed in a nested scope. A duplicate declaration in the same scope MUST be rejected.
7. Undefined names and calls to undefined functions MUST be rejected with source locations.
These seven rules are the approved, fixed scope and name-resolution contract. They define name resolution only; they do not constitute type checking, ownership checking, borrow checking, or a complete function-call type system.

The first analyzer also currently checks that `return` appears inside a function and that assignments respect the `let`/`mut` declaration flag. These are initial implementation checks, not additional rules approved in the scope decision above. The initial implementation recognizes `print` as a built-in output function; the final built-in and standard-library model remains open.

## 5. Type system

Mosh uses static typing. The compiler MAY infer a type when it can do so unambiguously.

Proposed initial primitive types:

- `Bool`
- `Int` (exact width and signedness still to be decided)
- `Int8`, `Int16`, `Int32`, `Int64`
- `UInt8`, `UInt16`, `UInt32`, `UInt64`
- `Float32`, `Float64`
- `Char`
- `String`
- `Unit`

Proposed compound features include structures, enumerations, arrays, tuples, functions, generics, and user-defined types.

The specification MUST later define integer overflow, numeric conversions, string encoding, mutability, nullability, and generic constraints. No implicit conversion rules are finalized in this draft.

## 6. Memory and low-level model

### 6.1 Safe code

Safe code uses ownership and borrowing. The intended rules are:

- each owned value has one logical owner;
- ownership transfers are explicit in the language semantics;
- shared and mutable borrows follow defined aliasing rules;
- references cannot outlive the values they refer to;
- safe concurrent access prevents data races.

The exact move, copy, borrow, lifetime, and destruction rules remain to be specified.

### 6.2 Low-level facilities

Mosh is intended to support:

- raw pointers and pointer arithmetic;
- explicit allocation and deallocation;
- custom allocators;
- stack and heap storage;
- explicit alignment and data layout;
- atomics and volatile memory operations;
- C interoperability;
- platform and hardware interfaces;
- inline assembly where supported by a target.

### 6.3 Unsafe operations

Operations that can violate safe ownership, aliasing, or lifetime rules MUST require an explicit unsafe context or equivalent declaration.

Unsafe code is not automatically safe. The programmer is responsible for satisfying the documented preconditions of unsafe operations. The compiler SHOULD still enforce all checks that remain possible.

The exact unsafe syntax and rules are open design questions.

## 7. Compilation and target support

The initial compilation strategy is ahead-of-time compilation through LLVM.

The project will add targets incrementally. A target is supported only when its ABI, linker behavior, runtime requirements, and tests are defined. Linux, Windows, Android, and embedded targets are goals to investigate, not current support claims.

## 8. Proposed syntax direction

The syntax aims for Python-inspired readability while retaining explicit types and systems-level constructs.

Illustrative example (not finalized grammar):

```mosh
function main():
    name: String = "Mosh"
    age: Int = 1

    print("Welcome to " + name)
    print(age)
```

Illustrative function:

```mosh
function add(a: Int, b: Int) -> Int:
    return a + b
```

The grammar, indentation rules, statement terminators, declaration keywords, mutability syntax, and module syntax remain undecided.

## 9. Runtime and standard library

Mosh will require a defined runtime/ABI strategy and a standard library. Their responsibilities, including startup, panic behavior, allocation interfaces, strings, collections, I/O, and platform services, remain to be designed.

A minimal runtime is preferred where practical. The language MUST distinguish compiler-provided behavior from standard-library functionality.

## 10. Reliability and security requirements

- Compiler errors SHOULD identify the source location and explain the violated rule.
- Parser, type-checker, ownership-checker, and code-generation stages MUST have automated tests.
- The compiler SHOULD have negative tests proving that invalid programs are rejected.
- Unsafe APIs MUST document preconditions and effects.
- Native code generation MUST be tested on each supported target.
- Performance claims MUST be backed by reproducible benchmarks.
- Memory-safety claims MUST be limited to the guarantees actually enforced by the language and compiler.

## 11. Development milestones

1. Language specification and syntax decisions.
2. Rust compiler workspace and command-line skeleton.
3. Lexer and parser.
4. AST, name resolution, and diagnostics.
5. Static type checker.
6. Mosh IR.
7. LLVM integration and first native executable.
8. Ownership and borrow checker.
9. Unsafe operations and low-level memory model.
10. Runtime and standard library.
11. Testing, optimization, and additional targets.

The order may be adjusted when dependencies require it. In particular, a meaningful safe-code guarantee cannot be claimed until the ownership and unsafe semantics are implemented.

## 12. Open decisions

The following must be resolved before the language can be considered fully specified:

- exact grammar and keyword set;
- mutability and variable declaration rules;
- integer widths, overflow, and conversions;
- ownership, moves, copies, borrowing, and lifetimes;
- pointer provenance, arithmetic, and aliasing;
- unsafe block/function rules;
- struct and enum layout;
- generics and trait/interface system;
- error handling and panic behavior;
- concurrency and atomic memory ordering;
- module and package model;
- runtime ABI and C interoperability;
- target platform support and licensing.

## 13. Implementation status

This document is a design draft. The lexer, parser, and initial semantic name/scope checks are implemented with tests. Static type checking, ownership and borrow checking, runtime, LLVM backend, standard library, and native executable generation are not yet implemented.
