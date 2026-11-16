use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::lexer::TokenKind;
use crate::mir::{
    validate, Instruction, InstructionKind, MirFunction, MirModule, MirType, Terminator, ValueId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenError {}

type Result<T> = std::result::Result<T, Vec<CodegenError>>;

fn error(message: impl Into<String>) -> CodegenError {
    CodegenError {
        message: message.into(),
    }
}

fn llvm_type(ty: MirType) -> &'static str {
    match ty {
        MirType::Int => "i64",
        MirType::Float => "double",
        MirType::Bool => "i1",
        MirType::String => "ptr",
        MirType::Unit => "void",
    }
}

fn escaped_bytes(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes().chain(std::iter::once(0)) {
        if (32..=126).contains(&byte) && byte != b'"' && byte != b'\\' {
            output.push(byte as char);
        } else {
            let _ = write!(output, "\\{:02X}", byte);
        }
    }
    output
}

fn string_constant(value: &str, pool: &mut HashMap<String, usize>) -> String {
    let next = pool.len();
    let index = *pool.entry(value.to_owned()).or_insert(next);
    format!(
        "getelementptr inbounds ([{} x i8], ptr @.mosh.str.{index}, i64 0, i64 0)",
        value.len() + 1
    )
}

fn function_symbol(name: &str) -> String {
    if name == "main" {
        "@mosh_user_main".into()
    } else {
        let mut symbol = String::from("@mosh_");
        for character in name.chars() {
            if character.is_ascii_alphanumeric() || character == '_' {
                symbol.push(character);
            } else {
                let _ = write!(symbol, "_{:x}", character as u32);
            }
        }
        symbol
    }
}

fn collect_signatures(module: &MirModule) -> HashMap<String, (Vec<MirType>, MirType)> {
    module
        .functions
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                (
                    function
                        .parameters
                        .iter()
                        .map(|parameter| parameter.ty)
                        .collect(),
                    function.return_type,
                ),
            )
        })
        .collect()
}

fn infer_value_type(
    value: ValueId,
    definitions: &HashMap<ValueId, &Instruction>,
    locals: &HashMap<String, MirType>,
    signatures: &HashMap<String, (Vec<MirType>, MirType)>,
    cache: &mut HashMap<ValueId, Option<MirType>>,
    active: &mut HashSet<ValueId>,
) -> Option<MirType> {
    if let Some(ty) = cache.get(&value) {
        return *ty;
    }
    if !active.insert(value) {
        return None;
    }
    let inferred = definitions
        .get(&value)
        .and_then(|instruction| match &instruction.kind {
            InstructionKind::Integer(_) => Some(MirType::Int),
            InstructionKind::Float(_) => Some(MirType::Float),
            InstructionKind::String(_) => Some(MirType::String),
            InstructionKind::Boolean(_) => Some(MirType::Bool),
            InstructionKind::Load { name } => locals.get(name).copied(),
            InstructionKind::Unary { operator, operand } => {
                let operand_type =
                    infer_value_type(*operand, definitions, locals, signatures, cache, active)?;
                match operator {
                    TokenKind::Minus if matches!(operand_type, MirType::Int | MirType::Float) => {
                        Some(operand_type)
                    }
                    TokenKind::Bang if operand_type == MirType::Bool => Some(MirType::Bool),
                    _ => None,
                }
            }
            InstructionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left = infer_value_type(*left, definitions, locals, signatures, cache, active)?;
                let right =
                    infer_value_type(*right, definitions, locals, signatures, cache, active)?;
                match operator {
                    TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Percent
                        if left == right && matches!(left, MirType::Int | MirType::Float) =>
                    {
                        Some(left)
                    }
                    TokenKind::Less
                    | TokenKind::LessEqual
                    | TokenKind::Greater
                    | TokenKind::GreaterEqual
                        if left == right && matches!(left, MirType::Int | MirType::Float) =>
                    {
                        Some(MirType::Bool)
                    }
                    TokenKind::Equal | TokenKind::NotEqual
                        if left == right && left != MirType::Unit =>
                    {
                        Some(MirType::Bool)
                    }
                    TokenKind::Ampersand | TokenKind::Pipe | TokenKind::Caret
                        if left == MirType::Int && right == MirType::Int =>
                    {
                        Some(MirType::Int)
                    }
                    _ => None,
                }
            }
            InstructionKind::Call { name, .. } if name == "print" => Some(MirType::Unit),
            InstructionKind::Call { name, .. } => signatures.get(name).map(|(_, result)| *result),
            InstructionKind::Declare { .. } | InstructionKind::Store { .. } => None,
        });
    active.remove(&value);
    cache.insert(value, inferred);
    inferred
}

fn function_locals(
    function: &MirFunction,
    signatures: &HashMap<String, (Vec<MirType>, MirType)>,
) -> Result<HashMap<String, MirType>> {
    let mut locals = HashMap::new();
    for parameter in &function.parameters {
        if locals
            .insert(parameter.name.clone(), parameter.ty)
            .is_some()
        {
            return Err(vec![error(format!(
                "function '{}' has duplicate local name '{}'",
                function.name, parameter.name
            ))]);
        }
    }

    let definitions: HashMap<ValueId, &Instruction> = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| instruction.result.map(|id| (id, instruction)))
        .collect();

    let mut errors = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let InstructionKind::Declare { name, value, .. } = &instruction.kind {
                let ty = value.and_then(|value| {
                    infer_value_type(
                        value,
                        &definitions,
                        &locals,
                        signatures,
                        &mut HashMap::new(),
                        &mut HashSet::new(),
                    )
                });
                if let Some(ty) = ty {
                    if locals.insert(name.clone(), ty).is_some() {
                        errors.push(error(format!(
                            "function '{}' declares local '{}' more than once; this backend version does not yet support shadowed local storage",
                            function.name, name
                        )));
                    }
                } else {
                    errors.push(error(format!(
                        "cannot determine the type of local '{}' in function '{}'",
                        name, function.name
                    )));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(locals)
    } else {
        Err(errors)
    }
}

struct FunctionEmitter<'a> {
    function: &'a MirFunction,
    locals: HashMap<String, MirType>,
    signatures: &'a HashMap<String, (Vec<MirType>, MirType)>,
    values: HashMap<ValueId, String>,
    value_types: HashMap<ValueId, MirType>,
    strings: &'a mut HashMap<String, usize>,
    output: String,
    errors: Vec<CodegenError>,
}

impl<'a> FunctionEmitter<'a> {
    fn new(
        function: &'a MirFunction,
        signatures: &'a HashMap<String, (Vec<MirType>, MirType)>,
        strings: &'a mut HashMap<String, usize>,
    ) -> Result<Self> {
        let locals = function_locals(function, signatures)?;
        let definitions: HashMap<ValueId, &Instruction> = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instruction| instruction.result.map(|id| (id, instruction)))
            .collect();
        let mut value_types = HashMap::new();
        for value in definitions.keys().copied() {
            if let Some(ty) = infer_value_type(
                value,
                &definitions,
                &locals,
                signatures,
                &mut HashMap::new(),
                &mut HashSet::new(),
            ) {
                value_types.insert(value, ty);
            }
        }
        Ok(Self {
            function,
            locals,
            signatures,
            values: HashMap::new(),
            value_types,
            strings,
            output: String::new(),
            errors: Vec::new(),
        })
    }

    fn value(&self, value: ValueId) -> Result<String> {
        self.values.get(&value).cloned().ok_or_else(|| {
            vec![error(format!(
                "MIR value {} has no LLVM representation in function '{}'",
                value.0, self.function.name
            ))]
        })
    }

    fn emit_instruction(&mut self, instruction: &Instruction) {
        match &instruction.kind {
            InstructionKind::Integer(text) => match text.replace('_', "").parse::<i64>() {
                Ok(value) => {
                    if let Some(id) = instruction.result {
                        self.values.insert(id, value.to_string());
                    }
                }
                Err(_) => self.errors.push(error(format!(
                    "integer literal '{text}' is outside the supported Int range"
                ))),
            },
            InstructionKind::Float(text) => match text.replace('_', "").parse::<f64>() {
                Ok(value) if value.is_finite() => {
                    if let Some(id) = instruction.result {
                        self.values.insert(id, format!("{value:.17e}"));
                    }
                }
                _ => self.errors.push(error(format!(
                    "invalid or non-finite Float literal '{text}'"
                ))),
            },
            InstructionKind::Boolean(value) => {
                if let Some(id) = instruction.result {
                    self.values
                        .insert(id, if *value { "true" } else { "false" }.into());
                }
            }
            InstructionKind::String(value) => {
                if value.contains('\0') {
                    self.errors.push(error("string literals containing NUL bytes are not supported by the native backend"));
                    return;
                }
                let operand = string_constant(value, self.strings);
                if let Some(id) = instruction.result {
                    self.values.insert(id, operand);
                }
            }
            InstructionKind::Declare { name, value, .. } => {
                if let Some(value) = value {
                    match self.value(*value) {
                        Ok(operand) => {
                            let ty = self.locals[name];
                            let _ = writeln!(
                                self.output,
                                "  store {} {}, ptr %local.{}",
                                llvm_type(ty),
                                operand,
                                local_slot(name)
                            );
                        }
                        Err(errors) => self.errors.extend(errors),
                    }
                }
            }
            InstructionKind::Load { name } => {
                let Some(ty) = self.locals.get(name).copied() else {
                    self.errors.push(error(format!(
                        "unknown local '{name}' during LLVM generation"
                    )));
                    return;
                };
                if let Some(id) = instruction.result {
                    let _ = writeln!(
                        self.output,
                        "  %v{} = load {}, ptr %local.{}",
                        id.0,
                        llvm_type(ty),
                        local_slot(name)
                    );
                    self.values.insert(id, format!("%v{}", id.0));
                }
            }
            InstructionKind::Store { name, value } => {
                match (self.locals.get(name).copied(), self.value(*value)) {
                    (Some(ty), Ok(operand)) => {
                        let _ = writeln!(
                            self.output,
                            "  store {} {}, ptr %local.{}",
                            llvm_type(ty),
                            operand,
                            local_slot(name)
                        );
                    }
                    (None, _) => self.errors.push(error(format!(
                        "unknown local '{name}' during LLVM generation"
                    ))),
                    (_, Err(errors)) => self.errors.extend(errors),
                }
            }
            InstructionKind::Unary { operator, operand } => {
                let Some(id) = instruction.result else { return };
                let Some(ty) = self.value_types.get(&id).copied() else {
                    self.errors
                        .push(error("cannot infer unary expression type"));
                    return;
                };
                match self.value(*operand) {
                    Ok(value) => {
                        let op = match (operator, ty) {
                            (TokenKind::Minus, MirType::Int) => format!("sub i64 0, {value}"),
                            (TokenKind::Minus, MirType::Float) => format!("fneg double {value}"),
                            (TokenKind::Bang, MirType::Bool) => format!("xor i1 {value}, true"),
                            _ => {
                                self.errors
                                    .push(error("unsupported unary operator/type in LLVM backend"));
                                return;
                            }
                        };
                        let _ = writeln!(self.output, "  %v{} = {op}", id.0);
                        self.values.insert(id, format!("%v{}", id.0));
                    }
                    Err(errors) => self.errors.extend(errors),
                }
            }
            InstructionKind::Binary {
                left,
                operator,
                right,
            } => {
                let Some(id) = instruction.result else { return };
                let Some(ty) = self.value_types.get(&id).copied() else {
                    self.errors
                        .push(error("cannot infer binary expression type"));
                    return;
                };
                let left_type = self.value_types.get(left).copied();
                let operands = self
                    .value(*left)
                    .and_then(|left| self.value(*right).map(|right| (left, right)));
                match operands {
                    Ok((left, right)) => {
                        if left_type == Some(MirType::String)
                            && matches!(operator, TokenKind::Equal | TokenKind::NotEqual)
                        {
                            let _ = writeln!(
                                self.output,
                                "  %str.cmp.{} = call i32 @strcmp(ptr {}, ptr {})",
                                id.0, left, right
                            );
                            let predicate = if *operator == TokenKind::Equal {
                                "eq"
                            } else {
                                "ne"
                            };
                            let _ = writeln!(
                                self.output,
                                "  %v{} = icmp {} i32 %str.cmp.{}, 0",
                                id.0, predicate, id.0
                            );
                            self.values.insert(id, format!("%v{}", id.0));
                            return;
                        }
                        let op = match (operator, ty) {
                            (TokenKind::Plus, MirType::Int) => format!("add i64 {left}, {right}"),
                            (TokenKind::Minus, MirType::Int) => format!("sub i64 {left}, {right}"),
                            (TokenKind::Star, MirType::Int) => format!("mul i64 {left}, {right}"),
                            (TokenKind::Slash, MirType::Int) => format!("sdiv i64 {left}, {right}"),
                            (TokenKind::Percent, MirType::Int) => {
                                format!("srem i64 {left}, {right}")
                            }
                            (TokenKind::Plus, MirType::Float) => {
                                format!("fadd double {left}, {right}")
                            }
                            (TokenKind::Minus, MirType::Float) => {
                                format!("fsub double {left}, {right}")
                            }
                            (TokenKind::Star, MirType::Float) => {
                                format!("fmul double {left}, {right}")
                            }
                            (TokenKind::Slash, MirType::Float) => {
                                format!("fdiv double {left}, {right}")
                            }
                            (TokenKind::Percent, MirType::Float) => {
                                format!("frem double {left}, {right}")
                            }
                            (TokenKind::Ampersand, MirType::Int) => {
                                format!("and i64 {left}, {right}")
                            }
                            (TokenKind::Pipe, MirType::Int) => format!("or i64 {left}, {right}"),
                            (TokenKind::Caret, MirType::Int) => format!("xor i64 {left}, {right}"),
                            (TokenKind::Equal, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp eq i64 {left}, {right}"),
                                Some(MirType::Bool) => format!("icmp eq i1 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp oeq double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("string and Unit equality are not supported by this LLVM backend"));
                                    return;
                                }
                            },
                            (TokenKind::NotEqual, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp ne i64 {left}, {right}"),
                                Some(MirType::Bool) => format!("icmp ne i1 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp one double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("string and Unit inequality are not supported by this LLVM backend"));
                                    return;
                                }
                            },
                            (TokenKind::Less, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp slt i64 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp olt double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("invalid comparison operand type"));
                                    return;
                                }
                            },
                            (TokenKind::LessEqual, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp sle i64 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp ole double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("invalid comparison operand type"));
                                    return;
                                }
                            },
                            (TokenKind::Greater, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp sgt i64 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp ogt double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("invalid comparison operand type"));
                                    return;
                                }
                            },
                            (TokenKind::GreaterEqual, MirType::Bool) => match left_type {
                                Some(MirType::Int) => format!("icmp sge i64 {left}, {right}"),
                                Some(MirType::Float) => format!("fcmp oge double {left}, {right}"),
                                _ => {
                                    self.errors.push(error("invalid comparison operand type"));
                                    return;
                                }
                            },
                            _ => {
                                self.errors.push(error(format!(
                                    "unsupported binary operation {operator:?} in LLVM backend"
                                )));
                                return;
                            }
                        };
                        let _ = writeln!(self.output, "  %v{} = {op}", id.0);
                        self.values.insert(id, format!("%v{}", id.0));
                    }
                    Err(errors) => self.errors.extend(errors),
                }
            }
            InstructionKind::Call { name, arguments } if name == "print" => {
                let mut format_text = String::new();
                let mut rendered_arguments = Vec::new();
                for argument in arguments {
                    let Some(ty) = self.value_types.get(argument).copied() else {
                        self.errors
                            .push(error("cannot determine print argument type"));
                        return;
                    };
                    let value = match self.value(*argument) {
                        Ok(value) => value,
                        Err(errors) => {
                            self.errors.extend(errors);
                            return;
                        }
                    };
                    if !format_text.is_empty() {
                        format_text.push(' ');
                    }
                    match ty {
                        MirType::String => {
                            format_text.push_str("%s");
                            rendered_arguments.push(format!("ptr {value}"));
                        }
                        MirType::Int => {
                            format_text.push_str("%lld");
                            rendered_arguments.push(format!("i64 {value}"));
                        }
                        MirType::Float => {
                            format_text.push_str("%g");
                            rendered_arguments.push(format!("double {value}"));
                        }
                        MirType::Bool => {
                            format_text.push_str("%d");
                            let call_id = instruction.result.map_or(0, |id| id.0);
                            let _ = writeln!(
                                self.output,
                                "  %print.bool.{call_id}.{} = zext i1 {value} to i32",
                                argument.0
                            );
                            rendered_arguments
                                .push(format!("i32 %print.bool.{call_id}.{}", argument.0));
                        }
                        MirType::Unit => {
                            self.errors.push(error("cannot print a Unit value"));
                            return;
                        }
                    }
                }
                format_text.push('\n');
                let format = string_constant(&format_text, self.strings);
                let arguments = if rendered_arguments.is_empty() {
                    String::new()
                } else {
                    format!(", {}", rendered_arguments.join(", "))
                };
                let _ = writeln!(
                    self.output,
                    "  call i32 (ptr, ...) @printf(ptr {format}{arguments})"
                );
            }
            InstructionKind::Call { name, arguments } => {
                let Some((parameter_types, result_type)) = self.signatures.get(name) else {
                    self.errors.push(error(format!(
                        "unknown function '{name}' during LLVM generation"
                    )));
                    return;
                };
                if arguments.len() != parameter_types.len() {
                    self.errors
                        .push(error(format!("wrong argument count for function '{name}'")));
                    return;
                }
                let mut rendered = Vec::new();
                for (argument, ty) in arguments.iter().zip(parameter_types) {
                    match self.value(*argument) {
                        Ok(value) => rendered.push(format!("{} {value}", llvm_type(*ty))),
                        Err(errors) => self.errors.extend(errors),
                    }
                }
                let call = format!(
                    "call {} {}({})",
                    llvm_type(*result_type),
                    function_symbol(name),
                    rendered.join(", ")
                );
                if *result_type == MirType::Unit {
                    let _ = writeln!(self.output, "  {call}");
                } else if let Some(id) = instruction.result {
                    let _ = writeln!(self.output, "  %v{} = {call}", id.0);
                    self.values.insert(id, format!("%v{}", id.0));
                }
            }
        }
    }

    fn emit_terminator(&mut self, terminator: &Terminator) {
        match terminator {
            Terminator::Jump(target) => {
                let _ = writeln!(self.output, "  br label %bb{}", target.0);
            }
            Terminator::Branch {
                condition,
                then_block,
                else_block,
            } => match self.value(*condition) {
                Ok(value) => {
                    let _ = writeln!(
                        self.output,
                        "  br i1 {value}, label %bb{}, label %bb{}",
                        then_block.0, else_block.0
                    );
                }
                Err(errors) => self.errors.extend(errors),
            },
            Terminator::Return(value) => match value {
                Some(value) => match self.value(*value) {
                    Ok(value) => {
                        let _ = writeln!(
                            self.output,
                            "  ret {} {value}",
                            llvm_type(self.function.return_type)
                        );
                    }
                    Err(errors) => self.errors.extend(errors),
                },
                None => self.output.push_str("  ret void\n"),
            },
        }
    }

    fn emit(mut self) -> Result<String> {
        let symbol = function_symbol(&self.function.name);
        let return_type = llvm_type(self.function.return_type);
        let parameters = self
            .function
            .parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| format!("{} %arg{index}", llvm_type(parameter.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            self.output,
            "define {return_type} {symbol}({parameters}) {{"
        );

        for block in &self.function.blocks {
            let _ = writeln!(self.output, "bb{}:", block.id.0);
            if block.id.0 == 0 {
                for (index, parameter) in self.function.parameters.iter().enumerate() {
                    let _ = writeln!(
                        self.output,
                        "  %local.{} = alloca {}",
                        local_slot(&parameter.name),
                        llvm_type(parameter.ty)
                    );
                    let _ = writeln!(
                        self.output,
                        "  store {} %arg{index}, ptr %local.{}",
                        llvm_type(parameter.ty),
                        local_slot(&parameter.name)
                    );
                }
                let mut emitted = HashSet::new();
                for instruction in self
                    .function
                    .blocks
                    .iter()
                    .flat_map(|b| b.instructions.iter())
                {
                    if let InstructionKind::Declare { name, .. } = &instruction.kind {
                        if emitted.insert(name.clone()) {
                            let ty = self.locals[name];
                            let _ = writeln!(
                                self.output,
                                "  %local.{} = alloca {}",
                                local_slot(name),
                                llvm_type(ty)
                            );
                        }
                    }
                }
            }
            for instruction in &block.instructions {
                self.emit_instruction(instruction);
            }
            if let Some(terminator) = &block.terminator {
                self.emit_terminator(terminator);
            } else {
                self.errors.push(error(format!(
                    "block {} in '{}' has no terminator",
                    block.id.0, self.function.name
                )));
            }
        }
        self.output.push_str("}\n");
        if self.errors.is_empty() {
            Ok(self.output)
        } else {
            Err(self.errors)
        }
    }
}

fn local_slot(name: &str) -> String {
    let mut output = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            output.push(character);
        } else {
            let _ = write!(output, "_{:x}", character as u32);
        }
    }
    output
}

/// Generate textual LLVM IR for the currently supported primitive MIR subset.
pub fn generate(module: &MirModule) -> Result<String> {
    if let Err(errors) = validate(module) {
        return Err(errors
            .into_iter()
            .map(|issue| error(issue.message))
            .collect());
    }
    let signatures = collect_signatures(module);
    let mut strings = HashMap::<String, usize>::new();
    let mut bodies = String::new();
    let mut errors = Vec::new();

    for function in &module.functions {
        match FunctionEmitter::new(function, &signatures, &mut strings)
            .and_then(FunctionEmitter::emit)
        {
            Ok(body) => bodies.push_str(&body),
            Err(mut found) => errors.append(&mut found),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    if !module
        .functions
        .iter()
        .any(|function| function.name == "main")
    {
        return Err(vec![error(
            "native executable requires a function named 'main'",
        )]);
    }
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    if !main.parameters.is_empty() {
        return Err(vec![error("main must not declare parameters")]);
    }
    if !matches!(main.return_type, MirType::Int | MirType::Unit) {
        return Err(vec![error("main must return Int or Unit")]);
    }
    if module
        .functions
        .iter()
        .filter(|function| function.name == "__mosh_init")
        .count()
        > 1
    {
        return Err(vec![error(
            "the function name '__mosh_init' is reserved for module initialization",
        )]);
    }

    let mut output =
        String::from("declare i32 @printf(ptr, ...)\ndeclare i32 @strcmp(ptr, ptr)\n\n");
    for (value, index) in &strings {
        let _ = writeln!(
            output,
            "@.mosh.str.{index} = private unnamed_addr constant [{} x i8] c\"{}\", align 1",
            value.len() + 1,
            escaped_bytes(value)
        );
    }
    output.push('\n');
    output.push_str(&bodies);
    output.push_str("define i32 @main() {\nentry:\n");
    if module
        .functions
        .iter()
        .any(|function| function.name == "__mosh_init")
    {
        output.push_str("  call void @mosh___mosh_init()\n");
    }
    if main.return_type == MirType::Int {
        output.push_str("  %mosh.result = call i64 @mosh_user_main()\n  %mosh.exit = trunc i64 %mosh.result to i32\n  ret i32 %mosh.exit\n");
    } else {
        output.push_str("  call void @mosh_user_main()\n  ret i32 0\n");
    }
    output.push_str("}\n");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::{lexer, mir, parser, semantic, typecheck};

    fn compile(source: &str) -> Result<String, Vec<super::CodegenError>> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        semantic::analyze(&program).unwrap();
        typecheck::check(&program).unwrap();
        let module = mir::lower(&program).unwrap();
        generate(&module)
    }

    #[test]
    fn emits_native_llvm_for_arithmetic_main() {
        let ir = compile("function main() -> Int:\n    return 40 + 2\n").unwrap();
        assert!(ir.contains("define i64 @mosh_user_main()"));
        assert!(ir.contains("add i64 40, 2"));
        assert!(ir.contains("trunc i64 %mosh.result to i32"));
    }

    #[test]
    fn emits_calls_and_prints_primitive_values() {
        let ir = compile(
            "function add(a: Int, b: Int) -> Int:\n    return a + b\nfunction main() -> Unit:\n    let answer = add(20, 22)\n    print(answer)\n",
        )
        .unwrap();
        assert!(ir.contains("define i64 @mosh_add(i64 %arg0, i64 %arg1)"));
        assert!(ir.contains("call i64 @mosh_add(i64 20, i64 22)"));
        assert!(ir.contains("@printf"));
    }

    #[test]
    fn print_accepts_multiple_primitive_arguments() {
        let ir =
            compile("function main() -> Unit:\n    print(\"value\", 42, true, 3.5)\n").unwrap();
        assert!(ir.contains("%s %lld %d %g"));
        assert!(ir.contains("i64 42"));
        assert!(ir.contains("double 3.50000000000000000e0"));
    }

    #[test]
    fn emits_control_flow_blocks() {
        let ir = compile(
            "function main() -> Int:\n    if true:\n        return 1\n    else:\n        return 0\n",
        )
        .unwrap();
        assert!(ir.contains("br i1 true, label %bb"));
        assert!(ir.contains("ret i64 1"));
    }

    #[test]
    fn emits_string_comparisons_and_native_string_output() {
        let ir = compile(
            "function main() -> Unit:\n    let greeting = \"hello\"\n    if greeting == \"hello\":\n        print(greeting)\n",
        )
        .unwrap();
        assert!(ir.contains("@strcmp"));
        assert!(ir.contains("@printf"));
        assert!(ir.contains("icmp eq i32"));
    }

    #[test]
    fn generated_llvm_is_accepted_by_clang() {
        use std::process::Command;

        let ir = compile(
            "function add(a: Int, b: Int) -> Int:\n    return a + b\nfunction main() -> Int:\n    return add(20, 22)\n",
        )
        .unwrap();
        let base = std::env::temp_dir().join(format!("mosh-llvm-smoke-{}", std::process::id()));
        let ir_path = base.with_extension("ll");
        let object_path = base.with_extension("o");
        std::fs::write(&ir_path, ir).unwrap();
        let result = Command::new("clang")
            .arg("-x")
            .arg("ir")
            .arg("-c")
            .arg(&ir_path)
            .arg("-o")
            .arg(&object_path)
            .output()
            .expect("clang must be installed for the LLVM backend test");
        let _ = std::fs::remove_file(&ir_path);
        let _ = std::fs::remove_file(&object_path);
        assert!(
            result.status.success(),
            "clang rejected generated LLVM IR: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn native_executable_runs_compiled_mosh_program() {
        use std::process::Command;

        let ir = compile(
            "function add(a: Int, b: Int) -> Int:\n    return a + b\nfunction main() -> Int:\n    let answer = add(20, 22)\n    print(\"answer\", answer)\n    return 0\n",
        )
        .unwrap();
        let base = std::env::temp_dir().join(format!("mosh-native-smoke-{}", std::process::id()));
        let ir_path = base.with_extension("ll");
        let executable = base.with_extension("bin");
        std::fs::write(&ir_path, ir).unwrap();
        let compiled = Command::new("clang")
            .arg("-x")
            .arg("ir")
            .arg("-O0")
            .arg(&ir_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("clang must be installed for the native backend test");
        assert!(
            compiled.status.success(),
            "clang rejected generated LLVM IR: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let executed = Command::new(&executable)
            .output()
            .expect("generated native executable should run");
        let _ = std::fs::remove_file(&ir_path);
        let _ = std::fs::remove_file(&executable);
        assert!(executed.status.success());
        assert_eq!(String::from_utf8_lossy(&executed.stdout), "answer 42\n");
    }

    #[test]
    fn native_executable_runs_mutable_while_loop() {
        use std::process::Command;

        let ir = compile(
            "function main() -> Int:\n    let mut count: Int = 0\n    while count < 3:\n        count = count + 1\n    print(count)\n    return 0\n",
        )
        .unwrap();
        let base = std::env::temp_dir().join(format!("mosh-loop-smoke-{}", std::process::id()));
        let ir_path = base.with_extension("ll");
        let executable = base.with_extension("bin");
        std::fs::write(&ir_path, ir).unwrap();
        let compiled = Command::new("clang")
            .arg("-x")
            .arg("ir")
            .arg("-O0")
            .arg(&ir_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("clang must be installed for the native backend test");
        assert!(
            compiled.status.success(),
            "clang rejected generated loop LLVM IR: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let executed = Command::new(&executable)
            .output()
            .expect("generated loop executable should run");
        let _ = std::fs::remove_file(&ir_path);
        let _ = std::fs::remove_file(&executable);
        assert!(executed.status.success());
        assert_eq!(String::from_utf8_lossy(&executed.stdout), "3\n");
    }

    #[test]
    fn rejects_main_with_an_unsupported_return_type() {
        let source = "function main() -> Bool:\n    return true\n";
        assert!(compile(source).is_err());
    }
}
