use std::collections::HashMap;
use std::fmt;

use crate::lexer::Span;
use crate::parser::{Expr, Program, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}:{}", self.message, self.span.line, self.span.column)
    }
}

impl std::error::Error for SemanticError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Variable { mutable: bool },
    Function,
}

#[derive(Debug, Default)]
struct Scope {
    symbols: HashMap<String, SymbolKind>,
}

impl Scope {
    fn define(&mut self, name: &str, kind: SymbolKind) -> bool {
        if self.symbols.contains_key(name) {
            false
        } else {
            self.symbols.insert(name.to_owned(), kind);
            true
        }
    }
}

#[derive(Debug, Default)]
struct Analyzer {
    scopes: Vec<Scope>,
    errors: Vec<SemanticError>,
    function_depth: usize,
}

pub fn analyze(program: &Program) -> Result<(), Vec<SemanticError>> {
    let mut analyzer = Analyzer::default();
    analyzer.push_scope();

    // Register module-level functions before checking bodies. This makes
    // declaration order irrelevant for functions and permits mutual recursion.
    for statement in &program.statements {
        if let Stmt::Function { name, span, .. } = statement {
            if !analyzer.define(name, SymbolKind::Function) {
                analyzer.error(format!("duplicate declaration of '{name}'"), *span);
            }
        }
    }

    for statement in &program.statements {
        analyzer.statement(statement);
    }

    if analyzer.errors.is_empty() {
        Ok(())
    } else {
        Err(analyzer.errors)
    }
}

impl Analyzer {
    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, kind: SymbolKind) -> bool {
        self.scopes.last_mut().expect("analyzer always has a scope").define(name, kind)
    }

    fn lookup(&self, name: &str) -> Option<SymbolKind> {
        self.scopes.iter().rev().find_map(|scope| scope.symbols.get(name).copied())
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(SemanticError { message: message.into(), span });
    }

    fn statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Function { name, params, body, span, .. } => {
                // Module functions were predeclared above. A nested function is
                // introduced at its declaration point in the current scope.
                if self.function_depth > 0 && !self.define(name, SymbolKind::Function) {
                    self.error(format!("duplicate declaration of '{name}'"), *span);
                }
                self.push_scope();
                self.function_depth += 1;
                for parameter in params {
                    if !self.define(&parameter.name, SymbolKind::Variable { mutable: false }) {
                        self.error(
                            format!("duplicate parameter name '{}'", parameter.name),
                            parameter.span,
                        );
                    }
                }
                for child in body {
                    self.statement(child);
                }
                self.function_depth -= 1;
                self.pop_scope();
            }
            Stmt::Let { name, mutable, value, span, .. } => {
                // Initializers are evaluated before the new name enters scope.
                if let Some(initializer) = value {
                    self.expression(initializer);
                }
                if !self.define(name, SymbolKind::Variable { mutable: *mutable }) {
                    self.error(format!("duplicate declaration of '{name}' in the same scope"), *span);
                }
            }
            Stmt::Return { value, span } => {
                if self.function_depth == 0 {
                    self.error("'return' is only valid inside a function", *span);
                }
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            Stmt::If { condition, then_body, else_body, .. } => {
                self.expression(condition);
                self.block(then_body);
                if let Some(body) = else_body {
                    self.block(body);
                }
            }
            Stmt::While { condition, body, .. } => {
                self.expression(condition);
                self.block(body);
            }
            Stmt::For { name, iterable, body, span } => {
                // The iterable is evaluated outside the loop-variable scope.
                self.expression(iterable);
                self.push_scope();
                if !self.define(name, SymbolKind::Variable { mutable: false }) {
                    self.error(format!("duplicate loop variable '{name}'"), *span);
                }
                for child in body {
                    self.statement(child);
                }
                self.pop_scope();
            }
            Stmt::Assign { target, value, .. } => {
                self.assignment_target(target);
                self.expression(value);
            }
            Stmt::Expression { expression, .. } => self.expression(expression),
        }
    }

    fn block(&mut self, statements: &[Stmt]) {
        self.push_scope();
        for statement in statements {
            self.statement(statement);
        }
        self.pop_scope();
    }

    fn assignment_target(&mut self, target: &Expr) {
        match target {
            Expr::Identifier { name, span } => match self.lookup(name) {
                None => self.error(format!("use of undefined name '{name}'"), *span),
                Some(SymbolKind::Variable { mutable: false }) => {
                    self.error(format!("cannot assign to immutable binding '{name}'"), *span);
                }
                Some(SymbolKind::Variable { mutable: true }) => {}
                Some(SymbolKind::Function) => {
                    self.error(format!("cannot assign to function '{name}'"), *span);
                }
            },
            Expr::Member { object, .. } => self.expression(object),
            Expr::Index { object, index, .. } => {
                self.expression(object);
                self.expression(index);
            }
            _ => {
                self.error("invalid assignment target", expr_span(target));
                self.expression(target);
            }
        }
    }

    fn expression(&mut self, expression: &Expr) {
        match expression {
            Expr::Identifier { name, span } => {
                if self.lookup(name).is_none() {
                    self.error(format!("use of undefined name '{name}'"), *span);
                }
            }
            Expr::Unary { operand, .. } => self.expression(operand),
            Expr::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            Expr::Call { callee, arguments, .. } => {
                if let Expr::Identifier { name, span } = callee.as_ref() {
                    // print is the compiler's initial built-in output function.
                    if name != "print" && self.lookup(name).is_none() {
                        self.error(format!("call to undefined function '{name}'"), *span);
                    }
                    if matches!(self.lookup(name), Some(SymbolKind::Variable { .. })) {
                        self.error(format!("'{name}' is a variable, not a function"), *span);
                    }
                } else {
                    self.expression(callee);
                }
                for argument in arguments {
                    self.expression(argument);
                }
            }
            Expr::Member { object, .. } => self.expression(object),
            Expr::Index { object, index, .. } => {
                self.expression(object);
                self.expression(index);
            }
            Expr::Integer { .. } | Expr::Float { .. } | Expr::String { .. }
            | Expr::Boolean { .. } => {}
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

    use super::analyze;

    fn checked(source: &str) -> Result<(), Vec<super::SemanticError>> {
        let tokens = tokenize(source).expect("test source should tokenize");
        let program = parse(&tokens).expect("test source should parse");
        analyze(&program)
    }

    #[test]
    fn permits_forward_calls_and_mutual_recursion() {
        assert!(checked(
            "function first():\n    second()\nfunction second():\n    first()\n"
        ).is_ok());
    }

    #[test]
    fn rejects_undefined_variables_and_functions() {
        let errors = checked(
            "function main():\n    print(missing)\n    absent()\n"
        ).unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("undefined name 'missing'")));
        assert!(errors.iter().any(|e| e.message.contains("undefined function 'absent'")));
    }

    #[test]
    fn rejects_use_before_declaration() {
        let errors = checked("function main():\n    print(value)\n    let value = 3\n").unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("undefined name 'value'")));
    }

    #[test]
    fn rejects_duplicate_names_in_one_scope() {
        let errors = checked("function main():\n    let value = 1\n    let value = 2\n").unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("duplicate declaration of 'value'")));
    }

    #[test]
    fn permits_shadowing_in_nested_blocks() {
        assert!(checked(
            "function main():\n    let value = 1\n    if true:\n        let value = 2\n        print(value)\n    print(value)\n"
        ).is_ok());
    }

    #[test]
    fn block_variables_do_not_escape_their_block() {
        let errors = checked(
            "function main():\n    if true:\n        let hidden = 1\n    print(hidden)\n"
        ).unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("undefined name 'hidden'")));
    }

    #[test]
    fn loop_variable_is_visible_only_inside_the_loop() {
        assert!(checked("function main():\n    for item in items:\n        print(item)\n").is_err());
        assert!(checked(
            "function main():\n    let items = 1\n    for item in items:\n        print(item)\n"
        ).is_ok());
        let errors = checked(
            "function main():\n    let items = 1\n    for item in items:\n        print(item)\n    print(item)\n"
        ).unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("undefined name 'item'")));
    }

    #[test]
    fn rejects_return_outside_a_function() {
        let errors = checked("return 1\n").unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("only valid inside a function")));
    }

    #[test]
    fn rejects_assignment_to_immutable_bindings() {
        let errors = checked("function main():\n    let count = 0\n    count = 1\n").unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("immutable binding 'count'")));
    }

    #[test]
    fn permits_assignment_to_mutable_bindings() {
        assert!(checked("function main():\n    mut count = 0\n    count = 1\n").is_ok());
    }

    #[test]
    fn reports_source_location_for_semantic_errors() {
        let errors = checked("function main():\n    print(unknown)\n").unwrap_err();
        let error = errors.iter().find(|e| e.message.contains("unknown")).unwrap();
        assert_eq!(error.span.line, 2);
        assert!(error.span.column > 1);
    }

    #[test]
    fn rejects_duplicate_parameter_names() {
        let errors = checked("function main(value: Int, value: Int):\n    return\n").unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("duplicate parameter name 'value'")));
    }
}
