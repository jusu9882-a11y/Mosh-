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

#[derive(Debug, Clone)]
struct LocalBinding {
    name: String,
    block: usize,
    instruction: Option<usize>,
    mutable: bool,
    ty: MirType,
    slot: String,
    parameter_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct ControlFlow {
    reachable: Vec<bool>,
    dominators: Vec<HashSet<usize>>,
}

fn compute_control_flow(function: &MirFunction) -> ControlFlow {
    let block_count = function.blocks.len();
    let mut predecessors = vec![Vec::<usize>::new(); block_count];
    for (source, block) in function.blocks.iter().enumerate() {
        let mut add_edge = |target: crate::mir::BlockId| {
            let target = target.0 as usize;
            if target < block_count && !predecessors[target].contains(&source) {
                predecessors[target].push(source);
            }
        };
        match block.terminator.as_ref() {
            Some(Terminator::Jump(target)) => add_edge(*target),
            Some(Terminator::Branch {
                then_block,
                else_block,
                ..
            }) => {
                add_edge(*then_block);
                add_edge(*else_block);
            }
            Some(Terminator::Return(_)) | None => {}
        }
    }

    let mut reachable = vec![false; block_count];
    if block_count > 0 {
        let mut queue = std::collections::VecDeque::from([0usize]);
        reachable[0] = true;
        while let Some(source) = queue.pop_front() {
            let targets: Vec<usize> = match function.blocks[source].terminator.as_ref() {
                Some(Terminator::Jump(target)) => vec![target.0 as usize],
                Some(Terminator::Branch {
                    then_block,
                    else_block,
                    ..
                }) => vec![then_block.0 as usize, else_block.0 as usize],
                Some(Terminator::Return(_)) | None => Vec::new(),
            };
            for target in targets {
                if target < block_count && !reachable[target] {
                    reachable[target] = true;
                    queue.push_back(target);
                }
            }
        }
    }

    let reachable_set: HashSet<usize> = reachable
        .iter()
        .enumerate()
        .filter_map(|(index, is_reachable)| is_reachable.then_some(index))
        .collect();
    let mut dominators = vec![HashSet::<usize>::new(); block_count];
    for index in 0..block_count {
        if reachable[index] {
            dominators[index] = if index == 0 {
                HashSet::from([0])
            } else {
                reachable_set.clone()
            };
        } else {
            dominators[index].insert(index);
        }
    }

    loop {
        let mut changed = false;
        for block in 1..block_count {
            if !reachable[block] {
                continue;
            }
            let incoming: Vec<usize> = predecessors[block]
                .iter()
                .copied()
                .filter(|predecessor| reachable[*predecessor])
                .collect();
            let mut next = if let Some(first) = incoming.first() {
                dominators[*first].clone()
            } else {
                HashSet::new()
            };
            for predecessor in incoming.iter().skip(1) {
                next.retain(|candidate| dominators[*predecessor].contains(candidate));
            }
            next.insert(block);
            if next != dominators[block] {
                dominators[block] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    ControlFlow {
        reachable,
        dominators,
    }
}

fn binding_is_visible(
    binding: &LocalBinding,
    use_block: usize,
    use_position: usize,
    flow: &ControlFlow,
) -> bool {
    if binding.parameter_index.is_some() {
        return true;
    }
    if binding.block == use_block {
        return binding
            .instruction
            .is_some_and(|position| position < use_position);
    }
    flow.reachable.get(use_block).copied().unwrap_or(false)
        && flow.reachable.get(binding.block).copied().unwrap_or(false)
        && flow.dominators[use_block].contains(&binding.block)
}

fn resolve_binding<'a>(
    name: &str,
    use_block: usize,
    use_position: usize,
    bindings: &'a [LocalBinding],
    flow: &ControlFlow,
) -> Option<&'a LocalBinding> {
    bindings
        .iter()
        .filter(|binding| {
            binding.name == name && binding_is_visible(binding, use_block, use_position, flow)
        })
        .max_by_key(|binding| {
            (
                flow.dominators.get(binding.block).map_or(0, HashSet::len),
                binding.instruction.unwrap_or(0),
            )
        })
}

fn infer_value_type(
    value: ValueId,
    definitions: &HashMap<ValueId, (usize, usize, &Instruction)>,
    bindings: &[LocalBinding],
    flow: &ControlFlow,
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
        .and_then(|(block, position, instruction)| match &instruction.kind {
            InstructionKind::Integer(_) => Some(MirType::Int),
            InstructionKind::Float(_) => Some(MirType::Float),
            InstructionKind::String(_) => Some(MirType::String),
            InstructionKind::Boolean(_) => Some(MirType::Bool),
            InstructionKind::Load { name } => {
                resolve_binding(name, *block, *position, bindings, flow).map(|binding| binding.ty)
            }
            InstructionKind::Unary { operator, operand } => {
                let operand_type = infer_value_type(
                    *operand,
                    definitions,
                    bindings,
                    flow,
                    signatures,
                    cache,
                    active,
                )?;
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
                let left = infer_value_type(
                    *left,
                    definitions,
                    bindings,
                    flow,
                    signatures,
                    cache,
                    active,
                )?;
                let right = infer_value_type(
                    *right,
                    definitions,
                    bindings,
                    flow,
                    signatures,
                    cache,
                    active,
                )?;
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

fn function_bindings(
    function: &MirFunction,
    signatures: &HashMap<String, (Vec<MirType>, MirType)>,
) -> Result<(Vec<LocalBinding>, ControlFlow)> {
    let flow = compute_control_flow(function);
    let mut bindings = Vec::new();
    for (index, parameter) in function.parameters.iter().enumerate() {
        if parameter.ty == MirType::Unit {
            return Err(vec![error(format!(
                "function '{}' cannot accept Unit parameters in the native backend",
                function.name
            ))]);
        }
        if bindings
            .iter()
            .any(|binding: &LocalBinding| binding.name == parameter.name)
        {
            return Err(vec![error(format!(
                "function '{}' has duplicate parameter name '{}'",
                function.name, parameter.name
            ))]);
        }
        bindings.push(LocalBinding {
            name: parameter.name.clone(),
            block: 0,
            instruction: None,
            mutable: false,
            ty: parameter.ty,
            slot: format!("param.{index}"),
            parameter_index: Some(index),
        });
    }

    let definitions: HashMap<ValueId, (usize, usize, &Instruction)> = function
        .blocks
        .iter()
        .enumerate()
        .flat_map(|(block_index, block)| {
            block.instructions.iter().enumerate().filter_map(
                move |(instruction_index, instruction)| {
                    instruction
                        .result
                        .map(|id| (id, (block_index, instruction_index, instruction)))
                },
            )
        })
        .collect();

    let mut errors = Vec::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if let InstructionKind::Declare {
                name,
                mutable,
                value,
            } = &instruction.kind
            {
                let ty = value.and_then(|value| {
                    infer_value_type(
                        value,
                        &definitions,
                        &bindings,
                        &flow,
                        signatures,
                        &mut HashMap::new(),
                        &mut HashSet::new(),
                    )
                });
                match ty {
                    Some(MirType::Unit) => errors.push(error(format!(
                        "local '{}' in function '{}' cannot have type Unit",
                        name, function.name
                    ))),
                    Some(ty) => bindings.push(LocalBinding {
                        name: name.clone(),
                        block: block_index,
                        instruction: Some(instruction_index),
                        mutable: *mutable,
                        ty,
                        slot: format!("local.{block_index}.{instruction_index}"),
                        parameter_index: None,
                    }),
                    None => errors.push(error(format!(
                        "cannot determine the type of local '{}' in function '{}'",
                        name, function.name
                    ))),
                }
            }
        }
    }
    if errors.is_empty() {
        Ok((bindings, flow))
    } else {
        Err(errors)
    }
}

struct FunctionEmitter<'a> {
    function: &'a MirFunction,
    bindings: Vec<LocalBinding>,
    flow: ControlFlow,
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
        let (bindings, flow) = function_bindings(function, signatures)?;
        let definitions: HashMap<ValueId, (usize, usize, &Instruction)> = function
            .blocks
            .iter()
            .enumerate()
            .flat_map(|(block_index, block)| {
                block.instructions.iter().enumerate().filter_map(
                    move |(instruction_index, instruction)| {
                        instruction
                            .result
                            .map(|id| (id, (block_index, instruction_index, instruction)))
                    },
                )
            })
            .collect();
        let mut value_types = HashMap::new();
        for value in definitions.keys().copied() {
            if let Some(ty) = infer_value_type(
                value,
                &definitions,
                &bindings,
                &flow,
                signatures,
                &mut HashMap::new(),
                &mut HashSet::new(),
            ) {
                value_types.insert(value, ty);
            }
        }
        Ok(Self {
            function,
            bindings,
            flow,
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

    fn emit_instruction(
        &mut self,
        instruction: &Instruction,
        block_index: usize,
        instruction_index: usize,
    ) {
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
                    let binding = self.bindings.iter().find(|binding| {
                        binding.name == *name
                            && binding.block == block_index
                            && binding.instruction == Some(instruction_index)
                    });
                    let Some(binding) = binding else {
                        self.errors
                            .push(error(format!("missing storage binding for local '{name}'")));
                        return;
                    };
                    match self.value(*value) {
                        Ok(operand) => {
                            let _ = writeln!(
                                self.output,
                                "  store {} {}, ptr %{}",
                                llvm_type(binding.ty),
                                operand,
                                binding.slot
                            );
                        }
                        Err(errors) => self.errors.extend(errors),
                    }
                }
            }
            InstructionKind::Load { name } => {
                let Some(binding) = resolve_binding(
                    name,
                    block_index,
                    instruction_index,
                    &self.bindings,
                    &self.flow,
                ) else {
                    self.errors.push(error(format!(
                        "unknown local '{name}' during LLVM generation"
                    )));
                    return;
                };
                if let Some(id) = instruction.result {
                    let _ = writeln!(
                        self.output,
                        "  %v{} = load {}, ptr %{}",
                        id.0,
                        llvm_type(binding.ty),
                        binding.slot
                    );
                    self.values.insert(id, format!("%v{}", id.0));
                }
            }
            InstructionKind::Store { name, value } => {
                let Some(binding) = resolve_binding(
                    name,
                    block_index,
                    instruction_index,
                    &self.bindings,
                    &self.flow,
                ) else {
                    self.errors.push(error(format!(
                        "unknown local '{name}' during LLVM generation"
                    )));
                    return;
                };
                if !binding.mutable {
                    self.errors.push(error(format!(
                        "cannot store to immutable local '{name}' during LLVM generation"
                    )));
                    return;
                }
                match self.value(*value) {
                    Ok(operand) => {
                        let _ = writeln!(
                            self.output,
                            "  store {} {}, ptr %{}",
                            llvm_type(binding.ty),
                            operand,
                            binding.slot
                        );
                    }
                    Err(errors) => self.errors.extend(errors),
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

        for (block_index, block) in self.function.blocks.iter().enumerate() {
            let _ = writeln!(self.output, "bb{}:", block.id.0);
            if block_index == 0 {
                for binding in &self.bindings {
                    let _ = writeln!(
                        self.output,
                        "  %{} = alloca {}",
                        binding.slot,
                        llvm_type(binding.ty)
                    );
                    if let Some(parameter_index) = binding.parameter_index {
                        let _ = writeln!(
                            self.output,
                            "  store {} %arg{parameter_index}, ptr %{}",
                            llvm_type(binding.ty),
                            binding.slot
                        );
                    }
                }
            }
            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                self.emit_instruction(instruction, block_index, instruction_index);
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

    let Some(main) = module
        .functions
        .iter()
        .find(|function| function.name == "main")
    else {
        return Err(vec![error(
            "native executable requires a function named 'main'",
        )]);
    };
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
    fn native_backend_preserves_shadowed_local_storage() {
        use std::process::Command;

        let ir = compile(
            "function main() -> Int:\n    let value = 1\n    if true:\n        mut value: String = \"inner\"\n        value = \"updated\"\n        print(value)\n    print(value)\n    return 0\n",
        )
        .unwrap();
        let base = std::env::temp_dir().join(format!("mosh-shadow-smoke-{}", std::process::id()));
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
            "clang rejected shadowing LLVM IR: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let executed = Command::new(&executable)
            .output()
            .expect("generated shadowing executable should run");
        let _ = std::fs::remove_file(&ir_path);
        let _ = std::fs::remove_file(&executable);
        assert!(executed.status.success());
        assert_eq!(String::from_utf8_lossy(&executed.stdout), "updated\n1\n");
    }

    #[test]
    fn native_executable_runs_mutable_while_loop() {
        use std::process::Command;

        let ir = compile(
            "function main() -> Int:\n    mut count: Int = 0\n    while count < 3:\n        count = count + 1\n    print(count)\n    return 0\n",
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
    fn rejects_unit_parameters_and_locals_before_emitting_invalid_llvm() {
        assert!(compile(
            "function take(value: Unit) -> Unit:\n    return\nfunction main() -> Unit:\n    return\n",
        )
        .is_err());
        assert!(
            compile("function main() -> Unit:\n    let result = print(1)\n    return\n",).is_err()
        );
    }

    #[test]
    fn rejects_main_with_an_unsupported_return_type() {
        let source = "function main() -> Bool:\n    return true\n";
        assert!(compile(source).is_err());
    }
}
