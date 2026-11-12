# Mosh Lexer — Provisional Rules

**Status:** Initial implementation baseline; subject to language-design review. Nothing in this document finalizes the language grammar.

The lexer converts UTF-8 source text into tokens. It does not parse expressions, validate types, or decide whether a token sequence is a valid Mosh program. The token inventory below records the implementation's current choices so that they can be reviewed explicitly.

## Provisional token inventory

### Identifiers and keywords

- Identifiers use ASCII letters or underscore initially, followed by ASCII letters, digits, or underscore.
- The current provisional keyword mapping is: `function`, `return`, `if`, `else`, `while`, `for`, `in`, `let`, `mut`, `true`, `false`, `unsafe`, `struct`, and `enum`.
- Keyword spelling and the final keyword set remain open language-design decisions. Other words are emitted as identifiers.
- Non-ASCII characters outside string literals are currently rejected rather than treated as identifier characters.

### Numeric literals

- Integers currently consist of decimal digits.
- Floats currently support a decimal fraction and/or a decimal exponent, for example `3.14`, `6e2`, and `7.0e-1`.
- A decimal point is part of a float only when at least one digit follows it. Thus `1.` is currently tokenized as an integer followed by a dot token.
- An exponent marker (`e` or `E`) must be followed by optional sign and at least one digit; malformed exponents are lexical errors.
- Signs are separate operator tokens, not part of numeric literals.
- Numeric separators, radix prefixes, suffixes, range checking, and floating-point overflow validation are not implemented. These rules are provisional.

### Strings

- Both single-quoted and double-quoted strings are recognized.
- The current accepted escapes are `\\n`, `\\r`, `\\t`, `\\0`, `\\\\`, `\\"`, and `\\'`.
- The lexer preserves the original quoted spelling. It does not decode escapes.
- Strings cannot cross a physical line. Raw strings, interpolation, character literals, and escape decoding are not implemented.

### Operators and punctuation

The current operator tokens are:

- Single-character: `+`, `-`, `*`, `/`, `%`, `=`, `!`, `<`, `>`, `&`, `|`, `^`, and `~`.
- Two-character: `==`, `!=`, `<=`, `>=`, and `->`.

The current punctuation/delimiter tokens are `(`, `)`, `[`, `]`, `{`, `}`, `:`, `,`, and `.`.

This is only the current lexer inventory, not a final operator or punctuation specification. For example, logical operators, shifts, compound assignments, ranges, and other proposed operators have no dedicated tokens yet. The parser and language-design work must agree on their eventual spellings and precedence before they are treated as part of the language.

### Comments and layout tokens

- A hash (`#`) starts a line comment outside a string.
- LF, CRLF, and CR line endings are accepted.
- Indentation is measured in leading spaces. A tab in leading indentation is an error; horizontal tabs between tokens are accepted as whitespace.
- An increase in indentation emits one `Indent`; a decrease emits one or more `Dedent` tokens.
- A dedent must match a previously observed indentation level.
- Blank lines and comment-only lines are ignored for indentation and do not emit newline tokens.
- A non-empty source line emits a `Newline` token when its line ending is encountered. If the final non-empty line has no line ending, a synthetic newline marker is emitted at EOF.
- Remaining indentation levels are closed with `Dedent` tokens before EOF.

Indentation significance, tab policy, blank-line treatment, comment syntax, and newline continuation behavior remain provisional. The lexer currently emits line terminators without understanding delimiter nesting or multiline expressions.

## Source locations and lexemes

Each token carries a span with:

- zero-based byte offsets into the original UTF-8 source;
- one-based line and byte-column positions;
- exclusive end offsets/columns.

Columns count UTF-8 bytes, not terminal display cells or Unicode grapheme clusters. An unknown non-ASCII character is reported with a span covering its complete UTF-8 encoding.

Source-backed tokens preserve their exact source spelling in `lexeme`, including string quotes and the original newline spelling. Synthetic newline, indentation-closing, dedentation, and EOF markers use an empty lexeme.

## Known limits

- Identifiers are ASCII-only.
- Numeric literals are not range-checked or converted to machine values.
- Strings are not decoded and cannot span physical lines.
- There is no character literal, raw string, interpolation, numeric suffix, or separator support.
- There are no dedicated logical, shift, compound-assignment, or range operators.
- Newline handling is not aware of parentheses, brackets, braces, or continuation syntax.
- The lexer does not establish that a source file is a valid Mosh program.

## Run the lexer

From the `compiler/` directory:

```sh
cargo run -- lex path/to/program.mosh
cargo test
```

The token command is a development/debugging interface, not a promise of stable CLI compatibility.
