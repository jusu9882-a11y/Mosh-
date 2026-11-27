use crate::lexer::{Span, TokenKind};
use crate::parser::{Expr, Program, Stmt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fmt;
use std::io::{self, Write};
use std::fs;
use std::cell::RefCell;
use std::rc::Rc;

# [derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<HashMap<String, Value>>>),
    Tuple(Vec<Value>),
    Module(Rc<RefCell<Interpreter>>),
    FunctionRef(Rc<RefCell<Interpreter>>, String),
    Nil,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::List(a), Self::List(b)) => *a.borrow() == *b.borrow(),
            (Self::Dict(a), Self::Dict(b)) => *a.borrow() == *b.borrow(),
            (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (Self::Module(a), Self::Module(b)) => Rc::ptr_eq(a, b),
            (Self::FunctionRef(a, an), Self::FunctionRef(b, bn)) => an == bn && Rc::ptr_eq(a, b),
            (Self::Nil, Self::Nil) => true,
            _ => false,
        }
    }
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
            Self::Tuple(values) => {
                write!(f, "(")?;
                for (index, value) in values.iter().enumerate() { if index > 0 { write!(f, ", ")?; } write!(f, "{value}")?; }
                if values.len() == 1 { write!(f, ",")?; }
                write!(f, ")")
            }
            Self::Module(_) => write!(f, "<module>"),
            Self::FunctionRef(_, name) => write!(f, "<function {name}>"),
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

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Debug, Clone)]
struct Function {
    params: Vec<String>,
    body: Vec<Stmt>,
}

enum Flow {
    Continue,
    Return(Value),
}

# [derive(Debug)]
pub struct Interpreter {
    scopes: Vec<HashMap<String, Binding>>,
    functions: HashMap<String, Function>,
    output: bool,
    source_path: Option<PathBuf>,
    module_cache: Rc<RefCell<HashMap<PathBuf, Rc<RefCell<Interpreter>>>>>,
    import_stack: Rc<RefCell<Vec<PathBuf>>>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            output: false,
            source_path: None,
            module_cache: Rc::new(RefCell::new(HashMap::new())),
            import_stack: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn with_module_state(cache: Rc<RefCell<HashMap<PathBuf, Rc<RefCell<Interpreter>>>>>, stack: Rc<RefCell<Vec<PathBuf>>>, source_path: PathBuf) -> Self {
        Self { scopes: vec![HashMap::new()], functions: HashMap::new(), output: false, source_path: Some(source_path), module_cache: cache, import_stack: stack }
    }

    pub fn set_source_path(&mut self, path: impl AsRef<Path>) {
        self.source_path = Some(path.as_ref().to_path_buf());
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

    fn load_module(&mut self, requested: &str, span: Span) -> Result<Rc<RefCell<Interpreter>>, RuntimeError> {
        let requested_path = Path::new(requested);
        let candidate = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            let base = self.source_path.as_ref().and_then(|p| p.parent()).ok_or_else(|| self.error(
                "relative imports require execution from a .mosh file; imports are unavailable in the REPL", span))?;
            base.join(requested_path)
        };
        let canonical = candidate.canonicalize().map_err(|error| self.error(
            &format!("cannot resolve imported module '{}': {error}", candidate.display()), span))?;
        if let Some(module) = self.module_cache.borrow().get(&canonical).cloned() { return Ok(module); }
        {
            let stack = self.import_stack.borrow();
            if let Some(position) = stack.iter().position(|path| path == &canonical) {
                let mut chain = stack[position..].iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
                chain.push(canonical.display().to_string());
                return Err(self.error(&format!("circular import detected: {}", chain.join(" -> ")), span));
            }
        }
        let source = fs::read_to_string(&canonical).map_err(|error| self.error(
            &format!("cannot read imported module '{}': {error}", canonical.display()), span))?;
        let tokens = crate::lexer::tokenize(&source).map_err(|error| self.error(
            &format!("{}: {error}", canonical.display()), span))?;
        let program = crate::parser::parse(&tokens).map_err(|error| self.error(
            &format!("{}: {error}", canonical.display()), span))?;
        self.import_stack.borrow_mut().push(canonical.clone());
        let module = Rc::new(RefCell::new(Interpreter::with_module_state(
            self.module_cache.clone(), self.import_stack.clone(), canonical.clone())));
        let execution = module.borrow_mut().execute(&program);
        self.import_stack.borrow_mut().pop();
        execution.map_err(|error| self.error(
            &format!("module '{}' failed: {error}", canonical.display()), span))?;
        self.module_cache.borrow_mut().insert(canonical, module.clone());
        Ok(module)
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
            Stmt::Import { path, alias, span } => {
                let module = self.load_module(path, *span)?;
                if self.scopes.last().is_some_and(|scope| scope.contains_key(alias)) { return Err(self.error(&format!("'{alias}' is already declared in this scope"), *span)); }
                self.scopes.last_mut().expect("interpreter always has a scope").insert(alias.clone(), Binding { value: Value::Module(module), mutable: false });
                Ok(Flow::Continue)
            }
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
            Stmt::Raise { value, span } => {
                let raised = self.eval(value)?;
                Err(self.error(&raised.to_string(), *span))
            }
            Stmt::Try { body, catch_name, catch_body, .. } => {
                match self.execute_block(body, true) {
                    Ok(flow) => Ok(flow),
                    Err(error) => {
                        self.scopes.push(HashMap::new());
                        self.scopes.last_mut().unwrap().insert(
                            catch_name.clone(),
                            Binding { value: Value::String(error.to_string()), mutable: false },
                        );
                        let result = self.execute_block(catch_body, true);
                        self.scopes.pop();
                        result
                    }
                }
            }
            Stmt::Return { value, .. } => Ok(Flow::Return(match value {
                Some(expr) => self.eval(expr)?,
                None => Value::Nil,
            })),
            Stmt::For { name, iterable, body, span } => {
                let values = match self.eval(iterable)? {
                    Value::List(values) => values.borrow().clone(),
                    Value::String(value) => value.chars().map(|ch| Value::String(ch.to_string())).collect(),
                    Value::Tuple(values) => values.clone(),
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
            Expr::Tuple { elements, .. } => {
                let values = elements.iter().map(|element| self.eval(element)).collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Tuple(values))
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
                let args = arguments.iter().map(|arg| self.eval(arg)).collect::<Result<Vec<_>, _>>()?;
                match callee.as_ref() {
                    Expr::Identifier { name, .. } => self.call(name, args, *span),
                    _ => match self.eval(callee)? {
                        Value::FunctionRef(module, name) => module.borrow_mut().call(&name, args, *span),
                        _ => Err(self.error("expression is not callable", *span)),
                    }
                }
            }
            Expr::Index { object, index, span } => {
                let object = self.eval(object)?;
                let index = self.eval(index)?;
                match object {
                    Value::List(values) => { let index = self.normalize_index(&index, values.borrow().len(), *span)?; Ok(values.borrow()[index].clone()) }
                    Value::Dict(entries) => { let Value::String(key) = index else { return Err(self.error("dictionary key must be a string", *span)); }; entries.borrow().get(&key).cloned().ok_or_else(|| self.error(&format!("dictionary has no key '{key}'"), *span)) }
                    Value::Tuple(values) => { let index = self.normalize_index(&index, values.len(), *span)?; Ok(values[index].clone()) }
                    _ => Err(self.error("indexing requires a list, tuple, or dictionary", *span)),
                }
            }
            Expr::Member { object, name, span } => {
                match self.eval(object)? {
                    Value::Module(module) => {
                        let module_ref = module.borrow();
                        if let Some(binding) = module_ref.scopes.first().and_then(|scope| scope.get(name)) {
                            Ok(binding.value.clone())
                        } else if module_ref.functions.contains_key(name) {
                            Ok(Value::FunctionRef(module.clone(), name.clone()))
                        } else {
                            Err(self.error(&format!("module has no exported member '{name}'"), *span))
                        }
                    }
                    _ => Err(self.error("member access is only supported on modules", *span)),
                }
            },
        }
    }

    fn call(&mut self, name: &str, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
        match name {
            "len" => {
                if args.len() != 1 { return Err(self.error("len() expects exactly one argument", span)); }
                let length = match &args[0] {
                    Value::List(values) => values.borrow().len(),
                    Value::Dict(values) => values.borrow().len(),
                    Value::Tuple(values) => values.len(),
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
            "upper" | "lower" | "trim" | "trim_start" | "trim_end" => {
                if args.len() != 1 { return Err(self.error("string transformation expects exactly one argument", span)); }
                let Value::String(value) = &args[0] else { return Err(self.error("string transformation expects a string", span)); };
                Ok(Value::String(match name { "upper" => value.to_uppercase(), "lower" => value.to_lowercase(), "trim" => value.trim().to_owned(), "trim_start" => value.trim_start().to_owned(), _ => value.trim_end().to_owned() }))
            }
            "contains" | "starts_with" | "ends_with" => {
                if args.len() != 2 { return Err(self.error("string predicate expects two string arguments", span)); }
                let (Value::String(value), Value::String(pattern)) = (&args[0], &args[1]) else { return Err(self.error("string predicate arguments must be strings", span)); };
                Ok(Value::Boolean(match name { "contains" => value.contains(pattern), "starts_with" => value.starts_with(pattern), _ => value.ends_with(pattern) }))
            }
            "replace" => {
                if args.len() != 3 { return Err(self.error("replace() expects a string, search text, and replacement", span)); }
                let (Value::String(value), Value::String(from), Value::String(to)) = (&args[0], &args[1], &args[2]) else { return Err(self.error("replace() arguments must be strings", span)); };
                Ok(Value::String(value.replace(from, to)))
            }
            "split" => {
                if args.len() != 2 { return Err(self.error("split() expects a string and separator", span)); }
                let (Value::String(value), Value::String(separator)) = (&args[0], &args[1]) else { return Err(self.error("split() arguments must be strings", span)); };
                if separator.is_empty() { return Err(self.error("split() separator cannot be empty", span)); }
                Ok(Value::List(Rc::new(RefCell::new(value.split(separator).map(|part| Value::String(part.to_owned())).collect()))))
            }
            "join" => {
                if args.len() != 2 { return Err(self.error("join() expects a list of strings and a separator", span)); }
                let Value::List(values) = &args[0] else { return Err(self.error("join() first argument must be a list", span)); };
                let Value::String(separator) = &args[1] else { return Err(self.error("join() separator must be a string", span)); };
                let values = values.borrow();
                let mut parts = Vec::with_capacity(values.len());
                for value in values.iter() { let Value::String(part) = value else { return Err(self.error("join() list must contain only strings", span)); }; parts.push(part.as_str()); }
                Ok(Value::String(parts.join(separator)))
            }
            "read_text" => {
                if args.len() != 1 { return Err(self.error("read_text() expects one path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("read_text() path must be a string", span)); };
                fs::read_to_string(path).map(Value::String).map_err(|e| self.error(&format!("read_text() failed for '{path}': {e}"), span))
            }
            "write_text" | "append_text" => {
                if args.len() != 2 { return Err(self.error("write_text()/append_text() expect a path and text", span)); }
                let (Value::String(path), Value::String(contents)) = (&args[0], &args[1]) else { return Err(self.error("file path and contents must be strings", span)); };
                let result = if name == "write_text" { fs::write(path, contents) } else {
                    use std::fs::OpenOptions;
                    OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(contents.as_bytes()))
                };
                result.map(|_| Value::Nil).map_err(|e| self.error(&format!("{name}() failed for '{path}': {e}"), span))
            }
            "file_exists" => {
                if args.len() != 1 { return Err(self.error("file_exists() expects one path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("file_exists() path must be a string", span)); };
                Ok(Value::Boolean(std::path::Path::new(path).exists()))
            }
            "create_dir" => {
                if args.len() != 1 { return Err(self.error("create_dir() expects one path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("create_dir() path must be a string", span)); };
                fs::create_dir_all(path).map(|_| Value::Nil).map_err(|e| self.error(&format!("create_dir() failed for '{path}': {e}"), span))
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
            Value::Tuple(v) => !v.is_empty(),
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
        | Stmt::Raise { span, .. } | Stmt::Try { span, .. }
        | Stmt::If { span, .. } | Stmt::While { span, .. } | Stmt::For { span, .. }
        | Stmt::Assign { span, .. } | Stmt::Expression { span, .. }
        | Stmt::Import { span, .. } => *span,
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
    fn imports_module_values_and_functions_in_an_isolated_namespace() {
        let root = std::env::temp_dir().join(format!("mosh-module-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let module_path = root.join("math.mosh");
        let main_path = root.join("main.mosh");
        std::fs::write(&module_path, "let answer = 40\nfunction add_two(value: Int) -> Int:\n    return value + 2\n").unwrap();
        let source = "import \"math.mosh\" as math\nmath.add_two(math.answer)\n";
        std::fs::write(&main_path, source).unwrap();
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let mut vm = Interpreter::new();
        vm.set_source_path(&main_path);
        assert_eq!(vm.execute(&program).unwrap(), Value::Integer(42));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_names_do_not_leak_into_the_importer() {
        let root = std::env::temp_dir().join(format!("mosh-module-isolation-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("values.mosh"), "let hidden = 9\n").unwrap();
        let source = "import \"values.mosh\" as values\nhidden\n";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let mut vm = Interpreter::new();
        vm.set_source_path(root.join("main.mosh"));
        let error = vm.execute(&program).unwrap_err();
        assert!(error.message.contains("undefined variable 'hidden'"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_imports_reuse_the_initialized_module() {
        let root = std::env::temp_dir().join(format!("mosh-module-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("state.mosh"), "mut count = 0\nfunction increment():\n    count = count + 1\n").unwrap();
        let source = "import \"state.mosh\" as first\nimport \"state.mosh\" as second\nfirst.increment()\nsecond.count\n";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let mut vm = Interpreter::new();
        vm.set_source_path(root.join("main.mosh"));
        assert_eq!(vm.execute(&program).unwrap(), Value::Integer(1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn detects_direct_and_indirect_circular_imports() {
        let root = std::env::temp_dir().join(format!("mosh-module-cycle-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.mosh"), "import \"b.mosh\" as b\n").unwrap();
        std::fs::write(root.join("b.mosh"), "import \"a.mosh\" as a\n").unwrap();
        let source = "import \"a.mosh\" as a\n";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let mut vm = Interpreter::new();
        vm.set_source_path(root.join("main.mosh"));
        let error = vm.execute(&program).unwrap_err();
        assert!(error.message.contains("circular import detected"));
        assert!(error.message.contains("a.mosh") && error.message.contains("b.mosh"));
        let _ = std::fs::remove_dir_all(root);
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
    fn supports_tuple_values_indexing_iteration_and_length() {
        assert_eq!(run("let pair = (10, \"Mosh\")\npair[1]\n").unwrap(), Value::String("Mosh".into()));
        assert_eq!(run("let pair = (10, 20, 30)\npair[-1]\n").unwrap(), Value::Integer(30));
        assert_eq!(run("let pair = (2, 3, 4)\nmut total = 0\nfor value in pair:\n    total = total + value\ntotal + len(pair)\n").unwrap(), Value::Integer(12));
        assert_eq!(run("let empty = ()\nlen(empty)\n").unwrap(), Value::Integer(0));
    }

    #[test]
    fn supports_string_transformations_search_replace_split_and_join() {
        assert_eq!(run("upper(\"mosh\")\n").unwrap(), Value::String("MOSH".into()));
        assert_eq!(run("lower(\"MOSH\")\n").unwrap(), Value::String("mosh".into()));
        assert_eq!(run("trim(\"  hello  \")\n").unwrap(), Value::String("hello".into()));
        assert_eq!(run("replace(\"one fish\", \"fish\", \"bird\")\n").unwrap(), Value::String("one bird".into()));
        assert_eq!(run("let words = split(\"red,green,blue\", \",\" )\njoin(words, \"-\")\n").unwrap(), Value::String("red-green-blue".into()));
        assert_eq!(run("contains(\"Mosh language\", \"language\")\n").unwrap(), Value::Boolean(true));
        assert_eq!(run("starts_with(\"Mosh\", \"Mo\")\n").unwrap(), Value::Boolean(true));
        assert_eq!(run("ends_with(\"Mosh\", \"sh\")\n").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn rejects_invalid_string_operation_arguments() {
        assert!(run("split(\"abc\", \"\")\n").unwrap_err().message.contains("cannot be empty"));
        assert!(run("join([\"a\", 2], \",\" )\n").unwrap_err().message.contains("only strings"));
    }

    #[test]
    fn catches_runtime_errors_and_exposes_error_message() {
        let source = "mut message = \"\"\ntry:\n    let value = 10 / 0\ncatch error:\n    message = error\nmessage\n";
        let result = run(source).unwrap();
        assert!(matches!(result, Value::String(message) if message.contains("division by zero")));
    }

    #[test]
    fn raises_explicit_errors_and_propagates_them_through_functions() {
        let source = "function fail():\n    raise \"deliberate failure\"\nmut message = \"\"\ntry:\n    fail()\ncatch error:\n    message = error\nmessage\n";
        let result = run(source).unwrap();
        assert!(matches!(result, Value::String(message) if message.contains("deliberate failure")));
    }

    #[test]
    fn try_success_skips_catch_and_nested_catch_handles_errors() {
        let source = "mut result = 0\ntry:\n    try:\n        result = 1\n    catch inner:\n        result = 2\ncatch outer:\n    result = 3\nresult\n";
        assert_eq!(run(source).unwrap(), Value::Integer(1));
    }

    #[test]
    fn filesystem_builtins_use_the_real_filesystem() {
        let root = std::env::temp_dir().join(format!("mosh-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("nested");
        let file = dir.join("notes.txt");
        let source = format!(
            "create_dir(\"{}\")\nwrite_text(\"{}\", \"hello\")\nappend_text(\"{}\", \" mosh\")\nread_text(\"{}\")\n",
            dir.display(), file.display(), file.display(), file.display()
        );
        assert_eq!(run(&source).unwrap(), Value::String("hello mosh".into()));
        let exists = format!("file_exists(\"{}\")\n", file.display());
        assert_eq!(run(&exists).unwrap(), Value::Boolean(true));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn filesystem_builtins_report_io_and_argument_errors() {
        assert!(run("read_text(\"/mosh-path-that-does-not-exist/file.txt\")\n").unwrap_err().message.contains("read_text() failed"));
        assert!(run("write_text(1, \"x\")\n").unwrap_err().message.contains("must be strings"));
        assert!(run("file_exists()\n").unwrap_err().message.contains("expects one path"));
    }

    #[test]
    fn reports_undefined_names() {
        assert!(run("unknown\n").unwrap_err().message.contains("undefined variable"));
    }
}
