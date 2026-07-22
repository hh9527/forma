use crate::ast::{
    BinaryOperator, BindingKind, Block, DictField, Expr, ExprKind, Identifier, MatchArm, Pattern,
    PatternKind, Program, StringPartKind, UnaryOperator,
};
use crate::bytecode::BytecodeFunction;
use crate::lexer::{FrontendError, SourceLocation};
use crate::lir::{self, ConstantId, Item, LabelId, Operation, RegisterId};
use crate::parser::parse_registered;
use crate::source::{Diagnostic, Location, Origin, SourceDatabase, SourceFile, WithOrigin};
use crate::types::{Analysis, analyze_program_registered};
use crate::value::{Atom, BuiltinAtom, Value};
use crate::{RuntimeError, Vm};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

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
    evaluation_fuel: usize,
) -> Result<Value, ExecutionError> {
    let function = compile_source(source_name, source)?;
    let mut sources = SourceDatabase::default();
    sources.add(source_name, source);
    Vm::new()
        .execute(&function, evaluation_fuel)
        .map_err(|error| ExecutionError::Runtime(error.with_sources(&sources)))
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
        items: Vec::new(),
        next_register: 0,
        next_label: 0,
        parameter_count: 0,
        capture_count: 0,
        closure_index: 0,
        resolved_types: HashMap::new(),
        retained_names: HashSet::new(),
        external_values: BTreeMap::new(),
        source_file: Some(source_file),
    };
    for (name, value) in bindings {
        let register = compiler.load_constant(value.clone(), expression.location);
        compiler.environment.insert(name.clone(), register);
    }
    let result = compiler.compile_expr(expression)?;
    compiler.emit_synthetic(Operation::Return { src: result }, expression.location);
    compiler.finish()
}

struct Compiler<'a> {
    source_name: &'a str,
    function_name: String,
    environment: HashMap<String, RegisterId>,
    constants: Vec<Value>,
    items: Vec<Item>,
    next_register: u32,
    next_label: u32,
    parameter_count: u32,
    capture_count: u32,
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
            items: Vec::new(),
            next_register: 0,
            next_label: 0,
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
                let register = compiler.load_constant(value.clone(), program.location);
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
                let register = compiler.load_constant(value, program.location);
                compiler.environment.insert(name.clone(), register);
            }
        }
        let result = compiler.compile_block(&program.value.body)?;
        compiler.emit_synthetic(Operation::Return { src: result }, program.location);
        compiler.finish()
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
                .insert(
                    parameter.value.clone(),
                    RegisterId(u32::try_from(index).map_err(|_| {
                        frontend_error(source_name, "too many function parameters")
                    })?),
                )
                .is_some()
            {
                return Err(frontend_error(
                    source_name,
                    format!("duplicate parameter {:?}", parameter.value),
                ));
            }
        }
        for (offset, capture) in captures.iter().enumerate() {
            let index = parameters
                .len()
                .checked_add(offset)
                .ok_or_else(|| frontend_error(source_name, "too many closure registers"))?;
            environment.insert(
                capture.clone(),
                RegisterId(
                    u32::try_from(index)
                        .map_err(|_| frontend_error(source_name, "too many closure captures"))?,
                ),
            );
        }
        let register_count = parameters
            .len()
            .checked_add(captures.len())
            .ok_or_else(|| frontend_error(source_name, "too many closure registers"))?;
        Ok(Self {
            source_name,
            function_name,
            environment,
            constants: Vec::new(),
            items: Vec::new(),
            next_register: u32::try_from(register_count)
                .map_err(|_| frontend_error(source_name, "too many closure registers"))?,
            next_label: 0,
            parameter_count: u32::try_from(parameters.len())
                .map_err(|_| frontend_error(source_name, "too many function parameters"))?,
            capture_count: u32::try_from(captures.len())
                .map_err(|_| frontend_error(source_name, "too many closure captures"))?,
            closure_index: 0,
            resolved_types: HashMap::new(),
            retained_names: HashSet::new(),
            external_values: BTreeMap::new(),
            source_file,
        })
    }

    fn finish_lir(self) -> lir::Function {
        lir::Function {
            name: self.function_name,
            parameter_count: self.parameter_count,
            capture_count: self.capture_count,
            register_count: self.next_register,
            constants: self.constants,
            items: self.items,
        }
    }

    fn finish(self) -> Result<BytecodeFunction, FrontendError> {
        let source_name = self.source_name;
        lir::assemble(self.finish_lir())
            .map_err(|error| frontend_error(source_name, error.to_string()))
    }

    fn compile_block(&mut self, block: &Block) -> Result<RegisterId, FrontendError> {
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
                        let register = self.load_constant(value, binding.location);
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
                    let register = self.load_constant(value, binding.location);
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

    fn compile_expr(&mut self, expression: &Expr) -> Result<RegisterId, FrontendError> {
        match &expression.value {
            ExprKind::Int(value) => Ok(self.load_constant(Value::Int(*value), expression.location)),
            ExprKind::Float(value) => {
                Ok(self.load_constant(Value::Float(*value), expression.location))
            }
            ExprKind::String(value) => {
                Ok(self.load_constant(Value::string(value.clone()), expression.location))
            }
            ExprKind::InterpolatedString(parts) => {
                let mut registers = Vec::with_capacity(parts.len());
                for part in parts {
                    registers.push(match &part.value {
                        StringPartKind::Text(text) => {
                            self.load_constant(Value::string(text.clone()), part.location)
                        }
                        StringPartKind::Expression(expression) => self.compile_expr(expression)?,
                    });
                }
                let dst = self.allocate();
                self.emit(
                    Operation::InterpolateString {
                        dst,
                        parts: registers,
                    },
                    expression.location,
                );
                Ok(dst)
            }
            ExprKind::Bytes(value) => {
                Ok(self.load_constant(Value::Bytes(value.clone().into()), expression.location))
            }
            ExprKind::Atom(name) => Ok(self.load_constant(atom_value(name), expression.location)),
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
                self.emit(Operation::MakeArray { dst, items }, expression.location);
                Ok(dst)
            }
            ExprKind::Tuple(items) => {
                let items = self.compile_many(items)?;
                let dst = self.allocate();
                self.emit(Operation::MakeTuple { dst, items }, expression.location);
                Ok(dst)
            }
            ExprKind::Dict(fields) => self.compile_dict(fields, expression.location),
            ExprKind::Block(block) => self.compile_block(block),
            ExprKind::Unary { operator, operand } => {
                let src = self.compile_expr(operand)?;
                let dst = self.allocate();
                match operator.value {
                    UnaryOperator::Negate => {
                        self.emit(Operation::Negate { dst, src }, expression.location);
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
                let operation = match operator.value {
                    BinaryOperator::Add => Operation::Add { dst, left, right },
                    BinaryOperator::Subtract => Operation::Subtract { dst, left, right },
                    BinaryOperator::Multiply => Operation::Multiply { dst, left, right },
                    BinaryOperator::Divide => Operation::Divide { dst, left, right },
                    BinaryOperator::LessThan => Operation::LessThan { dst, left, right },
                    BinaryOperator::Equal => Operation::Equal { dst, left, right },
                };
                self.emit(operation, expression.location);
                Ok(dst)
            }
            ExprKind::Field { receiver, field } => {
                let dict = self.compile_expr(receiver)?;
                let dst = self.allocate();
                self.emit(
                    Operation::GetField {
                        dst,
                        dict,
                        field: field.value.clone(),
                    },
                    expression.location,
                );
                Ok(dst)
            }
            ExprKind::Call { callee, arguments } => {
                let callee = self.compile_expr(callee)?;
                let arguments = self.compile_many(arguments)?;
                let argument_base = if arguments.is_empty() {
                    RegisterId(0)
                } else {
                    let base = self.allocate();
                    self.emit(
                        Operation::Move {
                            dst: base,
                            src: arguments[0],
                        },
                        expression.location,
                    );
                    for argument in arguments.iter().skip(1) {
                        let destination = self.allocate();
                        self.emit(
                            Operation::Move {
                                dst: destination,
                                src: *argument,
                            },
                            expression.location,
                        );
                    }
                    base
                };
                let dst = self.allocate();
                self.emit(
                    Operation::Call {
                        dst,
                        callee,
                        argument_base,
                        argument_count: u32::try_from(arguments.len()).map_err(|_| {
                            frontend_error(self.source_name, "too many call arguments")
                        })?,
                    },
                    expression.location,
                );
                Ok(dst)
            }
            ExprKind::Closure { parameters, body } => {
                self.compile_closure(parameters, body, expression.location)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.compile_if(condition, then_branch, else_branch, expression.location),
            ExprKind::Match { value, arms } => self.compile_match(value, arms, expression.location),
        }
    }

    fn compile_many(&mut self, expressions: &[Expr]) -> Result<Vec<RegisterId>, FrontendError> {
        expressions
            .iter()
            .map(|expression| self.compile_expr(expression))
            .collect()
    }

    fn compile_dict(
        &mut self,
        fields: &[DictField],
        location: Location,
    ) -> Result<RegisterId, FrontendError> {
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
        self.emit(
            Operation::MakeDict {
                dst,
                fields: compiled,
            },
            location,
        );
        Ok(dst)
    }

    fn compile_closure(
        &mut self,
        parameters: &[Identifier],
        body: &Block,
        location: Location,
    ) -> Result<RegisterId, FrontendError> {
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
        nested.emit_synthetic(Operation::Return { src: result }, body.location);
        let function = Box::new(nested.finish_lir());

        let dst = self.allocate();
        self.emit(
            Operation::MakeClosure {
                dst,
                function,
                captures: capture_registers,
            },
            location,
        );
        Ok(dst)
    }

    fn compile_if(
        &mut self,
        condition: &Expr,
        then_branch: &Block,
        else_branch: &Block,
        location: Location,
    ) -> Result<RegisterId, FrontendError> {
        let condition_location = condition.location;
        let condition = self.compile_expr(condition)?;
        let else_label = self.new_label();
        self.emit(
            Operation::JumpIfFalse {
                condition,
                target: else_label,
            },
            condition_location,
        );
        let then_value = self.compile_block(then_branch)?;
        let result = self.allocate();
        self.emit_synthetic(
            Operation::Move {
                dst: result,
                src: then_value,
            },
            then_branch.location,
        );
        let end_label = self.new_label();
        self.emit_synthetic(Operation::Jump { target: end_label }, location);
        self.mark_label(else_label);
        let else_value = self.compile_block(else_branch)?;
        self.emit_synthetic(
            Operation::Move {
                dst: result,
                src: else_value,
            },
            else_branch.location,
        );
        self.mark_label(end_label);
        Ok(result)
    }

    fn compile_match(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        location: Location,
    ) -> Result<RegisterId, FrontendError> {
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
            self.emit_synthetic(
                Operation::Move {
                    dst: result,
                    src: arm_value,
                },
                arm.location,
            );
            let end = self.new_label();
            self.emit_synthetic(Operation::Jump { target: end }, arm.location);
            end_jumps.push(end);
            for failure in failures {
                self.mark_label(failure);
            }
            self.environment = outer;
        }

        self.emit(
            Operation::Fail {
                message: "no match arm accepted the value".into(),
            },
            location,
        );
        for jump in end_jumps {
            self.mark_label(jump);
        }
        Ok(result)
    }

    fn compile_pattern(
        &mut self,
        pattern: &Pattern,
        value: RegisterId,
        failures: &mut Vec<LabelId>,
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
                let expected = self.load_constant(Value::Int(*item), pattern.location);
                self.emit_pattern_equality(value, expected, failures, pattern.location);
            }
            PatternKind::Float(item) => {
                let expected = self.load_constant(Value::Float(*item), pattern.location);
                self.emit_pattern_equality(value, expected, failures, pattern.location);
            }
            PatternKind::String(item) => {
                let expected = self.load_constant(Value::string(item.clone()), pattern.location);
                self.emit_pattern_equality(value, expected, failures, pattern.location);
            }
            PatternKind::Atom(item) => {
                let expected = self.load_constant(atom_value(item), pattern.location);
                self.emit_pattern_equality(value, expected, failures, pattern.location);
            }
            PatternKind::Tuple(items) => {
                let condition = self.allocate();
                self.emit(
                    Operation::TupleLengthEquals {
                        dst: condition,
                        value,
                        length: items.len(),
                    },
                    pattern.location,
                );
                let failure = self.new_label();
                self.emit(
                    Operation::JumpIfFalse {
                        condition,
                        target: failure,
                    },
                    pattern.location,
                );
                failures.push(failure);
                for (index, pattern) in items.iter().enumerate() {
                    let element = self.allocate();
                    self.emit(
                        Operation::GetTuple {
                            dst: element,
                            tuple: value,
                            index,
                        },
                        pattern.location,
                    );
                    self.compile_pattern(pattern, element, failures, bindings)?;
                }
            }
        }
        Ok(())
    }

    fn emit_pattern_equality(
        &mut self,
        value: RegisterId,
        expected: RegisterId,
        failures: &mut Vec<LabelId>,
        location: Location,
    ) {
        let condition = self.allocate();
        self.emit(
            Operation::Equal {
                dst: condition,
                left: value,
                right: expected,
            },
            location,
        );
        let failure = self.new_label();
        self.emit(
            Operation::JumpIfFalse {
                condition,
                target: failure,
            },
            location,
        );
        failures.push(failure);
    }

    fn load_constant(&mut self, value: Value, location: Location) -> RegisterId {
        let constant = self.constants.len();
        self.constants.push(value);
        let dst = self.allocate();
        self.emit(
            Operation::LoadConst {
                dst,
                constant: ConstantId(u32::try_from(constant).expect("constant pool exceeds u32")),
            },
            location,
        );
        dst
    }

    fn allocate(&mut self) -> RegisterId {
        let register = RegisterId(self.next_register);
        self.next_register = self
            .next_register
            .checked_add(1)
            .expect("register count exceeds u32");
        register
    }

    fn emit(&mut self, operation: Operation, location: Location) {
        self.items.push(Item::Operation(WithOrigin {
            value: operation,
            origin: Origin::Source(location),
        }));
    }

    fn emit_synthetic(&mut self, operation: Operation, derived_from: Location) {
        self.items.push(Item::Operation(WithOrigin {
            value: operation,
            origin: Origin::Synthetic {
                derived_from: Some(derived_from),
            },
        }));
    }

    fn new_label(&mut self) -> LabelId {
        let label = LabelId(self.next_label);
        self.next_label = self
            .next_label
            .checked_add(1)
            .expect("label count exceeds u32");
        label
    }

    fn mark_label(&mut self, label: LabelId) {
        self.items.push(Item::Label(label));
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
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPartKind::Expression(expression) = &part.value {
                    free_expr(expression, bound, free);
                }
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
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPartKind::Expression(expression) = &part.value {
                    collect_runtime_names(expression, names);
                }
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
    fn interpolates_strings_ints_and_atoms() {
        let value = run(
            r#"let name = "Ada"; let count = 3; let state = 'Ok; "hi, \{name} count=\{count} state=\{state}""#,
        )
        .unwrap();
        assert!(
            matches!(&value, Value::String(text) if text.as_ref() == "hi, Ada count=3 state=Ok")
        );

        let nested = run(r#""value=\{if 'True { "yes" } else { "no" }}""#).unwrap();
        assert!(matches!(&nested, Value::String(text) if text.as_ref() == "value=yes"));
    }

    #[test]
    fn checks_known_and_dynamic_unsupported_interpolation_values() {
        let static_error = run(r#""items=\{[1, 2]}""#).unwrap_err();
        assert!(
            static_error
                .to_string()
                .contains("does not support Array<Int>")
        );

        let dynamic_error = run(r#"fn render(x) { "x=\{x}" } render([1])"#).unwrap_err();
        assert!(matches!(
            dynamic_error,
            ExecutionError::Runtime(RuntimeError {
                kind: RuntimeErrorKind::TypeMismatch,
                ..
            })
        ));
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

    #[test]
    fn runtime_errors_retain_expression_origins_and_call_trace() {
        let error = run("let divide = fn(x) {\n  x / 0\n};\ndivide(4)").unwrap_err();
        let ExecutionError::Runtime(error) = error else {
            panic!("expected runtime error");
        };
        assert_eq!(error.kind, RuntimeErrorKind::DivisionByZero);
        assert_eq!(error.trace.len(), 2);
        let Origin::Source(location) = error.origin().expect("runtime origin") else {
            panic!("expected source origin");
        };
        assert_eq!(location.range.start, 23);
        assert!(error.to_string().contains("test:2:3"));
    }

    #[test]
    fn runtime_field_and_interpolation_errors_render_their_expressions() {
        let field = run("let value = {present: 1};\nvalue.missing").unwrap_err();
        assert!(field.to_string().contains("test:2:1"));

        let interpolation =
            run("fn render(value) {\n  \"value=\\{value}\"\n}\nrender([1])").unwrap_err();
        assert!(interpolation.to_string().contains("test:2:3"));
    }

    #[test]
    fn fuel_exhaustion_points_to_the_call_expression() {
        let error = run_source("test", "let f = fn() { 1 };\nf()", 0).unwrap_err();
        let ExecutionError::Runtime(error) = error else {
            panic!("expected runtime error");
        };
        assert_eq!(error.kind, RuntimeErrorKind::FuelExhausted);
        assert!(error.to_string().contains("test:2:1"));
    }
}
