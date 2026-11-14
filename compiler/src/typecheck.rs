use std::collections::HashMap;
use std::fmt;

use crate::lexer::{Span, TokenKind};
use crate::parser::{Expr, Program, Stmt, TypeExpr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Unknown,
}

impl Type {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Int" => Some(Self::Int),
            "Float" => Some(Self::Float),
            "Bool" => Some(Self::Bool),
            "String" => Some(Self::String),
            "Unit" => Some(Self::Unit),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Bool => "Bool",
            Self::String => "String",
            Self::Unit => "Unit",
            Self::Unknown => "an unknown type",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}:{}", self.message, self.span.line, self.span.column)
    }
}
impl std::error::Error for TypeError {}

#[derive(Debug, Clone)]
struct FunctionType {
    parameters: Vec<Type>,
    result: Type,
}

#[derive(Debug, Clone, Copy)]
struct Variable {
    ty: Type,
    mutable: bool,
}

#[derive(Debug, Default)]
struct Scope {
    variables: HashMap<String, Variable>,
}

#[derive(Debug, Default)]
struct Checker {
    scopes: Vec<Scope>,
    functions: HashMap<String, FunctionType>,
    errors: Vec<TypeError>,
    return_types: Vec<Type>,
}

pub fn check(program: &Program) -> Result<(), Vec<TypeError>> {
    let mut checker = Checker::default();
    checker.push_scope();

    // Function signatures are collected first, matching module-level
    // forward-reference and mutual-recursion name resolution.
    for statement in &program.statements {
        if let Stmt::Function { name, params, return_type, span, .. } = statement {
            let parameters = params.iter().filter_map(|parameter| {
                checker.resolve_type(&parameter.ty).map(|ty| (parameter.name.clone(), ty))
            }).collect::<Vec<_>>();
            let result = return_type.as_ref().map(|ty| checker.resolve_type(ty)).unwrap_or(Some(Type::Unit));
            if parameters.len() == params.len() {
                if let Some(result) = result {
                    if checker.functions.contains_key(name) {
                        checker.error(format!("duplicate function type declaration for '{name}'"), *span);
                    } else {
                        checker.functions.insert(name.clone(), FunctionType {
                            parameters: parameters.into_iter().map(|(_, ty)| ty).collect(),
                            result,
                        });
                    }
                }
            }
        }
    }

    for statement in &program.statements {
        checker.statement(statement);
    }

    if checker.errors.is_empty() { Ok(()) } else { Err(checker.errors) }
}

impl Checker {
    fn push_scope(&mut self) { self.scopes.push(Scope::default()); }
    fn pop_scope(&mut self) { self.scopes.pop(); }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(TypeError { message: message.into(), span });
    }

    fn resolve_type(&mut self, syntax: &TypeExpr) -> Option<Type> {
        match Type::from_name(&syntax.name) {
            Some(ty) => Some(ty),
            None => {
                self.error(format!("unknown type '{}'; expected Int, Float, Bool, String, or Unit", syntax.name), syntax.span);
                None
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Variable> {
        self.scopes.iter().rev().find_map(|scope| scope.variables.get(name).copied())
    }

    fn define(&mut self, name: &str, variable: Variable, span: Span) {
        let duplicate = self.scopes.last().expect("checker always has a scope").variables.contains_key(name);
        if duplicate {
            self.error(format!("duplicate typed binding '{name}'"), span);
        } else {
            self.scopes.last_mut().expect("checker always has a scope")
                .variables.insert(name.to_owned(), variable);
        }
    }

    fn expect_type(&mut self, actual: Type, expected: Type, span: Span, context: &str) {
        if actual != Type::Unknown && actual != expected {
            self.error(format!("{context}: expected {expected}, found {actual}"), span);
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Function { name, params, return_type, body, span } => {
                let result = return_type.as_ref()
                    .and_then(|ty| self.resolve_type(ty))
                    .unwrap_or(Type::Unit);
                let signature = self.functions.get(name).cloned();
                self.push_scope();
                self.return_types.push(result);
                for (index, parameter) in params.iter().enumerate() {
                    if let Some(ty) = self.resolve_type(&parameter.ty) {
                        if let Some(sig) = &signature {
                            if sig.parameters.get(index).copied() != Some(ty) {
                                self.error(format!("internal signature mismatch for parameter '{}'", parameter.name), parameter.span);
                            }
                        }
                        self.define(&parameter.name, Variable { ty, mutable: false }, parameter.span);
                    }
                }
                for child in body { self.statement(child); }
                self.return_types.pop();
                self.pop_scope();
                let _ = span;
            }
            Stmt::Let { name, mutable, ty, value, span } => {
                let annotation = ty.as_ref().and_then(|syntax| self.resolve_type(syntax));
                let inferred = value.as_ref().map(|expr| self.expression(expr));
                let final_type = match (annotation, inferred) {
                    (Some(expected), Some(actual)) => {
                        self.expect_type(actual, expected, value.as_ref().map(expr_span).unwrap_or(*span), "initializer type mismatch");
                        expected
                    }
                    (Some(expected), None) => expected,
                    (None, Some(actual)) => actual,
                    (None, None) => Type::Unknown,
                };
                self.define(name, Variable { ty: final_type, mutable: *mutable }, *span);
            }
            Stmt::Return { value, span } => {
                let actual = value.as_ref().map(|expr| self.expression(expr)).unwrap_or(Type::Unit);
                if let Some(expected) = self.return_types.last().copied() {
                    self.expect_type(actual, expected, *span, "return type mismatch");
                }
            }
            Stmt::If { condition, then_body, else_body, .. } => {
                let actual = self.expression(condition);
                self.expect_type(actual, Type::Bool, expr_span(condition), "if condition must be Bool");
                self.block(then_body);
                if let Some(body) = else_body { self.block(body); }
            }
            Stmt::While { condition, body, .. } => {
                let actual = self.expression(condition);
                self.expect_type(actual, Type::Bool, expr_span(condition), "while condition must be Bool");
                self.block(body);
            }
            Stmt::For { name, iterable, body, span } => {
                // Iterable/collection types are deliberately outside this first
                // primitive-type milestone. Name resolution still validates it.
                let _ = self.expression(iterable);
                self.push_scope();
                self.define(name, Variable { ty: Type::Unknown, mutable: false }, *span);
                for child in body { self.statement(child); }
                self.pop_scope();
            }
            Stmt::Assign { target, value, span } => {
                let actual = self.expression(value);
                if let Expr::Identifier { name, span: target_span } = target {
                    match self.lookup(name) {
                        Some(variable) => {
                            if !variable.mutable {
                                self.error(format!("cannot assign to immutable binding '{name}'"), *target_span);
                            }
                            self.expect_type(actual, variable.ty, *span, "assignment type mismatch");
                        }
                        None => {}
                    }
                } else {
                    // Member/index assignment needs compound types and is deferred.
                    let _ = self.expression(target);
                }
            }
            Stmt::Expression { expression, .. } => { self.expression(expression); }
        }
    }

    fn block(&mut self, statements: &[Stmt]) {
        self.push_scope();
        for statement in statements { self.statement(statement); }
        self.pop_scope();
    }

    fn expression(&mut self, expression: &Expr) -> Type {
        match expression {
            Expr::Integer { .. } => Type::Int,
            Expr::Float { .. } => Type::Float,
            Expr::String { .. } => Type::String,
            Expr::Boolean { .. } => Type::Bool,
            Expr::Identifier { name, span } => match self.lookup(name) {
                Some(variable) => variable.ty,
                None => Type::Unknown,
            },
            Expr::Unary { operator, operand, span } => {
                let operand_type = self.expression(operand);
                match operator {
                    TokenKind::Minus if matches!(operand_type, Type::Int | Type::Float | Type::Unknown) => operand_type,
                    TokenKind::Bang if operand_type == Type::Bool || operand_type == Type::Unknown => Type::Bool,
                    TokenKind::Minus => { self.error(format!("unary '-' requires a numeric operand, found {operand_type}"), *span); Type::Unknown }
                    TokenKind::Bang => { self.error(format!("'!' requires Bool, found {operand_type}"), *span); Type::Unknown }
                    _ => { self.error("pointer and address operators are not part of the primitive type-checking milestone", *span); Type::Unknown }
                }
            }
            Expr::Binary { left, operator, right, span } => {
                let lhs = self.expression(left);
                let rhs = self.expression(right);
                self.binary_type(lhs, operator, rhs, *span)
            }
            Expr::Call { callee, arguments, span } => {
                if let Expr::Identifier { name, span: callee_span } = callee.as_ref() {
                    if name == "print" {
                        for argument in arguments { self.expression(argument); }
                        return Type::Unit;
                    }
                    if let Some(signature) = self.functions.get(name).cloned() {
                        if arguments.len() != signature.parameters.len() {
                            self.error(format!("function '{name}' expects {} argument(s), found {}", signature.parameters.len(), arguments.len()), *span);
                        }
                        for (index, argument) in arguments.iter().enumerate() {
                            let actual = self.expression(argument);
                            if let Some(expected) = signature.parameters.get(index).copied() {
                                self.expect_type(actual, expected, expr_span(argument), &format!("argument {} to '{name}'", index + 1));
                            }
                        }
                        signature.result
                    } else {
                        self.error(format!("cannot determine function signature for '{name}'"), *callee_span);
                        for argument in arguments { self.expression(argument); }
                        Type::Unknown
                    }
                } else {
                    self.error("calling a value is not supported by the initial type checker", *span);
                    self.expression(callee);
                    for argument in arguments { self.expression(argument); }
                    Type::Unknown
                }
            }
            Expr::Member { object, span, .. } => {
                self.expression(object);
                self.error("member access requires compound types, which are not in the initial type checker", *span);
                Type::Unknown
            }
            Expr::Index { object, index, span } => {
                self.expression(object);
                self.expression(index);
                self.error("indexing requires collection types, which are not in the initial type checker", *span);
                Type::Unknown
            }
        }
    }

    fn binary_type(&mut self, lhs: Type, operator: &TokenKind, rhs: Type, span: Span) -> Type {
        if lhs == Type::Unknown || rhs == Type::Unknown { return Type::Unknown; }
        match operator {
            TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash | TokenKind::Percent => {
                if lhs == rhs && matches!(lhs, Type::Int | Type::Float) { lhs }
                else {
                    self.error(format!("arithmetic operator requires matching numeric types, found {lhs} and {rhs}"), span);
                    Type::Unknown
                }
            }
            TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => {
                if lhs == rhs && matches!(lhs, Type::Int | Type::Float) { Type::Bool }
                else {
                    self.error(format!("comparison requires matching numeric types, found {lhs} and {rhs}"), span);
                    Type::Unknown
                }
            }
            TokenKind::Equal | TokenKind::NotEqual => {
                if lhs == rhs && lhs != Type::Unit { Type::Bool }
                else {
                    self.error(format!("equality requires matching non-Unit types, found {lhs} and {rhs}"), span);
                    Type::Unknown
                }
            }
            TokenKind::Ampersand | TokenKind::Pipe | TokenKind::Caret => {
                if lhs == Type::Int && rhs == Type::Int { Type::Int }
                else {
                    self.error(format!("bitwise operator requires Int operands, found {lhs} and {rhs}"), span);
                    Type::Unknown
                }
            }
            _ => { self.error("unsupported binary operator in the initial type checker", span); Type::Unknown }
        }
    }
}

fn expr_span(expression: &Expr) -> Span {
    match expression {
        Expr::Identifier { span, .. } | Expr::Integer { span, .. } | Expr::Float { span, .. }
        | Expr::String { span, .. } | Expr::Boolean { span, .. } | Expr::Unary { span, .. }
        | Expr::Binary { span, .. } | Expr::Call { span, .. } | Expr::Member { span, .. }
        | Expr::Index { span, .. } => *span,
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use super::check;

    fn checked(source: &str) -> Result<(), Vec<super::TypeError>> {
        let tokens = tokenize(source).expect("source should tokenize");
        let program = parse(&tokens).expect("source should parse");
        crate::semantic::analyze(&program).expect("source should pass name resolution");
        check(&program)
    }

    #[test]
    fn accepts_annotated_and_inferred_values() {
        assert!(checked("let age: Int = 25\nlet name: String = \"Mosh\"\nlet total = age + 5\n").is_ok());
    }

    #[test]
    fn rejects_mismatched_initializer_and_assignment() {
        assert!(checked("let age: Int = \"twenty\"\n").unwrap_err().iter().any(|e| e.message.contains("initializer type mismatch")));
        assert!(checked("function main():\n    mut age: Int = 1\n    age = 2.5\n").unwrap_err().iter().any(|e| e.message.contains("assignment type mismatch")));
    }

    #[test]
    fn rejects_mixed_numeric_arithmetic_without_conversion() {
        assert!(checked("let result = 1 + 2.0\n").unwrap_err().iter().any(|e| e.message.contains("matching numeric types")));
    }

    #[test]
    fn requires_boolean_conditions() {
        assert!(checked("if 1:\n    print(1)\n").unwrap_err().iter().any(|e| e.message.contains("if condition must be Bool")));
        assert!(checked("while true:\n    print(1)\n").is_ok());
    }

    #[test]
    fn checks_function_arguments_and_return_types() {
        assert!(checked("function add(a: Int, b: Int) -> Int:\n    return a + b\nlet answer = add(1, 2)\n").is_ok());
        assert!(checked("function add(a: Int, b: Int) -> Int:\n    return a + b\nlet answer = add(1, \"two\")\n").unwrap_err().iter().any(|e| e.message.contains("argument 2")));
        assert!(checked("function wrong() -> Int:\n    return \"no\"\n").unwrap_err().iter().any(|e| e.message.contains("return type mismatch")));
    }

    #[test]
    fn omitted_function_return_type_means_unit() {
        assert!(checked("function greet():\n    print(\"hello\")\n").is_ok());
        assert!(checked("function wrong():\n    return 1\n").unwrap_err().iter().any(|e| e.message.contains("return type mismatch")));
    }

    #[test]
    fn rejects_unknown_type_names() {
        assert!(checked("let value: Decimal = 1\n").unwrap_err().iter().any(|e| e.message.contains("unknown type 'Decimal'")));
    }

    #[test]
    fn allows_print_as_the_provisional_polymorphic_builtin() {
        assert!(checked("print(1, \"hello\", true)\n").is_ok());
    }
}
