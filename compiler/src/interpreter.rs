use crate::lexer::{Span, TokenKind};
use crate::parser::{Expr, Program, Stmt};
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<HashMap<String, Value>>>),
    Nil,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v}"),
            Self::Boolean(v) => write!(f, "{v}"),
            Self::List(values) => {
                let values = values.borrow();
                write!(f, "[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 { write!(f, ", ")?; }
                    write!(f, "{value}")?;
                }
                write!(f, "]")
            }
            Self::Dict(entries) => {
                let entries = entries.borrow();
                let mut keys: Vec<_> = entries.keys().collect(); keys.sort();
                write!(f, "{{")?;
                for (index, key) in keys.iter().enumerate() { if index > 0 { write!(f, ", ")?; } write!(f, "\"{}\": {}", key, entries[*key])?; }
                write!(f, "}}")
            }
            Self::Nil => write!(f, "nil"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}:{}", self.message, self.span.line, self.span.column)
    }
}
impl std::error::Error for RuntimeError {}

#[derive(Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Clone)]
struct Function {
    params: Vec<String>,
    body: Vec<Stmt>,
}

enum Flow {
    Continue,
    Return(Value),
}

pub struct Interpreter {
    scopes: Vec<HashMap<String, Binding>>,
    functions: HashMap<String, Function>,
    output: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            output: false,
        }
    }

    pub fn set_echo(&mut self, enabled: bool) {
        self.output = enabled;
    }

    pub fn execute(&mut self, program: &Program) -> Result<Value, RuntimeError> {
        let mut last = Value::Nil;
        for statement in &program.statements {
            if let Stmt::Expression { expression, .. } = statement {
                last = self.eval(expression)?;
                if self.output { println!("{last}"); }
                continue;
            }
            match self.execute_stmt(statement)? {
                Flow::Continue => {}
                Flow::Return(_) => return Err(self.error("return used outside a function", stmt_span(statement))),
            }
        }
        Ok(last)
    }

    fn execute_block(&mut self, statements: &[Stmt], scoped: bool) -> Result<Flow, RuntimeError> {
        if scoped { self.scopes.push(HashMap::new()); }
        let result = (|| {
            for statement in statements {
                match self.execute_stmt(statement)? {
                    Flow::Continue => {}
                    flow @ Flow::Return(_) => return Ok(flow),
                }
            }
            Ok(Flow::Continue)
        })();
        if scoped { self.scopes.pop(); }
        result
    }

    fn execute_stmt(&mut self, stmt: &Stmt) -> Result<Flow, RuntimeError> {
        match stmt {
            Stmt::Function { name, params, body, .. } => {
                self.functions.insert(name.clone(), Function {
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    body: body.clone(),
                });
                Ok(Flow::Continue)
            }
            Stmt::Let { name, mutable, value, span, .. } => {
                let value = match value {
                    Some(expr) => self.eval(expr)?,
                    None => Value::Nil,
                };
                let scope = self.scopes.last_mut().expect("interpreter always has a scope");
                if scope.contains_key(name) {
                    return Err(RuntimeError { message: format!("'{name}' is already declared in this scope"), span: *span });
                }
                scope.insert(name.clone(), Binding { value, mutable: *mutable });
                Ok(Flow::Continue)
            }
            Stmt::Assign { target, value, span } => {
                let next = self.eval(value)?;
                match target {
                    Expr::Identifier { name, .. } => {
                        for scope in self.scopes.iter_mut().rev() {
                            if let Some(binding) = scope.get_mut(name) {
                                if !binding.mutable {
                                    return Err(RuntimeError { message: format!("cannot assign to immutable variable '{name}'; declare it with 'mut'"), span: *span });
                                }
                                binding.value = next;
                                return Ok(Flow::Continue);
                            }
                        }
                        Err(self.error(&format!("undefined variable '{name}'"), *span))
                    }
                    Expr::Index { object, index, .. } => {
                        let object = self.eval(object)?;
                        let index = self.eval(index)?;
                        match object {
                            Value::List(values) => { let index = self.normalize_index(&index, values.borrow().len(), *span)?; values.borrow_mut()[index] = next; }
                            Value::Dict(entries) => { let Value::String(key) = index else { return Err(self.error("dictionary key must be a string", *span)); }; entries.borrow_mut().insert(key, next); }
                            _ => return Err(self.error("indexed assignment requires a list or dictionary", *span)),
                        }
                        Ok(Flow::Continue)
                    }
                    _ => Err(self.error("assignment target must be a variable or list index", *span)),
                }
            }
            Stmt::Expression { expression, .. } => {
                self.eval(expression)?;
                Ok(Flow::Continue)
            }
            Stmt::If { condition, then_body, else_body, span } => {
                let condition_value = self.eval(condition)?;
                if self.truthy(&condition_value) {
                    self.execute_block(then_body, true)
                } else if let Some(body) = else_body {
                    self.execute_block(body, true)
                } else {
                    let _ = span;
                    Ok(Flow::Continue)
                }
            }
            Stmt::While { condition, body, span } => {
                let mut iterations = 0usize;
                loop {
                    let condition_value = self.eval(condition)?;
                    if !self.truthy(&condition_value) { break; }
                    iterations += 1;
                    if iterations > 1_000_000 {
                        return Err(self.error("loop iteration safety limit exceeded (1,000,000)", *span));
                    }
                    if let flow @ Flow::Return(_) = self.execute_block(body, true)? { return Ok(flow); }
                }
                Ok(Flow::Continue)
            }
            Stmt::Return { value, .. } => Ok(Flow::Return(match value {
                Some(expr) => self.eval(expr)?,
                None => Value::Nil,
            })),
            Stmt::For { name, iterable, body, span } => {
                let values = match self.eval(iterable)? {
                    Value::List(values) => values.borrow().clone(),
                    Value::String(value) => value.chars().map(|ch| Value::String(ch.to_string())).collect(),
                    Value::Dict(entries) => { let mut keys: Vec<_> = entries.borrow().keys().cloned().collect(); keys.sort(); keys.into_iter().map(Value::String).collect() }
                    _ => return Err(self.error("for-loop requires a list, dictionary, or string", *span)),
                };
                if values.len() > 1_000_000 {
                    return Err(self.error("for-loop iteration safety limit exceeded (1,000,000)", *span));
                }
                for value in values {
                    self.scopes.push(HashMap::new());
                    self.scopes.last_mut().unwrap().insert(name.clone(), Binding { value, mutable: false });
                    let result = self.execute_block(body, true);
                    self.scopes.pop();
                    if let flow @ Flow::Return(_) = result? { return Ok(flow); }
                }
                Ok(Flow::Continue)
            },
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Integer { value, span } => value.parse::<i64>()
                .map(Value::Integer).map_err(|_| self.error("integer literal is out of range", *span)),
            Expr::Float { value, span } => value.parse::<f64>()
                .map(Value::Float).map_err(|_| self.error("invalid floating-point literal", *span)),
            Expr::String { value, span } => decode_string(value)
                .map(Value::String).ok_or_else(|| self.error("invalid string literal", *span)),
            Expr::Boolean { value, .. } => Ok(Value::Boolean(*value)),
            Expr::List { elements, .. } => {
                let values = elements.iter().map(|element| self.eval(element)).collect::<Result<Vec<_>, _>>()?;
                Ok(Value::List(Rc::new(RefCell::new(values))))
            }
            Expr::Dict { entries, span } => {
                let mut values = HashMap::new();
                for (key, value) in entries {
                    let Expr::String { value: raw_key, .. } = key else { return Err(self.error("dictionary keys must be strings", *span)); };
                    let key = decode_string(raw_key).ok_or_else(|| self.error("invalid dictionary key", *span))?;
                    values.insert(key, self.eval(value)?);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(values))))
            }
            Expr::Identifier { name, span } => {
                for scope in self.scopes.iter().rev() {
                    if let Some(binding) = scope.get(name) { return Ok(binding.value.clone()); }
                }
                Err(self.error(&format!("undefined variable '{name}'"), *span))
            }
            Expr::Unary { operator, operand, span } => {
                let value = self.eval(operand)?;
                match (operator, value) {
                    (TokenKind::Minus, Value::Integer(v)) => v.checked_neg().map(Value::Integer).ok_or_else(|| self.error("integer overflow", *span)),
                    (TokenKind::Minus, Value::Float(v)) => Ok(Value::Float(-v)),
                    (TokenKind::Bang, v) => Ok(Value::Boolean(!self.truthy(&v))),
                    _ => Err(self.error("unsupported unary operation", *span)),
                }
            }
            Expr::Binary { left, operator, right, span } => {
                let lhs = self.eval(left)?;
                if *operator == TokenKind::Ampersand && !self.truthy(&lhs) { return Ok(Value::Boolean(false)); }
                if *operator == TokenKind::Pipe && self.truthy(&lhs) { return Ok(Value::Boolean(true)); }
                let rhs = self.eval(right)?;
                self.binary(lhs, operator, rhs, *span)
            }
            Expr::Call { callee, arguments, span } => {
                let Expr::Identifier { name, .. } = callee.as_ref() else {
                    return Err(self.error("only named function calls are supported", *span));
                };
                let args = arguments.iter().map(|arg| self.eval(arg)).collect::<Result<Vec<_>, _>>()?;
                self.call(name, args, *span)
            }
            Expr::Index { object, index, span } => {
                let object = self.eval(object)?;
                let index = self.eval(index)?;
                match object {
                    Value::List(values) => { let index = self.normalize_index(&index, values.borrow().len(), *span)?; Ok(values.borrow()[index].clone()) }
                    Value::Dict(entries) => { let Value::String(key) = index else { return Err(self.error("dictionary key must be a string", *span)); }; entries.borrow().get(&key).cloned().ok_or_else(|| self.error(&format!("dictionary has no key '{key}'"), *span)) }
                    _ => Err(self.error("indexing requires a list or dictionary", *span)),
                }
            }
            Expr::Member { span, .. } =>
                Err(self.error("member access is not implemented yet", *span)),
        }
    }

    fn call(&mut self, name: &str, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
        match name {
            "len" => {
                if args.len() != 1 { return Err(self.error("len() expects exactly one argument", span)); }
                let length = match &args[0] {
                    Value::List(values) => values.borrow().len(),
                    Value::Dict(values) => values.borrow().len(),
                    Value::String(value) => value.chars().count(),
                    _ => return Err(self.error("len() expects a list, dictionary, or string", span)),
                };
                Ok(Value::Integer(length as i64))
            }
            "push" => {
                if args.len() != 2 { return Err(self.error("push() expects a list and a value", span)); }
                let Value::List(values) = &args[0] else {
                    return Err(self.error("push() first argument must be a list", span));
                };
                values.borrow_mut().push(args[1].clone());
                Ok(Value::Nil)
            }
            "pop" => {
                if args.len() != 1 { return Err(self.error("pop() expects exactly one list", span)); }
                let Value::List(values) = &args[0] else {
                    return Err(self.error("pop() argument must be a list", span));
                };
                values.borrow_mut().pop().ok_or_else(|| self.error("cannot pop from an empty list", span))
            }
            "keys" => {
                if args.len() != 1 { return Err(self.error("keys() expects exactly one dictionary", span)); }
                let Value::Dict(entries) = &args[0] else { return Err(self.error("keys() argument must be a dictionary", span)); };
                let mut keys: Vec<_> = entries.borrow().keys().cloned().collect(); keys.sort();
                Ok(Value::List(Rc::new(RefCell::new(keys.into_iter().map(Value::String).collect()))))
            }
            "has_key" => {
                if args.len() != 2 { return Err(self.error("has_key() expects a dictionary and string key", span)); }
                let Value::Dict(entries) = &args[0] else { return Err(self.error("has_key() first argument must be a dictionary", span)); };
                let Value::String(key) = &args[1] else { return Err(self.error("has_key() key must be a string", span)); };
                Ok(Value::Boolean(entries.borrow().contains_key(key)))
            }
            "remove" => {
                if args.len() != 2 { return Err(self.error("remove() expects a dictionary and string key", span)); }
                let Value::Dict(entries) = &args[0] else { return Err(self.error("remove() first argument must be a dictionary", span)); };
                let Value::String(key) = &args[1] else { return Err(self.error("remove() key must be a string", span)); };
                Ok(entries.borrow_mut().remove(key).unwrap_or(Value::Nil))
            }
            "print" => {
                for (index, value) in args.iter().enumerate() {
                    if index > 0 { print!(" "); }
                    print!("{value}");
                }
                println!();
                Ok(Value::Nil)
            }
            "input" => {
                if args.len() > 1 { return Err(self.error("input() accepts zero or one prompt argument", span)); }
                if let Some(prompt) = args.first() { print!("{prompt}"); io::stdout().flush().ok(); }
                let mut line = String::new();
                io::stdin().read_line(&mut line).map_err(|e| self.error(&format!("input failed: {e}"), span))?;
                if line.ends_with('\n') { line.pop(); if line.ends_with('\r') { line.pop(); } }
                Ok(Value::String(line))
            }
            _ => {
                let Some(function) = self.functions.get(name).cloned() else {
                    return Err(self.error(&format!("undefined function '{name}'"), span));
                };
                if args.len() != function.params.len() {
                    return Err(self.error(&format!("function '{name}' expects {} argument(s), got {}", function.params.len(), args.len()), span));
                }
                self.scopes.push(HashMap::new());
                for (param, value) in function.params.iter().zip(args) {
                    self.scopes.last_mut().unwrap().insert(param.clone(), Binding { value, mutable: false });
                }
                let result = self.execute_block(&function.body, false);
                self.scopes.pop();
                match result? { Flow::Continue => Ok(Value::Nil), Flow::Return(value) => Ok(value) }
            }
        }
    }

    fn binary(&self, lhs: Value, op: &TokenKind, rhs: Value, span: Span) -> Result<Value, RuntimeError> {
        use TokenKind::*;
        match (lhs, op, rhs) {
            (Value::String(a), Plus, Value::String(b)) => Ok(Value::String(a + &b)),
            (Value::Integer(a), Plus, Value::Integer(b)) => a.checked_add(b).map(Value::Integer).ok_or_else(|| self.error("integer overflow", span)),
            (Value::Integer(a), Minus, Value::Integer(b)) => a.checked_sub(b).map(Value::Integer).ok_or_else(|| self.error("integer overflow", span)),
            (Value::Integer(a), Star, Value::Integer(b)) => a.checked_mul(b).map(Value::Integer).ok_or_else(|| self.error("integer overflow", span)),
            (Value::Integer(_), Slash, Value::Integer(0)) | (Value::Integer(_), Percent, Value::Integer(0)) => Err(self.error("division by zero", span)),
            (Value::Integer(a), Slash, Value::Integer(b)) => a.checked_div(b).map(Value::Integer).ok_or_else(|| self.error("integer overflow", span)),
            (Value::Integer(a), Percent, Value::Integer(b)) => a.checked_rem(b).map(Value::Integer).ok_or_else(|| self.error("integer overflow", span)),
            (Value::Float(a), Plus, Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Float(a), Minus, Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Float(a), Star, Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Float(_), Slash, Value::Float(b)) if b == 0.0 => Err(self.error("division by zero", span)),
            (Value::Float(a), Slash, Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Integer(a), Equal, Value::Integer(b)) => Ok(Value::Boolean(a == b)),
            (Value::Float(a), Equal, Value::Float(b)) => Ok(Value::Boolean(a == b)),
            (Value::String(a), Equal, Value::String(b)) => Ok(Value::Boolean(a == b)),
            (Value::Boolean(a), Equal, Value::Boolean(b)) => Ok(Value::Boolean(a == b)),
            (Value::Nil, Equal, Value::Nil) => Ok(Value::Boolean(true)),
            (Value::Integer(a), NotEqual, Value::Integer(b)) => Ok(Value::Boolean(a != b)),
            (Value::String(a), NotEqual, Value::String(b)) => Ok(Value::Boolean(a != b)),
            (Value::Boolean(a), NotEqual, Value::Boolean(b)) => Ok(Value::Boolean(a != b)),
            (Value::Integer(a), Less, Value::Integer(b)) => Ok(Value::Boolean(a < b)),
            (Value::Integer(a), LessEqual, Value::Integer(b)) => Ok(Value::Boolean(a <= b)),
            (Value::Integer(a), Greater, Value::Integer(b)) => Ok(Value::Boolean(a > b)),
            (Value::Integer(a), GreaterEqual, Value::Integer(b)) => Ok(Value::Boolean(a >= b)),
            (Value::Float(a), Less, Value::Float(b)) => Ok(Value::Boolean(a < b)),
            (Value::Float(a), LessEqual, Value::Float(b)) => Ok(Value::Boolean(a <= b)),
            (Value::Float(a), Greater, Value::Float(b)) => Ok(Value::Boolean(a > b)),
            (Value::Float(a), GreaterEqual, Value::Float(b)) => Ok(Value::Boolean(a >= b)),
            (a, Pipe, b) => Ok(Value::Boolean(self.truthy(&a) || self.truthy(&b))),
            (a, Ampersand, b) => Ok(Value::Boolean(self.truthy(&a) && self.truthy(&b))),
            (_, _, _) => Err(self.error("operator is not defined for these value types", span)),
        }
    }

    fn truthy(&self, value: &Value) -> bool {
        match value {
            Value::Boolean(v) => *v,
            Value::Integer(v) => *v != 0,
            Value::Float(v) => *v != 0.0,
            Value::String(v) => !v.is_empty(),
            Value::List(v) => !v.borrow().is_empty(),
            Value::Dict(v) => !v.borrow().is_empty(),
            Value::Nil => false,
        }
    }

    fn normalize_index(&self, value: &Value, length: usize, span: Span) -> Result<usize, RuntimeError> {
        let Value::Integer(index) = value else {
            return Err(self.error("list index must be an integer", span));
        };
        let normalized = if *index < 0 { length as i64 + *index } else { *index };
        if normalized < 0 || normalized as usize >= length {
            return Err(self.error(&format!("list index {index} is out of range for length {length}"), span));
        }
        Ok(normalized as usize)
    }

    fn error(&self, message: &str, span: Span) -> RuntimeError {
        RuntimeError { message: message.to_owned(), span }
    }
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Function { span, .. } | Stmt::Let { span, .. } | Stmt::Return { span, .. }
        | Stmt::If { span, .. } | Stmt::While { span, .. } | Stmt::For { span, .. }
        | Stmt::Assign { span, .. } | Stmt::Expression { span, .. } => *span,
    }
}

fn decode_string(raw: &str) -> Option<String> {
    let mut chars = raw.chars();
    let quote = chars.next()?;
    if (quote != '"' && quote != '\'') || raw.chars().last()? != quote { return None; }
    let inner = &raw[quote.len_utf8()..raw.len() - quote.len_utf8()];
    let mut result = String::new();
    let mut iter = inner.chars();
    while let Some(ch) = iter.next() {
        if ch != '\\' { result.push(ch); continue; }
        result.push(match iter.next()? {
            'n' => '\n', 'r' => '\r', 't' => '\t', '0' => '\0',
            '\\' => '\\', '"' => '"', '\'' => '\'',
            _ => return None,
        });
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn run(source: &str) -> Result<Value, RuntimeError> {
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let mut vm = Interpreter::new();
        vm.output = false;
        vm.execute(&program)
    }

    #[test]
    fn evaluates_arithmetic_and_variables() {
        assert_eq!(run("let answer = 6 * 7\nanswer\n").unwrap(), Value::Integer(42));
    }

    #[test]
    fn mut_variables_can_be_reassigned() {
        assert_eq!(run("mut count = 1\ncount = count + 1\ncount\n").unwrap(), Value::Integer(2));
    }

    #[test]
    fn immutable_variables_reject_assignment() {
        let error = run("let count = 1\ncount = 2\n").unwrap_err();
        assert!(error.message.contains("immutable"));
    }

    #[test]
    fn runs_if_else_and_while_loops() {
        assert_eq!(run("mut n = 0\nwhile n < 4:\n    n = n + 1\nmut result = 0\nif n == 4:\n    result = n\nelse:\n    result = 0\nresult\n").unwrap(), Value::Integer(4));
    }

    #[test]
    fn runs_functions_and_returns() {
        assert_eq!(run("function add(a: Int, b: Int) -> Int:\n    return a + b\nadd(20, 22)\n").unwrap(), Value::Integer(42));
    }

    #[test]
    fn detects_division_by_zero() {
        assert!(run("1 / 0\n").unwrap_err().message.contains("division by zero"));
    }

    #[test]
    fn decodes_string_escapes_and_concatenates() {
        assert_eq!(run("let name = \"Mo\\nsh\"\nname + \"!\"\n").unwrap(), Value::String("Mo\nsh!".into()));
    }

    #[test]
    fn creates_lists_and_reads_positive_and_negative_indexes() {
        assert_eq!(run("let values = [10, 20, 30]\nvalues[0]\n").unwrap(), Value::Integer(10));
        assert_eq!(run("let values = [10, 20, 30]\nvalues[-1]\n").unwrap(), Value::Integer(30));
    }

    #[test]
    fn mutates_list_items_and_supports_push_pop_and_len() {
        assert_eq!(run("mut values = [1, 2]\nvalues[0] = 9\npush(values, 3)\nlet removed = pop(values)\nlen(values) + values[0] + removed\n").unwrap(), Value::Integer(14));
    }

    #[test]
    fn iterates_over_lists_and_strings() {
        assert_eq!(run("mut total = 0\nfor value in [1, 2, 3, 4]:\n    total = total + value\ntotal\n").unwrap(), Value::Integer(10));
        assert_eq!(run("mut count = 0\nfor character in \"mosh\":\n    count = count + 1\ncount\n").unwrap(), Value::Integer(4));
    }

    #[test]
    fn reports_invalid_list_indexes_and_empty_pop() {
        assert!(run("let values = [1]\nvalues[2]\n").unwrap_err().message.contains("out of range"));
        assert!(run("let values = []\npop(values)\n").unwrap_err().message.contains("empty list"));
    }

    #[test]
    fn creates_indexes_mutates_and_iterates_dictionaries() {
        assert_eq!(run("let user = {\"name\": \"Mosh\", \"score\": 7}\nuser[\"name\"]\n").unwrap(), Value::String("Mosh".into()));
        assert_eq!(run("mut user = {\"score\": 1}\nuser[\"score\"] = 9\nuser[\"score\"]\n").unwrap(), Value::Integer(9));
        assert_eq!(run("mut user = {\"b\": 2, \"a\": 1}\nlet names = keys(user)\nmut total = 0\nfor name in names:\n    total = total + user[name]\ntotal + len(user)\n").unwrap(), Value::Integer(5));
        assert_eq!(run("mut user = {\"name\": \"Mosh\"}\nlet found = has_key(user, \"name\")\nlet removed = remove(user, \"name\")\nlen(user)\n").unwrap(), Value::Integer(0));
    }

    #[test]
    fn reports_missing_dictionary_keys_and_invalid_key_types() {
        assert!(run("let data = {\"x\": 1}\ndata[\"missing\"]\n").unwrap_err().message.contains("no key"));
        assert!(run("let data = {\"x\": 1}\ndata[0]\n").unwrap_err().message.contains("key must be a string"));
    }

    #[test]
    fn reports_undefined_names() {
        assert!(run("unknown\n").unwrap_err().message.contains("undefined variable"));
    }
}
