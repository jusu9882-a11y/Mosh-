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
    /// Exact source spelling. Synthetic layout/EOF tokens have an empty lexeme.
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
                Some(b' ' | b'\\t') => self.bump(),
                Some(b'#') => self.skip_comment(),
                Some(b'\\n' | b'\\r') => self.consume_newline(),
                Some(b'"') | Some(b'\\'') => self.scan_string()?,
                Some(b'0'..=b'9') => self.scan_number()?,
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => self.scan_identifier(),
                Some(_) => self.scan_operator_or_punctuation()?,
                None => break,
            }
        }

        let eof_span = self.point_span();
        if self.tokens.last().is_some_and(|token| {
            !matches!(
                token.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
            )
        }) {
            // Give a final non-empty source line the same terminator as a newline-ended line.
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
                b'\\t' => {
                    return Err(LexError {
                        message: "tabs are not allowed in indentation; use spaces".into(),
                        span: self.span_with_width(1),
                    });
                }
                _ => break,
            }
        }

        // Blank and comment-only lines do not affect the indentation stack.
        if matches!(self.peek(), None | Some(b'\\n' | b'\\r')) {
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

        let current = *self.indents.last().expect("indent stack has a base level");
        if width > current {
            self.indents.push(width);
            self.tokens.push(Token {
                kind: TokenKind::Indent,
                lexeme: self.source[start.start..self.position].to_string(),
                span: self.span_from(start),
            });
        } else if width < current {
            while width < *self.indents.last().expect("indent stack has a base level") {
                self.indents.pop();
                self.tokens.push(Token {
                    kind: TokenKind::Dedent,
                    lexeme: String::new(),
                    span: self.point_span(),
                });
            }
            if width != *self.indents.last().expect("indent stack has a base level") {
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
        // A decimal point belongs to a float only when followed by a digit.
        if self.peek() == Some(b'.') && self.peek_next().is_some_and(|byte| byte.is_ascii_digit())
        {
            is_float = true;
            self.bump();
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }

        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exponent_start = self.point_span();
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(LexError {
                    message: "numeric exponent requires at least one digit".into(),
                    span: self.span_from(exponent_start),
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
                None | Some(b'\\n' | b'\\r') => {
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
                Some(b'\\\\') => {
                    let escape_start = self.point_span();
                    self.bump();
                    match self.peek() {
                        Some(b'n' | b'r' | b't' | b'0' | b'\\\\' | b'"' | b'\\'') => {
                            self.bump();
                        }
                        Some(_) => {
                            let width = self
                                .source
                                .get(self.position..)
                                .and_then(|remaining| remaining.chars().next())
                                .map(char::len_utf8)
                                .unwrap_or(1);
                            let escaped = &self.source[self.position..self.position + width];
                            return Err(LexError {
                                message: format!("unsupported escape sequence '\\{escaped}'"),
                                span: Span {
                                    start: escape_start.start,
                                    end: self.position + width,
                                    line: escape_start.line,
                                    column: escape_start.column,
                                    end_line: self.line,
                                    end_column: self.column + width,
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
                        let character = self.source[self.position..]
                            .chars()
                            .next()
                            .expect("position is before the end of source");
                        let span = self.span_with_width(character.len_utf8());
                        return Err(LexError {
                            message: format!("unexpected character '{character}'"),
                            span,
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
        while !matches!(self.peek(), None | Some(b'\\n' | b'\\r')) {
            self.bump();
        }
    }

    fn consume_newline(&mut self) {
        let start = self.point_span();
        if self.peek() == Some(b'\\r') {
            self.position += 1;
            if self.peek() == Some(b'\\n') {
                self.position += 1;
            }
        } else if self.peek() == Some(b'\\n') {
            self.position += 1;
        }

        let lexeme = self.source[start.start..self.position].to_string();
        self.line += 1;
        self.column = 1;
        self.at_line_start = true;

        // Collapse blank/comment-only lines; retain the original line ending on emitted tokens.
        if self
            .tokens
            .last()
            .is_some_and(|token| token.kind != TokenKind::Newline)
        {
            self.tokens.push(Token {
                kind: TokenKind::Newline,
                lexeme,
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

    fn span_with_width(&self, width: usize) -> Span {
        Span {
            start: self.position,
            end: self.position + width,
            line: self.line,
            column: self.column,
            end_line: self.line,
            end_column: self.column + width,
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

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .expect("source should tokenize")
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn recognizes_provisional_keywords_and_ascii_identifiers() {
        assert_eq!(
            kinds(
                "function function_name return returning if iff else while for in let mut true false unsafe struct enum"
            ),
            vec![
                TokenKind::Function,
                TokenKind::Identifier,
                TokenKind::Return,
                TokenKind::Identifier,
                TokenKind::If,
                TokenKind::Identifier,
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
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn recognizes_all_current_operators_and_punctuation() {
        assert_eq!(
            kinds("()[]{}:,. + - * / % = == ! != < <= > >= -> & | ^ ~"),
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
    fn recognizes_integer_decimal_and_exponent_forms() {
        let tokens = tokenize("0 42 3.14 6e2 7.0e-1 1. 1..2").unwrap();
        let values: Vec<_> = tokens
            .iter()
            .map(|token| (token.kind.clone(), token.lexeme.as_str()))
            .collect();

        assert_eq!(
            values,
            vec![
                (TokenKind::Integer, "0"),
                (TokenKind::Integer, "42"),
                (TokenKind::Float, "3.14"),
                (TokenKind::Float, "6e2"),
                (TokenKind::Float, "7.0e-1"),
                (TokenKind::Integer, "1"),
                (TokenKind::Dot, "."),
                (TokenKind::Integer, "1"),
                (TokenKind::Dot, "."),
                (TokenKind::Dot, "."),
                (TokenKind::Integer, "2"),
                (TokenKind::Newline, ""),
                (TokenKind::Eof, ""),
            ]
        );
    }

    #[test]
    fn preserves_single_and_double_quoted_strings_and_supported_escapes() {
        let source = r#""a\n\r\t\0\\\"\'" 'b\''"#;
        let tokens = tokenize(source).unwrap();

        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[1].kind, TokenKind::String);
        assert_eq!(
            &source[tokens[0].span.start..tokens[0].span.end],
            tokens[0].lexeme
        );
        assert_eq!(
            &source[tokens[1].span.start..tokens[1].span.end],
            tokens[1].lexeme
        );
        assert_eq!(tokens[2].kind, TokenKind::Newline);
        assert_eq!(tokens[3].kind, TokenKind::Eof);
    }

    #[test]
    fn emits_nested_indentation_and_dedentation_in_order() {
        assert_eq!(
            kinds("if true:\n    if false:\n        return\n    return\nreturn\n"),
            vec![
                TokenKind::If,
                TokenKind::True,
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::If,
                TokenKind::False,
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Return,
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Return,
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Return,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn emits_all_required_dedents_at_eof_without_a_final_newline() {
        assert_eq!(
            kinds("if true:\n    if true:\n        return"),
            vec![
                TokenKind::If,
                TokenKind::True,
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::If,
                TokenKind::True,
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Return,
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn ignores_blank_and_comment_only_lines_and_recognizes_trailing_comments() {
        let tokens = tokenize(
            "# heading\n\nlet value = 1 # trailing\n  # indented comment\n\nlet next = 2\n",
        )
        .unwrap();

        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::Newline)
                .count(),
            2
        );
        assert!(!tokens.iter().any(|token| {
            matches!(token.kind, TokenKind::Indent | TokenKind::Dedent)
        }));
    }

    #[test]
    fn reports_indentation_that_does_not_match_an_outer_level() {
        let error = tokenize("if true:\n    return\n  return\n").unwrap_err();
        assert!(error.message.contains("indentation"));
        assert_eq!(error.span.line, 3);
        assert_eq!(error.span.column, 3);
    }

    #[test]
    fn reports_tabs_in_indentation_but_allows_horizontal_tabs_between_tokens() {
        let error = tokenize("if true:\n\treturn\n").unwrap_err();
        assert!(error.message.contains("tabs"));
        assert_eq!(error.span.line, 2);
        assert_eq!(error.span.column, 1);

        assert_eq!(
            kinds("let\tname = 1"),
            vec![
                TokenKind::Let,
                TokenKind::Identifier,
                TokenKind::Assign,
                TokenKind::Integer,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn reports_unterminated_strings_at_eof_and_before_a_newline() {
        let eof_error = tokenize("\"unfinished").unwrap_err();
        assert!(eof_error.message.contains("unterminated string"));
        assert_eq!(eof_error.span.line, 1);
        assert_eq!(eof_error.span.column, 1);

        let newline_error = tokenize("\"first line\nsecond line\"").unwrap_err();
        assert!(newline_error.message.contains("unterminated string"));
        assert_eq!(newline_error.span.line, 1);
    }

    #[test]
    fn reports_unsupported_and_unterminated_escape_sequences_with_spans() {
        let error = tokenize(r#""bad\q""#).unwrap_err();
        assert!(error.message.contains("unsupported escape"));
        assert_eq!(error.span.start, 4);
        assert_eq!(error.span.end, 6);
        assert_eq!(error.span.column, 5);
        assert_eq!(error.span.end_column, 7);

        let trailing_escape = tokenize("\"unfinished\\").unwrap_err();
        assert!(trailing_escape.message.contains("unterminated escape"));
    }

    #[test]
    fn reports_malformed_numeric_exponents_with_the_exponent_span() {
        let error = tokenize("42e+").unwrap_err();
        assert!(error.message.contains("exponent"));
        assert_eq!(error.span.start, 2);
        assert_eq!(error.span.end, 4);
        assert_eq!(error.span.column, 3);
        assert_eq!(error.span.end_column, 5);

        assert!(tokenize("1E").unwrap_err().message.contains("exponent"));
    }

    #[test]
    fn reports_unknown_ascii_and_unicode_characters_with_full_spans() {
        let ascii_error = tokenize("let x = @").unwrap_err();
        assert_eq!(ascii_error.span.line, 1);
        assert_eq!(ascii_error.span.column, 9);
        assert_eq!(ascii_error.span.end_column, 10);
        assert!(ascii_error.message.contains("@"));

        let unicode_error = tokenize("let x = é").unwrap_err();
        assert_eq!(unicode_error.span.start, 8);
        assert_eq!(unicode_error.span.end, 10);
        assert_eq!(unicode_error.span.column, 9);
        assert_eq!(unicode_error.span.end_column, 11);
        assert!(unicode_error.message.contains("é"));
    }

    #[test]
    fn records_token_spans_as_utf8_byte_offsets_and_byte_columns() {
        let source = "let x = \"é\"\n";
        let tokens = tokenize(source).unwrap();
        let string = tokens
            .iter()
            .find(|token| token.kind == TokenKind::String)
            .unwrap();

        assert_eq!(string.span.start, 8);
        assert_eq!(string.span.end, 12);
        assert_eq!(string.span.line, 1);
        assert_eq!(string.span.column, 9);
        assert_eq!(string.span.end_line, 1);
        assert_eq!(string.span.end_column, 13);

        let newline = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Newline)
            .unwrap();
        assert_eq!(newline.span.start, 12);
        assert_eq!(newline.span.end, 13);
        assert_eq!(newline.span.end_line, 2);
        assert_eq!(newline.lexeme, "\n");
    }

    #[test]
    fn preserves_crlf_and_cr_newline_spellings_and_locations() {
        let crlf_tokens = tokenize("x\r\n").unwrap();
        let crlf = crlf_tokens
            .iter()
            .find(|token| token.kind == TokenKind::Newline)
            .unwrap();
        assert_eq!(crlf.lexeme, "\r\n");
        assert_eq!(crlf.span.start, 1);
        assert_eq!(crlf.span.end, 3);
        assert_eq!(crlf.span.line, 1);
        assert_eq!(crlf.span.column, 2);
        assert_eq!(crlf.span.end_line, 2);
        assert_eq!(crlf.span.end_column, 1);

        let cr_tokens = tokenize("x\r").unwrap();
        let cr = cr_tokens
            .iter()
            .find(|token| token.kind == TokenKind::Newline)
            .unwrap();
        assert_eq!(cr.lexeme, "\r");
        assert_eq!(cr.span.end_line, 2);
    }

    #[test]
    fn emits_eof_for_empty_source_and_a_synthetic_newline_for_unterminated_lines() {
        let empty = tokenize("").unwrap();
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].kind, TokenKind::Eof);
        assert_eq!(empty[0].span.line, 1);
        assert_eq!(empty[0].span.column, 1);

        let tokens = tokenize("let answer = 42").unwrap();
        assert_eq!(tokens[tokens.len() - 2].kind, TokenKind::Newline);
        assert!(tokens[tokens.len() - 2].lexeme.is_empty());
        assert_eq!(tokens[tokens.len() - 1].kind, TokenKind::Eof);
    }

}
