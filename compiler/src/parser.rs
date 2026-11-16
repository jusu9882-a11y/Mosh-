use std::fmt;

use crate::lexer::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExpr {
    pub name: String,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Function {
        name: String,
        params: Vec<Parameter>,
        return_type: Option<TypeExpr>,
        body: Vec<Stmt>,
        span: Span,
    },
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeExpr>,
        value: Option<Expr>,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    For {
        name: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    Expression {
        expression: Expr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Identifier {
        name: String,
        span: Span,
    },
    Integer {
        value: String,
        span: Span,
    },
    Float {
        value: String,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Boolean {
        value: bool,
        span: Span,
    },
    Unary {
        operator: TokenKind,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        operator: TokenKind,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
        span: Span,
    },
    Member {
        object: Box<Expr>,
        name: String,
        span: Span,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}:{}",
            self.message, self.span.line, self.span.column
        )
    }
}
impl std::error::Error for ParseError {}

pub struct ParseOutput {
    pub program: Program,
    pub errors: Vec<ParseError>,
}

#[cfg(test)]
pub fn parse(tokens: &[Token]) -> Result<Program, ParseError> {
    let output = parse_recovering(tokens)?;
    match output.errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(output.program),
    }
}

/// Parses as much of the program as possible and collects independent syntax errors.
///
/// The strict `parse` API remains available for callers that require an error-free AST.
pub fn parse_recovering(tokens: &[Token]) -> Result<ParseOutput, ParseError> {
    let Some(last) = tokens.last() else {
        return Err(ParseError {
            message: "token stream is empty; expected EOF".into(),
            span: Span {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            },
        });
    };

    if last.kind != TokenKind::Eof {
        return Err(ParseError {
            message: "token stream must end with EOF".into(),
            span: last.span,
        });
    }

    if let Some(eof) = tokens[..tokens.len() - 1]
        .iter()
        .find(|token| token.kind == TokenKind::Eof)
    {
        return Err(ParseError {
            message: "unexpected token after EOF".into(),
            span: eof.span,
        });
    }

    let mut parser = Parser {
        tokens,
        current: 0,
        errors: Vec::new(),
    };
    let program = parser.parse_program();
    Ok(ParseOutput {
        program,
        errors: parser.errors,
    })
}

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Program {
        let mut statements = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Eof) {
            if self.check(&TokenKind::Indent) {
                self.errors
                    .push(self.error("unexpected indentation at top level"));
                self.advance();
                self.synchronize_statement();
                self.skip_newlines();
                continue;
            }
            if self.check(&TokenKind::Dedent) {
                self.errors.push(self.error("unexpected dedentation"));
                self.advance();
                self.skip_newlines();
                continue;
            }

            match self.statement() {
                Ok(statement) => {
                    statements.push(statement);
                    if let Err(error) = self.statement_end() {
                        self.errors.push(error);
                        self.synchronize_statement();
                    }
                }
                Err(error) => {
                    self.errors.push(error);
                    self.synchronize_statement();
                }
            }
            self.skip_newlines();
        }
        Program { statements }
    }

    fn statement(&mut self) -> Result<Stmt, ParseError> {
        match self.kind() {
            TokenKind::Function => self.function(),
            TokenKind::Let | TokenKind::Mut => self.binding(false),
            TokenKind::Return => self.return_stmt(),
            TokenKind::If => self.if_stmt(),
            TokenKind::While => self.while_stmt(),
            TokenKind::For => self.for_stmt(),
            TokenKind::Identifier if self.next_kind() == Some(&TokenKind::Colon) => {
                self.binding(true)
            }
            _ => self.assignment_or_expr(),
        }
    }

    fn function(&mut self) -> Result<Stmt, ParseError> {
        let start = self
            .expect(TokenKind::Function, "expected 'function'")?
            .span;
        let name = self.identifier("expected function name")?;
        self.expect(TokenKind::LeftParen, "expected '(' after function name")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                let pstart = self.peek().span;
                let pname = self.identifier("expected parameter name")?;
                self.expect(TokenKind::Colon, "expected ':' after parameter name")?;
                let ty = self.type_expr()?;
                params.push(Parameter {
                    name: pname,
                    span: join(pstart, ty.span),
                    ty,
                });
                if !self.take(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::RightParen) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected ')' after parameters")?;
        let return_type = if self.take(&TokenKind::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(Stmt::Function {
            name,
            params,
            return_type,
            body,
            span: join(start, self.previous().span),
        })
    }

    fn binding(&mut self, typed_without_let: bool) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        let mutable;
        let name;
        if typed_without_let {
            mutable = false;
            name = self.identifier("expected variable name")?;
        } else {
            mutable = self.take(&TokenKind::Mut);
            if !mutable {
                self.expect(TokenKind::Let, "expected 'let' or 'mut'")?;
            }
            name = self.identifier("expected variable name")?;
        }
        let ty = if self.take(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let value = if self.take(&TokenKind::Assign) {
            Some(self.expression(0)?)
        } else {
            None
        };
        if ty.is_none() && value.is_none() {
            return Err(self.error("a variable declaration needs a type or an initializer"));
        }
        let end = value
            .as_ref()
            .map(expr_span)
            .or_else(|| ty.as_ref().map(|t| t.span))
            .unwrap_or(start);
        Ok(Stmt::Let {
            name,
            mutable,
            ty,
            value,
            span: join(start, end),
        })
    }

    fn return_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::Return, "expected 'return'")?.span;
        let value = if self.at_statement_end() {
            None
        } else {
            Some(self.expression(0)?)
        };
        let end = value.as_ref().map(expr_span).unwrap_or(start);
        Ok(Stmt::Return {
            value,
            span: join(start, end),
        })
    }

    fn if_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::If, "expected 'if'")?.span;
        let condition = self.expression(0)?;
        let then_body = self.block()?;
        let else_body = if self.take(&TokenKind::Else) {
            Some(self.block()?)
        } else {
            None
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
            span: join(start, self.previous().span),
        })
    }

    fn while_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::While, "expected 'while'")?.span;
        let condition = self.expression(0)?;
        let body = self.block()?;
        Ok(Stmt::While {
            condition,
            body,
            span: join(start, self.previous().span),
        })
    }

    fn for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::For, "expected 'for'")?.span;
        let name = self.identifier("expected loop variable")?;
        self.expect(TokenKind::In, "expected 'in' after loop variable")?;
        let iterable = self.expression(0)?;
        let body = self.block()?;
        Ok(Stmt::For {
            name,
            iterable,
            body,
            span: join(start, self.previous().span),
        })
    }

    fn block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(TokenKind::Colon, "expected ':' before block")?;
        self.expect(TokenKind::Newline, "expected newline after ':'")?;
        self.skip_newlines();
        self.expect(TokenKind::Indent, "expected an indented block")?;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            match self.statement() {
                Ok(statement) => {
                    body.push(statement);
                    if let Err(error) = self.statement_end() {
                        self.errors.push(error);
                        self.synchronize_statement();
                    }
                }
                Err(error) => {
                    self.errors.push(error);
                    self.synchronize_statement();
                }
            }
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent, "expected end of indented block")?;
        Ok(body)
    }

    fn assignment_or_expr(&mut self) -> Result<Stmt, ParseError> {
        let target = self.expression(0)?;
        if self.take(&TokenKind::Assign) {
            let value = self.expression(0)?;
            Ok(Stmt::Assign {
                span: join(expr_span(&target), expr_span(&value)),
                target,
                value,
            })
        } else {
            let span = expr_span(&target);
            Ok(Stmt::Expression {
                expression: target,
                span,
            })
        }
    }

    fn type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        if !self.check(&TokenKind::Identifier) {
            return Err(self.error("expected a type name"));
        }
        let token = self.advance().clone();
        Ok(TypeExpr {
            name: token.lexeme,
            span: token.span,
        })
    }

    fn expression(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut left = self.prefix()?;
        loop {
            if self.check(&TokenKind::LeftParen) {
                self.advance();
                let mut args = Vec::new();
                if !self.check(&TokenKind::RightParen) {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RightParen) {
                            break;
                        }
                    }
                }
                let close = self
                    .expect(TokenKind::RightParen, "expected ')' after arguments")?
                    .span;
                let span = join(expr_span(&left), close);
                left = Expr::Call {
                    callee: Box::new(left),
                    arguments: args,
                    span,
                };
                continue;
            }
            if self.take(&TokenKind::Dot) {
                let name = self.identifier("expected member name after '.'")?;
                let span = join(expr_span(&left), self.previous().span);
                left = Expr::Member {
                    object: Box::new(left),
                    name,
                    span,
                };
                continue;
            }
            if self.take(&TokenKind::LeftBracket) {
                let index = self.expression(0)?;
                let close = self
                    .expect(TokenKind::RightBracket, "expected ']' after index")?
                    .span;
                let span = join(expr_span(&left), close);
                left = Expr::Index {
                    object: Box::new(left),
                    index: Box::new(index),
                    span,
                };
                continue;
            }
            let Some((prec, right_assoc)) = precedence(&self.kind()) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            let op = self.advance().kind.clone();
            let right = self.expression(if right_assoc { prec } else { prec + 1 })?;
            let span = join(expr_span(&left), expr_span(&right));
            left = Expr::Binary {
                left: Box::new(left),
                operator: op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier => Ok(Expr::Identifier {
                name: token.lexeme,
                span: token.span,
            }),
            TokenKind::Integer => Ok(Expr::Integer {
                value: token.lexeme,
                span: token.span,
            }),
            TokenKind::Float => Ok(Expr::Float {
                value: token.lexeme,
                span: token.span,
            }),
            TokenKind::String => Ok(Expr::String {
                value: decode_string_literal(&token.lexeme),
                span: token.span,
            }),
            TokenKind::True => Ok(Expr::Boolean {
                value: true,
                span: token.span,
            }),
            TokenKind::False => Ok(Expr::Boolean {
                value: false,
                span: token.span,
            }),
            TokenKind::Minus | TokenKind::Bang | TokenKind::Ampersand | TokenKind::Star => {
                let operand = self.expression(8)?;
                let span = join(token.span, expr_span(&operand));
                Ok(Expr::Unary {
                    operator: token.kind,
                    operand: Box::new(operand),
                    span,
                })
            }
            TokenKind::LeftParen => {
                let expr = self.expression(0)?;
                self.expect(TokenKind::RightParen, "expected ')' after expression")?;
                Ok(expr)
            }
            _ => Err(ParseError {
                message: format!("expected expression, found {:?}", token.kind),
                span: token.span,
            }),
        }
    }

    fn statement_end(&mut self) -> Result<(), ParseError> {
        if self.take(&TokenKind::Newline)
            || self.check(&TokenKind::Dedent)
            || self.check(&TokenKind::Eof)
            || self.previous().kind == TokenKind::Dedent
        {
            Ok(())
        } else {
            Err(self.error("expected end of statement"))
        }
    }
    fn at_statement_end(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        )
    }
    fn skip_newlines(&mut self) {
        while self.take(&TokenKind::Newline) {}
    }

    /// Resume at the next logical statement boundary without consuming a block's
    /// closing DEDENT or the program's EOF.
    fn synchronize_statement(&mut self) {
        if self.previous().kind == TokenKind::Newline {
            return;
        }

        while !matches!(
            self.kind(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        ) {
            self.advance();
        }
        self.take(&TokenKind::Newline);

        // If a malformed compound statement left its indented body behind,
        // discard that body as one balanced unit rather than misparsing it as
        // statements belonging to the surrounding scope.
        if self.take(&TokenKind::Indent) {
            let mut depth = 1usize;
            while depth > 0 && !self.check(&TokenKind::Eof) {
                match self.kind() {
                    TokenKind::Indent => {
                        depth += 1;
                        self.advance();
                    }
                    TokenKind::Dedent => {
                        depth -= 1;
                        self.advance();
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
        }
    }
    fn identifier(&mut self, message: &str) -> Result<String, ParseError> {
        if self.check(&TokenKind::Identifier) {
            Ok(self.advance().lexeme.clone())
        } else {
            Err(self.error(message))
        }
    }
    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<&Token, ParseError> {
        if self.check(&kind) {
            Ok(self.advance())
        } else {
            Err(self.error(message))
        }
    }
    fn take(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn check(&self, kind: &TokenKind) -> bool {
        self.kind() == *kind
    }
    fn kind(&self) -> TokenKind {
        self.peek().kind.clone()
    }
    fn next_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.current + 1).map(|t| &t.kind)
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.current.min(self.tokens.len() - 1)]
    }
    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }
    fn advance(&mut self) -> &Token {
        let index = self.current;
        if self.current < self.tokens.len() - 1 {
            self.current += 1;
        }
        &self.tokens[index]
    }
    fn error(&self, message: &str) -> ParseError {
        ParseError {
            message: message.into(),
            span: self.peek().span,
        }
    }
}

fn precedence(kind: &TokenKind) -> Option<(u8, bool)> {
    let p = match kind {
        TokenKind::Pipe => 1,
        TokenKind::Caret => 2,
        TokenKind::Ampersand => 3,
        TokenKind::Equal | TokenKind::NotEqual => 4,
        TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => 5,
        TokenKind::Plus | TokenKind::Minus => 6,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 7,
        _ => return None,
    };
    Some((p, false))
}

fn decode_string_literal(raw: &str) -> String {
    let Some(body) = raw.get(1..raw.len().saturating_sub(1)) else {
        return String::new();
    };
    let mut characters = body.chars();
    let mut decoded = String::with_capacity(body.len());
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            break;
        };
        decoded.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            other => other,
        });
    }
    decoded
}

fn expr_span(expr: &Expr) -> Span {
    match expr {
        Expr::Identifier { span, .. }
        | Expr::Integer { span, .. }
        | Expr::Float { span, .. }
        | Expr::String { span, .. }
        | Expr::Boolean { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Call { span, .. }
        | Expr::Member { span, .. }
        | Expr::Index { span, .. } => *span,
    }
}
fn join(a: Span, b: Span) -> Span {
    Span {
        start: a.start,
        end: b.end,
        line: a.line,
        column: a.column,
        end_line: b.end_line,
        end_column: b.end_column,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Expr, Stmt};
    use crate::lexer::tokenize;

    fn parsed(source: &str) -> Result<super::Program, super::ParseError> {
        let tokens = tokenize(source).expect("source should tokenize");
        parse(&tokens)
    }

    #[test]
    fn recovering_parser_reports_multiple_independent_errors_and_keeps_valid_statements() {
        let tokens = tokenize("let first = )\nlet valid = 2\nlet second = ]\nlet last = 4\n")
            .expect("source should tokenize");
        let output = super::parse_recovering(&tokens).expect("token stream should be valid");

        assert_eq!(output.errors.len(), 2);
        assert_eq!(output.errors[0].span.line, 1);
        assert_eq!(output.errors[1].span.line, 3);
        assert_eq!(output.program.statements.len(), 2);
        assert!(matches!(&output.program.statements[0], Stmt::Let { name, .. } if name == "valid"));
        assert!(matches!(&output.program.statements[1], Stmt::Let { name, .. } if name == "last"));
    }

    #[test]
    fn recovering_parser_collects_errors_inside_blocks_and_continues_after_them() {
        let source = "function main():\n    let broken = )\n    let good = 1\n    let also_broken = ]\n    return good\nlet outside = 3\n";
        let tokens = tokenize(source).expect("source should tokenize");
        let output = super::parse_recovering(&tokens).expect("token stream should be valid");

        assert_eq!(output.errors.len(), 2);
        assert_eq!(output.errors[0].span.line, 2);
        assert_eq!(output.errors[1].span.line, 4);
        assert_eq!(output.program.statements.len(), 2);
        match &output.program.statements[0] {
            Stmt::Function { body, .. } => {
                assert_eq!(body.len(), 2);
                assert!(matches!(&body[0], Stmt::Let { name, .. } if name == "good"));
                assert!(matches!(&body[1], Stmt::Return { .. }));
            }
            _ => panic!("expected function"),
        }
        assert!(
            matches!(&output.program.statements[1], Stmt::Let { name, .. } if name == "outside")
        );
    }

    #[test]
    fn strict_parser_still_returns_the_first_recovered_syntax_error() {
        let error = parsed("let broken = )\nlet also_broken = ]\n").unwrap_err();
        assert_eq!(error.span.line, 1);
    }

    #[test]
    fn rejects_an_empty_token_stream_without_panicking() {
        let error = parse(&[]).unwrap_err();
        assert!(error.message.contains("token stream is empty"));
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 1);
    }

    #[test]
    fn rejects_a_token_stream_without_a_final_eof() {
        let mut tokens = tokenize("let value = 1\n").unwrap();
        tokens.pop();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("must end with EOF"));
        assert_eq!(error.span.line, 1);
    }

    #[test]
    fn rejects_tokens_after_an_embedded_eof() {
        let mut tokens = tokenize("let value = 1\n").unwrap();
        let eof = tokens.pop().unwrap();
        tokens.push(eof.clone());
        tokens.push(eof);
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("after EOF"));
    }

    #[test]
    fn rejects_malformed_declarations_and_function_signatures() {
        for source in [
            "let = 1\n",
            "let value =\n",
            "function add(value):\n    return value\n",
            "function add(value: Int -> Int:\n    return value\n",
        ] {
            assert!(parsed(source).is_err(), "source should fail: {source:?}");
        }
    }

    #[test]
    fn rejects_malformed_calls_indexes_and_member_access() {
        for source in [
            "call(1,,2)\n",
            "let value = items[]\n",
            "let value = item.\n",
            "let value = call(1\n",
            "let value = items[0\n",
        ] {
            assert!(parsed(source).is_err(), "source should fail: {source:?}");
        }
    }

    #[test]
    fn reports_the_full_span_of_a_variable_declaration() {
        let p = parsed("let value = 123\n").unwrap();
        match &p.statements[0] {
            Stmt::Let { span, .. } => {
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 15);
                assert_eq!(span.line, 1);
                assert_eq!(span.column, 1);
                assert_eq!(span.end_line, 1);
                assert_eq!(span.end_column, 16);
            }
            _ => panic!("expected variable declaration"),
        }
    }

    #[test]
    fn decodes_string_literal_quotes_and_escape_sequences() {
        let program = parsed(r#"let message = "hello\n\"mosh\"\\world""#).unwrap();
        let Stmt::Let {
            value: Some(Expr::String { value, .. }),
            ..
        } = &program.statements[0]
        else {
            panic!("expected a string binding");
        };
        assert_eq!(value, "hello\n\"mosh\"\\world");
    }

    #[test]
    fn parses_function_signature_and_return() {
        let p = parsed("function add(a: Int, b: Int) -> Int:\n    return a + b\n").unwrap();
        match &p.statements[0] {
            Stmt::Function {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(return_type.as_ref().unwrap().name, "Int");
                assert!(matches!(&body[0], Stmt::Return { .. }));
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parses_if_else_and_bindings() {
        let p = parsed("if true:\n    let x: Int = 1\nelse:\n    let x: Int = 2\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::If {
                else_body: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn parses_operator_precedence() {
        let p = parsed("let result = 1 + 2 * 3\n").unwrap();
        match &p.statements[0] {
            Stmt::Let {
                value: Some(Expr::Binary { right, .. }),
                ..
            } => assert!(matches!(
                right.as_ref(),
                Expr::Binary {
                    operator: crate::lexer::TokenKind::Star,
                    ..
                }
            )),
            _ => panic!("unexpected AST"),
        }
    }

    #[test]
    fn parses_calls_and_member_access() {
        let p = parsed("print(user.name)\n").unwrap();
        assert!(
            matches!(&p.statements[0], Stmt::Expression { expression: Expr::Call { arguments, .. }, .. } if matches!(arguments[0], Expr::Member { .. }))
        );
    }

    #[test]
    fn parses_mutable_and_inferred_bindings() {
        let p = parsed("mut count: Int = 0\nlet answer = 42\n").unwrap();
        assert!(matches!(&p.statements[0], Stmt::Let { mutable: true, .. }));
        assert!(matches!(&p.statements[1], Stmt::Let { ty: None, .. }));
    }

    #[test]
    fn parses_loops() {
        let p = parsed("while true:\n    work()\nfor item in items:\n    print(item)\n").unwrap();
        assert!(matches!(&p.statements[0], Stmt::While { .. }));
        assert!(matches!(&p.statements[1], Stmt::For { .. }));
    }

    #[test]
    fn parses_multiple_top_level_functions() {
        let p = parsed("function first():\n    return\nfunction second():\n    return\n").unwrap();
        assert_eq!(p.statements.len(), 2);
        assert!(matches!(&p.statements[0], Stmt::Function { name, .. } if name == "first"));
        assert!(matches!(&p.statements[1], Stmt::Function { name, .. } if name == "second"));
    }

    #[test]
    fn parses_indexing_and_unary_expressions() {
        let p = parsed("let value = -items[0]\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::Let { value: Some(Expr::Unary { operand, .. }), .. }
                if matches!(operand.as_ref(), Expr::Index { .. })
        ));
    }

    #[test]
    fn rejects_missing_indented_block() {
        let e = parsed("function main():\nreturn\n").unwrap_err();
        assert!(e.message.contains("indented block"));
    }

    #[test]
    fn rejects_missing_closing_parenthesis() {
        let e = parsed("print(1\n").unwrap_err();
        assert!(e.message.contains("expected ')'"));
    }

    #[test]
    fn rejects_unexpected_top_level_dedent() {
        let e = parsed("    let x = 1\n").unwrap_err();
        assert!(e.message.contains("indentation"));
    }

    #[test]
    fn reports_invalid_expression_location() {
        let e = parsed("let x = )\n").unwrap_err();
        assert_eq!(e.span.line, 1);
    }

    #[test]
    fn parses_chained_calls_members_and_indexes() {
        let p = parsed("let result = factory().items[0].name\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::Let {
                value: Some(Expr::Member { object, name, .. }),
                ..
            } if name == "name"
                && matches!(object.as_ref(), Expr::Index { .. })
        ));
    }

    #[test]
    fn parses_subtraction_as_left_associative() {
        let p = parsed("let value = 10 - 3 - 2\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::Let {
                value: Some(Expr::Binary { left, operator: crate::lexer::TokenKind::Minus, .. }),
                ..
            } if matches!(left.as_ref(), Expr::Binary {
                operator: crate::lexer::TokenKind::Minus, ..
            })
        ));
    }

    #[test]
    fn parses_assignment_to_indexed_member() {
        let p = parsed("items[0].value = 7\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::Assign {
                target: Expr::Member { object, name, .. },
                value: Expr::Integer { value, .. },
                ..
            } if name == "value"
                && value == "7"
                && matches!(object.as_ref(), Expr::Index { .. })
        ));
    }

    #[test]
    fn parses_bare_return_inside_function() {
        let p = parsed("function stop():\n    return\n").unwrap();
        assert!(matches!(
            &p.statements[0],
            Stmt::Function { body, .. }
                if matches!(&body[0], Stmt::Return { value: None, .. })
        ));
    }

    #[test]
    fn accepts_an_empty_program() {
        let p = parsed("").unwrap();
        assert!(p.statements.is_empty());
    }

    #[test]
    fn rejects_a_missing_right_hand_expression() {
        let error = parsed("let total = 1 +\n").unwrap_err();
        assert!(error.message.contains("expected expression"));
        assert_eq!(error.span.line, 1);
    }

    #[test]
    fn rejects_two_statements_on_one_line_without_a_separator() {
        let error = parsed("let first = 1 let second = 2\n").unwrap_err();
        assert!(error.message.contains("end of statement"));
    }

    #[test]
    fn parses_nested_control_flow_blocks() {
        let source = "function main():\n    if true:\n        while false:\n            work()\n    else:\n        stop()\n";
        let program = parsed(source).unwrap();
        assert!(matches!(
            &program.statements[0],
            Stmt::Function { body, .. }
                if matches!(
                    &body[0],
                    Stmt::If {
                        then_body,
                        else_body: Some(_),
                        ..
                    } if matches!(&then_body[0], Stmt::While { .. })
                )
        ));
    }

    #[test]
    fn parses_boolean_comparison_and_arithmetic_precedence_tiers() {
        let program = parsed("let result = 1 + 2 * 3 < 8 == true | false\n").unwrap();
        assert!(matches!(
            &program.statements[0],
            Stmt::Let {
                value: Some(Expr::Binary {
                    operator: crate::lexer::TokenKind::Pipe,
                    left,
                    ..
                }),
                ..
            } if matches!(
                left.as_ref(),
                Expr::Binary {
                    operator: crate::lexer::TokenKind::Equal,
                    left: comparison,
                    ..
                } if matches!(
                    comparison.as_ref(),
                    Expr::Binary {
                        operator: crate::lexer::TokenKind::Less,
                        left: arithmetic,
                        ..
                    } if matches!(
                        arithmetic.as_ref(),
                        Expr::Binary {
                            operator: crate::lexer::TokenKind::Plus,
                            right,
                            ..
                        } if matches!(
                            right.as_ref(),
                            Expr::Binary {
                                operator: crate::lexer::TokenKind::Star,
                                ..
                            }
                        )
                    )
                )
            )
        ));
    }

    #[test]
    fn parses_parentheses_as_an_explicit_precedence_override() {
        let program = parsed("let result = (1 + 2) * 3\n").unwrap();
        assert!(matches!(
            &program.statements[0],
            Stmt::Let {
                value: Some(Expr::Binary {
                    operator: crate::lexer::TokenKind::Star,
                    left,
                    ..
                }),
                ..
            } if matches!(
                left.as_ref(),
                Expr::Binary {
                    operator: crate::lexer::TokenKind::Plus,
                    ..
                }
            )
        ));
    }

    #[test]
    fn reports_precise_column_for_invalid_expression() {
        let error = parsed("let value = )\n").unwrap_err();
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 13);
    }

    #[test]
    fn accepts_trailing_commas_in_calls_and_function_parameters() {
        let program =
            parsed("function combine(left: Int, right: Int,):\n    return merge(left, right,)\n")
                .unwrap();
        assert!(matches!(
            &program.statements[0],
            Stmt::Function { params, body, .. }
                if params.len() == 2
                    && matches!(
                        &body[0],
                        Stmt::Return {
                            value: Some(Expr::Call { arguments, .. }),
                            ..
                        } if arguments.len() == 2
                    )
        ));
    }
}
