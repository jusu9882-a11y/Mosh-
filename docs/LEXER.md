# Mosh Lexer — Provisional Rules

**Status:** Initial implementation baseline; subject to language-design review.

The lexer converts UTF-8 source text into tokens. It does not parse expressions, validate types, or decide whether a token sequence is a valid program.

## Token categories

The initial implementation recognizes:

- Identifiers: ASCII letters or underscore, followed by ASCII letters, digits, or underscore.
- Keywords: `function`, `return`, `if`, `else`, `while`, `for`, `in`, `let`, `mut`, `true`, `false`, `unsafe`, `struct`, and `enum`.
- Integer and decimal/exponent-form floating-point literals.
- Single-quoted and double-quoted strings, with the escapes `\\n`, `\\r`, `\\t`, `\\0`, `\\\\`, `\\"`, and `\\'`.
- Arithmetic, comparison, assignment, arrow, bitwise-symbol, delimiter, and punctuation tokens.
- Newline, indentation, dedentation, and end-of-file markers.
- Hash-prefixed line comments.

## Initial indentation policy

- Indentation is measured in spaces.
- A tab at the beginning of a line is an error.
- Blank lines and comment-only lines do not create indentation tokens.
- An increase in indentation emits one `Indent`.
- A decrease emits one or more `Dedent` tokens.
- A dedent must match a previously observed indentation level.
- CRLF and LF line endings are accepted.

This policy is intentionally conservative. The language design must still decide whether indentation is permanently significant, whether tabs should be configurable, and how multiline expressions interact with newlines.

## Source locations

Each token carries a span with zero-based UTF-8 byte offsets and one-based line and byte-column positions. Byte columns are deliberate in this first implementation; user-facing diagnostics may later add Unicode display-column calculation.

## Known limits

- Identifiers are ASCII-only.
- Strings cannot span physical lines.
- There is no character literal, raw string, interpolation, escape decoding, or numeric suffix support yet.
- The lexer emits physical newlines; grouping and continuation rules belong to later grammar/parser work.
- The lexer does not validate numeric range or floating-point overflow.
- The lexer is not a parser and does not establish that a source file is a valid Mosh program.

## Run the lexer

From the `compiler/` directory:

```sh
cargo run -- lex path/to/program.mosh
cargo test
```

The token command is a development/debugging interface, not a promise of stable CLI compatibility.
