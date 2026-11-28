use crate::device::{DeviceRegistry, PermissionState};
use crate::lexer::{Span, TokenKind};
use crate::parser::{Expr, Program, Stmt};
use crate::module_registry::ModuleRegistry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fmt;
use std::io::{self, Write};
use std::fs;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
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
    DeviceRoot,
    DeviceModule(String),
    DeviceFunction(String, String),
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
            (Self::DeviceRoot, Self::DeviceRoot) => true,
            (Self::DeviceModule(a), Self::DeviceModule(b)) => a == b,
            (Self::DeviceFunction(am, an), Self::DeviceFunction(bm, bn)) => am == bm && an == bn,
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
            Self::DeviceRoot => write!(f, "<device namespace>"),
            Self::DeviceModule(name) => write!(f, "<device module {name}>"),
            Self::DeviceFunction(module, name) => write!(f, "<device function {module}.{name}>"),
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

#[derive(Debug)]
pub struct Interpreter {
    scopes: Vec<HashMap<String, Binding>>,
    functions: HashMap<String, Function>,
    output: bool,
    source_path: Option<PathBuf>,
    module_cache: Rc<RefCell<HashMap<PathBuf, Rc<RefCell<Interpreter>>>>>,
    import_stack: Rc<RefCell<Vec<PathBuf>>>,
    device_registry: DeviceRegistry,
    module_registry: ModuleRegistry,
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
            device_registry: DeviceRegistry::system_default(),
            module_registry: ModuleRegistry::new(),
        }
    }

    pub fn with_device_backend(backend: std::sync::Arc<dyn crate::device::DeviceBackend>) -> Self {
        let mut interpreter = Self::new();
        interpreter.device_registry = DeviceRegistry::new(backend);
        interpreter
    }

    fn with_module_state(cache: Rc<RefCell<HashMap<PathBuf, Rc<RefCell<Interpreter>>>>>, stack: Rc<RefCell<Vec<PathBuf>>>, source_path: PathBuf, device_registry: DeviceRegistry, module_registry: ModuleRegistry) -> Self {
        Self { scopes: vec![HashMap::new()], functions: HashMap::new(), output: false, source_path: Some(source_path), module_cache: cache, import_stack: stack, device_registry, module_registry }
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
            self.module_cache.clone(), self.import_stack.clone(), canonical.clone(), self.device_registry.clone(), self.module_registry.clone())));
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
                if name == "device" { Ok(Value::DeviceRoot) } else { Err(self.error(&format!("undefined variable '{name}'"), *span)) }
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
                        Value::DeviceFunction(module, name) => self.call_device(&module, &name, args, *span),
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
                    Value::DeviceRoot => Ok(Value::DeviceModule(name.clone())),
                    Value::DeviceModule(module) => {
                        const METHODS: &[&str] = &["is_supported", "is_available", "capabilities", "status", "permission", "request_permission", "read", "volumes"];
                        if METHODS.contains(&name.as_str()) {
                            Ok(Value::DeviceFunction(module.clone(), name.clone()))
                        } else {
                            Err(self.error(&format!("device module has no method '{name}'"), *span))
                        }
                    }
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

    fn call_device(&self, module: &str, method: &str, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
        if !args.is_empty() {
            return Err(self.error(&format!("device.{module}.{method}() expects no arguments"), span));
        }
        let canonical_module = match module {
            "ram" => "memory",
            "power" => "battery",
            "fs" => "storage",
            _ => module,
        };
        let name = format!("device.{canonical_module}");
        let capability = self.device_registry.capability(&name);
        match method {
            "is_supported" => Ok(Value::Boolean(capability.supported)),
            "is_available" => Ok(Value::Boolean(capability.supported && capability.available)),
            "capabilities" | "status" => {
                let mut values = HashMap::new();
                values.insert("module".into(), Value::String(capability.module));
                values.insert("platform".into(), Value::String(format!("{:?}", capability.platform).to_lowercase()));
                values.insert("supported".into(), Value::Boolean(capability.supported));
                values.insert("available".into(), Value::Boolean(capability.available));
                values.insert("permission".into(), Value::String(permission_name(capability.permission).into()));
                values.insert("features".into(), Value::List(Rc::new(RefCell::new(
                    capability.features.into_iter().map(Value::String).collect()
                ))));
                values.insert("limitations".into(), Value::List(Rc::new(RefCell::new(
                    capability.limitations.into_iter().map(Value::String).collect()
                ))));
                Ok(Value::Dict(Rc::new(RefCell::new(values))))
            }
            "permission" => {
                if !capability.supported {
                    return Ok(Value::String("unknown".into()));
                }
                let state = self.device_registry.permission_state(&name)
                    .map_err(|error| self.error(&error.to_string(), span))?;
                Ok(Value::String(permission_name(state).into()))
            }
            "read" if canonical_module == "battery" => {
                let reading = self.device_registry.read_battery()
                    .map_err(|error| self.error(&error.to_string(), span))?;
                let mut values = HashMap::new();
                values.insert("present".into(), Value::Boolean(reading.present));
                values.insert("capacity_percent".into(), reading.capacity_percent.map(Value::Float).unwrap_or(Value::Nil));
                values.insert("state".into(), reading.state.map(Value::String).unwrap_or(Value::Nil));
                values.insert("voltage_mv".into(), reading.voltage_mv.map(Value::Float).unwrap_or(Value::Nil));
                values.insert("current_ma".into(), reading.current_ma.map(Value::Float).unwrap_or(Value::Nil));
                values.insert("temperature_c".into(), reading.temperature_c.map(Value::Float).unwrap_or(Value::Nil));
                values.insert("technology".into(), reading.technology.map(Value::String).unwrap_or(Value::Nil));
                Ok(Value::Dict(Rc::new(RefCell::new(values))))
            }
            "read" if canonical_module == "memory" => {
                let reading = self.device_registry.read_memory()
                    .map_err(|error| self.error(&error.to_string(), span))?;
                let mut values = HashMap::new();
                values.insert("total_bytes".into(), Value::Integer(i64::try_from(reading.total_bytes).map_err(|_| self.error("total memory exceeds Mosh integer range", span))?));
                values.insert("available_bytes".into(), Value::Integer(i64::try_from(reading.available_bytes).map_err(|_| self.error("available memory exceeds Mosh integer range", span))?));
                values.insert("free_bytes".into(), Value::Integer(i64::try_from(reading.free_bytes).map_err(|_| self.error("free memory exceeds Mosh integer range", span))?));
                values.insert("swap_total_bytes".into(), Value::Integer(i64::try_from(reading.swap_total_bytes).map_err(|_| self.error("swap memory exceeds Mosh integer range", span))?));
                values.insert("swap_free_bytes".into(), Value::Integer(i64::try_from(reading.swap_free_bytes).map_err(|_| self.error("free swap memory exceeds Mosh integer range", span))?));
                Ok(Value::Dict(Rc::new(RefCell::new(values))))
            }
            "read" if canonical_module == "cpu" => {
                let reading = self.device_registry.read_cpu()
                    .map_err(|error| self.error(&error.to_string(), span))?;
                let mut values = HashMap::new();
                values.insert("model".into(), Value::String(reading.model));
                values.insert("architecture".into(), Value::String(reading.architecture));
                values.insert("logical_cores".into(), Value::Integer(reading.logical_cores as i64));
                values.insert("frequency_mhz".into(), reading.frequency_mhz.map(Value::Float).unwrap_or(Value::Nil));
                Ok(Value::Dict(Rc::new(RefCell::new(values))))
            }
            "volumes" if canonical_module == "storage" => {
                let volumes = self.device_registry.read_storage_volumes()
                    .map_err(|error| self.error(&error.to_string(), span))?;
                let values = volumes.into_iter().map(|volume| {
                    let mut fields = HashMap::new();
                    fields.insert("source".into(), Value::String(volume.source));
                    fields.insert("mount_point".into(), Value::String(volume.mount_point));
                    fields.insert("filesystem".into(), Value::String(volume.filesystem));
                    fields.insert("options".into(), Value::String(volume.options));
                    fields.insert("read_only".into(), Value::Boolean(volume.read_only));
                    Value::Dict(Rc::new(RefCell::new(fields)))
                }).collect();
                Ok(Value::List(Rc::new(RefCell::new(values))))
            }
            "volumes" => Err(self.error(&format!("device.{module}.volumes() is not implemented by the active backend"), span)),
            "read" => Err(self.error(&format!("device.{module}.read() is not implemented by the active backend"), span)),
            "request_permission" => {
                let state = self.device_registry.request_permission(&name)
                    .map_err(|error| self.error(&error.to_string(), span))?;
                Ok(Value::String(permission_name(state).into()))
            }
            _ => Err(self.error(&format!("unknown device method '{method}'"), span)),
        }
    }

    fn call(&mut self, name: &str, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
        match name {
            "modules" => {
                if !args.is_empty() { return Err(self.error("modules() expects no arguments", span)); }
                let names = self.module_registry.namespaces().map(|name| Value::String(name.to_owned())).collect();
                Ok(Value::List(Rc::new(RefCell::new(names))))
            }
            "implemented_modules" => {
                if !args.is_empty() { return Err(self.error("implemented_modules() expects no arguments", span)); }
                let names = self.module_registry.implemented_namespaces().map(|name| Value::String(name.to_owned())).collect();
                Ok(Value::List(Rc::new(RefCell::new(names))))
            }
            "module_info" => {
                if args.len() != 1 { return Err(self.error("module_info() expects exactly one namespace string", span)); }
                let Value::String(namespace) = &args[0] else { return Err(self.error("module_info() namespace must be a string", span)); };
                let Some(descriptor) = self.module_registry.get(namespace) else {
                    return Err(self.error(&format!("unknown module namespace '{namespace}'"), span));
                };
                let mut info = HashMap::new();
                info.insert("namespace".into(), Value::String(descriptor.namespace.into()));
                info.insert("implemented".into(), Value::Boolean(descriptor.implemented));
                info.insert("description".into(), Value::String(descriptor.description.into()));
                info.insert("modules".into(), Value::List(Rc::new(RefCell::new(
                    descriptor.modules.iter().map(|name| Value::String((*name).into())).collect()
                ))));
                Ok(Value::Dict(Rc::new(RefCell::new(info))))
            }
            "type" => {
                if args.len() != 1 { return Err(self.error("type() expects exactly one argument", span)); }
                let kind = match &args[0] {
                    Value::Integer(_) => "int", Value::Float(_) => "float",
                    Value::String(_) => "string", Value::Boolean(_) => "bool",
                    Value::List(_) => "list", Value::Dict(_) => "dict",
                    Value::Tuple(_) => "tuple", Value::Module(_) => "module",
                    Value::FunctionRef(_, _) => "function", Value::DeviceRoot => "device_namespace",
                    Value::DeviceModule(_) => "device_module", Value::DeviceFunction(_, _) => "function", Value::Nil => "nil",
                };
                Ok(Value::String(kind.to_owned()))
            }
            "str" => {
                if args.len() != 1 { return Err(self.error("str() expects exactly one argument", span)); }
                Ok(Value::String(args[0].to_string()))
            }
            "int" => {
                if args.len() != 1 { return Err(self.error("int() expects exactly one argument", span)); }
                match &args[0] {
                    Value::Integer(value) => Ok(Value::Integer(*value)),
                    Value::Float(value) if value.is_finite() && *value >= i64::MIN as f64 && *value < 9_223_372_036_854_775_808.0 => Ok(Value::Integer(value.trunc() as i64)),
                    Value::String(value) => value.parse::<i64>().map(Value::Integer).map_err(|_| self.error("int() cannot convert this string to an integer", span)),
                    _ => Err(self.error("int() expects an integer, finite float, or integer string", span)),
                }
            }
            "float" => {
                if args.len() != 1 { return Err(self.error("float() expects exactly one argument", span)); }
                match &args[0] {
                    Value::Integer(value) => Ok(Value::Float(*value as f64)),
                    Value::Float(value) => Ok(Value::Float(*value)),
                    Value::String(value) => value.parse::<f64>().ok().filter(|v| v.is_finite()).map(Value::Float).ok_or_else(|| self.error("float() cannot convert this string to a finite number", span)),
                    _ => Err(self.error("float() expects an integer, float, or numeric string", span)),
                }
            }
            "abs" => {
                if args.len() != 1 { return Err(self.error("abs() expects exactly one number", span)); }
                match &args[0] {
                    Value::Integer(value) => value.checked_abs().map(Value::Integer).ok_or_else(|| self.error("abs() integer overflow", span)),
                    Value::Float(value) => Ok(Value::Float(value.abs())),
                    _ => Err(self.error("abs() argument must be a number", span)),
                }
            }
            "sqrt" | "floor" | "ceil" | "round" => {
                if args.len() != 1 { return Err(self.error(&format!("{name}() expects exactly one number"), span)); }
                let value = match &args[0] { Value::Integer(v) => *v as f64, Value::Float(v) => *v, _ => return Err(self.error(&format!("{name}() argument must be a number"), span)) };
                if name == "sqrt" && value < 0.0 { return Err(self.error("sqrt() cannot accept a negative number", span)); }
                let result = match name { "sqrt" => value.sqrt(), "floor" => value.floor(), "ceil" => value.ceil(), _ => value.round() };
                if !result.is_finite() { return Err(self.error(&format!("{name}() result is not finite"), span)); }
                Ok(Value::Float(result))
            }
            "pow" => {
                if args.len() != 2 { return Err(self.error("pow() expects two numbers", span)); }
                match (&args[0], &args[1]) {
                    (Value::Integer(base), Value::Integer(exponent)) if *exponent >= 0 && *exponent <= u32::MAX as i64 => base.checked_pow(*exponent as u32).map(Value::Integer).ok_or_else(|| self.error("pow() integer overflow", span)),
                    (Value::Integer(base), Value::Integer(exponent)) if *exponent < 0 => {
                        if *base == 0 { return Err(self.error("pow() division by zero", span)); }
                        Ok(Value::Float((*base as f64).powf(*exponent as f64)))
                    }
                    (Value::Integer(a), Value::Float(b)) => Ok(Value::Float((*a as f64).powf(*b))),
                    (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a.powf(*b as f64))),
                    (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),
                    _ => Err(self.error("pow() arguments must be numbers", span)),
                }
            }
            "min" | "max" => {
                if args.is_empty() { return Err(self.error(&format!("{name}() expects at least one number"), span)); }
                let has_float = args.iter().any(|v| matches!(v, Value::Float(_)));
                if !has_float {
                    let mut numbers = Vec::with_capacity(args.len());
                    for value in &args {
                        let Value::Integer(number) = value else { return Err(self.error(&format!("{name}() arguments must all be numbers"), span)); };
                        numbers.push(*number);
                    }
                    let selected = if name == "min" { *numbers.iter().min().unwrap() } else { *numbers.iter().max().unwrap() };
                    Ok(Value::Integer(selected))
                } else {
                    let mut numbers = Vec::with_capacity(args.len());
                    for value in &args {
                        match value {
                            Value::Integer(v) => numbers.push(*v as f64),
                            Value::Float(v) => numbers.push(*v),
                            _ => return Err(self.error(&format!("{name}() arguments must all be numbers"), span)),
                        }
                    }
                    let selected = if name == "min" { numbers.into_iter().fold(f64::INFINITY, f64::min) } else { numbers.into_iter().fold(f64::NEG_INFINITY, f64::max) };
                    Ok(Value::Float(selected))
                }
            }
            "sum" => {
                if args.len() != 1 { return Err(self.error("sum() expects exactly one list or tuple", span)); }
                let values = match &args[0] { Value::List(v) => v.borrow().clone(), Value::Tuple(v) => v.clone(), _ => return Err(self.error("sum() expects a list or tuple", span)) };
                let mut integer_total = 0i64;
                let mut float_total = 0.0f64;
                let mut has_float = false;
                for value in values {
                    match value {
                        Value::Integer(v) if !has_float => integer_total = integer_total.checked_add(v).ok_or_else(|| self.error("sum() integer overflow", span))?,
                        Value::Integer(v) => float_total += v as f64,
                        Value::Float(v) => { if !has_float { float_total = integer_total as f64; has_float = true; } float_total += v; }
                        _ => return Err(self.error("sum() values must all be numbers", span)),
                    }
                }
                if has_float { Ok(Value::Float(float_total)) } else { Ok(Value::Integer(integer_total)) }
            }
            "range" => {
                if !(1..=3).contains(&args.len()) { return Err(self.error("range() expects one, two, or three integer arguments", span)); }
                let mut numbers = Vec::with_capacity(args.len());
                for value in &args {
                    let Value::Integer(number) = value else { return Err(self.error("range() arguments must be integers", span)); };
                    numbers.push(*number);
                }
                let (start, stop, step) = match numbers.as_slice() { [stop] => (0, *stop, 1), [start, stop] => (*start, *stop, 1), [start, stop, step] => (*start, *stop, *step), _ => unreachable!() };
                if step == 0 { return Err(self.error("range() step cannot be zero", span)); }
                let mut values = Vec::new();
                let mut current = start;
                while if step > 0 { current < stop } else { current > stop } {
                    if values.len() >= 1_000_000 { return Err(self.error("range() exceeds the 1,000,000 item safety limit", span)); }
                    values.push(Value::Integer(current));
                    current = current.checked_add(step).ok_or_else(|| self.error("range() integer overflow", span))?;
                }
                Ok(Value::List(Rc::new(RefCell::new(values))))
            }
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
            "values" => {
                if args.len() != 1 { return Err(self.error("values() expects exactly one dictionary", span)); }
                let Value::Dict(entries) = &args[0] else { return Err(self.error("values() argument must be a dictionary", span)); };
                let entries = entries.borrow();
                let mut keys: Vec<_> = entries.keys().collect();
                keys.sort();
                Ok(Value::List(Rc::new(RefCell::new(keys.into_iter().map(|key| entries[key].clone()).collect()))))
            }
            "sorted" => {
                if args.len() != 1 { return Err(self.error("sorted() expects exactly one list or tuple", span)); }
                let mut values = match &args[0] {
                    Value::List(values) => values.borrow().clone(),
                    Value::Tuple(values) => values.clone(),
                    _ => return Err(self.error("sorted() expects a list or tuple", span)),
                };
                let numeric = values.iter().all(|v| matches!(v, Value::Integer(_) | Value::Float(_)));
                let strings = values.iter().all(|v| matches!(v, Value::String(_)));
                if !numeric && !strings && !values.is_empty() {
                    return Err(self.error("sorted() values must all be numbers or all be strings", span));
                }
                if numeric {
                    values.sort_by(|a, b| match (a, b) {
                        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
                        (Value::Integer(a), Value::Float(b)) => (*a as f64).partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
                        (Value::Float(a), Value::Integer(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(std::cmp::Ordering::Equal),
                        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
                        _ => unreachable!(),
                    });
                } else {
                    values.sort_by(|a, b| {
                        let (Value::String(a), Value::String(b)) = (a, b) else { unreachable!() };
                        a.cmp(b)
                    });
                }
                Ok(Value::List(Rc::new(RefCell::new(values))))
            }
            "reverse" => {
                if args.len() != 1 { return Err(self.error("reverse() expects exactly one list, tuple, or string", span)); }
                match &args[0] {
                    Value::List(values) => {
                        let mut reversed = values.borrow().clone();
                        reversed.reverse();
                        Ok(Value::List(Rc::new(RefCell::new(reversed))))
                    }
                    Value::Tuple(values) => {
                        let mut reversed = values.clone();
                        reversed.reverse();
                        Ok(Value::List(Rc::new(RefCell::new(reversed))))
                    }
                    Value::String(value) => Ok(Value::String(value.chars().rev().collect())),
                    _ => Err(self.error("reverse() expects a list, tuple, or string", span)),
                }
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
            "path_join" => {
                if args.is_empty() { return Err(self.error("path_join() expects at least one path string", span)); }
                let mut path = PathBuf::new();
                for value in &args {
                    let Value::String(part) = value else { return Err(self.error("path_join() arguments must be strings", span)); };
                    path.push(part);
                }
                Ok(Value::String(path.to_string_lossy().into_owned()))
            }
            "is_file" | "is_dir" => {
                if args.len() != 1 { return Err(self.error(&format!("{name}() expects one path string"), span)); }
                let Value::String(path) = &args[0] else { return Err(self.error(&format!("{name}() path must be a string"), span)); };
                let metadata = fs::metadata(path);
                Ok(Value::Boolean(metadata.is_ok_and(|m| if name == "is_file" { m.is_file() } else { m.is_dir() })))
            }
            "file_size" => {
                if args.len() != 1 { return Err(self.error("file_size() expects one path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("file_size() path must be a string", span)); };
                let size = fs::metadata(path).and_then(|m| {
                    if !m.is_file() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is not a file")); }
                    i64::try_from(m.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is too large"))
                }).map_err(|e| self.error(&format!("file_size() failed for '{path}': {e}"), span))?;
                Ok(Value::Integer(size))
            }
            "list_dir" => {
                if args.len() != 1 { return Err(self.error("list_dir() expects one directory path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("list_dir() path must be a string", span)); };
                let entries = fs::read_dir(path).map_err(|e| self.error(&format!("list_dir() failed for '{path}': {e}"), span))?;
                let mut names = entries.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                    .collect::<Result<Vec<_>, _>>().map_err(|e| self.error(&format!("list_dir() failed for '{path}': {e}"), span))?;
                names.sort();
                Ok(Value::List(Rc::new(RefCell::new(names.into_iter().map(Value::String).collect()))))
            }
            "current_dir" => {
                if !args.is_empty() { return Err(self.error("current_dir() expects no arguments", span)); }
                std::env::current_dir().map(|p| Value::String(p.to_string_lossy().into_owned()))
                    .map_err(|e| self.error(&format!("current_dir() failed: {e}"), span))
            }
            "absolute_path" => {
                if args.len() != 1 { return Err(self.error("absolute_path() expects one path string", span)); }
                let Value::String(path) = &args[0] else { return Err(self.error("absolute_path() path must be a string", span)); };
                let path = Path::new(path);
                let absolute = if path.is_absolute() { path.to_path_buf() } else {
                    std::env::current_dir().map_err(|e| self.error(&format!("absolute_path() failed: {e}"), span))?.join(path)
                };
                Ok(Value::String(absolute.to_string_lossy().into_owned()))
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
            Value::Module(_) | Value::FunctionRef(_, _) | Value::DeviceRoot | Value::DeviceModule(_) | Value::DeviceFunction(_, _) => true,
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

fn permission_name(state: PermissionState) -> &'static str {
    match state {
        PermissionState::NotRequired => "not_required",
        PermissionState::NotRequested => "not_requested",
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Restricted => "restricted",
        PermissionState::Unknown => "unknown",
    }
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
    fn supports_numeric_math_and_conversions() {
        assert_eq!(run("abs(-9)\n").unwrap(), Value::Integer(9));
        assert_eq!(run("sqrt(81)\n").unwrap(), Value::Float(9.0));
        assert_eq!(run("floor(4.8)\n").unwrap(), Value::Float(4.0));
        assert_eq!(run("ceil(4.2)\n").unwrap(), Value::Float(5.0));
        assert_eq!(run("round(4.6)\n").unwrap(), Value::Float(5.0));
        assert_eq!(run("pow(2, 10)\n").unwrap(), Value::Integer(1024));
        assert_eq!(run("pow(2, -2)\n").unwrap(), Value::Float(0.25));
        assert_eq!(run("int(\"42\")\n").unwrap(), Value::Integer(42));
        assert_eq!(run("float(\"2.5\")\n").unwrap(), Value::Float(2.5));
        assert_eq!(run("str(42)\n").unwrap(), Value::String("42".into()));
        assert_eq!(run("type([1])\n").unwrap(), Value::String("list".into()));
    }

    #[test]
    fn supports_min_max_sum_and_range() {
        assert_eq!(run("min(8, 2, 5)\n").unwrap(), Value::Integer(2));
        assert_eq!(run("max(8, 2, 5.5)\n").unwrap(), Value::Float(8.0));
        assert_eq!(run("sum([1, 2, 3, 4])\n").unwrap(), Value::Integer(10));
        assert_eq!(run("sum((1, 2.5, 3))\n").unwrap(), Value::Float(6.5));
        assert_eq!(run("mut total = 0\nfor n in range(1, 6):\n    total = total + n\ntotal\n").unwrap(), Value::Integer(15));
        assert_eq!(run("range(5, 0, -2)[2]\n").unwrap(), Value::Integer(1));
    }

    #[test]
    fn numeric_builtins_reject_invalid_inputs_and_overflow() {
        assert!(run("sqrt(-1)\n").unwrap_err().message.contains("negative"));
        assert!(run("abs(-9223372036854775807 - 1)\n").unwrap_err().message.contains("overflow"));
        assert!(run("pow(2, 100)\n").unwrap_err().message.contains("overflow"));
        assert!(run("int(\"hello\")\n").unwrap_err().message.contains("cannot convert"));
        assert!(run("range(1, 5, 0)\n").unwrap_err().message.contains("cannot be zero"));
        assert!(run("sum([1, \"bad\"])\n").unwrap_err().message.contains("must all be numbers"));
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
        assert_eq!(run("let name = \"Mo\\nsh\"\\nname + \"!\"\\n").unwrap(), Value::String("Mo\nsh!".into()));
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
        let source = r#"mut message = ""
try:
    let value = 10 / 0
catch error:
    message = error
message
"#;
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
    fn path_utilities_inspect_join_and_list_real_paths() {
        let root = std::env::temp_dir().join(format!("mosh-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested").join("b.txt"), "hello").unwrap();
        std::fs::write(root.join("nested").join("a.txt"), "mosh!").unwrap();
        let dir = root.join("nested").to_string_lossy().into_owned();
        let source = format!(
            "let dir = \"{}\"\nlet file = path_join(dir, \"a.txt\")\nlet entries = list_dir(dir)\nfile_size(file) + len(entries)\n",
            dir
        );
        assert_eq!(run(&source).unwrap(), Value::Integer(7));
        assert_eq!(run(&format!("is_file(\"{}\")\n", root.join("nested/a.txt").display())).unwrap(), Value::Boolean(true));
        assert_eq!(run(&format!("is_dir(\"{}\")\n", root.join("nested").display())).unwrap(), Value::Boolean(true));
        let listing = run(&format!("list_dir(\"{}\")\n", dir)).unwrap();
        assert_eq!(listing, Value::List(Rc::new(RefCell::new(vec![Value::String("a.txt".into()), Value::String("b.txt".into())]))));
        assert!(matches!(run("current_dir()\n").unwrap(), Value::String(_)));
        assert!(matches!(run(&format!("absolute_path(\"{}\")\n", dir)).unwrap(), Value::String(_)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn path_utilities_validate_arguments_and_report_file_errors() {
        assert!(run("path_join()\n").unwrap_err().message.contains("at least one"));
        assert!(run("path_join(\"a\", 1)\n").unwrap_err().message.contains("must be strings"));
        assert!(run("file_size(\"/mosh-missing-file\")\n").unwrap_err().message.contains("file_size() failed"));
        assert!(run("list_dir(\"/mosh-missing-dir\")\n").unwrap_err().message.contains("list_dir() failed"));
        assert!(run("current_dir(\"extra\")\n").unwrap_err().message.contains("no arguments"));
    }

    #[test]
    fn supports_sorted_reverse_and_dictionary_values() {
        assert_eq!(run("sorted([4, 1, 3, 2])\n").unwrap(),
            Value::List(Rc::new(RefCell::new(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3), Value::Integer(4)]))));
        assert_eq!(run("sorted([\"pear\", \"apple\", \"mango\"])\n").unwrap(),
            Value::List(Rc::new(RefCell::new(vec![Value::String("apple".into()), Value::String("mango".into()), Value::String("pear".into())]))));
        assert_eq!(run("reverse([1, 2, 3])[0]\n").unwrap(), Value::Integer(3));
        assert_eq!(run("reverse(\"mosh\")\n").unwrap(), Value::String("hsom".into()));
        assert_eq!(run("let data = {\"b\": 20, \"a\": 10}\nvalues(data)[0]\n").unwrap(), Value::Integer(10));
        assert!(run("sorted([1, \"two\"])\n").unwrap_err().message.contains("all be numbers or all be strings"));
        assert!(run("values([1, 2])\n").unwrap_err().message.contains("dictionary"));
    }

    #[test]
    fn exposes_device_namespace_without_claiming_unregistered_hardware() {
        assert_eq!(run("device.camera.is_supported()\n").unwrap(), Value::Boolean(false));
        assert_eq!(run("device.wifi_direct.is_available()\n").unwrap(), Value::Boolean(false));
        assert_eq!(run("device.camera.permission()\n").unwrap(), Value::String("unknown".into()));
        let status = run("device.camera.capabilities()\n").unwrap();
        let Value::Dict(status) = status else { panic!("expected capability dictionary"); };
        assert_eq!(status.borrow().get("supported"), Some(&Value::Boolean(false)));
        assert_eq!(status.borrow().get("available"), Some(&Value::Boolean(false)));
        assert!(matches!(status.borrow().get("limitations"), Some(Value::List(_))));
    }

    #[test]
    fn device_namespace_rejects_unknown_methods_and_arguments() {
        assert!(run("device.camera.capture()\n").unwrap_err().message.contains("no method"));
        assert!(run("device.camera.is_supported(1)\n").unwrap_err().message.contains("expects no arguments"));
    }

    #[test]
    fn device_permission_request_is_an_explicit_unsupported_error_without_adapter() {
        let source = r#"mut message = ""
try:
    device.camera.request_permission()
catch error:
    message = error
message
"#;
        let result = run(source).unwrap();
        assert!(matches!(result, Value::String(message) if message.contains("Unsupported")));
    }

    #[test]
    fn battery_read_uses_injected_sysfs_backend() {
        let root = std::env::temp_dir().join(format!("mosh-interpreter-battery-{}", std::process::id()));
        let battery = root.join("BAT0");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&battery).unwrap();
        fs::write(battery.join("type"), "Battery\n").unwrap();
        fs::write(battery.join("present"), "1\n").unwrap();
        fs::write(battery.join("capacity"), "64\n").unwrap();
        fs::write(battery.join("status"), "Charging\n").unwrap();

        let backend = std::sync::Arc::new(crate::device::SysfsBackend::with_root(&root, crate::device::Platform::Linux));
        let mut interpreter = Interpreter::with_device_backend(backend);
        let source = "device.battery.read()\n";
        let program = parse(&tokenize(source).unwrap()).unwrap();
        let result = interpreter.execute(&program).unwrap();
        let Value::Dict(values) = result else { panic!("expected battery reading dictionary"); };
        assert_eq!(values.borrow().get("capacity_percent"), Some(&Value::Float(64.0)));
        assert_eq!(values.borrow().get("state"), Some(&Value::String("Charging".into())));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn memory_ram_alias_and_cpu_reads_use_injected_procfs_backend() {
        let root = std::env::temp_dir().join(format!("mosh-interpreter-proc-{}", std::process::id()));
        let power = root.join("power");
        let proc = root.join("proc");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&power).unwrap();
        fs::create_dir_all(&proc).unwrap();
        fs::write(proc.join("meminfo"), "MemTotal: 8192000 kB\nMemFree: 1024000 kB\nMemAvailable: 2048000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n").unwrap();
        fs::write(proc.join("cpuinfo"), "processor : 0\nmodel name : Mosh Test CPU\nprocessor : 1\n").unwrap();

        let backend = std::sync::Arc::new(crate::device::SysfsBackend::with_paths(
            &power, &proc, crate::device::Platform::Linux
        ));
        let mut interpreter = Interpreter::with_device_backend(backend);
        let source = r#"let memory = device.memory.read()
let ram = device.ram.read()
let cpu = device.cpu.read()
memory["total_bytes"] + ram["available_bytes"] + cpu["logical_cores"]
"#;
        let program = parse(&tokenize(source).unwrap()).unwrap();
        assert_eq!(interpreter.execute(&program).unwrap(), Value::Integer(10_485_760_002));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn storage_and_fs_alias_list_real_mount_inventory_from_backend() {
        let root = std::env::temp_dir().join(format!("mosh-interpreter-mounts-{}", std::process::id()));
        let power = root.join("power");
        let proc = root.join("proc");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&power).unwrap();
        fs::create_dir_all(&proc).unwrap();
        fs::write(proc.join("mounts"), "/dev/root / ext4 rw,relatime 0 0\n/dev/sda1 /mnt/data ext4 ro,nosuid 0 0\n").unwrap();
        let backend = std::sync::Arc::new(crate::device::SysfsBackend::with_paths(
            &power, &proc, crate::device::Platform::Linux
        ));
        let mut interpreter = Interpreter::with_device_backend(backend);
        let source = r#"let mounts = device.storage.volumes()
let alias_mounts = device.fs.volumes()
len(mounts) + len(alias_mounts)
"#;
        let program = parse(&tokenize(source).unwrap()).unwrap();
        assert_eq!(interpreter.execute(&program).unwrap(), Value::Integer(4));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn module_registry_lists_namespaces_and_reports_implementation_status() {
        let all = run("modules()\n").unwrap();
        let Value::List(all) = all else { panic!("expected namespace list"); };
        assert!(all.borrow().contains(&Value::String("ai".into())));
        assert!(all.borrow().contains(&Value::String("device".into())));

        let implemented = run("implemented_modules()\n").unwrap();
        let Value::List(implemented) = implemented else { panic!("expected implemented namespace list"); };
        assert!(implemented.borrow().contains(&Value::String("std".into())));
        assert!(implemented.borrow().contains(&Value::String("device".into())));
        assert!(!implemented.borrow().contains(&Value::String("ai".into())));

        let info = run("module_info(\"web\")\n").unwrap();
        let Value::Dict(info) = info else { panic!("expected module info dictionary"); };
        assert_eq!(info.borrow().get("implemented"), Some(&Value::Boolean(false)));
        assert_eq!(info.borrow().get("namespace"), Some(&Value::String("web".into())));
    }

    #[test]
    fn module_registry_validates_arguments_and_unknown_namespaces() {
        assert!(run("modules(1)\n").unwrap_err().message.contains("expects no arguments"));
        assert!(run("module_info()\n").unwrap_err().message.contains("exactly one"));
        assert!(run("module_info(1)\n").unwrap_err().message.contains("must be a string"));
        assert!(run("module_info(\"missing\")\n").unwrap_err().message.contains("unknown module namespace"));
    }

    #[test]
    fn reports_undefined_names() {
        assert!(run("unknown\n").unwrap_err().message.contains("undefined variable"));
    }
}
