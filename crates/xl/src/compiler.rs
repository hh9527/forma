use crate::ast::{
    BinaryOperator, BindingKind, Block, DictField, Expr, ExprKind, Identifier, MatchArm, Pattern,
    PatternKind, Program, UnaryOperator,
};
use crate::bytecode::{BytecodeFunction, Instruction, Register};
use crate::lexer::{FrontendError, SourceLocation};
use crate::parser::parse_registered;
use crate::source::{Diagnostic, Location, SourceDatabase, SourceFile};
use crate::types::{Analysis, analyze_program_registered};
use crate::value::{Atom, BuiltinAtom, Value};
use crate::{RuntimeError, Vm};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
pub enum ExecutionError {
    Frontend(FrontendError),
    Runtime(RuntimeError),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frontend(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<FrontendError> for ExecutionError {
    fn from(value: FrontendError) -> Self {
        Self::Frontend(value)
    }
}

impl From<RuntimeError> for ExecutionError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

pub fn compile_source(source_name: &str, source: &str) -> Result<BytecodeFunction, FrontendError> {
    let mut sources = SourceDatabase::default();
    let source_id = sources.add(source_name, source);
    let parsed = parse_registered(&sources, source_id);
    let program = parsed.program.ok_or_else(|| {
        FrontendError::from_diagnostic(
            &sources,
            parsed
                .diagnostics
                .into_iter()
                .next()
                .expect("failed parse has a diagnostic"),
        )
    })?;
    let analysis = analyze_program_registered(source_name, &sources, &program, 100_000)?;
    compile_program_analyzed_in(sources.get(source_id), &program, &analysis)
}

pub(crate) fn compile_program_analyzed_in(
    source_file: &SourceFile,
    program: &Program,
    analysis: &Analysis,
) -> Result<BytecodeFunction, FrontendError> {
    Compiler::program_in(
        source_file.name.as_ref(),
        Some(source_file),
        program,
        analysis,
    )
}

pub fn run_source(
    source_name: &str,
    source: &str,
    instruction_budget: usize,
) -> Result<Value, ExecutionError> {
    let function = compile_source(source_name, source)?;
    Ok(Vm::new().execute(&function, instruction_budget)?)
}

pub(crate) fn compile_expression_with_bindings(
    source_name: &str,
    function_name: &str,
    expression: &Expr,
    bindings: &BTreeMap<String, Value>,
    source_file: &SourceFile,
) -> Result<BytecodeFunction, FrontendError> {
    let mut compiler = Compiler {
        source_name,
        function_name: function_name.to_owned(),
        environment: HashMap::new(),
        constants: Vec::new(),
        instructions: Vec::new(),
        next_register: 0,
        parameter_count: 0,
        capture_count: 0,
        closure_index: 0,
        resolved_types: HashMap::new(),
        retained_names: HashSet::new(),
        external_values: BTreeMap::new(),
        source_file: Some(source_file),
    };
    for (name, value) in bindings {
        let register = compiler.load_constant(value.clone());
        compiler.environment.insert(name.clone(), register);
    }
    let result = compiler.compile_expr(expression)?;
    compiler
        .instructions
        .push(Instruction::Return { src: result });
    Ok(compiler.finish())
}

struct Compiler<'a> {
    source_name: &'a str,
    function_name: String,
    environment: HashMap<String, Register>,
    constants: Vec<Value>,
    instructions: Vec<Instruction>,
    next_register: usize,
    parameter_count: usize,
    capture_count: usize,
    closure_index: usize,
    resolved_types: HashMap<String, Value>,
    retained_names: HashSet<String>,
    external_values: BTreeMap<String, Value>,
    source_file: Option<&'a SourceFile>,
}

impl<'a> Compiler<'a> {
    fn error_at(&self, location: Location, message: impl Into<String>) -> FrontendError {
        let message = message.into();
        if let Some(source_file) = self.source_file {
            let position = source_file.position(location.range.start);
            let diagnostic = Diagnostic::error(message.clone(), location);
            FrontendError {
                source_name: source_file.name.to_string(),
                location: SourceLocation {
                    offset: location.range.start as usize,
                    line: position.line,
                    column: position.column,
                },
                message,
                diagnostic: Some(Box::new(diagnostic)),
            }
        } else {
            unreachable!("located compiler errors require their source file")
        }
    }

    fn program_in(
        source_name: &'a str,
        source_file: Option<&'a SourceFile>,
        program: &Program,
        analysis: &Analysis,
    ) -> Result<BytecodeFunction, FrontendError> {
        let mut retained_names = HashSet::new();
        collect_runtime_names_block(&program.value.body, &mut retained_names);
        let mut compiler = Self {
            source_name,
            function_name: source_name.to_owned(),
            environment: HashMap::new(),
            constants: Vec::new(),
            instructions: Vec::new(),
            next_register: 0,
            parameter_count: 0,
            capture_count: 0,
            closure_index: 0,
            resolved_types: analysis.resolved_types.clone(),
            retained_names,
            external_values: analysis.external_values.clone(),
            source_file,
        };
        for (name, value) in &analysis.prelude {
            if compiler.retained_names.contains(name) {
                let register = compiler.load_constant(value.clone());
                compiler.environment.insert(name.clone(), register);
            }
        }
        for name in &analysis.dynamic_bindings {
            if compiler.retained_names.contains(name) {
                let value = analysis
                    .external_values
                    .get(name)
                    .expect("analyzed dynamic binding")
                    .clone();
                let register = compiler.load_constant(value);
                compiler.environment.insert(name.clone(), register);
            }
        }
        let result = compiler.compile_block(&program.value.body)?;
        compiler
            .instructions
            .push(Instruction::Return { src: result });
        Ok(compiler.finish())
    }

    fn nested(
        source_name: &'a str,
        source_file: Option<&'a SourceFile>,
        function_name: String,
        parameters: &[Identifier],
        captures: &[String],
    ) -> Result<Self, FrontendError> {
        let mut environment = HashMap::new();
        for (index, parameter) in parameters.iter().enumerate() {
            if environment
                .insert(parameter.value.clone(), Register(index))
                .is_some()
            {
                return Err(frontend_error(
                    source_name,
                    format!("duplicate parameter {:?}", parameter.value),
                ));
            }
        }
        for (offset, capture) in captures.iter().enumerate() {
            environment.insert(capture.clone(), Register(parameters.len() + offset));
        }
        Ok(Self {
            source_name,
            function_name,
            environment,
            constants: Vec::new(),
            instructions: Vec::new(),
            next_register: parameters.len() + captures.len(),
            parameter_count: parameters.len(),
            capture_count: captures.len(),
            closure_index: 0,
            resolved_types: HashMap::new(),
            retained_names: HashSet::new(),
            external_values: BTreeMap::new(),
            source_file,
        })
    }

    fn finish(self) -> BytecodeFunction {
        BytecodeFunction::with_signature(
            self.function_name,
            self.parameter_count,
            self.capture_count,
            self.next_register,
            self.constants,
            self.instructions,
        )
    }

    fn compile_block(&mut self, block: &Block) -> Result<Register, FrontendError> {
        let outer = self.environment.clone();
        for binding in &block.value.bindings {
            match binding.value.kind {
                BindingKind::Type => {
                    if self.retained_names.contains(&binding.value.name.value) {
                        let value = self
                            .resolved_types
                            .get(&binding.value.name.value)
                            .cloned()
                            .ok_or_else(|| {
                                frontend_error(
                                    self.source_name,
                                    "nested type declarations are not supported in the MVP",
                                )
                            })?;
                        let register = self.load_constant(value);
                        self.environment
                            .insert(binding.value.name.value.clone(), register);
                    }
                    continue;
                }
                BindingKind::Import => {
                    let value = self
                        .external_values
                        .get(&binding.value.name.value)
                        .cloned()
                        .ok_or_else(|| {
                            frontend_error(
                                self.source_name,
                                format!(
                                    "import {} has not been resolved",
                                    binding.value.name.value
                                ),
                            )
                        })?;
                    let register = self.load_constant(value);
                    self.environment
                        .insert(binding.value.name.value.clone(), register);
                    continue;
                }
                BindingKind::Let => {}
            }
            let value = self.compile_expr(&binding.value.value)?;
            self.environment
                .insert(binding.value.name.value.clone(), value);
        }
        let result = self.compile_expr(&block.value.result)?;
        self.environment = outer;
        Ok(result)
    }

    fn compile_expr(&mut self, expression: &Expr) -> Result<Register, FrontendError> {
        match &expression.value {
            ExprKind::Int(value) => Ok(self.load_constant(Value::Int(*value))),
            ExprKind::Float(value) => Ok(self.load_constant(Value::Float(*value))),
            ExprKind::String(value) => Ok(self.load_constant(Value::string(value.clone()))),
            ExprKind::Bytes(value) => Ok(self.load_constant(Value::Bytes(value.clone().into()))),
            ExprKind::Atom(name) => Ok(self.load_constant(atom_value(name))),
            ExprKind::Variable(name) => {
                self.environment.get(&name.value).copied().ok_or_else(|| {
                    self.error_at(
                        expression.location,
                        format!("unknown binding {:?}", name.value),
                    )
                })
            }
            ExprKind::Array(items) => {
                let items = self.compile_many(items)?;
                let dst = self.allocate();
                self.instructions
                    .push(Instruction::MakeArray { dst, items });
                Ok(dst)
            }
            ExprKind::Tuple(items) => {
                let items = self.compile_many(items)?;
                let dst = self.allocate();
                self.instructions
                    .push(Instruction::MakeTuple { dst, items });
                Ok(dst)
            }
            ExprKind::Dict(fields) => self.compile_dict(fields),
            ExprKind::Block(block) => self.compile_block(block),
            ExprKind::Unary { operator, operand } => {
                let src = self.compile_expr(operand)?;
                let dst = self.allocate();
                match operator.value {
                    UnaryOperator::Negate => {
                        self.instructions.push(Instruction::Negate { dst, src });
                    }
                }
                Ok(dst)
            }
            ExprKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.compile_expr(left)?;
                let right = self.compile_expr(right)?;
                let dst = self.allocate();
                self.instructions.push(match operator.value {
                    BinaryOperator::Add => Instruction::Add { dst, left, right },
                    BinaryOperator::Subtract => Instruction::Subtract { dst, left, right },
                    BinaryOperator::Multiply => Instruction::Multiply { dst, left, right },
                    BinaryOperator::Divide => Instruction::Divide { dst, left, right },
                    BinaryOperator::LessThan => Instruction::LessThan { dst, left, right },
                    BinaryOperator::Equal => Instruction::Equal { dst, left, right },
                });
                Ok(dst)
            }
            ExprKind::Field { receiver, field } => {
                let dict = self.compile_expr(receiver)?;
                let dst = self.allocate();
                self.instructions.push(Instruction::GetField {
                    dst,
                    dict,
                    field: field.value.clone(),
                });
                Ok(dst)
            }
            ExprKind::Call { callee, arguments } => {
                let callee = self.compile_expr(callee)?;
                let arguments = self.compile_many(arguments)?;
                let dst = self.allocate();
                self.instructions.push(Instruction::Call {
                    dst,
                    callee,
                    arguments,
                });
                Ok(dst)
            }
            ExprKind::Closure { parameters, body } => self.compile_closure(parameters, body),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.compile_if(condition, then_branch, else_branch),
            ExprKind::Match { value, arms } => self.compile_match(value, arms),
        }
    }

    fn compile_many(&mut self, expressions: &[Expr]) -> Result<Vec<Register>, FrontendError> {
        expressions
            .iter()
            .map(|expression| self.compile_expr(expression))
            .collect()
    }

    fn compile_dict(&mut self, fields: &[DictField]) -> Result<Register, FrontendError> {
        let mut seen = HashSet::new();
        let mut compiled = Vec::with_capacity(fields.len());
        for field in fields {
            let name = &field.value.name.value;
            if !seen.insert(name) {
                return Err(frontend_error(
                    self.source_name,
                    format!("duplicate Dict field {name:?}"),
                ));
            }
            compiled.push((name.clone(), self.compile_expr(&field.value.value)?));
        }
        let dst = self.allocate();
        self.instructions.push(Instruction::MakeDict {
            dst,
            fields: compiled,
        });
        Ok(dst)
    }

    fn compile_closure(
        &mut self,
        parameters: &[Identifier],
        body: &Block,
    ) -> Result<Register, FrontendError> {
        let mut bound = parameters
            .iter()
            .map(|parameter| parameter.value.clone())
            .collect::<HashSet<_>>();
        if bound.len() != parameters.len() {
            return Err(frontend_error(
                self.source_name,
                "duplicate closure parameter",
            ));
        }
        let mut free = BTreeSet::new();
        free_block(body, &mut bound, &mut free);
        let captures = free.into_iter().collect::<Vec<_>>();
        let capture_registers = captures
            .iter()
            .map(|name| {
                self.environment.get(name).copied().ok_or_else(|| {
                    frontend_error(self.source_name, format!("unknown binding {name:?}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let name = format!("{}::closure{}", self.function_name, self.closure_index);
        self.closure_index += 1;
        let mut nested = Self::nested(
            self.source_name,
            self.source_file,
            name,
            parameters,
            &captures,
        )?;
        let result = nested.compile_block(body)?;
        nested
            .instructions
            .push(Instruction::Return { src: result });
        let function = Arc::new(nested.finish());

        let dst = self.allocate();
        self.instructions.push(Instruction::MakeClosure {
            dst,
            function,
            captures: capture_registers,
        });
        Ok(dst)
    }

    fn compile_if(
        &mut self,
        condition: &Expr,
        then_branch: &Block,
        else_branch: &Block,
    ) -> Result<Register, FrontendError> {
        let condition = self.compile_expr(condition)?;
        let jump_else = self.emit_jump_if_false(condition);
        let then_value = self.compile_block(then_branch)?;
        let result = self.allocate();
        self.instructions.push(Instruction::Move {
            dst: result,
            src: then_value,
        });
        let jump_end = self.emit_jump();
        self.patch_jump(jump_else, self.instructions.len());
        let else_value = self.compile_block(else_branch)?;
        self.instructions.push(Instruction::Move {
            dst: result,
            src: else_value,
        });
        self.patch_jump(jump_end, self.instructions.len());
        Ok(result)
    }

    fn compile_match(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
    ) -> Result<Register, FrontendError> {
        let value = self.compile_expr(value)?;
        let result = self.allocate();
        let mut end_jumps = Vec::new();

        for arm in arms {
            let outer = self.environment.clone();
            let mut failures = Vec::new();
            let mut pattern_bindings = HashSet::new();
            self.compile_pattern(
                &arm.value.pattern,
                value,
                &mut failures,
                &mut pattern_bindings,
            )?;
            let arm_value = self.compile_expr(&arm.value.value)?;
            self.instructions.push(Instruction::Move {
                dst: result,
                src: arm_value,
            });
            end_jumps.push(self.emit_jump());
            let next_arm = self.instructions.len();
            for failure in failures {
                self.patch_jump(failure, next_arm);
            }
            self.environment = outer;
        }

        self.instructions.push(Instruction::Fail {
            message: "no match arm accepted the value".into(),
        });
        let end = self.instructions.len();
        for jump in end_jumps {
            self.patch_jump(jump, end);
        }
        Ok(result)
    }

    fn compile_pattern(
        &mut self,
        pattern: &Pattern,
        value: Register,
        failures: &mut Vec<usize>,
        bindings: &mut HashSet<String>,
    ) -> Result<(), FrontendError> {
        match &pattern.value {
            PatternKind::Wildcard => {}
            PatternKind::Binding(name) => {
                if !bindings.insert(name.value.clone()) {
                    return Err(frontend_error(
                        self.source_name,
                        format!("duplicate pattern binding {:?}", name.value),
                    ));
                }
                self.environment.insert(name.value.clone(), value);
            }
            PatternKind::Int(item) => {
                let expected = self.load_constant(Value::Int(*item));
                self.emit_pattern_equality(value, expected, failures);
            }
            PatternKind::Float(item) => {
                let expected = self.load_constant(Value::Float(*item));
                self.emit_pattern_equality(value, expected, failures);
            }
            PatternKind::String(item) => {
                let expected = self.load_constant(Value::string(item.clone()));
                self.emit_pattern_equality(value, expected, failures);
            }
            PatternKind::Atom(item) => {
                let expected = self.load_constant(atom_value(item));
                self.emit_pattern_equality(value, expected, failures);
            }
            PatternKind::Tuple(items) => {
                let condition = self.allocate();
                self.instructions.push(Instruction::TupleLengthEquals {
                    dst: condition,
                    value,
                    length: items.len(),
                });
                failures.push(self.emit_jump_if_false(condition));
                for (index, pattern) in items.iter().enumerate() {
                    let element = self.allocate();
                    self.instructions.push(Instruction::GetTuple {
                        dst: element,
                        tuple: value,
                        index,
                    });
                    self.compile_pattern(pattern, element, failures, bindings)?;
                }
            }
        }
        Ok(())
    }

    fn emit_pattern_equality(
        &mut self,
        value: Register,
        expected: Register,
        failures: &mut Vec<usize>,
    ) {
        let condition = self.allocate();
        self.instructions.push(Instruction::Equal {
            dst: condition,
            left: value,
            right: expected,
        });
        failures.push(self.emit_jump_if_false(condition));
    }

    fn load_constant(&mut self, value: Value) -> Register {
        let constant = self.constants.len();
        self.constants.push(value);
        let dst = self.allocate();
        self.instructions
            .push(Instruction::LoadConst { dst, constant });
        dst
    }

    fn allocate(&mut self) -> Register {
        let register = Register(self.next_register);
        self.next_register += 1;
        register
    }

    fn emit_jump_if_false(&mut self, condition: Register) -> usize {
        let index = self.instructions.len();
        self.instructions.push(Instruction::JumpIfFalse {
            condition,
            target: usize::MAX,
        });
        index
    }

    fn emit_jump(&mut self) -> usize {
        let index = self.instructions.len();
        self.instructions
            .push(Instruction::Jump { target: usize::MAX });
        index
    }

    fn patch_jump(&mut self, instruction: usize, target: usize) {
        match &mut self.instructions[instruction] {
            Instruction::Jump {
                target: jump_target,
            }
            | Instruction::JumpIfFalse {
                target: jump_target,
                ..
            } => *jump_target = target,
            _ => unreachable!("compiler only patches jump instructions"),
        }
    }
}

fn atom_value(name: &str) -> Value {
    let builtin = match name {
        "None" => Some(BuiltinAtom::None),
        "Some" => Some(BuiltinAtom::Some),
        "Ok" => Some(BuiltinAtom::Ok),
        "Err" => Some(BuiltinAtom::Err),
        "True" => Some(BuiltinAtom::True),
        "False" => Some(BuiltinAtom::False),
        _ => None,
    };
    Value::Atom(match builtin {
        Some(builtin) => Atom::builtin(builtin),
        None => Atom::named(name),
    })
}

fn free_block(block: &Block, bound: &mut HashSet<String>, free: &mut BTreeSet<String>) {
    for binding in &block.value.bindings {
        free_expr(&binding.value.value, bound, free);
        bound.insert(binding.value.name.value.clone());
    }
    free_expr(&block.value.result, bound, free);
}

fn free_expr(expression: &Expr, bound: &HashSet<String>, free: &mut BTreeSet<String>) {
    match &expression.value {
        ExprKind::Variable(name) => {
            if !bound.contains(&name.value) {
                free.insert(name.value.clone());
            }
        }
        ExprKind::Array(items) | ExprKind::Tuple(items) => {
            for item in items {
                free_expr(item, bound, free);
            }
        }
        ExprKind::Dict(fields) => {
            for field in fields {
                free_expr(&field.value.value, bound, free);
            }
        }
        ExprKind::Block(block) => {
            let mut inner = bound.clone();
            free_block(block, &mut inner, free);
        }
        ExprKind::Unary { operand, .. } => free_expr(operand, bound, free),
        ExprKind::Binary { left, right, .. } => {
            free_expr(left, bound, free);
            free_expr(right, bound, free);
        }
        ExprKind::Field { receiver, .. } => free_expr(receiver, bound, free),
        ExprKind::Call { callee, arguments } => {
            free_expr(callee, bound, free);
            for argument in arguments {
                free_expr(argument, bound, free);
            }
        }
        ExprKind::Closure { parameters, body } => {
            let mut closure_bound = parameters
                .iter()
                .map(|parameter| parameter.value.clone())
                .collect::<HashSet<_>>();
            let mut closure_free = BTreeSet::new();
            free_block(body, &mut closure_bound, &mut closure_free);
            for name in closure_free {
                if !bound.contains(&name) {
                    free.insert(name);
                }
            }
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            free_expr(condition, bound, free);
            let mut then_bound = bound.clone();
            free_block(then_branch, &mut then_bound, free);
            let mut else_bound = bound.clone();
            free_block(else_branch, &mut else_bound, free);
        }
        ExprKind::Match { value, arms } => {
            free_expr(value, bound, free);
            for arm in arms {
                let mut arm_bound = bound.clone();
                bind_pattern(&arm.value.pattern, &mut arm_bound);
                free_expr(&arm.value.value, &arm_bound, free);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bytes(_)
        | ExprKind::Atom(_) => {}
    }
}

fn bind_pattern(pattern: &Pattern, bound: &mut HashSet<String>) {
    match &pattern.value {
        PatternKind::Binding(name) => {
            bound.insert(name.value.clone());
        }
        PatternKind::Tuple(items) => {
            for item in items {
                bind_pattern(item, bound);
            }
        }
        PatternKind::Wildcard
        | PatternKind::Int(_)
        | PatternKind::Float(_)
        | PatternKind::String(_)
        | PatternKind::Atom(_) => {}
    }
}

fn collect_runtime_names_block(block: &Block, names: &mut HashSet<String>) {
    for binding in &block.value.bindings {
        if binding.value.kind == BindingKind::Let {
            collect_runtime_names(&binding.value.value, names);
        }
    }
    collect_runtime_names(&block.value.result, names);
}

fn collect_runtime_names(expression: &Expr, names: &mut HashSet<String>) {
    match &expression.value {
        ExprKind::Variable(name) => {
            names.insert(name.value.clone());
        }
        ExprKind::Array(items) | ExprKind::Tuple(items) => {
            for item in items {
                collect_runtime_names(item, names);
            }
        }
        ExprKind::Dict(fields) => {
            for field in fields {
                collect_runtime_names(&field.value.value, names);
            }
        }
        ExprKind::Block(block) => collect_runtime_names_block(block, names),
        ExprKind::Unary { operand, .. } => collect_runtime_names(operand, names),
        ExprKind::Binary { left, right, .. } => {
            collect_runtime_names(left, names);
            collect_runtime_names(right, names);
        }
        ExprKind::Field { receiver, .. } => collect_runtime_names(receiver, names),
        ExprKind::Call { callee, arguments } => {
            collect_runtime_names(callee, names);
            for argument in arguments {
                collect_runtime_names(argument, names);
            }
        }
        ExprKind::Closure { body, .. } => collect_runtime_names_block(body, names),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_runtime_names(condition, names);
            collect_runtime_names_block(then_branch, names);
            collect_runtime_names_block(else_branch, names);
        }
        ExprKind::Match { value, arms } => {
            collect_runtime_names(value, names);
            for arm in arms {
                collect_runtime_names(&arm.value.value, names);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bytes(_)
        | ExprKind::Atom(_) => {}
    }
}

fn frontend_error(source_name: &str, message: impl Into<String>) -> FrontendError {
    FrontendError::new(
        source_name,
        SourceLocation {
            offset: 0,
            line: 1,
            column: 1,
        },
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeErrorKind;

    fn run(source: &str) -> Result<Value, ExecutionError> {
        run_source("test", source, 10_000)
    }

    #[test]
    fn executes_precedence_blocks_and_dict_access() {
        let value = run("let x = 2 + 3 * 4; {b: x, a: 1}.b").unwrap();
        assert!(matches!(value, Value::Int(14)));
    }

    #[test]
    fn captures_values_and_calls_closures() {
        let value = run("let base = 40; let add = fn(value) { base + value }; add(2)").unwrap();
        assert!(matches!(value, Value::Int(42)));
    }

    #[test]
    fn pipeline_inserts_the_first_argument() {
        let value = run("let add = fn(a, b) { a + b }; 40 |> add(2)").unwrap();
        assert!(matches!(value, Value::Int(42)));
    }

    #[test]
    fn if_evaluates_only_the_selected_branch() {
        let value = run("if 'True { 42 } else { 1 / 0 }").unwrap();
        assert!(matches!(value, Value::Int(42)));
    }

    #[test]
    fn match_destructures_tagged_tuples() {
        let value = run("match ('Ok, 42) { ('Err, _) => 0, ('Ok, value) => value }").unwrap();
        assert!(matches!(value, Value::Int(42)));
    }

    #[test]
    fn non_exhaustive_match_has_a_dedicated_error() {
        let error = run("match 'None { 'Some => 1 }").unwrap_err();
        let ExecutionError::Runtime(error) = error else {
            panic!("expected runtime error");
        };
        assert_eq!(error.kind, RuntimeErrorKind::NoPatternMatched);
    }

    #[test]
    fn reports_unknown_bindings_and_arity_errors() {
        let unknown = compile_source("test", "let present = 1;\nmissing").unwrap_err();
        assert!(unknown.message.contains("unknown binding"));
        assert_eq!(unknown.location.line, 2);
        assert_eq!(unknown.location.column, 1);

        let error = run("let f = fn(a) { a }; f(1, 2)").unwrap_err();
        let ExecutionError::Runtime(error) = error else {
            panic!("expected runtime error");
        };
        assert!(error.message.contains("expected 1 arguments"));
    }
}
