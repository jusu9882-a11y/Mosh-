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


## Provisional spelling and tokenization details

These choices describe the current lexer implementation only. They do not settle the language grammar.

- The reserved words are: **function, return, if, else, while, for, in, let, mut, true, false, unsafe, struct, and enum**. A longer identifier such as **function_name** remains an identifier.
- Decimal integers contain one or more decimal digits. Floating-point tokens are either digits followed by a decimal point and at least one digit, or digits followed by an exponent (**e** or **E**, optional sign, and at least one digit). An exponent makes the token a float even without a decimal point.
- A decimal point is included in a number only when followed by a digit. Thus **42.** is currently tokenized as an integer followed by a dot. Leading-dot numbers, radix prefixes, digit separators, suffixes, and range/overflow validation are not implemented.
- The recognized operators are **+ - * / % = == ! != < <= > >= -> & | ^ ~**. Multi-character operators use longest matching. Logical operators, shifts, compound assignments, and other unlisted spellings are not recognized as single tokens.
- The punctuation tokens are **( ) [ ] { } : , .**. There is no semicolon token in this provisional set.
- Both single-quoted and double-quoted strings are accepted. Their token lexeme preserves the original quotes and escape spelling; the lexer does not decode the string value. The only accepted escapes are **\\n, \\r, \\t, \\0, \\\\, \\"**, and **\\'**.
- A hash character begins a line comment outside a string. A hash inside a quoted string is ordinary string content.
- LF, CRLF, and lone CR are treated as physical line endings. Blank and comment-only lines do not emit logical Newline tokens. If the final non-empty source line has no line ending, the lexer synthesizes one Newline before closing indentation levels and emitting Eof.
- Horizontal tabs are rejected in indentation prefixes. A tab between tokens on a line is currently accepted as whitespace. This distinction is provisional.

## Error and location details

An error returns one LexError and no partial token list. Error spans use zero-based UTF-8 byte offsets and one-based byte columns, consistent with token spans. For an unsupported non-ASCII character outside a string, the error span covers the complete UTF-8 encoding of that character. String contents may contain Unicode; identifier recognition remains ASCII-only.
