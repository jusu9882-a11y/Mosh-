use std::fmt;

use crate::lexer::{Span, TokenKind};
use crate::parser::{Expr, Program, Stmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirType {
    Int,
    Float,
    Bool,
    String,
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum InstructionKind {
    Integer(String),
    Float(String),
    String(String),
    Boolean(bool),
    Load {
        name: String,
    },
    Declare {
        name: String,
        mutable: bool,
        value: Option<ValueId>,
    },
    Store {
        name: String,
        value: ValueId,
    },
    Unary {
        operator: TokenKind,
        operand: ValueId,
    },
    Binary {
        left: ValueId,
        operator: TokenKind,
        right: ValueId,
    },
    Call {
        name: String,
        arguments: Vec<ValueId>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub result: Option<ValueId>,
    pub kind: InstructionKind,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    Jump(BlockId),
    Branch {
        condition: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Return(Option<ValueId>),
}
#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MirParameter {
    pub name: String,
    pub ty: MirType,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub name: String,
    pub parameters: Vec<MirParameter>,
    pub return_type: MirType,
    pub blocks: Vec<BasicBlock>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub message: String,
    pub span: Span,
}
impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}:{}",
            self.message, self.span.line, self.span.column
        )
    }
}
impl std::error::Error for LowerError {}

fn validation_span() -> Span {
    Span {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
    }
}

/// Validate the structural invariants required before MIR can be consumed.
pub fn validate(module: &MirModule) -> Result<(), Vec<LowerError>> {
    let mut errors = Vec::new();
    for function in &module.functions {
        if function.blocks.is_empty() {
            errors.push(LowerError {
                message: format!("MIR function '{}' has no entry block", function.name),
                span: validation_span(),
            });
            continue;
        }

        let mut definitions = std::collections::HashSet::new();
        let mut uses = Vec::new();
        for (index, block) in function.blocks.iter().enumerate() {
            if block.id.0 as usize != index {
                errors.push(LowerError {
                    message: format!(
                        "MIR function '{}' has invalid block ID {} at position {}",
                        function.name, block.id.0, index
                    ),
                    span: block
                    .instructions
                    .first()
                    .map(|i| i.span)
                    .unwrap_or_else(validation_span),
                });
            }
            if block.terminator.is_none() {
                errors.push(LowerError {
                    message: format!(
                        "MIR function '{}' block {} has no terminator",
                        function.name, block.id.0
                    ),
                    span: block
                        .instructions
                        .last()
                        .map(|i| i.span)
                        .unwrap_or_else(validation_span),
                });
            }

            for instruction in &block.instructions {
                let expects_result = !matches!(
                    &instruction.kind,
                    InstructionKind::Declare { .. } | InstructionKind::Store { .. }
                );
                if instruction.result.is_some() != expects_result {
                    errors.push(LowerError {
                        message: format!(
                            "MIR function '{}' block {} has an invalid result slot for an instruction",
                            function.name, block.id.0
                        ),
                        span: instruction.span,
                    });
                }
                if let Some(result) = instruction.result {
                    if !definitions.insert(result) {
                        errors.push(LowerError {
                            message: format!(
                                "MIR function '{}' defines value {} more than once",
                                function.name, result.0
                            ),
                            span: instruction.span,
                        });
                    }
                }
                match &instruction.kind {
                    InstructionKind::Declare { value, .. } => {
                        uses.extend(value.iter().copied().map(|v| (v, instruction.span)));
                    }
                    InstructionKind::Store { value, .. } => uses.push((*value, instruction.span)),
                    InstructionKind::Unary { operand, .. } => {
                        uses.push((*operand, instruction.span))
                    }
                    InstructionKind::Binary { left, right, .. } => {
                        uses.push((*left, instruction.span));
                        uses.push((*right, instruction.span));
                    }
                    InstructionKind::Call { arguments, .. } => {
                        uses.extend(arguments.iter().copied().map(|v| (v, instruction.span)));
                    }
                    InstructionKind::Integer(_)
                    | InstructionKind::Float(_)
                    | InstructionKind::String(_)
                    | InstructionKind::Boolean(_)
                    | InstructionKind::Load { .. } => {}
                }
            }

            if let Some(terminator) = &block.terminator {
                match terminator {
                    Terminator::Jump(target) => {
                        if target.0 as usize >= function.blocks.len() {
                            errors.push(LowerError {
                                message: format!(
                                    "MIR function '{}' block {} jumps to missing block {}",
                                    function.name, block.id.0, target.0
                                ),
                                span: block
                                .instructions
                                .last()
                                .map(|i| i.span)
                                .unwrap_or_else(validation_span),
                            });
                        }
                    }
                    Terminator::Branch {
                        condition,
                        then_block,
                        else_block,
                    } => {
                        uses.push((
                            *condition,
                            block
                                .instructions
                                .last()
                                .map(|i| i.span)
                                .unwrap_or_else(validation_span),
                        ));
                        for target in [then_block, else_block] {
                            if target.0 as usize >= function.blocks.len() {
                                errors.push(LowerError {
                                    message: format!(
                                        "MIR function '{}' block {} branches to missing block {}",
                                        function.name, block.id.0, target.0
                                    ),
                                    span: block
                                .instructions
                                .last()
                                .map(|i| i.span)
                                .unwrap_or_else(validation_span),
                                });
                            }
                        }
                    }
                    Terminator::Return(value) => {
                        let valid = match (function.return_type, value) {
                            (MirType::Unit, None) => true,
                            (MirType::Unit, Some(_)) | (_, None) => false,
                            (_, Some(_)) => true,
                        };
                        if !valid {
                            errors.push(LowerError {
                                message: format!(
                                    "MIR function '{}' has a return value inconsistent with its declared return type",
                                    function.name
                                ),
                                span: block
                                .instructions
                                .last()
                                .map(|i| i.span)
                                .unwrap_or_else(validation_span),
                            });
                        }
                        uses.extend(value.iter().copied().map(|v| {
                            (
                                v,
                                block
                                    .instructions
                                    .last()
                                    .map(|i| i.span)
                                    .unwrap_or_else(validation_span),
                            )
                        }));
                    }
                }
            }
        }

        for (value, span) in uses {
            if !definitions.contains(&value) {
                errors.push(LowerError {
                    message: format!(
                        "MIR function '{}' uses undefined value {}",
                        function.name, value.0
                    ),
                    span,
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn type_from_name(name: &str) -> Option<MirType> {
    match name {
        "Int" => Some(MirType::Int),
        "Float" => Some(MirType::Float),
        "Bool" => Some(MirType::Bool),
        "String" => Some(MirType::String),
        "Unit" => Some(MirType::Unit),
        _ => None,
    }
}

pub fn lower(program: &Program) -> Result<MirModule, Vec<LowerError>> {
    let mut module = MirModule::default();
    let mut errors = Vec::new();
    let mut top_level = Vec::new();

    for statement in &program.statements {
        if let Stmt::Function {
            name,
            params,
            return_type,
            body,
            span,
        } = statement
        {
            let mut parameters = Vec::new();
            let mut valid = true;
            for parameter in params {
                match type_from_name(&parameter.ty.name) {
                    Some(ty) => parameters.push(MirParameter {
                        name: parameter.name.clone(),
                        ty,
                    }),
                    None => {
                        valid = false;
                        errors.push(LowerError {
                            message: format!("unsupported parameter type '{}'", parameter.ty.name),
                            span: parameter.ty.span,
                        });
                    }
                }
            }
            let result = match return_type.as_ref() {
                Some(ty) => match type_from_name(&ty.name) {
                    Some(result) => result,
                    None => {
                        valid = false;
                        errors.push(LowerError {
                            message: format!("unsupported return type '{}'", ty.name),
                            span: ty.span,
                        });
                        MirType::Unit
                    }
                },
                None => MirType::Unit,
            };
            if valid {
                match Builder::new(name.clone(), parameters, result).body(body) {
                    Ok(function) => module.functions.push(function),
                    Err(mut found) => errors.append(&mut found),
                }
            } else {
                let _ = span;
            }
        } else {
            top_level.push(statement.clone());
        }
    }

    if !top_level.is_empty() {
        match Builder::new("__mosh_init".into(), Vec::new(), MirType::Unit).body(&top_level) {
            Ok(function) => module.functions.insert(0, function),
            Err(mut found) => errors.append(&mut found),
        }
    }
    if errors.is_empty() {
        validate(&module)?;
        Ok(module)
    } else {
        Err(errors)
    }
}

struct Builder {
    function: MirFunction,
    next_value: u32,
    errors: Vec<LowerError>,
}
impl Builder {
    fn new(name: String, parameters: Vec<MirParameter>, return_type: MirType) -> Self {
        Self {
            function: MirFunction {
                name,
                parameters,
                return_type,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: Vec::new(),
                    terminator: None,
                }],
            },
            next_value: 0,
            errors: Vec::new(),
        }
    }
    fn block(&mut self) -> BlockId {
        let id = BlockId(self.function.blocks.len() as u32);
        self.function.blocks.push(BasicBlock {
            id,
            instructions: Vec::new(),
            terminator: None,
        });
        id
    }
    fn emit(
        &mut self,
        block: BlockId,
        kind: InstructionKind,
        span: Span,
        result: bool,
    ) -> Option<ValueId> {
        let id = if result {
            let id = ValueId(self.next_value);
            self.next_value += 1;
            Some(id)
        } else {
            None
        };
        self.function.blocks[block.0 as usize]
            .instructions
            .push(Instruction {
                result: id,
                kind,
                span,
            });
        id
    }
    fn end(&mut self, block: BlockId, term: Terminator) {
        if self.function.blocks[block.0 as usize].terminator.is_none() {
            self.function.blocks[block.0 as usize].terminator = Some(term);
        }
    }
    fn body(mut self, statements: &[Stmt]) -> Result<MirFunction, Vec<LowerError>> {
        self.statements(statements, BlockId(0));
        for block in &mut self.function.blocks {
            if block.terminator.is_none() {
                if self.function.return_type == MirType::Unit {
                    block.terminator = Some(Terminator::Return(None));
                } else {
                    let span = block
                        .instructions
                        .last()
                        .map(|instruction| instruction.span)
                        .unwrap_or(Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            column: 1,
                            end_line: 1,
                            end_column: 1,
                        });
                    self.errors.push(LowerError {
                        message: format!(
                            "non-Unit function '{}' can reach the end without returning a value",
                            self.function.name
                        ),
                        span,
                    });
                }
            }
        }
        if self.errors.is_empty() {
            Ok(self.function)
        } else {
            Err(self.errors)
        }
    }
    fn statements(&mut self, statements: &[Stmt], mut current: BlockId) -> BlockId {
        for statement in statements {
            if self.function.blocks[current.0 as usize]
                .terminator
                .is_some()
            {
                break;
            }
            current = self.statement(statement, current);
        }
        current
    }
    fn statement(&mut self, statement: &Stmt, current: BlockId) -> BlockId {
        match statement {
            Stmt::Let {
                name,
                mutable,
                value,
                span,
                ..
            } => {
                let value = value
                    .as_ref()
                    .and_then(|expr| self.expression(expr, current));
                self.emit(
                    current,
                    InstructionKind::Declare {
                        name: name.clone(),
                        mutable: *mutable,
                        value,
                    },
                    *span,
                    false,
                );
                current
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                if let Some(value) = self.expression(value, current) {
                    if let Expr::Identifier { name, .. } = target {
                        self.emit(
                            current,
                            InstructionKind::Store {
                                name: name.clone(),
                                value,
                            },
                            *span,
                            false,
                        );
                    } else {
                        self.error(
                            "MIR assignment currently supports local variables only",
                            *span,
                        );
                    }
                }
                current
            }
            Stmt::Return { value, .. } => {
                let value = value
                    .as_ref()
                    .and_then(|expr| self.expression(expr, current));
                self.end(current, Terminator::Return(value));
                current
            }
            Stmt::Expression { expression, .. } => {
                let _ = self.expression(expression, current);
                current
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let Some(condition) = self.expression(condition, current) else {
                    return current;
                };
                let then_block = self.block();
                let else_block = self.block();
                self.end(
                    current,
                    Terminator::Branch {
                        condition,
                        then_block,
                        else_block,
                    },
                );
                let then_end = self.statements(then_body, then_block);
                let else_end = if let Some(body) = else_body {
                    self.statements(body, else_block)
                } else {
                    else_block
                };
                let then_terminated = self.function.blocks[then_end.0 as usize]
                    .terminator
                    .is_some();
                let else_terminated = self.function.blocks[else_end.0 as usize]
                    .terminator
                    .is_some();

                if then_terminated && else_terminated {
                    // Both paths leave control flow; a merge block would be unreachable.
                    then_end
                } else {
                    let merge = self.block();
                    if !then_terminated {
                        self.end(then_end, Terminator::Jump(merge));
                    }
                    if !else_terminated {
                        self.end(else_end, Terminator::Jump(merge));
                    }
                    merge
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                let check = self.block();
                let loop_body = self.block();
                let exit = self.block();
                self.end(current, Terminator::Jump(check));
                if let Some(condition) = self.expression(condition, check) {
                    self.end(
                        check,
                        Terminator::Branch {
                            condition,
                            then_block: loop_body,
                            else_block: exit,
                        },
                    );
                    let body_end = self.statements(body, loop_body);
                    if self.function.blocks[body_end.0 as usize]
                        .terminator
                        .is_none()
                    {
                        self.end(body_end, Terminator::Jump(check));
                    }
                } else {
                    self.end(check, Terminator::Jump(exit));
                }
                exit
            }
            Stmt::For { span, .. } => {
                self.error(
                    "for-loop lowering is deferred until iterable types are implemented",
                    *span,
                );
                current
            }
            Stmt::Function { span, .. } => {
                self.error(
                    "nested function lowering is not supported in the initial MIR",
                    *span,
                );
                current
            }
        }
    }
    fn expression(&mut self, expression: &Expr, block: BlockId) -> Option<ValueId> {
        match expression {
            Expr::Integer { value, span } => {
                self.emit(block, InstructionKind::Integer(value.clone()), *span, true)
            }
            Expr::Float { value, span } => {
                self.emit(block, InstructionKind::Float(value.clone()), *span, true)
            }
            Expr::String { value, span } => {
                self.emit(block, InstructionKind::String(value.clone()), *span, true)
            }
            Expr::Boolean { value, span } => {
                self.emit(block, InstructionKind::Boolean(*value), *span, true)
            }
            Expr::List { span, .. } => {
                self.error("list literals are not supported by the current MIR backend", *span);
                None
            }
            Expr::Dict { span, .. } => {
                self.error("dictionary literals are not supported by the current MIR backend", *span);
                None
            }
            Expr::Tuple { span, .. } => {
                self.error("tuple literals are not supported by the current MIR backend", *span);
                None
            }
            Expr::Identifier { name, span } => self.emit(
                block,
                InstructionKind::Load { name: name.clone() },
                *span,
                true,
            ),
            Expr::Unary {
                operator,
                operand,
                span,
            } => {
                let operand = self.expression(operand, block)?;
                self.emit(
                    block,
                    InstructionKind::Unary {
                        operator: operator.clone(),
                        operand,
                    },
                    *span,
                    true,
                )
            }
            Expr::Binary {
                left,
                operator,
                right,
                span,
            } => {
                let left = self.expression(left, block)?;
                let right = self.expression(right, block)?;
                self.emit(
                    block,
                    InstructionKind::Binary {
                        left,
                        operator: operator.clone(),
                        right,
                    },
                    *span,
                    true,
                )
            }
            Expr::Call {
                callee,
                arguments,
                span,
            } => {
                let Expr::Identifier { name, .. } = callee.as_ref() else {
                    self.error("MIR supports calls to named functions only", *span);
                    return None;
                };
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.expression(argument, block)?);
                }
                self.emit(
                    block,
                    InstructionKind::Call {
                        name: name.clone(),
                        arguments: values,
                    },
                    *span,
                    true,
                )
            }
            Expr::Member { span, .. } | Expr::Index { span, .. } => {
                self.error(
                    "member access and indexing require compound MIR types",
                    *span,
                );
                None
            }
        }
    }
    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.errors.push(LowerError {
            message: message.into(),
            span,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{lower, InstructionKind, Terminator};
    use crate::{lexer::tokenize, parser::parse};

    fn compile(source: &str) -> Result<super::MirModule, Vec<super::LowerError>> {
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        crate::semantic::analyze(&program).unwrap();
        crate::typecheck::check(&program).unwrap();
        lower(&program)
    }

    #[test]
    fn lowers_functions_arithmetic_and_returns() {
        let module = compile(
            "function add(a: Int, b: Int) -> Int:\n    let total = a + b\n    return total\n",
        )
        .unwrap();
        let function = &module.functions[0];
        assert_eq!(function.name, "add");
        assert_eq!(function.parameters.len(), 2);
        assert!(function.blocks[0]
            .instructions
            .iter()
            .any(|i| matches!(&i.kind, InstructionKind::Binary { .. })));
        assert!(matches!(
            function.blocks[0].terminator.as_ref(),
            Some(Terminator::Return(Some(_)))
        ));
    }
    #[test]
    fn lowers_if_else_to_control_flow_blocks() {
        let module = compile("function choose(flag: Bool) -> Int:\n    if flag:\n        return 1\n    else:\n        return 2\n").unwrap();
        assert!(module.functions[0]
            .blocks
            .iter()
            .any(|b| matches!(b.terminator.as_ref(), Some(Terminator::Branch { .. }))));
        assert!(module.functions[0].blocks.len() >= 3);
    }
    #[test]
    fn lowers_while_to_a_loop_back_edge() {
        let module =
            compile("function repeat() -> Unit:\n    while true:\n        return\n").unwrap();
        assert!(module.functions[0]
            .blocks
            .iter()
            .any(|b| matches!(b.terminator, Some(Terminator::Branch { .. }))));
        assert!(module.functions[0]
            .blocks
            .iter()
            .any(|b| matches!(b.terminator.as_ref(), Some(Terminator::Jump(_)))));
    }
    #[test]
    fn lowers_module_statements_into_an_initializer() {
        let module = compile("let answer: Int = 42\nprint(answer)\n").unwrap();
        assert_eq!(module.functions[0].name, "__mosh_init");
    }

    #[test]
    fn does_not_create_an_unreachable_merge_when_both_if_branches_return() {
        let module = compile(
            "function choose(flag: Bool) -> Int:\n    if flag:\n        return 1\n    else:\n        return 2\n",
        )
        .unwrap();
        let function = &module.functions[0];
        assert_eq!(function.blocks.len(), 3);
        assert!(function
            .blocks
            .iter()
            .all(|block| block.terminator.is_some()));
        assert!(function.blocks.iter().all(|block| !matches!(
            block.terminator,
            Some(Terminator::Jump(target)) if target.0 as usize >= function.blocks.len()
        )));
    }

    #[test]
    fn stops_lowering_after_both_nested_if_paths_return() {
        let module = compile(
            "function choose(flag: Bool, other: Bool) -> Int:\n    if flag:\n        if other:\n            return 1\n        else:\n            return 2\n    else:\n        return 3\n    print(4)\n",
        )
        .unwrap();
        let function = &module.functions[0];
        assert!(function
            .blocks
            .iter()
            .all(|block| block.terminator.is_some()));
        assert!(!function.blocks.iter().any(|block| block.instructions.iter().any(
            |instruction| matches!(&instruction.kind, InstructionKind::Call { name, .. } if name == "print")
        )));
    }

    #[test]
    fn rejects_non_unit_fallthrough_during_lowering() {
        let tokens = tokenize("function missing() -> Int:\n    let value = 1\n").unwrap();
        let program = parse(&tokens).unwrap();
        crate::semantic::analyze(&program).unwrap();
        let errors = lower(&program).unwrap_err();
        assert!(errors.iter().any(|error| error
            .message
            .contains("can reach the end without returning a value")));
    }

    #[test]
    fn validator_rejects_missing_terminators_and_invalid_targets() {
        let mut module = compile("function main():\n    print(1)\n").unwrap();
        module.functions[0].blocks[0].terminator = Some(Terminator::Jump(super::BlockId(99)));
        let errors = super::validate(&module).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("missing block 99")));

        module.functions[0].blocks[0].terminator = None;
        let errors = super::validate(&module).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("has no terminator")));
    }

    #[test]
    fn validator_rejects_undefined_values_and_inconsistent_returns() {
        let mut module = compile("function main() -> Int:\n    return 1\n").unwrap();
        module.functions[0].blocks[0].terminator =
            Some(Terminator::Return(Some(super::ValueId(999))));
        let errors = super::validate(&module).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.message.contains("uses undefined value 999")));

        module.functions[0].blocks[0].terminator = Some(Terminator::Return(None));
        let errors = super::validate(&module).unwrap_err();
        assert!(errors.iter().any(|error| error
            .message
            .contains("inconsistent with its declared return type")));
    }

    #[test]
    fn validator_accepts_lowered_control_flow() {
        let module = compile(
            "function choose(flag: Bool) -> Int:\n    if flag:\n        return 1\n    else:\n        return 2\n",
        ).unwrap();
        assert!(super::validate(&module).is_ok());
    }

    #[test]
    fn reports_deferred_for_loop_lowering() {
        let tokens = tokenize("let items: Int = 1\nfor item in items:\n    print(item)\n").unwrap();
        let program = parse(&tokens).unwrap();
        crate::semantic::analyze(&program).unwrap();
        assert!(lower(&program).is_err());
    }
}
