use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Zero-based byte offsets into the original UTF-8 source.
    pub start: usize,
    pub end: usize,
    /// One-based source line and byte column.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    Integer,
    Float,
    String,

    Function,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Let,
    Mut,
    True,
    False,
    Unsafe,
    Struct,
    Enum,

    Newline,
    Indent,
    Dedent,
    Eof,

    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Dot,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Equal,
    Bang,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Arrow,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Exact source spelling, including quotes for strings.
    pub lexeme: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}:{}",
            self.message, self.span.line, self.span.column
        )
    }
}

impl std::error::Error for LexError {}

pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(source).tokenize()
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
    line: usize,
    column: usize,
    at_line_start: bool,
    indents: Vec<usize>,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            position: 0,
            line: 1,
            column: 1,
            at_line_start: true,
            indents: vec![0],
            tokens: Vec::new(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        while self.position < self.bytes.len() {
            if self.at_line_start {
                self.scan_indentation()?;
                if self.position >= self.bytes.len() {
                    break;
                }
                if self.at_line_start {
                    continue;
                }
            }

            match self.peek() {
                Some(b' ' | b'\t') => self.bump(),
                Some(b'#') => self.skip_comment(),
                Some(b'\n' | b'\r') => self.consume_newline(),
                Some(b'"') | Some(b'\'') => self.scan_string()?,
                Some(b'0'..=b'9') => self.scan_number()?,
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => self.scan_identifier(),
                Some(_) => self.scan_operator_or_punctuation()?,
                None => break,
            }
        }

        let eof_span = self.point_span();
        if self.tokens.last().is_some_and(|t| {
            !matches!(
                t.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
            )
        }) {
            self.tokens.push(Token {
                kind: TokenKind::Newline,
                lexeme: String::new(),
                span: eof_span,
            });
        }

        while self.indents.len() > 1 {
            self.indents.pop();
            self.tokens.push(Token {
                kind: TokenKind::Dedent,
                lexeme: String::new(),
                span: eof_span,
            });
        }

        self.tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            span: eof_span,
        });
        Ok(self.tokens)
    }

    fn scan_indentation(&mut self) -> Result<(), LexError> {
        let start = self.point_span();
        let mut width = 0usize;

        while let Some(byte) = self.peek() {
            match byte {
                b' ' => {
                    width += 1;
                    self.bump();
                }
                b'\t' => {
                    return Err(LexError {
                        message: "tabs are not allowed in indentation; use spaces".into(),
                        span: self.next_char_span(),
                    });
                }
                _ => break,
            }
        }

        // Ignore blank lines and comment-only lines.
        if matches!(self.peek(), None | Some(b'\n' | b'\r')) {
            if self.peek().is_some() {
                self.consume_newline();
            }
            return Ok(());
        }
        if self.peek() == Some(b'#') {
            self.skip_comment();
            if self.peek().is_some() {
                self.consume_newline();
            }
            return Ok(());
        }

        let current = *self.indents.last().expect("indent stack always has a base");
        if width > current {
            self.indents.push(width);
            self.tokens.push(Token {
                kind: TokenKind::Indent,
                lexeme: self.source[start.start..self.position].to_string(),
                span: self.span_from(start),
            });
        } else if width < current {
            while width < *self.indents.last().expect("base indentation exists") {
                self.indents.pop();
                self.tokens.push(Token {
                    kind: TokenKind::Dedent,
                    lexeme: String::new(),
                    span: self.point_span(),
                });
            }
            if width != *self.indents.last().expect("base indentation exists") {
                return Err(LexError {
                    message: "indentation does not match any outer block".into(),
                    span: self.point_span(),
                });
            }
        }

        self.at_line_start = false;
        Ok(())
    }

    fn scan_identifier(&mut self) {
        let start = self.point_span();
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.bump();
        }
        let text = &self.source[start.start..self.position];
        let kind = match text {
            "function" => TokenKind::Function,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "unsafe" => TokenKind::Unsafe,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            _ => TokenKind::Identifier,
        };
        self.push(kind, start);
    }

    fn scan_number(&mut self) -> Result<(), LexError> {
        let start = self.point_span();
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') && self.peek_next().is_some_and(|b| b.is_ascii_digit()) {
            is_float = true;
            self.bump();
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }

        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exponent_start = self.position;
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                let error_span = Span {
                    start: exponent_start,
                    end: self.position,
                    line: self.line,
                    column: self.column.saturating_sub(self.position - exponent_start),
                    end_line: self.line,
                    end_column: self.column,
                };
                return Err(LexError {
                    message: "numeric exponent requires at least one digit".into(),
                    span: error_span,
                });
            }
            is_float = true;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        self.push(
            if is_float {
                TokenKind::Float
            } else {
                TokenKind::Integer
            },
            start,
        );
        Ok(())
    }

    fn scan_string(&mut self) -> Result<(), LexError> {
        let start = self.point_span();
        let quote = self.peek().expect("string begins with a quote");
        self.bump();

        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => {
                    return Err(LexError {
                        message: "unterminated string literal".into(),
                        span: self.span_from(start),
                    });
                }
                Some(byte) if byte == quote => {
                    self.bump();
                    self.push(TokenKind::String, start);
                    return Ok(());
                }
                Some(b'\\') => {
                    let escape_start = self.point_span();
                    self.bump();
                    match self.peek() {
                        Some(b'n' | b'r' | b't' | b'0' | b'\\' | b'"' | b'\'') => self.bump(),
                        Some(_) => {
                            let invalid_char = self.next_char_span();
                            return Err(LexError {
                                message: "unsupported escape sequence".into(),
                                span: Span {
                                    start: escape_start.start,
                                    end: invalid_char.end,
                                    line: escape_start.line,
                                    column: escape_start.column,
                                    end_line: invalid_char.end_line,
                                    end_column: invalid_char.end_column,
                                },
                            });
                        }
                        None => {
                            return Err(LexError {
                                message: "unterminated escape sequence".into(),
                                span: self.span_from(escape_start),
                            });
                        }
                    }
                }
                Some(_) => self.bump(),
            }
        }
    }

    fn scan_operator_or_punctuation(&mut self) -> Result<(), LexError> {
        let start = self.point_span();
        let first = self.peek().expect("position is in bounds");
        let second = self.peek_next();

        let (kind, width) = match (first, second) {
            (b'-', Some(b'>')) => (TokenKind::Arrow, 2),
            (b'=', Some(b'=')) => (TokenKind::Equal, 2),
            (b'!', Some(b'=')) => (TokenKind::NotEqual, 2),
            (b'<', Some(b'=')) => (TokenKind::LessEqual, 2),
            (b'>', Some(b'=')) => (TokenKind::GreaterEqual, 2),
            _ => {
                let kind = match first {
                    b'(' => TokenKind::LeftParen,
                    b')' => TokenKind::RightParen,
                    b'[' => TokenKind::LeftBracket,
                    b']' => TokenKind::RightBracket,
                    b'{' => TokenKind::LeftBrace,
                    b'}' => TokenKind::RightBrace,
                    b':' => TokenKind::Colon,
                    b',' => TokenKind::Comma,
                    b'.' => TokenKind::Dot,
                    b'+' => TokenKind::Plus,
                    b'-' => TokenKind::Minus,
                    b'*' => TokenKind::Star,
                    b'/' => TokenKind::Slash,
                    b'%' => TokenKind::Percent,
                    b'=' => TokenKind::Assign,
                    b'!' => TokenKind::Bang,
                    b'<' => TokenKind::Less,
                    b'>' => TokenKind::Greater,
                    b'&' => TokenKind::Ampersand,
                    b'|' => TokenKind::Pipe,
                    b'^' => TokenKind::Caret,
                    b'~' => TokenKind::Tilde,
                    _ => {
                        let unexpected = self.source[self.position..]
                            .chars()
                            .next()
                            .expect("position is in bounds");
                        return Err(LexError {
                            message: format!("unexpected character '{unexpected}'"),
                            span: self.next_char_span(),
                        });
                    }
                };
                (kind, 1)
            }
        };

        for _ in 0..width {
            self.bump();
        }
        self.push(kind, start);
        Ok(())
    }

    fn push(&mut self, kind: TokenKind, start: Span) {
        let span = self.span_from(start);
        self.tokens.push(Token {
            kind,
            lexeme: self.source[start.start..self.position].to_string(),
            span,
        });
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn consume_newline(&mut self) {
        let start = self.point_span();
        if self.peek() == Some(b'\r') {
            self.position += 1;
            if self.peek() == Some(b'\n') {
                self.position += 1;
            }
        } else if self.peek() == Some(b'\n') {
            self.position += 1;
        }
        self.line += 1;
        self.column = 1;
        self.at_line_start = true;
        if self
            .tokens
            .last()
            .is_some_and(|t| t.kind != TokenKind::Newline)
        {
            self.tokens.push(Token {
                kind: TokenKind::Newline,
                lexeme: String::new(),
                span: Span {
                    start: start.start,
                    end: self.position,
                    line: start.line,
                    column: start.column,
                    end_line: self.line,
                    end_column: 1,
                },
            });
        }
    }

    fn bump(&mut self) {
        if self.position < self.bytes.len() {
            self.position += 1;
            self.column += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.position + 1).copied()
    }

    fn next_char_span(&self) -> Span {
        let start = self.point_span();
        let width = self.source[self.position..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        Span {
            end: self.position + width,
            end_column: self.column + width,
            ..start
        }
    }

    fn point_span(&self) -> Span {
        Span {
            start: self.position,
            end: self.position,
            line: self.line,
            column: self.column,
            end_line: self.line,
            end_column: self.column,
        }
    }

    fn span_from(&self, start: Span) -> Span {
        Span {
            start: start.start,
            end: self.position,
            line: start.line,
            column: start.column,
            end_line: self.line,
            end_column: self.column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{tokenize, TokenKind};

    #[test]
    fn recognizes_keywords_identifiers_and_punctuation() {
        let tokens = tokenize("function main():\n    return true\n").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Function,
                TokenKind::Identifier,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Return,
                TokenKind::True,
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn recognizes_integer_float_and_exponent() {
        let tokens = tokenize("12 3.5 6e2 7.0e-1").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Integer,
                TokenKind::Float,
                TokenKind::Float,
                TokenKind::Float,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn preserves_quoted_strings_and_escapes() {
        let tokens = tokenize(r#""hello\nworld" 'mosh'"#).unwrap();
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, r#""hello\nworld""#);
        assert_eq!(tokens[1].kind, TokenKind::String);
    }

    #[test]
    fn recognizes_operators_longest_first() {
        let tokens = tokenize("a==b != c <= d >= e -> f").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier,
                TokenKind::Equal,
                TokenKind::Identifier,
                TokenKind::NotEqual,
                TokenKind::Identifier,
                TokenKind::LessEqual,
                TokenKind::Identifier,
                TokenKind::GreaterEqual,
                TokenKind::Identifier,
                TokenKind::Arrow,
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn emits_nested_indentation_and_dedentation() {
        let tokens = tokenize("if true:\n    if false:\n        return\n    return\n").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == TokenKind::Indent).count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == TokenKind::Dedent).count(), 2);
    }

    #[test]
    fn ignores_blank_and_comment_only_lines() {
        let tokens = tokenize("# heading\n\nlet value = 1 # trailing\n").unwrap();
        assert_eq!(
            tokens

                .iter()

                .filter(|t| t.kind == TokenKind::Newline)

                .count(),
            1
        );
    }

    #[test]
    fn reports_unmatched_indentation() {
        let error = tokenize("if true:\n    return\n  return\n").unwrap_err();
        assert!(error.message.contains("indentation"));
        assert_eq!(error.span.line, 3);
    }

    #[test]
    fn reports_tabs_in_indentation() {
        let error = tokenize("if true:\n\treturn\n").unwrap_err();
        assert!(error.message.contains("tabs"));
        assert_eq!(error.span.line, 2);
    }

    #[test]
    fn reports_unterminated_strings_and_bad_escapes() {
        assert!(tokenize(r#""unfinished"#)
            .unwrap_err()
            .message
            .contains("unterminated"));
        assert!(tokenize(r#""bad\q""#)
            .unwrap_err()
            .message
            .contains("escape"));
    }

    #[test]
    fn reports_unknown_characters_with_source_position() {
        let error = tokenize("let x = @").unwrap_err();
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 9);
    }

    #[test]
    fn reports_malformed_numeric_exponents() {
        let error = tokenize("42e+").unwrap_err();
        assert!(error.message.contains("exponent"));
    }

    #[test]
    fn newline_span_points_to_the_actual_line_end() {
        let tokens = tokenize("x = 1\n").unwrap();
        let newline = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Newline)
            .unwrap();
        assert_eq!(newline.span.line, 1);
        assert_eq!(newline.span.column, 6);
        assert_eq!(newline.span.end_line, 2);
    }

    #[test]
    fn supports_crlf_line_endings() {
        let tokens = tokenize("let x = 1\r\nlet y = 2\r\n").unwrap();
        assert_eq!(
            tokens

                .iter()

                .filter(|t| t.kind == TokenKind::Newline)

                .count(),
            2
        );
    }

    #[test]
    fn recognizes_all_provisional_keywords_and_identifier_prefixes() {
        let tokens = tokenize(
            "function return if else while for in let mut true false unsafe struct enum function_name",
        )
        .unwrap();
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind.clone()).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Function,
                TokenKind::Return,
                TokenKind::If,
                TokenKind::Else,
                TokenKind::While,
                TokenKind::For,
                TokenKind::In,
                TokenKind::Let,
                TokenKind::Mut,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Unsafe,
                TokenKind::Struct,
                TokenKind::Enum,
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[14].lexeme, "function_name");
    }

    #[test]
    fn recognizes_every_current_operator_and_punctuation_token() {
        let tokens = tokenize("() [] {} : , . + - * / % = == ! != < <= > >= -> & | ^ ~").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind.clone()).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::LeftBracket,
                TokenKind::RightBracket,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Colon,
                TokenKind::Comma,
                TokenKind::Dot,
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Assign,
                TokenKind::Equal,
                TokenKind::Bang,
                TokenKind::NotEqual,
                TokenKind::Less,
                TokenKind::LessEqual,
                TokenKind::Greater,
                TokenKind::GreaterEqual,
                TokenKind::Arrow,
                TokenKind::Ampersand,
                TokenKind::Pipe,
                TokenKind::Caret,
                TokenKind::Tilde,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn recognizes_conservative_decimal_and_exponent_number_forms() {
        let tokens = tokenize("0 123 1.0 2e3 4.5E-6 7E+8 42.").unwrap();
        let actual: Vec<_> = tokens
            .iter()
            .map(|token| (token.kind.clone(), token.lexeme.as_str()))
            .collect();

        assert_eq!(
            actual,
            vec![
                (TokenKind::Integer, "0"),
                (TokenKind::Integer, "123"),
                (TokenKind::Float, "1.0"),
                (TokenKind::Float, "2e3"),
                (TokenKind::Float, "4.5E-6"),
                (TokenKind::Float, "7E+8"),
                (TokenKind::Integer, "42"),
                (TokenKind::Dot, "."),
                (TokenKind::Newline, ""),
                (TokenKind::Eof, ""),
            ]
        );
    }

    #[test]
    fn preserves_unicode_string_lexemes_and_hash_characters() {
        let tokens = tokenize("let text = \"héllo # mosh\" # trailing comment\n").unwrap();
        let string = tokens
            .iter()
            .find(|token| token.kind == TokenKind::String)
            .unwrap();

        assert_eq!(string.lexeme, "\"héllo # mosh\"");
        assert_eq!(string.span.start, 11);
        assert_eq!(string.span.end, 26);
        assert_eq!(string.span.line, 1);
        assert_eq!(string.span.column, 12);
        assert_eq!(string.span.end_column, 27);
        assert_eq!(
            tokens

                .iter()

                .filter(|token| token.kind == TokenKind::Newline)

                .count(),
            1
        );
    }

    #[test]
    fn reports_unsupported_escape_with_the_full_escape_span() {
        let error = tokenize(r#""bad\q""#).unwrap_err();

        assert!(error.message.contains("escape"));
        assert_eq!(error.span.start, 4);
        assert_eq!(error.span.end, 6);
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 5);
        assert_eq!(error.span.end_column, 7);
    }

    #[test]
    fn reports_non_ascii_unknown_characters_with_utf8_byte_spans() {
        let error = tokenize("let café = 1").unwrap_err();

        assert_eq!(error.message, "unexpected character 'é'");
        assert_eq!(error.span.start, 7);
        assert_eq!(error.span.end, 9);
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 8);
        assert_eq!(error.span.end_column, 10);
    }

    #[test]
    fn indentation_and_dedentation_tokens_have_source_locations() {
        let tokens = tokenize("if true:\n    return\n").unwrap();
        let indent = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Indent)
            .unwrap();
        let dedent = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Dedent)
            .unwrap();

        assert_eq!(indent.lexeme, "    ");
        assert_eq!(indent.span.start, 9);
        assert_eq!(indent.span.end, 13);
        assert_eq!(indent.span.line, 2);
        assert_eq!(indent.span.column, 1);
        assert_eq!(indent.span.end_column, 5);
        assert_eq!(dedent.span.line, 3);
        assert_eq!(dedent.span.column, 1);
        assert_eq!(dedent.span.start, dedent.span.end);
    }

    #[test]
    fn supports_lf_crlf_and_lone_cr_line_endings() {
        let tokens = tokenize("a\nb\r\nc\rd").unwrap();
        let identifiers: Vec<_> = tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Identifier)
            .collect();

        assert_eq!(identifiers.len(), 4);
        assert_eq!(identifiers[0].span.line, 1);
        assert_eq!(identifiers[1].span.line, 2);
        assert_eq!(identifiers[2].span.line, 3);
        assert_eq!(identifiers[3].span.line, 4);
        assert_eq!(
            tokens

                .iter()

                .filter(|token| token.kind == TokenKind::Newline)

                .count(),
            4
        );
    }

    #[test]
    fn accepts_tabs_between_tokens_but_rejects_them_in_indentation() {
        let tokens = tokenize("let\tvalue = 1").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[1].kind, TokenKind::Identifier);

        let error = tokenize("if true:\n\treturn\n").unwrap_err();
        assert!(error.message.contains("tabs"));
        assert_eq!(error.span.start, 9);
        assert_eq!(error.span.end, 10);
        assert_eq!(error.span.line, 2);
        assert_eq!(error.span.column, 1);
        assert_eq!(error.span.end_column, 2);
    }

    #[test]
    fn empty_source_produces_only_an_eof_token() {
        let tokens = tokenize("").unwrap();

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Eof);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 0);
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.column, 1);
    }

    #[test]
    fn emits_only_one_logical_newline_for_final_line_and_blank_tail() {
        for source in ["value", "value\n", "value\n\n\n"] {
            let tokens = tokenize(source).unwrap();
            assert_eq!(
                tokens

                    .iter()

                    .filter(|token| token.kind == TokenKind::Newline)

                    .count(),
                1,
                "source: {source:?}"
            );
            assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
        }
    }
}
