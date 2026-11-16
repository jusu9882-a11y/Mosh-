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
        write!(
            f,
            "{} at {}:{}",
            self.message, self.span.line, self.span.column
        )
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
    initialized: bool,
}

#[derive(Debug, Default, Clone)]
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
        if let Stmt::Function {
            name,
            params,
            return_type,
            span,
            ..
        } = statement
        {
            let parameters = params
                .iter()
                .filter_map(|parameter| {
                    checker
                        .resolve_parameter_type(&parameter.ty)
                        .map(|ty| (parameter.name.clone(), ty))
                })
                .collect::<Vec<_>>();
            let result = return_type
                .as_ref()
                .map(|ty| checker.resolve_type(ty))
                .unwrap_or(Some(Type::Unit));
            if parameters.len() == params.len() {
                if let Some(result) = result {
                    if checker.functions.contains_key(name) {
                        checker.error(
                            format!("duplicate function type declaration for '{name}'"),
                            *span,
                        );
                    } else {
                        checker.functions.insert(
                            name.clone(),
                            FunctionType {
                                parameters: parameters.into_iter().map(|(_, ty)| ty).collect(),
                                result,
                            },
                        );
                    }
                }
            }
        }
    }

    for statement in &program.statements {
        if !matches!(statement, Stmt::Function { .. }) {
            checker.error(
                "module-level executable statements and global bindings are not supported",
                statement_span(statement),
            );
        }
        checker.statement(statement);
    }

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

impl Checker {
    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(TypeError {
            message: message.into(),
            span,
        });
    }

    fn resolve_type(&mut self, syntax: &TypeExpr) -> Option<Type> {
        match Type::from_name(&syntax.name) {
            Some(ty) => Some(ty),
            None => {
                self.error(
                    format!(
                        "unknown type '{}'; expected Int, Float, Bool, String, or Unit",
                        syntax.name
                    ),
                    syntax.span,
                );
                None
            }
        }
    }

    fn resolve_parameter_type(&mut self, syntax: &TypeExpr) -> Option<Type> {
        match self.resolve_type(syntax) {
            Some(Type::Unit) => {
                self.error("function parameters cannot have type Unit", syntax.span);
                None
            }
            other => other,
        }
    }

    fn lookup(&self, name: &str) -> Option<Variable> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.variables.get(name).copied())
    }

    fn define(&mut self, name: &str, variable: Variable, span: Span) {
        let duplicate = self
            .scopes
            .last()
            .expect("checker always has a scope")
            .variables
            .contains_key(name);
        if duplicate {
            self.error(format!("duplicate typed binding '{name}'"), span);
        } else {
            self.scopes
                .last_mut()
                .expect("checker always has a scope")
                .variables
                .insert(name.to_owned(), variable);
        }
    }

    fn expect_type(&mut self, actual: Type, expected: Type, span: Span, context: &str) {
        if actual != Type::Unknown && actual != expected {
            self.error(
                format!("{context}: expected {expected}, found {actual}"),
                span,
            );
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Function {
                name,
                params,
                return_type,
                body,
                span,
            } => {
                if !self.return_types.is_empty() {
                    self.error("nested function declarations are not supported", *span);
                    return;
                }
                let result = return_type
                    .as_ref()
                    .and_then(|ty| self.resolve_type(ty))
                    .unwrap_or(Type::Unit);
                let signature = self.functions.get(name).cloned();
                self.push_scope();
                self.return_types.push(result);
                for (index, parameter) in params.iter().enumerate() {
                    if let Some(ty) = self.resolve_type(&parameter.ty) {
                        if ty == Type::Unit {
                            continue;
                        }
                        if let Some(sig) = &signature {
                            if sig.parameters.get(index).copied() != Some(ty) {
                                self.error(
                                    format!(
                                        "internal signature mismatch for parameter '{}'",
                                        parameter.name
                                    ),
                                    parameter.span,
                                );
                            }
                        }
                        self.define(
                            &parameter.name,
                            Variable {
                                ty,
                                mutable: false,
                                initialized: true,
                            },
                            parameter.span,
                        );
                    }
                }
                for child in body {
                    self.statement(child);
                }
                if result != Type::Unit && !block_guarantees_return(body) {
                    self.error(
                        format!("function '{name}' may finish without returning {result}"),
                        *span,
                    );
                }
                self.return_types.pop();
                self.pop_scope();
                let _ = span;
            }
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                span,
            } => {
                let annotation = ty.as_ref().and_then(|syntax| self.resolve_type(syntax));
                let inferred = value.as_ref().map(|expr| self.expression(expr));
                let final_type = match (annotation, inferred) {
                    (Some(expected), Some(actual)) => {
                        self.expect_type(
                            actual,
                            expected,
                            value.as_ref().map(expr_span).unwrap_or(*span),
                            "initializer type mismatch",
                        );
                        expected
                    }
                    (Some(expected), None) => expected,
                    (None, Some(actual)) => actual,
                    (None, None) => Type::Unknown,
                };
                if final_type == Type::Unit {
                    self.error("local bindings cannot have type Unit", *span);
                }
                self.define(
                    name,
                    Variable {
                        ty: final_type,
                        mutable: *mutable,
                        initialized: value.is_some(),
                    },
                    *span,
                );
            }
            Stmt::Return { value, span } => {
                let actual = value
                    .as_ref()
                    .map(|expr| self.expression(expr))
                    .unwrap_or(Type::Unit);
                if let Some(expected) = self.return_types.last().copied() {
                    self.expect_type(actual, expected, *span, "return type mismatch");
                    if expected == Type::Unit && value.is_some() && actual == Type::Unit {
                        self.error("a Unit function must use a bare return", *span);
                    }
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let actual = self.expression(condition);
                self.expect_type(
                    actual,
                    Type::Bool,
                    expr_span(condition),
                    "if condition must be Bool",
                );
                let before = self.scopes.clone();
                self.block(then_body);
                let after_then = self.scopes.clone();
                self.scopes = before.clone();
                if let Some(body) = else_body {
                    self.block(body);
                }
                let after_else = self.scopes.clone();
                let then_returns = block_guarantees_return(then_body);
                let else_returns = else_body
                    .as_ref()
                    .is_some_and(|body| block_guarantees_return(body));
                for (scope_index, scope) in self.scopes.iter_mut().enumerate() {
                    for (name, variable) in &mut scope.variables {
                        let then_initialized = after_then
                            .get(scope_index)
                            .and_then(|scope| scope.variables.get(name))
                            .is_some_and(|state| state.initialized);
                        let before_initialized = before[scope_index]
                            .variables
                            .get(name)
                            .is_some_and(|state| state.initialized);
                        let else_initialized = if else_body.is_some() {
                            after_else
                                .get(scope_index)
                                .and_then(|scope| scope.variables.get(name))
                                .is_some_and(|state| state.initialized)
                        } else {
                            before_initialized
                        };

                        // A branch that always returns cannot reach the join.
                        // Only merge states from branches that can continue.
                        variable.initialized =
                            match (then_returns, else_returns, else_body.is_some()) {
                                (true, false, true) => else_initialized,
                                (false, true, true) => then_initialized,
                                (true, true, true) => then_initialized || else_initialized,
                                _ => then_initialized && else_initialized,
                            };
                    }
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                let actual = self.expression(condition);
                self.expect_type(
                    actual,
                    Type::Bool,
                    expr_span(condition),
                    "while condition must be Bool",
                );
                let before = self.scopes.clone();
                self.block(body);
                // A while-loop may execute zero times, so assignments made only
                // inside its body are not definitely initialized afterwards.
                self.scopes = before;
            }
            Stmt::For {
                name,
                iterable,
                body,
                span,
            } => {
                // The parser recognizes for-loops, but iteration semantics and
                // MIR/native lowering are not implemented yet. Reject them here
                // so the check command cannot report success for an unbuildable loop.
                self.error(
                    "for-loops are not supported by the initial compiler pipeline",
                    *span,
                );
                let _ = self.expression(iterable);
                let before = self.scopes.clone();
                self.push_scope();
                self.define(
                    name,
                    Variable {
                        ty: Type::Unknown,
                        mutable: false,
                        initialized: true,
                    },
                    *span,
                );
                for child in body {
                    self.statement(child);
                }
                self.pop_scope();
                // A for-loop can have zero iterations; do not carry body-only
                // initialization facts beyond the loop.
                self.scopes = before;
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                let actual = self.expression(value);
                if let Expr::Identifier {
                    name,
                    span: target_span,
                } = target
                {
                    if let Some(variable) = self.lookup(name) {
                        if !variable.mutable {
                            self.error(
                                format!("cannot assign to immutable binding '{name}'"),
                                *target_span,
                            );
                        }
                        self.expect_type(
                            actual,
                            variable.ty,
                            *span,
                            "assignment type mismatch",
                        );
                        if variable.mutable && actual != Type::Unknown {
                            if let Some(binding) = self
                                .scopes
                                .iter_mut()
                                .rev()
                                .find_map(|scope| scope.variables.get_mut(name))
                            {
                                binding.initialized = true;
                            }
                        }
                    }
                } else {
                    // Member/index assignment needs compound types and is deferred.
                    let _ = self.expression(target);
                }
            }
            Stmt::Expression { expression, .. } => {
                self.expression(expression);
            }
        }
    }

    fn block(&mut self, statements: &[Stmt]) {
        self.push_scope();
        for statement in statements {
            self.statement(statement);
        }
        self.pop_scope();
    }

    fn expression(&mut self, expression: &Expr) -> Type {
        match expression {
            Expr::Integer { .. } => Type::Int,
            Expr::Float { .. } => Type::Float,
            Expr::String { .. } => Type::String,
            Expr::Boolean { .. } => Type::Bool,
            Expr::Identifier { name, span } => match self.lookup(name) {
                Some(variable) => {
                    if !variable.initialized {
                        self.error(
                            format!("binding '{name}' may be used before it is initialized"),
                            *span,
                        );
                    }
                    variable.ty
                }
                None => Type::Unknown,
            },
            Expr::Unary {
                operator,
                operand,
                span,
            } => {
                let operand_type = self.expression(operand);
                match operator {
                    TokenKind::Minus
                        if matches!(operand_type, Type::Int | Type::Float | Type::Unknown) =>
                    {
                        operand_type
                    }
                    TokenKind::Bang
                        if operand_type == Type::Bool || operand_type == Type::Unknown =>
                    {
                        Type::Bool
                    }
                    TokenKind::Minus => {
                        self.error(
                            format!("unary '-' requires a numeric operand, found {operand_type}"),
                            *span,
                        );
                        Type::Unknown
                    }
                    TokenKind::Bang => {
                        self.error(format!("'!' requires Bool, found {operand_type}"), *span);
                        Type::Unknown
                    }
                    _ => {
                        self.error("pointer and address operators are not part of the primitive type-checking milestone", *span);
                        Type::Unknown
                    }
                }
            }
            Expr::Binary {
                left,
                operator,
                right,
                span,
            } => {
                let lhs = self.expression(left);
                let rhs = self.expression(right);
                self.binary_type(lhs, operator, rhs, *span)
            }
            Expr::Call {
                callee,
                arguments,
                span,
            } => {
                if let Expr::Identifier {
                    name,
                    span: callee_span,
                } = callee.as_ref()
                {
                    if name == "print" {
                        for argument in arguments {
                            if self.expression(argument) == Type::Unit {
                                self.error("cannot print a Unit value", expr_span(argument));
                            }
                        }
                        return Type::Unit;
                    }
                    if let Some(signature) = self.functions.get(name).cloned() {
                        if arguments.len() != signature.parameters.len() {
                            self.error(
                                format!(
                                    "function '{name}' expects {} argument(s), found {}",
                                    signature.parameters.len(),
                                    arguments.len()
                                ),
                                *span,
                            );
                        }
                        for (index, argument) in arguments.iter().enumerate() {
                            let actual = self.expression(argument);
                            if let Some(expected) = signature.parameters.get(index).copied() {
                                self.expect_type(
                                    actual,
                                    expected,
                                    expr_span(argument),
                                    &format!("argument {} to '{name}'", index + 1),
                                );
                            }
                        }
                        signature.result
                    } else {
                        self.error(
                            format!("cannot determine function signature for '{name}'"),
                            *callee_span,
                        );
                        for argument in arguments {
                            self.expression(argument);
                        }
                        Type::Unknown
                    }
                } else {
                    self.error(
                        "calling a value is not supported by the initial type checker",
                        *span,
                    );
                    self.expression(callee);
                    for argument in arguments {
                        self.expression(argument);
                    }
                    Type::Unknown
                }
            }
            Expr::Member { object, span, .. } => {
                self.expression(object);
                self.error("member access requires compound types, which are not in the initial type checker", *span);
                Type::Unknown
            }
            Expr::Index {
                object,
                index,
                span,
            } => {
                self.expression(object);
                self.expression(index);
                self.error(
                    "indexing requires collection types, which are not in the initial type checker",
                    *span,
                );
                Type::Unknown
            }
        }
    }

    fn binary_type(&mut self, lhs: Type, operator: &TokenKind, rhs: Type, span: Span) -> Type {
        if lhs == Type::Unknown || rhs == Type::Unknown {
            return Type::Unknown;
        }
        match operator {
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent => {
                if lhs == rhs && matches!(lhs, Type::Int | Type::Float) {
                    lhs
                } else {
                    self.error(format!("arithmetic operator requires matching numeric types, found {lhs} and {rhs}"), span);
                    Type::Unknown
                }
            }
            TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual => {
                if lhs == rhs && matches!(lhs, Type::Int | Type::Float) {
                    Type::Bool
                } else {
                    self.error(
                        format!(
                            "comparison requires matching numeric types, found {lhs} and {rhs}"
                        ),
                        span,
                    );
                    Type::Unknown
                }
            }
            TokenKind::Equal | TokenKind::NotEqual => {
                if lhs == rhs && lhs != Type::Unit {
                    Type::Bool
                } else {
                    self.error(
                        format!("equality requires matching non-Unit types, found {lhs} and {rhs}"),
                        span,
                    );
                    Type::Unknown
                }
            }
            TokenKind::Ampersand | TokenKind::Pipe | TokenKind::Caret => {
                if lhs == Type::Int && rhs == Type::Int {
                    Type::Int
                } else {
                    self.error(
                        format!("bitwise operator requires Int operands, found {lhs} and {rhs}"),
                        span,
                    );
                    Type::Unknown
                }
            }
            _ => {
                self.error(
                    "unsupported binary operator in the initial type checker",
                    span,
                );
                Type::Unknown
            }
        }
    }
}

// A non-Unit function must return on every statically guaranteed path.
// Loops are deliberately not treated as guaranteed returns: even a
// syntactically constant condition is not yet part of control-flow analysis.
fn block_guarantees_return(statements: &[Stmt]) -> bool {
    statements.iter().any(statement_guarantees_return)
}

fn statement_guarantees_return(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return { .. } => true,
        Stmt::If {
            then_body,
            else_body: Some(else_body),
            ..
        } => block_guarantees_return(then_body) && block_guarantees_return(else_body),
        _ => false,
    }
}

fn statement_span(statement: &Stmt) -> Span {
    match statement {
        Stmt::Function { span, .. }
        | Stmt::Let { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::Expression { span, .. } => *span,
    }
}

fn expr_span(expression: &Expr) -> Span {
    match expression {
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

#[cfg(test)]
mod tests {
    use super::check;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn checked(source: &str) -> Result<(), Vec<super::TypeError>> {
        let tokens = tokenize(source).expect("source should tokenize");
        let program = parse(&tokens).expect("source should parse");
        crate::semantic::analyze(&program).expect("source should pass name resolution");
        check(&program)
    }

    #[test]
    fn rejects_nested_functions_until_lowering_is_implemented() {
        let errors =
            checked("function main():\n    function helper():\n        print(\"nested\")\n")
                .unwrap_err();
        assert!(errors.iter().any(|error| error
            .message
            .contains("nested function declarations are not supported")));
    }

    #[test]
    fn rejects_module_level_statements_and_global_bindings() {
        let errors = checked("let global: Int = 5\n").unwrap_err();
        assert!(errors.iter().any(|error| error
            .message
            .contains("module-level executable statements and global bindings are not supported")));
    }

    #[test]
    fn rejects_reading_a_local_before_initialization() {
        let errors =
            checked("function main() -> Int:\n    mut value: Int\n    return value\n").unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("before it is initialized")));
    }

    #[test]
    fn accepts_assignment_before_first_read() {
        assert!(checked(
            "function main() -> Int:\n    mut value: Int\n    value = 7\n    return value\n",
        )
        .is_ok());
    }

    #[test]
    fn requires_initialization_on_every_if_branch() {
        let source = "function main(flag: Bool) -> Int:\n    mut value: Int\n    if flag:\n        value = 1\n    return value\n";
        assert!(checked(source)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("before it is initialized")));
        let both_branches = "function main(flag: Bool) -> Int:\n    mut value: Int\n    if flag:\n        value = 1\n    else:\n        value = 2\n    return value\n";
        assert!(checked(both_branches).is_ok());
    }

    #[test]
    fn ignores_initialization_state_from_a_branch_that_returns() {
        let then_returns = "function choose(flag: Bool) -> Int:\n    mut value: Int\n    if flag:\n        return 0\n    else:\n        value = 7\n    return value\n";
        assert!(checked(then_returns).is_ok());

        let else_returns = "function choose(flag: Bool) -> Int:\n    mut value: Int\n    if flag:\n        value = 7\n    else:\n        return 0\n    return value\n";
        assert!(checked(else_returns).is_ok());
    }

    #[test]
    fn rejects_for_loops_until_iteration_is_implemented() {
        let errors =
            checked("function main():\n    for item in 1:\n        print(item)\n").unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("for-loops are not supported")));
    }

    #[test]
    fn for_loop_assignment_is_not_assumed_to_execute() {
        let source = "function main() -> Int:\n    mut value: Int\n    for item in 1:\n        value = item\n    return value\n";
        assert!(checked(source)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("before it is initialized")));
    }

    #[test]
    fn loop_body_assignment_is_not_assumed_to_execute() {
        let source = "function main() -> Int:\n    mut value: Int\n    while false:\n        value = 1\n    return value\n";
        assert!(checked(source)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("before it is initialized")));
    }

    #[test]
    fn accepts_annotated_and_inferred_values() {
        assert!(
            checked("function main():\n    let age: Int = 25\n    let name: String = \"Mosh\"\n    let total = age + 5\n")
                .is_ok()
        );
    }

    #[test]
    fn rejects_mismatched_initializer_and_assignment() {
        assert!(checked("let age: Int = \"twenty\"\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("initializer type mismatch")));
        assert!(
            checked("function main():\n    mut age: Int = 1\n    age = 2.5\n")
                .unwrap_err()
                .iter()
                .any(|e| e.message.contains("assignment type mismatch"))
        );
    }

    #[test]
    fn rejects_mixed_numeric_arithmetic_without_conversion() {
        assert!(checked("function main():\n    let result = 1 + 2.0\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("matching numeric types")));
    }

    #[test]
    fn requires_boolean_conditions() {
        assert!(checked("function main():\n    if 1:\n        print(1)\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("if condition must be Bool")));
        assert!(checked("function main():\n    while true:\n        print(1)\n").is_ok());
    }

    #[test]
    fn checks_function_arguments_and_return_types() {
        assert!(checked(
            "function add(a: Int, b: Int) -> Int:\n    return a + b\nfunction main():\n    let answer = add(1, 2)\n"
        )
        .is_ok());
        assert!(checked("function add(a: Int, b: Int) -> Int:\n    return a + b\nfunction main():\n    let answer = add(1, \"two\")\n").unwrap_err().iter().any(|e| e.message.contains("argument 2")));
        assert!(checked("function wrong() -> Int:\n    return \"no\"\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("return type mismatch")));
    }

    #[test]
    fn omitted_function_return_type_means_unit() {
        assert!(checked("function greet():\n    print(\"hello\")\n").is_ok());
        assert!(checked("function wrong():\n    return 1\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("return type mismatch")));
    }

    #[test]
    fn rejects_non_unit_functions_with_a_fallthrough_path() {
        let errors =
            checked("function maybe(flag: Bool) -> Int:\n    if flag:\n        return 1\n")
                .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("may finish without returning Int")));
    }

    #[test]
    fn accepts_non_unit_functions_when_all_if_branches_return() {
        assert!(checked(
            "function choose(flag: Bool) -> Int:\n    if flag:\n        return 1\n    else:\n        return 2\n"
        )
        .is_ok());
    }

    #[test]
    fn does_not_assume_while_loop_returns() {
        let errors =
            checked("function maybe() -> Int:\n    while true:\n        return 1\n").unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("may finish without returning Int")));
    }

    #[test]
    fn rejects_unknown_type_names() {
        assert!(checked("function main():\n    let value: Decimal = 1\n")
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("unknown type 'Decimal'")));
    }

    #[test]
    fn rejects_unit_local_bindings_and_parameters() {
        let local_errors = checked("function main():\n    let value: Unit\n").unwrap_err();
        assert!(local_errors.iter().any(|error| error
            .message
            .contains("local bindings cannot have type Unit")));

        let parameter_errors =
            checked("function greet(value: Unit):\n    print(\"hello\")\n").unwrap_err();
        assert!(parameter_errors.iter().any(|error| error
            .message
            .contains("function parameters cannot have type Unit")));
    }

    #[test]
    fn rejects_explicit_unit_return_expressions() {
        let errors = checked("function main():\n    return print(1)\n").unwrap_err();
        assert!(errors.iter().any(|error| error
            .message
            .contains("a Unit function must use a bare return")));
    }

    #[test]
    fn rejects_printing_unit_values() {
        let errors = checked(
            "function greet():\n    print(\"hello\")\nfunction main():\n    print(greet())\n",
        )
        .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("cannot print a Unit value")));
    }

    #[test]
    fn allows_print_as_the_provisional_polymorphic_builtin() {
        assert!(checked("function main():\n    print(1, \"hello\", true)\n").is_ok());
    }
}
