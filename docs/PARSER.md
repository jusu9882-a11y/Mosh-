# Mosh Parser — Provisional Grammar

**Status:** Initial parser implementation; grammar is not yet a stable language contract.

The parser consumes lexer tokens and builds an abstract syntax tree (AST). It does not resolve names, validate types, check ownership, or generate machine code.

## Supported statement forms

- Function declarations: function name(arg: Type) -> Type:
- Bindings: let name = expression, let name: Type = expression, and mut name: Type = expression
- Typed bindings without a declaration keyword: name: Type = expression
- Return statements, with or without a value
- if / else blocks
- while loops
- for name in iterable loops
- Assignment statements
- Expression statements

Blocks use the lexer's newline and indentation tokens.

## Supported expressions

- Identifiers, integer/float/string/boolean literals
- Parenthesized expressions
- Unary -, !, &, and *
- Binary arithmetic, comparison, and bitwise-symbol operators
- Function calls
- Member access with dot
- Indexing with square brackets

Binary expressions use a precedence-climbing parser. Postfix calls, member access, and indexing bind most tightly.

## Current limitations

- No logical and / or operators yet.
- No break, continue, classes, imports, lambdas, generics, arrays/tuple literals, or pattern matching.
- else if is not a dedicated construct yet; nested if can be expressed inside an else block.
- Assignment targets are parsed as expressions; semantic validation of assignable targets is a later stage.
- Types are currently simple names, not a complete type grammar.
- Error recovery is fail-fast: parsing stops at the first syntax error.
- AST nodes carry source spans, but diagnostic rendering is still minimal.

## Run the parser

From compiler/:

    cargo run -- parse path/to/program.mosh
    cargo test

The command prints the AST for development. It is not a stable compiler interface.