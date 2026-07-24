use crate::ast::{BindingKind, Expr, ExprKind, Program, StringPartKind};
use crate::compiler::{
    compile_metadata_initializer, compile_program_analyzed_in, compile_program_with_promoted_types,
    function_contract_arity, type_link_key,
};
use crate::core::module_specs;
use crate::heap::{Heap, PersistentValue, publish_root, publish_value};
use crate::json::{Provenance, SourcedValue, parse_json_registered};
use crate::parser::parse_registered;
use crate::source::SourceDatabase;
use crate::types::{Analysis, analyze_program_with_bindings_observed};
use crate::{
    BytecodeFunction, Closure, DebugSink, DiscardDebugSink, Quota, QuotaAccount, Value, Vm,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct ModuleError {
    message: String,
}

impl ModuleError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ModuleError {}

#[derive(Clone, Debug)]
pub struct LoadedModule {
    pub path: PathBuf,
    pub dependencies: Vec<PathBuf>,
    pub analysis: Analysis,
    pub function: BytecodeFunction,
    pub sources: SourceDatabase,
    runtime: Arc<ModuleRuntime>,
}

struct ModuleRuntime {
    main: FrozenMainWorld,
    externals: HashMap<String, PersistentValue>,
}

struct MainWorld {
    heap: Heap,
}

impl MainWorld {
    fn building() -> Self {
        Self { heap: Heap::main() }
    }

    fn seal(self) -> FrozenMainWorld {
        FrozenMainWorld { heap: self.heap }
    }
}

struct FrozenMainWorld {
    heap: Heap,
}

fn install_core_modules(
    main: &mut MainWorld,
    sources: &mut SourceDatabase,
    debug_sink: &Arc<dyn DebugSink>,
) -> Result<HashMap<&'static str, (Value, PersistentValue)>, ModuleError> {
    let mut modules = HashMap::new();
    for spec in module_specs() {
        let source_name = format!("<{}>", spec.name);
        let source_id = sources.add(source_name.clone(), spec.source);
        let parsed = parse_registered(sources, source_id);
        let program = parsed.program.ok_or_else(|| {
            ModuleError::new(
                parsed
                    .diagnostics
                    .iter()
                    .map(|diagnostic| sources.render(diagnostic))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })?;
        let implementations = spec.functions.into_iter().collect::<HashMap<_, _>>();
        let mut external_values = BTreeMap::new();
        let mut external_roots = HashMap::new();
        for binding in &program.value.body.value.bindings {
            if binding.value.kind != BindingKind::Native {
                continue;
            }
            let symbol = binding.value.name.value.as_str();
            let implementation = implementations.get(symbol).copied().ok_or_else(|| {
                ModuleError::new(sources.render(&crate::source::Diagnostic::error(
                    format!(
                        "native symbol {symbol:?} is not registered for {}",
                        spec.name
                    ),
                    binding.location,
                )))
            })?;
            let declared_arity = binding
                .value
                .annotation
                .as_ref()
                .and_then(function_contract_arity)
                .expect("native grammar requires a function contract");
            if declared_arity as usize != implementation.arity() {
                return Err(ModuleError::new(sources.render(
                    &crate::source::Diagnostic::error(
                        format!(
                            "native symbol {symbol:?} declares arity {declared_arity}, but its implementation has arity {}",
                            implementation.arity()
                        ),
                        binding.location,
                    ),
                )));
            }
            let value = Value::Func(Arc::new(Closure::native(implementation)));
            let root = publish_value(&mut main.heap, &value)
                .map_err(|error| ModuleError::new(error.to_string()))?;
            external_values.insert(symbol.to_owned(), value);
            external_roots.insert(symbol.to_owned(), root);
        }
        if external_values.len() != implementations.len() {
            let undeclared = implementations
                .keys()
                .find(|symbol| !external_values.contains_key(**symbol))
                .expect("registry size differs");
            return Err(ModuleError::new(format!(
                "native symbol {undeclared:?} for {} has no XL declaration",
                spec.name
            )));
        }
        let mut account = QuotaAccount::new(Quota::new(100_000, 1_000, u64::MAX));
        let analysis = analyze_program_with_bindings_observed(
            &source_name,
            &program,
            &mut account,
            &external_values,
            &HashSet::new(),
            sources,
            &BTreeMap::new(),
            debug_sink,
        )
        .map_err(|error| {
            error.diagnostic.as_ref().map_or_else(
                || ModuleError::new(error.to_string()),
                |diagnostic| ModuleError::new(sources.render(diagnostic)),
            )
        })?;
        let function = compile_program_analyzed_in(sources.get(source_id), &program, &analysis)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let arena = Vm::new()
            .with_debug_sink(Arc::clone(debug_sink))
            .execute_in_work(&main.heap, &external_roots, &function, &[], &mut account)
            .map_err(|error| ModuleError::new(error.with_sources(sources).to_string()))?;
        let value = arena
            .export(&main.heap)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let root = arena
            .publish(&mut main.heap)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        modules.insert(spec.name, (value, root));
    }
    Ok(modules)
}

impl fmt::Debug for ModuleRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleRuntime")
            .finish_non_exhaustive()
    }
}

impl LoadedModule {
    pub fn execute(&self, evaluation_fuel: usize) -> Result<Value, crate::RuntimeError> {
        self.execute_with_quota(Quota::with_fuel(evaluation_fuel))
    }

    pub fn execute_with_quota(&self, quota: Quota) -> Result<Value, crate::RuntimeError> {
        self.execute_with_quota_and_debug_sink(quota, Arc::new(DiscardDebugSink))
    }

    pub fn execute_with_quota_and_debug_sink(
        &self,
        quota: Quota,
        debug_sink: Arc<dyn DebugSink>,
    ) -> Result<Value, crate::RuntimeError> {
        let mut account = QuotaAccount::new(quota);
        let arena = Vm::new()
            .with_debug_sink(debug_sink)
            .execute_in_work(
                &self.runtime.main.heap,
                &self.runtime.externals,
                &self.function,
                &[],
                &mut account,
            )
            .map_err(|error| error.with_sources(&self.sources))?;
        arena
            .export(&self.runtime.main.heap)
            .map_err(|error| crate::RuntimeError::from_heap_error(&self.function, error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineConfig {
    pub module_quota: Quota,
    pub session_quota: Quota,
}

pub struct Engine {
    config: EngineConfig,
    debug_sink: Arc<dyn DebugSink>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            debug_sink: Arc::new(DiscardDebugSink),
        }
    }

    pub fn with_debug_sink(mut self, debug_sink: Arc<dyn DebugSink>) -> Self {
        self.debug_sink = debug_sink;
        self
    }

    pub const fn config(&self) -> EngineConfig {
        self.config
    }

    pub fn load_module(
        &self,
        path: impl AsRef<Path>,
        external_bindings: BTreeMap<String, Value>,
    ) -> Result<LoadedModule, ModuleError> {
        load_module_with_quota_and_debug_sink(
            path,
            external_bindings,
            self.config.module_quota,
            Arc::clone(&self.debug_sink),
        )
    }

    pub fn execute(&self, module: &LoadedModule) -> Result<Value, crate::RuntimeError> {
        module.execute_with_quota_and_debug_sink(
            self.config.session_quota,
            Arc::clone(&self.debug_sink),
        )
    }
}

pub fn load_module(
    path: impl AsRef<Path>,
    external_bindings: BTreeMap<String, Value>,
    evaluation_fuel: usize,
) -> Result<LoadedModule, ModuleError> {
    load_module_with_quota(path, external_bindings, Quota::with_fuel(evaluation_fuel))
}

pub fn load_module_with_quota(
    path: impl AsRef<Path>,
    external_bindings: BTreeMap<String, Value>,
    module_quota: Quota,
) -> Result<LoadedModule, ModuleError> {
    load_module_with_quota_and_debug_sink(
        path,
        external_bindings,
        module_quota,
        Arc::new(DiscardDebugSink),
    )
}

pub fn load_module_with_quota_and_debug_sink(
    path: impl AsRef<Path>,
    external_bindings: BTreeMap<String, Value>,
    module_quota: Quota,
    debug_sink: Arc<dyn DebugSink>,
) -> Result<LoadedModule, ModuleError> {
    let root = canonicalize(path.as_ref())?;
    if root.extension().and_then(|extension| extension.to_str()) != Some("xl") {
        return Err(ModuleError::new("root module must have an .xl extension"));
    }
    let mut main = MainWorld::building();
    let mut sources = SourceDatabase::default();
    let core_modules = install_core_modules(&mut main, &mut sources, &debug_sink)?;
    let mut loader = ModuleLoader {
        cache: HashMap::new(),
        core_modules,
        main,
        visiting: Vec::new(),
        dependencies: BTreeSet::new(),
        module_quota,
        debug_sink,
        sources,
    };
    loader.load_root(root, external_bindings)
}

struct ModuleLoader {
    cache: HashMap<PathBuf, ModuleState>,
    core_modules: HashMap<&'static str, (Value, PersistentValue)>,
    main: MainWorld,
    visiting: Vec<PathBuf>,
    dependencies: BTreeSet<PathBuf>,
    module_quota: Quota,
    debug_sink: Arc<dyn DebugSink>,
    sources: SourceDatabase,
}

#[derive(Clone)]
enum ModuleState {
    Ready {
        root: PersistentValue,
        sourced: SourcedValue,
        opaque: bool,
    },
}

impl ModuleLoader {
    fn load_root(
        &mut self,
        path: PathBuf,
        external_bindings: BTreeMap<String, Value>,
    ) -> Result<LoadedModule, ModuleError> {
        self.enter(&path)?;
        let mut account = QuotaAccount::new(self.module_quota);
        let result = self.compile_xl(&path, external_bindings, true, &mut account);
        self.leave(&path);
        let (analysis, function, externals) = result?;
        let main = std::mem::replace(&mut self.main, MainWorld::building()).seal();
        Ok(LoadedModule {
            path,
            dependencies: self.dependencies.iter().cloned().collect(),
            analysis,
            function,
            sources: self.sources.clone(),
            runtime: Arc::new(ModuleRuntime { main, externals }),
        })
    }

    fn load_value(&mut self, path: &Path) -> Result<SourcedValue, ModuleError> {
        let path = canonicalize(path)?;
        if let Some(ModuleState::Ready { root, sourced, .. }) = self.cache.get(&path) {
            let _persistent_root = root;
            return Ok(sourced.clone());
        }
        self.enter(&path)?;
        let result: Result<(SourcedValue, PersistentValue, bool), ModuleError> =
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("json") => {
                    let source = read(&path)?;
                    let source_id = self.sources.add(path.display().to_string(), source);
                    let parsed = parse_json_registered(&self.sources, source_id);
                    parsed
                        .value
                        .ok_or_else(|| {
                            ModuleError::new(
                                parsed
                                    .diagnostics
                                    .iter()
                                    .map(|diagnostic| self.sources.render(diagnostic))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            )
                        })
                        .and_then(|sourced| {
                            let mut local = Heap::work();
                            let local_root = local
                                .import_sourced_value(Some(&self.main.heap), &sourced)
                                .map_err(|error| ModuleError::new(error.to_string()))?;
                            let root = publish_root(&mut self.main.heap, &local, local_root)
                                .map_err(|error| ModuleError::new(error.to_string()))?;
                            Ok((sourced, root, false))
                        })
                }
                Some("xl") => {
                    let mut account = QuotaAccount::new(self.module_quota);
                    self.compile_xl(&path, BTreeMap::new(), false, &mut account)
                        .and_then(|(_, function, externals)| {
                            let arena = Vm::new()
                                .with_debug_sink(Arc::clone(&self.debug_sink))
                                .execute_in_work(
                                    &self.main.heap,
                                    &externals,
                                    &function,
                                    &[],
                                    &mut account,
                                )
                                .map_err(|error| {
                                    ModuleError::new(error.with_sources(&self.sources).to_string())
                                })?;
                            let (value, opaque) = match arena.export(&self.main.heap) {
                                Ok(value) => (value, false),
                                Err(error) if error.is_legacy_cycle() => (Value::none(), true),
                                Err(error) => return Err(ModuleError::new(error.to_string())),
                            };
                            let root = arena
                                .publish(&mut self.main.heap)
                                .map_err(|error| ModuleError::new(error.to_string()))?;
                            Ok((
                                SourcedValue {
                                    value,
                                    provenance: Provenance::default(),
                                },
                                root,
                                opaque,
                            ))
                        })
                }
                Some(extension) => Err(ModuleError::new(format!(
                    "unsupported module extension .{extension}: {}",
                    path.display()
                ))),
                None => Err(ModuleError::new(format!(
                    "module path has no extension: {}",
                    path.display()
                ))),
            };
        self.leave(&path);
        let (sourced, root, opaque) = result?;
        self.cache.insert(
            path,
            ModuleState::Ready {
                root,
                sourced: sourced.clone(),
                opaque,
            },
        );
        Ok(sourced)
    }

    fn compile_xl(
        &mut self,
        path: &Path,
        mut external_bindings: BTreeMap<String, Value>,
        is_root: bool,
        account: &mut QuotaAccount,
    ) -> Result<(Analysis, BytecodeFunction, HashMap<String, PersistentValue>), ModuleError> {
        let source = read(path)?;
        let source_name = path.display().to_string();
        let source_id = self.sources.add(source_name.clone(), source);
        let parsed = parse_registered(&self.sources, source_id);
        let program = parsed.program.ok_or_else(|| {
            ModuleError::new(
                parsed
                    .diagnostics
                    .iter()
                    .map(|diagnostic| self.sources.render(diagnostic))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })?;
        reject_nested_imports(&program, &source_name)?;
        let mut external_provenance = BTreeMap::new();
        let mut external_roots = HashMap::new();
        let mut opaque_bindings = HashSet::new();

        for binding in &program.value.body.value.bindings {
            if binding.value.kind != BindingKind::Import {
                continue;
            }
            if external_bindings.contains_key(&binding.value.name.value) {
                return Err(ModuleError::new(format!(
                    "duplicate module binding {:?} in {source_name}",
                    binding.value.name.value
                )));
            }
            let ExprKind::String(relative) = &binding.value.value.value else {
                return Err(ModuleError::new("import path must be a string"));
            };
            if relative.starts_with("core:") {
                let (value, root) = self.load_core_module(relative)?;
                external_roots.insert(binding.value.name.value.clone(), root);
                external_bindings.insert(binding.value.name.value.clone(), value);
                continue;
            }
            let imported = path
                .parent()
                .expect("canonical module path has a parent")
                .join(relative);
            let sourced = self.load_value(&imported)?;
            let imported = canonicalize(&imported)?;
            let ModuleState::Ready { root, opaque, .. } = self
                .cache
                .get(&imported)
                .expect("loaded module has a ready cache entry");
            external_roots.insert(binding.value.name.value.clone(), *root);
            if *opaque {
                opaque_bindings.insert(binding.value.name.value.clone());
            }
            if !sourced.provenance.values.is_empty() {
                external_provenance.insert(binding.value.name.value.clone(), sourced.provenance);
            }
            external_bindings.insert(binding.value.name.value.clone(), sourced.value);
        }
        if let Some(binding) = program
            .value
            .body
            .value
            .bindings
            .iter()
            .find(|binding| binding.value.kind == BindingKind::Native)
        {
            return Err(ModuleError::new(self.sources.render(
                &crate::source::Diagnostic::error(
                    format!(
                        "native symbol {:?} is not registered for this module",
                        binding.value.name.value
                    ),
                    binding.location,
                ),
            )));
        }

        let mut dynamic_bindings = opaque_bindings;
        if is_root && external_bindings.contains_key("input") {
            dynamic_bindings.insert("input".to_owned());
        }
        let has_type_bindings = program
            .value
            .body
            .value
            .bindings
            .iter()
            .any(|binding| binding.value.kind == BindingKind::Type);
        let bootstrap_sink: Arc<dyn DebugSink> = if has_type_bindings {
            Arc::new(DiscardDebugSink)
        } else {
            Arc::clone(&self.debug_sink)
        };
        let mut bootstrap_account;
        let analysis_account = if has_type_bindings {
            bootstrap_account = QuotaAccount::new(account.quota());
            &mut bootstrap_account
        } else {
            &mut *account
        };
        let mut analysis = analyze_program_with_bindings_observed(
            &source_name,
            &program,
            analysis_account,
            &external_bindings,
            &dynamic_bindings,
            &self.sources,
            &external_provenance,
            &bootstrap_sink,
        )
        .map_err(|error| {
            error.diagnostic.as_ref().map_or_else(
                || ModuleError::new(error.to_string()),
                |diagnostic| ModuleError::new(self.sources.render(diagnostic)),
            )
        })?;
        let source_file = self.sources.get(source_id);
        let mut promoted_types = HashSet::new();
        let mut promoted_type_roots = BTreeMap::new();
        let mut erased_metadata_bindings = HashSet::new();
        if let Some(metadata) = compile_metadata_initializer(source_file, &program, &analysis)
            .map_err(|error| ModuleError::new(error.to_string()))?
        {
            erased_metadata_bindings = metadata.erased_bindings;
            let arena = Vm::new()
                .with_debug_sink(Arc::clone(&self.debug_sink))
                .execute_in_work(
                    &self.main.heap,
                    &external_roots,
                    &metadata.function,
                    &[],
                    account,
                )
                .map_err(|error| ModuleError::new(error.with_sources(&self.sources).to_string()))?;
            let metadata_root = arena
                .publish(&mut self.main.heap)
                .map_err(|error| ModuleError::new(error.to_string()))?;
            for name in metadata.type_names {
                let root = metadata_root
                    .dict_get(&self.main.heap, &name)
                    .map_err(|error| ModuleError::new(error.to_string()))?
                    .ok_or_else(|| {
                        ModuleError::new(format!("metadata initializer omitted type root {name:?}"))
                    })?;
                external_roots.insert(type_link_key(&name), root);
                promoted_type_roots.insert(name.clone(), root);
                promoted_types.insert(name);
            }
            analysis
                .install_promoted_types(&self.main.heap, &promoted_type_roots)
                .map_err(ModuleError::new)?;
        }
        let function = if promoted_types.is_empty() {
            compile_program_analyzed_in(source_file, &program, &analysis)
        } else {
            compile_program_with_promoted_types(
                source_file,
                &program,
                &analysis,
                &promoted_types,
                &erased_metadata_bindings,
            )
        }
        .map_err(|error| ModuleError::new(error.to_string()))?;
        Ok((analysis, function, external_roots))
    }

    fn load_core_module(&mut self, name: &str) -> Result<(Value, PersistentValue), ModuleError> {
        self.core_modules
            .get(name)
            .map(|(value, root)| (value.clone(), *root))
            .ok_or_else(|| ModuleError::new(format!("unknown core module {name:?}")))
    }

    fn enter(&mut self, path: &Path) -> Result<(), ModuleError> {
        if let Some(index) = self.visiting.iter().position(|candidate| candidate == path) {
            let mut cycle = self.visiting[index..]
                .iter()
                .map(|item| item.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(path.display().to_string());
            return Err(ModuleError::new(format!(
                "module import cycle: {}",
                cycle.join(" -> ")
            )));
        }
        self.visiting.push(path.to_owned());
        self.dependencies.insert(path.to_owned());
        Ok(())
    }

    fn leave(&mut self, path: &Path) {
        let popped = self.visiting.pop();
        debug_assert_eq!(popped.as_deref(), Some(path));
    }
}

fn reject_nested_imports(program: &Program, source_name: &str) -> Result<(), ModuleError> {
    for binding in &program.value.body.value.bindings {
        if matches!(
            binding.value.kind,
            BindingKind::Let | BindingKind::Def | BindingKind::NamedFunction
        ) && expression_has_import(&binding.value.value)
        {
            return Err(ModuleError::new(format!(
                "{source_name}: imports and native declarations are only allowed at module top level"
            )));
        }
    }
    if expression_has_import(&program.value.body.value.result) {
        return Err(ModuleError::new(format!(
            "{source_name}: imports and native declarations are only allowed at module top level"
        )));
    }
    Ok(())
}

fn expression_has_import(expression: &Expr) -> bool {
    match &expression.value {
        ExprKind::Block(block) => {
            block.value.bindings.iter().any(|binding| {
                matches!(
                    binding.value.kind,
                    BindingKind::Import | BindingKind::Native
                )
            }) || block
                .value
                .bindings
                .iter()
                .any(|binding| expression_has_import(&binding.value.value))
                || expression_has_import(&block.value.result)
        }
        ExprKind::Array(items) | ExprKind::Tuple(items) => items.iter().any(expression_has_import),
        ExprKind::InterpolatedString(parts) => parts.iter().any(|part| match &part.value {
            StringPartKind::Text(_) => false,
            StringPartKind::Expression(expression) => expression_has_import(expression),
        }),
        ExprKind::Dict(fields) => fields
            .iter()
            .any(|field| expression_has_import(&field.value.value)),
        ExprKind::Unary { operand, .. } => expression_has_import(operand),
        ExprKind::Binary { left, right, .. } => {
            expression_has_import(left) || expression_has_import(right)
        }
        ExprKind::Field { receiver, .. } => expression_has_import(receiver),
        ExprKind::Call { callee, arguments } => {
            expression_has_import(callee) || arguments.iter().any(expression_has_import)
        }
        ExprKind::Closure { body, .. } => {
            body.value
                .bindings
                .iter()
                .any(|binding| binding.value.kind == BindingKind::Import)
                || body
                    .value
                    .bindings
                    .iter()
                    .any(|binding| expression_has_import(&binding.value.value))
                || expression_has_import(&body.value.result)
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression_has_import(condition)
                || then_branch
                    .value
                    .bindings
                    .iter()
                    .chain(&else_branch.value.bindings)
                    .any(|binding| {
                        binding.value.kind == BindingKind::Import
                            || expression_has_import(&binding.value.value)
                    })
                || expression_has_import(&then_branch.value.result)
                || expression_has_import(&else_branch.value.result)
        }
        ExprKind::Match { value, arms } => {
            expression_has_import(value)
                || arms
                    .iter()
                    .any(|arm| expression_has_import(&arm.value.value))
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bytes(_)
        | ExprKind::Atom(_)
        | ExprKind::Variable(_) => false,
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf, ModuleError> {
    fs::canonicalize(path).map_err(|error| {
        ModuleError::new(format!("cannot resolve module {}: {error}", path.display()))
    })
}

fn read(path: &Path) -> Result<String, ModuleError> {
    fs::read_to_string(path).map_err(|error| {
        ModuleError::new(format!("cannot read module {}: {error}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_json;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("xl-module-test-{unique}"));
        fs::create_dir(&path).unwrap();
        path
    }

    #[derive(Default)]
    struct CapturingDebugSink {
        events: Mutex<Vec<crate::DebugEvent>>,
    }

    impl crate::DebugSink for CapturingDebugSink {
        fn emit(&self, event: crate::DebugEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn core_debug_observes_values_without_changing_results() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import debug from "core:debug";
               let identity = fn(value) { value };
               let data = { text: "line\nnext", items: [1, 'Ok, (2,)] };
               let observed = debug.dbg_with("loaded\nvalue", data);
               let seen_identity = debug.dbg(identity);
               let seen_value = debug.dbg(observed);
               if seen_identity == identity { seen_value } else { data }"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        let engine = Engine::new(EngineConfig {
            module_quota: Quota::with_fuel(100_000),
            session_quota: Quota::with_fuel(100_000),
        })
        .with_debug_sink(sink.clone());
        let module = engine
            .load_module(directory.join("main.xl"), BTreeMap::new())
            .unwrap();
        assert_eq!(
            engine.execute(&module).unwrap().to_string(),
            "{items: [1, 'Ok, (2)], text: \"line\\nnext\"}"
        );
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 6);
        for phase in events.chunks_exact(3) {
            assert_eq!(phase[0].label.as_deref(), Some("loaded\nvalue"));
            assert_eq!(
                phase[0].value,
                "{\"items\": [1, 'Ok, (2,)], \"text\": \"line\\nnext\"}"
            );
            assert!(phase[1].value.starts_with("<fn "));
            assert_eq!(phase[2].value, phase[0].value);
        }
        drop(events);

        fs::write(
            directory.join("bad-label.xl"),
            r#"import debug from "core:debug"; debug.dbg_with(1, 42)"#,
        )
        .unwrap();
        let bad = engine
            .load_module(directory.join("bad-label.xl"), BTreeMap::new())
            .unwrap()
            .execute(100_000)
            .unwrap_err();
        assert!(bad.message.contains("String"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_debug_uses_one_fuel_no_xl_allocation_and_observes_module_init() {
        let directory = fixture_dir();
        fs::write(directory.join("data.json"), r#"{"value":42}"#).unwrap();
        fs::write(
            directory.join("dependency.xl"),
            r#"import debug from "core:debug"; debug.dbg_with("tool", 41)"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import debug from "core:debug";
               import dependency from "./dependency.xl";
               import data from "./data.json";
               type Observed = debug.dbg(Int);
               debug.dbg(data)"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        let engine = Engine::new(EngineConfig {
            module_quota: Quota::with_fuel(100_000),
            session_quota: Quota::with_fuel(100_000),
        })
        .with_debug_sink(sink.clone());
        let module = engine
            .load_module(directory.join("main.xl"), BTreeMap::new())
            .unwrap();
        {
            let events = sink.events.lock().unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].label.as_deref(), Some("tool"));
            assert!(events[1].value.contains("\"kind\": 'Int"));
        }

        let mut exact = QuotaAccount::new(Quota::new(1, 1_000, 0));
        let arena = Vm::new()
            .with_debug_sink(sink.clone())
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut exact,
            )
            .unwrap();
        assert_eq!(exact.requested_allocation_bytes(), 0);
        assert_eq!(
            arena.export(&module.runtime.main.heap).unwrap().to_string(),
            "{value: 42}"
        );
        assert_eq!(sink.events.lock().unwrap().len(), 3);

        let mut second = QuotaAccount::new(Quota::new(1, 1_000, 0));
        Vm::new()
            .with_debug_sink(sink.clone())
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut second,
            )
            .unwrap();
        assert_eq!(
            sink.events.lock().unwrap().len(),
            4,
            "the type RHS must not execute again in a later session"
        );

        let mut no_fuel = QuotaAccount::new(Quota::new(0, 1_000, 0));
        assert_eq!(
            Vm::new()
                .with_debug_sink(sink)
                .execute_in_work(
                    &module.runtime.main.heap,
                    &module.runtime.externals,
                    &module.function,
                    &[],
                    &mut no_fuel,
                )
                .err()
                .expect("debug call must consume fuel")
                .kind,
            crate::RuntimeErrorKind::FuelExhausted
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn metadata_only_helpers_are_erased_but_runtime_helpers_are_retained() {
        let directory = fixture_dir();
        fs::write(
            directory.join("erased.xl"),
            r#"import debug from "core:debug";
               fn observe(value) { debug.dbg_with("metadata", value) }
               type Observed = observe(Int);
               0"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        let erased = load_module_with_quota_and_debug_sink(
            directory.join("erased.xl"),
            BTreeMap::new(),
            Quota::with_fuel(100_000),
            sink.clone(),
        )
        .unwrap();
        assert_eq!(sink.events.lock().unwrap().len(), 1);
        assert_eq!(
            erased
                .execute_with_quota(Quota::new(0, 1_000, 0))
                .unwrap()
                .to_string(),
            "0"
        );
        assert_eq!(sink.events.lock().unwrap().len(), 1);

        fs::write(
            directory.join("retained.xl"),
            r#"import debug from "core:debug";
               fn observe(value) { debug.dbg_with("observed", value) }
               type Observed = observe(Int);
               observe(1)"#,
        )
        .unwrap();
        let retained = load_module_with_quota_and_debug_sink(
            directory.join("retained.xl"),
            BTreeMap::new(),
            Quota::with_fuel(100_000),
            sink.clone(),
        )
        .unwrap();
        assert_eq!(sink.events.lock().unwrap().len(), 2);
        retained
            .execute_with_quota_and_debug_sink(Quota::with_fuel(2), sink.clone())
            .unwrap();
        assert_eq!(sink.events.lock().unwrap().len(), 3);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bootstrap_shadow_does_not_consume_the_module_initialization_quota() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import debug from "core:debug";
               type Observed = debug.dbg(Int);
               0"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        load_module_with_quota_and_debug_sink(
            directory.join("main.xl"),
            BTreeMap::new(),
            Quota::new(1, 1_000, u64::MAX),
            sink.clone(),
        )
        .unwrap();
        assert_eq!(
            sink.events.lock().unwrap().len(),
            1,
            "only authoritative MetadataInit is observable and charged"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn derived_codec_normalizes_options_and_pretty_prints_json() {
        let directory = fixture_dir();
        fs::write(
            directory.join("User.xl"),
            r#"import codec from "core:codec";
               import result from "core:result";
               @struct type Type = {v: Option(String)};
               let decode = fn(value) { codec.decode(Type, value) };
               let encode = fn(value) {
                   codec.encode(Type, value) |> result.unwrap
               };
               {Type: Type, decode: decode, encode: encode}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import data from "./abc.json";
               import User from "./User.xl";
               import result from "core:result";
               import json from "core:json";
               let user = data |> User.decode |> result.unwrap;
               user |> User.encode |> json.stringify_pretty(2)"#,
        )
        .unwrap();

        let expected = [
            (r#"{"v":"abc"}"#, "{\n  \"v\": \"abc\"\n}"),
            (r#"{"v":null}"#, "{\n  \"v\": null\n}"),
            (r#"{}"#, "{\n  \"v\": null\n}"),
        ];
        for (source, output) in expected {
            fs::write(directory.join("abc.json"), source).unwrap();
            let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000)
                .unwrap_or_else(|error| panic!("failed to load {source}: {error}"));
            assert_eq!(
                module.execute(100_000).unwrap().to_string(),
                format!("{output:?}")
            );
        }

        fs::write(directory.join("abc.json"), r#"{"v":1}"#).unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let failure = module.execute(100_000).unwrap_err();
        assert!(failure.message.contains("$.v"), "{}", failure.message);
        assert!(failure.message.contains("String"), "{}", failure.message);
        let data_location = failure
            .data_location()
            .expect("codec failure must retain the invalid JSON value location");
        assert_eq!(
            module.sources.get(data_location.source).name.as_ref(),
            directory.join("abc.json").display().to_string()
        );
        assert_eq!(
            module
                .sources
                .get(data_location.source)
                .slice(data_location),
            Some("1")
        );
        let rendered = failure.to_string();
        assert!(rendered.contains("abc.json:1:6:"), "{rendered}");
        assert!(
            rendered.contains("contract rule declared here"),
            "{rendered}"
        );
        assert!(rendered.contains("User.xl:3:47:"), "{rendered}");

        fs::write(
            directory.join("inspect.xl"),
            r#"import data from "./abc.json";
               import User from "./User.xl";
               data |> User.decode"#,
        )
        .unwrap();
        let inspected = load_module(directory.join("inspect.xl"), BTreeMap::new(), 100_000)
            .unwrap()
            .execute(100_000)
            .unwrap();
        let Value::Tuple(result) = inspected else {
            panic!("codec must return a tagged Result")
        };
        assert_eq!(result[0].to_string(), "'Err");
        let Value::Dict(payload) = &result[1] else {
            panic!("codec failure must be an ordinary diagnostic Dict")
        };
        assert!(payload.get("message").is_some());
        assert_eq!(payload.get("data").unwrap().to_string(), "1");
        assert_eq!(payload.get("rule").unwrap().to_string(), "{kind: 'String}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn codec_accepts_user_computed_canonical_type_metadata() {
        let directory = fixture_dir();
        fs::write(directory.join("data.json"), r#"{"v":"plain"}"#).unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import data from "./data.json";
               import codec from "core:codec";
               import result from "core:result";
               type StringRule = {kind: 'String};
               type UserRule = {kind: 'Struct, fields: {v: StringRule}};
               codec.decode(UserRule, data) |> result.unwrap"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "{v: \"plain\"}"
        );

        fs::write(
            directory.join("legacy.xl"),
            r#"import result from "core:result"; result.unwrap(('Err, "legacy"))"#,
        )
        .unwrap();
        let legacy = load_module(directory.join("legacy.xl"), BTreeMap::new(), 100_000)
            .unwrap()
            .execute(100_000)
            .unwrap_err();
        assert_eq!(legacy.message, "legacy");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn codec_rejects_struct_shape_errors_and_json_is_strict() {
        let directory = fixture_dir();
        let cases = [
            (
                r#"import codec from "core:codec";
                   import result from "core:result";
                   @struct type T = {name: String};
                   codec.decode(T, {}) |> result.unwrap"#,
                "$.name: missing required field",
            ),
            (
                r#"import codec from "core:codec";
                   import result from "core:result";
                   @struct type T = {name: String};
                   codec.decode(T, {name: "Ada", extra: 1}) |> result.unwrap"#,
                "$.extra: unknown field",
            ),
            (
                r#"import json from "core:json"; json.stringify((1, 2))"#,
                "JSON cannot encode Tuple",
            ),
            (
                r#"import json from "core:json"; json.stringify_pretty(17)"#,
                "indent must be between 0 and 16",
            ),
        ];
        for (index, (source, expected)) in cases.into_iter().enumerate() {
            let path = directory.join(format!("case-{index}.xl"));
            fs::write(&path, source).unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            let failure = module.execute(100_000).unwrap_err();
            assert!(failure.message.contains(expected), "{}", failure.message);
        }

        let path = directory.join("compact.xl");
        fs::write(
            &path,
            r#"import json from "core:json";
               json.stringify({z: [1, 'True], a: "line\nnext"})"#,
        )
        .unwrap();
        let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            r#""{\"a\":\"line\\nnext\",\"z\":[1,true]}""#
        );
        assert_eq!(
            module
                .execute_with_quota(Quota::new(100_000, 1_000, 1))
                .unwrap_err()
                .kind,
            crate::RuntimeErrorKind::AllocationQuotaExceeded
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_json_and_xl_modules_with_types() {
        let directory = fixture_dir();
        fs::write(directory.join("user.json"), r#"{"name":"Ada","age":36}"#).unwrap();
        fs::write(directory.join("answer.xl"), "40 + 2").unwrap();
        fs::write(
            directory.join("main.xl"),
            "import user from \"./user.json\";\
             import answer from \"./answer.xl\";\
             @struct type User = {name: String, age: Int};\
             let checked: User = user;\
             (checked.name, answer)",
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(module.dependencies.len(), 3);
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "(\"Ada\", 42)"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_module_cycles() {
        let directory = fixture_dir();
        fs::write(directory.join("a.xl"), "import b from \"./b.xl\"; b").unwrap();
        fs::write(directory.join("b.xl"), "import a from \"./a.xl\"; a").unwrap();
        let error = load_module(directory.join("a.xl"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.message().contains("cycle"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unregistered_and_nested_native_declarations_with_locations() {
        let directory = fixture_dir();
        fs::write(
            directory.join("missing-native.xl"),
            "native missing: fn(Int) -> Int; missing(1)",
        )
        .unwrap();
        let missing = load_module(
            directory.join("missing-native.xl"),
            BTreeMap::new(),
            100_000,
        )
        .unwrap_err();
        assert!(missing.message().contains("not registered"));
        assert!(missing.to_string().contains("missing-native.xl:1:1"));

        fs::write(
            directory.join("nested-native.xl"),
            "let value = { native hidden: fn(Int) -> Int; 1 }; value",
        )
        .unwrap();
        let nested =
            load_module(directory.join("nested-native.xl"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(
            nested
                .message()
                .contains("only allowed at module top level")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn imports_recursive_function_roots_from_the_persistent_world() {
        let directory = fixture_dir();
        fs::write(
            directory.join("countdown.xl"),
            "fn countdown(n) { if n < 1 { 0 } else { countdown(n - 1) } } countdown",
        )
        .unwrap();
        fs::write(
            directory.join("main.xl"),
            "import countdown from \"./countdown.xl\"; countdown(4)",
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(module.execute(100_000).unwrap().to_string(), "0");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn external_input_is_any_and_available_at_runtime() {
        let directory = fixture_dir();
        fs::write(directory.join("main.xl"), "input").unwrap();
        let input = parse_json("input", r#"{"value":42}"#).unwrap();
        let module = load_module(
            directory.join("main.xl"),
            BTreeMap::from([("input".into(), input)]),
            100_000,
        )
        .unwrap();
        assert_eq!(
            module
                .analysis
                .types
                .node(module.analysis.binding_types["input"]),
            &crate::TypeNode::Any
        );
        assert_eq!(module.execute(100_000).unwrap().to_string(), "{value: 42}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn annotation_error_labels_json_data_and_xl_type_declaration() {
        let directory = fixture_dir();
        fs::write(directory.join("user.json"), r#"{"name":"Ada","age":"old"}"#).unwrap();
        fs::write(
            directory.join("main.xl"),
            "import user from \"./user.json\";\n\
             @struct type User = {name: String, age: Int};\n\
             let checked: User = user;\n\
             checked",
        )
        .unwrap();
        let error = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap_err();
        let message = error.message();
        assert!(
            message.contains("user.json:1:21: binding checked has type"),
            "{message}"
        );
        assert!(
            message.contains("main.xl:2:1: type requirement declared here"),
            "{message}"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn module_execution_uses_evaluation_fuel_semantics() {
        let directory = fixture_dir();
        fs::write(directory.join("straight.xl"), "40 + 2").unwrap();
        let straight = load_module(directory.join("straight.xl"), BTreeMap::new(), 0).unwrap();
        assert_eq!(straight.execute(0).unwrap().to_string(), "42");

        fs::write(
            directory.join("call.xl"),
            "let identity = fn(value) { value }; identity(42)",
        )
        .unwrap();
        let call = load_module(directory.join("call.xl"), BTreeMap::new(), 0).unwrap();
        assert_eq!(
            call.execute(0).unwrap_err().kind,
            crate::RuntimeErrorKind::FuelExhausted
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn engine_applies_module_and_session_quotas_at_separate_boundaries() {
        let directory = fixture_dir();
        fs::write(
            directory.join("typed.xl"),
            "type First = Array(Int); type Second = Array(Int); 0",
        )
        .unwrap();
        let module_limited = Engine::new(EngineConfig {
            module_quota: Quota::new(1, 1_000, u64::MAX),
            session_quota: Quota::new(100, 1_000, u64::MAX),
        });
        let error = module_limited
            .load_module(directory.join("typed.xl"), BTreeMap::new())
            .unwrap_err();
        assert!(error.message().contains("fuel"));

        fs::write(directory.join("value.xl"), "[1]").unwrap();
        let session_limited = Engine::new(EngineConfig {
            module_quota: Quota::new(100, 1_000, u64::MAX),
            session_quota: Quota::new(100, 1_000, 0),
        });
        let module = session_limited
            .load_module(directory.join("value.xl"), BTreeMap::new())
            .unwrap();
        assert_eq!(
            session_limited.execute(&module).unwrap_err().kind,
            crate::RuntimeErrorKind::AllocationQuotaExceeded
        );
        assert_eq!(
            session_limited.execute(&module).unwrap_err().kind,
            crate::RuntimeErrorKind::AllocationQuotaExceeded
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ready_module_root_is_promoted_once_into_the_shared_world() {
        let directory = fixture_dir();
        let data = directory.join("data.json");
        fs::write(&data, r#"{"name":"Ada","items":[1,2,3]}"#).unwrap();
        let mut loader = ModuleLoader {
            cache: HashMap::new(),
            core_modules: HashMap::new(),
            main: MainWorld::building(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
        };

        let first = loader.load_value(&data).unwrap();
        let counts = loader.main.heap.counts();
        let first_root = match loader.cache.get(&canonicalize(&data).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };
        let second = loader.load_value(&data).unwrap();
        let second_root = match loader.cache.get(&canonicalize(&data).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };

        assert_eq!(first.value.to_string(), second.value.to_string());
        assert_eq!(first_root, second_root);
        assert_eq!(counts, loader.main.heap.counts());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn sessions_use_fresh_work_worlds_and_leave_frozen_main_unchanged() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import arrays from "core:array"; arrays.map([1, 2], fn(x) { x + 1 })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let main_counts = module.runtime.main.heap.counts();
        assert!(main_counts.0 > 0, "core modules must be installed in Main");

        assert_eq!(module.execute(100_000).unwrap().to_string(), "[2, 3]");
        assert_eq!(module.execute(100_000).unwrap().to_string(), "[2, 3]");
        assert_eq!(module.runtime.main.heap.counts(), main_counts);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_array_module_runs_higher_order_operations_and_nested_callbacks() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import arrays from "core:array";
               let values = [1, 2, 3];
               {
                   length: arrays.length(values),
                   mapped: arrays.map(values, fn(value) { value + 10 }),
                   filtered: arrays.filter(values, fn(value) { 1 < value }),
                   flattened: arrays.flat_map(values, fn(value) { [value, value] }),
                   folded: arrays.fold(values, 0, fn(total, value) { total + value }),
                   empty_map: arrays.map([], fn(value) { value / 0 }),
                   empty_filter: arrays.filter([], fn(value) { value }),
                   empty_flat_map: arrays.flat_map([], fn(value) { value }),
                   empty_fold: arrays.fold([], 42, fn(total, value) { total + value }),
                   nested: arrays.map(values, fn(value) {
                       arrays.fold([value, value], 0, fn(total, item) { total + item })
                   }),
                   native_callback: arrays.map([Int], Array),
                   pipelined: values |> arrays.map\(_, fn(value) { value + 20 }),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let value = module.execute(100_000).unwrap();
        let Value::Dict(result) = value else {
            panic!("expected Dict result")
        };
        assert_eq!(result.get("length").unwrap().to_string(), "3");
        assert_eq!(result.get("mapped").unwrap().to_string(), "[11, 12, 13]");
        assert_eq!(result.get("filtered").unwrap().to_string(), "[2, 3]");
        assert_eq!(
            result.get("flattened").unwrap().to_string(),
            "[1, 1, 2, 2, 3, 3]"
        );
        assert_eq!(result.get("folded").unwrap().to_string(), "6");
        assert_eq!(result.get("empty_map").unwrap().to_string(), "[]");
        assert_eq!(result.get("empty_filter").unwrap().to_string(), "[]");
        assert_eq!(result.get("empty_flat_map").unwrap().to_string(), "[]");
        assert_eq!(result.get("empty_fold").unwrap().to_string(), "42");
        assert_eq!(result.get("nested").unwrap().to_string(), "[2, 4, 6]");
        assert_eq!(result.get("pipelined").unwrap().to_string(), "[21, 22, 23]");
        let Value::Array(native) = result.get("native_callback").unwrap() else {
            panic!("expected native callback Array")
        };
        assert_eq!(native.len(), 1);
        assert!(matches!(native[0], Value::Dict(_)));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_array_callbacks_share_fuel_allocation_and_tool_stage_execution() {
        let directory = fixture_dir();
        let item_count = 1_500usize;
        let data = format!("[{}]", vec!["1"; item_count].join(","));
        fs::write(directory.join("values.json"), data).unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import arrays from "core:array";
               import values from "./values.json";
               arrays.map(values, fn(value) { value + 1 })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();

        let mut exact = QuotaAccount::new(Quota::new(1_501, 1_000, u64::MAX));
        let arena = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut exact,
            )
            .unwrap();
        assert_eq!(
            exact.requested_allocation_bytes(),
            item_count as u64 * std::mem::size_of::<Value>() as u64
        );
        let Value::Array(mapped) = arena.export(&module.runtime.main.heap).unwrap() else {
            panic!("expected mapped Array")
        };
        assert_eq!(mapped.len(), item_count);

        let mut fuel_short = QuotaAccount::new(Quota::new(1_500, 1_000, u64::MAX));
        assert_eq!(
            Vm::new()
                .execute_in_work(
                    &module.runtime.main.heap,
                    &module.runtime.externals,
                    &module.function,
                    &[],
                    &mut fuel_short,
                )
                .err()
                .expect("fuel must be exhausted")
                .kind,
            crate::RuntimeErrorKind::FuelExhausted
        );

        let requested = item_count as u64 * std::mem::size_of::<Value>() as u64;
        let mut allocation_short = QuotaAccount::new(Quota::new(1_501, 1_000, requested - 1));
        assert_eq!(
            Vm::new()
                .execute_in_work(
                    &module.runtime.main.heap,
                    &module.runtime.externals,
                    &module.function,
                    &[],
                    &mut allocation_short,
                )
                .err()
                .expect("allocation must be exhausted")
                .kind,
            crate::RuntimeErrorKind::AllocationQuotaExceeded
        );

        fs::write(
            directory.join("types.xl"),
            r#"import arrays from "core:array";
               type Pair = Tuple(arrays.map([Int, String], fn(item) { item }));
               let pair: Pair = (1, "one");
               pair"#,
        )
        .unwrap();
        let types = load_module(directory.join("types.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(types.execute(100_000).unwrap().to_string(), "(1, \"one\")");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_array_reports_boundary_and_callback_result_errors() {
        let directory = fixture_dir();
        let run_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(
                &path,
                format!("import arrays from \"core:array\"; {expression}"),
            )
            .unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            module.execute(100_000).unwrap_err()
        };

        assert!(
            run_error("length.xl", "arrays.length(1)")
                .message
                .contains("Array")
        );
        assert!(
            run_error("arity.xl", "arrays.map([1], fn(a, b) { a + b })")
                .message
                .contains("callback must accept 1")
        );
        assert!(
            run_error("filter.xl", "arrays.filter([1], fn(value) { value })")
                .message
                .contains("must return 'True or 'False")
        );
        assert!(
            run_error("flat-map.xl", "arrays.flat_map([1], fn(value) { value })")
                .message
                .contains("must return an Array")
        );
        let callback = run_error("callback.xl", "arrays.map([1], fn(value) { value / 0 })");
        assert!(callback.to_string().contains("callback.xl:1:"));
        assert!(
            callback
                .trace
                .iter()
                .any(|frame| frame.function == "core:array.map")
        );

        let nested_depth = run_error(
            "nested-depth.xl",
            "decl nest: fn(Int) -> Int;
             def nest = fn(n) {
                 if n < 1 { 0 } else {
                     arrays.fold([n], 0, fn(total, value) { nest(value - 1) })
                 }
             };
             nest(1100)",
        );
        assert_eq!(
            nested_depth.kind,
            crate::RuntimeErrorKind::CallDepthExceeded
        );

        let unknown_path = directory.join("unknown-core.xl");
        fs::write(
            &unknown_path,
            "import unknown from \"core:unknown\"; unknown",
        )
        .unwrap();
        assert!(
            load_module(unknown_path, BTreeMap::new(), 100_000)
                .unwrap_err()
                .to_string()
                .contains("unknown core module")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_dict_enumerates_constructs_and_merges_in_canonical_order() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import dicts from "core:dict";
               let source = { z: 3, a: 1, middle: 2 };
               {
                   keys: dicts.keys(source),
                   values: dicts.values(source),
                   pairs: dicts.pairs(source),
                   round_trip: dicts.from_pairs(dicts.pairs(source)),
                   merged: dicts.merge(
                       { a: 1, nested: { left: 1 } },
                       { b: 2, nested: { right: 2 } },
                   ),
                   empty_keys: dicts.keys({}),
                   empty_pairs: dicts.pairs({}),
                   empty_from_pairs: dicts.from_pairs([]),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert_eq!(
            result.get("keys").unwrap().to_string(),
            "[\"a\", \"middle\", \"z\"]"
        );
        assert_eq!(result.get("values").unwrap().to_string(), "[1, 2, 3]");
        assert_eq!(
            result.get("pairs").unwrap().to_string(),
            "[(\"a\", 1), (\"middle\", 2), (\"z\", 3)]"
        );
        assert_eq!(
            result.get("round_trip").unwrap().to_string(),
            "{a: 1, middle: 2, z: 3}"
        );
        assert_eq!(
            result.get("merged").unwrap().to_string(),
            "{a: 1, b: 2, nested: {right: 2}}"
        );
        assert_eq!(result.get("empty_keys").unwrap().to_string(), "[]");
        assert_eq!(result.get("empty_pairs").unwrap().to_string(), "[]");
        assert_eq!(result.get("empty_from_pairs").unwrap().to_string(), "{}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_dict_supports_tool_stage_and_exact_output_quota() {
        let directory = fixture_dir();
        fs::write(directory.join("data.json"), r#"{"a":1,"long":2}"#).unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import dicts from "core:dict";
               import data from "./data.json";
               dicts.keys(data)"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let requested = 2 * std::mem::size_of::<Value>() as u64 + 5;
        let mut exact = QuotaAccount::new(Quota::new(1, 1_000, requested));
        let arena = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut exact,
            )
            .unwrap();
        assert_eq!(exact.requested_allocation_bytes(), requested);
        assert_eq!(
            arena.export(&module.runtime.main.heap).unwrap().to_string(),
            "[\"a\", \"long\"]"
        );

        let mut short = QuotaAccount::new(Quota::new(1, 1_000, requested - 1));
        assert_eq!(
            Vm::new()
                .execute_in_work(
                    &module.runtime.main.heap,
                    &module.runtime.externals,
                    &module.function,
                    &[],
                    &mut short,
                )
                .err()
                .expect("allocation must be exhausted")
                .kind,
            crate::RuntimeErrorKind::AllocationQuotaExceeded
        );

        fs::write(
            directory.join("types.xl"),
            r#"import dicts from "core:dict";
               type Pair = Tuple(dicts.values({ first: Int, second: String }));
               let pair: Pair = (1, "one");
               pair"#,
        )
        .unwrap();
        let types = load_module(directory.join("types.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(types.execute(100_000).unwrap().to_string(), "(1, \"one\")");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_attributes_normalizes_flattens_and_inspects_arbitrary_values() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import attributes from "core:attributes";
               let nested = {
                   kind: 'WithAttributes,
                   inner: {
                       kind: 'WithAttributes,
                       inner: 42,
                       attributes: { shared: "inner", only_inner: 1 },
                   },
                   attributes: { shared: "outer", only_outer: 2 },
               };
               let augmented = attributes.add(
                   nested,
                   { shared: "addition", "vendor:acme.flag": 'True },
               );
               {
                   normalized: attributes.normalize(nested),
                   all: attributes.all(augmented),
                   shared: attributes.get(augmented, "shared"),
                   missing: attributes.get(augmented, "missing"),
                   has: attributes.has(augmented, "vendor:acme.flag"),
                   lacks: attributes.has(augmented, "missing"),
                   stripped: attributes.strip(augmented),
                   plain: attributes.normalize("plain"),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert_eq!(
            result.get("all").unwrap().to_string(),
            "{only_inner: 1, only_outer: 2, shared: \"addition\", vendor:acme.flag: 'True}"
        );
        assert_eq!(
            result.get("shared").unwrap().to_string(),
            "('Some, \"addition\")"
        );
        assert_eq!(result.get("missing").unwrap().to_string(), "'None");
        assert_eq!(result.get("has").unwrap().to_string(), "'True");
        assert_eq!(result.get("lacks").unwrap().to_string(), "'False");
        assert_eq!(result.get("stripped").unwrap().to_string(), "42");

        let Value::Dict(normalized) = result.get("normalized").unwrap() else {
            panic!("expected normalized wrapper")
        };
        assert_eq!(
            normalized.get("attributes").unwrap().to_string(),
            "{only_inner: 1, only_outer: 2, shared: \"outer\"}"
        );
        assert_eq!(normalized.get("inner").unwrap().to_string(), "42");
        let Value::Dict(plain) = result.get("plain").unwrap() else {
            panic!("expected plain wrapper")
        };
        assert_eq!(plain.get("attributes").unwrap().to_string(), "{}");
        assert_eq!(plain.get("inner").unwrap().to_string(), "\"plain\"");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn attributed_type_metadata_is_transparent_and_preserved() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import attributes from "core:attributes";
               import codec from "core:codec";
               let rename = fn(name) {
                   fn(ctx, value) {
                       attributes.add(value, { "core:json.rename": name })
                   }
               };
               let model = fn(ctx, value) {
                   attributes.add(struct(ctx, value), { "vendor:acme.model": ctx.name })
               };
               @model
               type User = {
                   @rename("type")
                   ty: String,
               };
               let user: User = { ty: "admin" };
               {
                   metadata: User,
                   checked: validate(User, user),
                   decoded: codec.decode(User, { "type": "member" }),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert!(
            result
                .get("checked")
                .unwrap()
                .to_string()
                .starts_with("('Ok,")
        );
        assert!(
            result
                .get("decoded")
                .unwrap()
                .to_string()
                .starts_with("('Ok,")
        );

        let Value::Dict(metadata) = result.get("metadata").unwrap() else {
            panic!("expected attributed type metadata")
        };
        assert_eq!(metadata.get("kind").unwrap().to_string(), "'WithAttributes");
        let Value::Dict(model_attributes) = metadata.get("attributes").unwrap() else {
            panic!("expected model attributes")
        };
        assert_eq!(
            model_attributes
                .get("vendor:acme.model")
                .unwrap()
                .to_string(),
            "\"User\""
        );
        let Value::Dict(struct_metadata) = metadata.get("inner").unwrap() else {
            panic!("expected Struct metadata")
        };
        let Value::Dict(fields) = struct_metadata.get("fields").unwrap() else {
            panic!("expected Struct fields")
        };
        let Value::Dict(field) = fields.get("ty").unwrap() else {
            panic!("expected attributed field metadata")
        };
        assert_eq!(field.get("kind").unwrap().to_string(), "'WithAttributes");
        let Value::Dict(field_attributes) = field.get("attributes").unwrap() else {
            panic!("expected field attributes")
        };
        assert_eq!(
            field_attributes
                .get("core:json.rename")
                .unwrap()
                .to_string(),
            "\"type\""
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn normalized_struct_and_enum_models_preserve_uniform_member_attributes() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import attributes from "core:attributes";
               let annotate = fn(key, payload) {
                   fn(ctx, value) { attributes.add(value, { marker: (key, payload) }) }
               };

               @annotate("model", 1)
               @struct
               type User = {
                   name: String,
                   @annotate("field", 2)
                   role: String,
               };

               @annotate("enum", 3)
               @enum
               type Choice = {
                   None: 'None,
                   User: User,
               };

               @union
               type Scalar = [
                   attributes.add(Int, { marker: ("union", 4) }),
                   String,
               ];

               let explicit = struct('None, { value: Int });
               let explicit_union = union('None, [Int, String]);
               let unit: Choice = 'None;
               let payload: Choice = ('User, { name: "Ada", role: "admin" });
               let scalar_value: Scalar = 42;
               {
                   user: User,
                   choice: Choice,
                   explicit: explicit,
                   explicit_union: explicit_union,
                   scalar: Scalar,
                   scalar_value: scalar_value,
                   unit: validate(Choice, unit),
                   payload: validate(Choice, payload),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected model result")
        };
        assert!(result.get("unit").unwrap().to_string().starts_with("('Ok,"));
        assert!(
            result
                .get("payload")
                .unwrap()
                .to_string()
                .starts_with("('Ok,")
        );

        fn assert_wrapper(value: &Value) -> &crate::Dict {
            let Value::Dict(wrapper) = value else {
                panic!("expected WithAttributes wrapper")
            };
            assert_eq!(wrapper.get("kind").unwrap().to_string(), "'WithAttributes");
            assert!(matches!(wrapper.get("attributes"), Some(Value::Dict(_))));
            wrapper
        }
        let user = assert_wrapper(result.get("user").unwrap());
        let Value::Dict(user_metadata) = user.get("inner").unwrap() else {
            panic!("expected Struct metadata")
        };
        assert_eq!(user_metadata.get("kind").unwrap().to_string(), "'Struct");
        let Value::Dict(fields) = user_metadata.get("fields").unwrap() else {
            panic!("expected normalized fields")
        };
        let name = assert_wrapper(fields.get("name").unwrap());
        assert_eq!(name.get("attributes").unwrap().to_string(), "{}");
        let role = assert_wrapper(fields.get("role").unwrap());
        assert_eq!(
            role.get("attributes").unwrap().to_string(),
            "{marker: (\"field\", 2)}"
        );
        assert_eq!(
            user.get("attributes").unwrap().to_string(),
            "{marker: (\"model\", 1)}"
        );

        let choice = assert_wrapper(result.get("choice").unwrap());
        let Value::Dict(enum_metadata) = choice.get("inner").unwrap() else {
            panic!("expected Enum metadata")
        };
        assert_eq!(enum_metadata.get("kind").unwrap().to_string(), "'Enum");
        let Value::Dict(variants) = enum_metadata.get("variants").unwrap() else {
            panic!("expected normalized variants")
        };
        for variant in variants.values() {
            assert_wrapper(variant);
        }
        let none = assert_wrapper(variants.get("None").unwrap());
        assert_eq!(none.get("inner").unwrap().to_string(), "'None");
        assert_eq!(
            choice.get("attributes").unwrap().to_string(),
            "{marker: (\"enum\", 3)}"
        );

        let scalar = assert_wrapper(result.get("scalar").unwrap());
        let Value::Dict(union_metadata) = scalar.get("inner").unwrap() else {
            panic!("expected Union metadata")
        };
        assert_eq!(union_metadata.get("kind").unwrap().to_string(), "'Union");
        let Value::Array(union_variants) = union_metadata.get("variants").unwrap() else {
            panic!("expected normalized Union variants")
        };
        assert_eq!(union_variants.len(), 2);
        let first = assert_wrapper(&union_variants[0]);
        assert_eq!(
            first.get("attributes").unwrap().to_string(),
            "{marker: (\"union\", 4)}"
        );
        let second = assert_wrapper(&union_variants[1]);
        assert_eq!(second.get("attributes").unwrap().to_string(), "{}");

        let explicit = assert_wrapper(result.get("explicit").unwrap());
        let Value::Dict(explicit_metadata) = explicit.get("inner").unwrap() else {
            panic!("expected explicit Struct metadata")
        };
        let Value::Dict(explicit_fields) = explicit_metadata.get("fields").unwrap() else {
            panic!("expected explicit fields")
        };
        assert_wrapper(explicit_fields.get("value").unwrap());
        let explicit_union = assert_wrapper(result.get("explicit_union").unwrap());
        let Value::Dict(explicit_union_metadata) = explicit_union.get("inner").unwrap() else {
            panic!("expected explicit Union metadata")
        };
        let Value::Array(explicit_variants) = explicit_union_metadata.get("variants").unwrap()
        else {
            panic!("expected explicit Union variants")
        };
        for variant in explicit_variants.iter() {
            assert_wrapper(variant);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn enum_validation_rejects_unknown_tags_and_payload_shape_mismatches() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               @enum
               type Choice = { None: 'None, Number: Int };
               {
                   unknown: validate(Choice, 'Other),
                   missing: validate(Choice, 'Number),
                   unexpected: validate(Choice, ('None, 1)),
                   wrong: validate(Choice, ('Number, "one")),
                   codec: codec.decode(Choice, "None"),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected validation results")
        };
        for field in ["unknown", "missing", "unexpected", "wrong"] {
            assert!(result.get(field).unwrap().to_string().starts_with("('Err,"));
        }
        assert_eq!(result.get("codec").unwrap().to_string(), "('Ok, 'None)");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn enum_json_codecs_round_trip_external_and_untagged_representations() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               import result from "core:result";
               @struct type User = {name: String};
               @json.rename_all('CamelCase)
               @enum type Event = {
                   Idle: 'None,
                   UserJoined: User,
                   @json.rename("fatal") FatalError: String,
               };
               @json.untagged
               @enum type Scalar = {Text: String, Count: Int};
               @struct type Envelope = {event: Event};
               {
                   idle: codec.decode(Event, "idle") |> result.unwrap,
                   joined: codec.decode(Event, {userJoined: {name: "Ada"}}) |> result.unwrap,
                   fatal: codec.encode(Event, ('FatalError, "boom")) |> result.unwrap,
                   nested: codec.encode(Envelope, {event: ('UserJoined, {name: "Lin"})}) |> result.unwrap,
                   text: codec.decode(Scalar, "hello") |> result.unwrap,
                   count: codec.encode(Scalar, ('Count, 3)) |> result.unwrap,
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected Enum codec results")
        };
        assert_eq!(output.get("idle").unwrap().to_string(), "'Idle");
        assert_eq!(
            output.get("joined").unwrap().to_string(),
            "('UserJoined, {name: \"Ada\"})"
        );
        assert_eq!(
            output.get("fatal").unwrap().to_string(),
            "{fatal: \"boom\"}"
        );
        assert_eq!(
            output.get("nested").unwrap().to_string(),
            "{event: {userJoined: {name: \"Lin\"}}}"
        );
        assert_eq!(
            output.get("text").unwrap().to_string(),
            "('Text, \"hello\")"
        );
        assert_eq!(output.get("count").unwrap().to_string(), "3");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn untagged_enum_json_codec_rejects_no_match_and_ambiguity() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               @json.untagged @enum type Scalar = {Text: String, Count: Int};
               @json.untagged @enum type Ambiguous = {Anything: Any, Text: String};
               {
                   no_match: codec.decode(Scalar, []),
                   ambiguous: codec.decode(Ambiguous, "text"),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected failures")
        };
        assert!(
            output
                .get("no_match")
                .unwrap()
                .to_string()
                .contains("matches no untagged")
        );
        assert!(
            output
                .get("ambiguous")
                .unwrap()
                .to_string()
                .contains("ambiguously matches")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_schema_and_codecs_share_one_vertical_model_plan() {
        let directory = fixture_dir();
        fs::write(
            directory.join("data.json"),
            r#"{"userId":7,"city_name":"London","event":{"userJoined":{"name":"Ada"}},"scalar":"active","notes":""}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.xl"),
            r#"import data from "./data.json";
               import codec from "core:codec";
               import json from "core:json";
               import result from "core:result";
               @struct type User = {name: String};
               @struct type Details = {city_name: String};
               @json.rename_all('CamelCase)
               @enum type Event = {Idle: 'None, UserJoined: User};
               @json.untagged @enum type Scalar = {Text: String, Count: Int};
               @json.rename_all('CamelCase)
               @struct type Model = {
                   user_id: Int,
                   @json.flatten details: Details,
                   @json.default('None) nickname: Option(String),
                   event: Event,
                   scalar: Scalar,
                   @json.skip_serializing_if('Empty) notes: String,
               };
               let decoded = codec.decode(Model, data) |> result.unwrap;
               let schema = json.schema(Model);
               {
                   decoded: decoded,
                   encoded: codec.encode(Model, decoded) |> result.unwrap,
                   schema: schema,
                   schema_text: json.stringify(schema),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected vertical model output")
        };
        let Value::Dict(schema) = output.get("schema").unwrap() else {
            panic!("expected schema Dict")
        };
        assert_eq!(schema.get("type").unwrap().to_string(), "\"object\"");
        assert_eq!(
            schema.get("additionalProperties").unwrap().to_string(),
            "'False"
        );
        let Value::Dict(properties) = schema.get("properties").unwrap() else {
            panic!("expected properties")
        };
        for key in [
            "userId",
            "city_name",
            "nickname",
            "event",
            "scalar",
            "notes",
        ] {
            assert!(
                properties.get(key).is_some(),
                "missing schema property {key}"
            );
        }
        assert!(
            schema
                .get("required")
                .unwrap()
                .to_string()
                .contains("userId")
        );
        assert!(
            !schema
                .get("required")
                .unwrap()
                .to_string()
                .contains("nickname")
        );
        assert!(
            output
                .get("schema_text")
                .unwrap()
                .to_string()
                .contains("$schema")
        );
        assert!(!output.get("encoded").unwrap().to_string().contains("notes"));
        assert!(
            output
                .get("encoded")
                .unwrap()
                .to_string()
                .contains("userId")
        );

        fs::write(
            directory.join("data.json"),
            r#"{"userId":"wrong","city_name":"London","event":"idle","scalar":1,"notes":""}"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let failure = module.execute(100_000).unwrap_err();
        assert!(failure.message.contains("$.userId"));
        assert!(failure.data_location().is_some());
        assert!(failure.rule_location().is_some());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_schema_maps_composites_and_obeys_allocation_quota() {
        let directory = fixture_dir();
        let path = directory.join("main.xl");
        fs::write(
            &path,
            r#"import json from "core:json";
               json.schema(union('None, [Int, Array(String), {kind: 'Tuple, items: [Int, String]}]))"#,
        )
        .unwrap();
        let module = load_module(&path, BTreeMap::new(), 100_000).unwrap();
        let output = module.execute(100_000).unwrap().to_string();
        assert!(output.contains("anyOf"));
        assert!(output.contains("prefixItems"));
        assert!(output.contains("items"));

        let mut account = QuotaAccount::new(Quota::new(10, 1_000, 1));
        let error = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut account,
            )
            .err()
            .expect("schema generation must exhaust allocation quota");
        assert_eq!(error.kind, crate::RuntimeErrorKind::AllocationQuotaExceeded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn builtin_bool_and_option_keep_natural_json_codec_and_schema_forms() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               import result from "core:result";
               {
                   boolean: codec.decode(Bool, 'True) |> result.unwrap,
                   none: codec.decode(Option(Int), 'None) |> result.unwrap,
                   some: codec.decode(Option(Int), 3) |> result.unwrap,
                   encoded: codec.encode(Option(Int), ('Some, 4)) |> result.unwrap,
                   bool_schema: json.schema(Bool),
                   option_schema: json.schema(Option(Int)),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let output = module.execute(100_000).unwrap().to_string();
        assert!(output.contains("boolean: 'True"), "{output}");
        assert!(output.contains("none: 'None"), "{output}");
        assert!(output.contains("some: ('Some, 3)"), "{output}");
        assert!(output.contains("encoded: 4"), "{output}");
        assert!(output.contains("type: \"boolean\""), "{output}");
        assert!(output.contains("type: \"null\""), "{output}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recursive_type_metadata_publishes_and_drives_codecs_and_schema_refs() {
        let directory = fixture_dir();
        fs::write(
            directory.join("Types.xl"),
            r#"import json from "core:json";
               @struct type Node = {
                   value: Int,
                   children: Array(Node),
               };
               @struct type Left = {@json.rename("rightValue") right: Option(Right)};
               @struct type Right = {left: Option(Left)};
               {Node: Node, Left: Left, Right: Right}"#,
        )
        .unwrap();
        let types_module =
            load_module(directory.join("Types.xl"), BTreeMap::new(), 100_000).unwrap();
        let node = types_module.analysis.declared_types["Node"];
        let crate::TypeNode::Struct(fields) = types_module.analysis.types.node(node) else {
            panic!("Node must be a Struct in the authoritative type graph");
        };
        let crate::TypeNode::Array(children) = types_module.analysis.types.node(fields["children"])
        else {
            panic!("Node.children must be an Array");
        };
        assert_eq!(
            *children, node,
            "the recursive edge must retain TypeId identity"
        );
        assert_eq!(
            types_module.analysis.display(node),
            "{children: Array<Node>, value: Int}"
        );
        assert!(types_module.analysis.types.is_assignable(node, node));

        fs::write(
            directory.join("main.xl"),
            r#"import Types from "./Types.xl";
               import codec from "core:codec";
               import json from "core:json";
               import result from "core:result";
               let node = codec.decode(Types.Node, {
                   value: 1,
                   children: [{value: 2, children: []}],
               }) |> result.unwrap;
               let pair = codec.decode(Types.Left, {
                   rightValue: {left: 'None},
               }) |> result.unwrap;
               {
                   node: node,
                   encoded: codec.encode(Types.Node, node) |> result.unwrap,
                   pair: pair,
                   schema: json.schema(Types.Node),
                   mutual_schema: json.schema(Types.Left),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let output = module.execute(100_000).unwrap().to_string();
        assert!(
            output.contains("children: [{children: [], value: 2}]"),
            "{output}"
        );
        assert!(
            output.contains("pair: {right: ('Some, {left: 'None})}"),
            "{output}"
        );
        assert!(output.contains("$defs"), "{output}");
        assert!(output.contains("#/$defs/Type0"), "{output}");
        assert!(output.contains("#/$defs/Type1"), "{output}");

        fs::write(
            directory.join("bad.json"),
            r#"{"value":1,"children":[{"value":"wrong","children":[]}]}"#,
        )
        .unwrap();
        fs::write(
            directory.join("bad.xl"),
            r#"import data from "./bad.json";
               import Types from "./Types.xl";
               import codec from "core:codec";
               import result from "core:result";
               codec.decode(Types.Node, data) |> result.unwrap"#,
        )
        .unwrap();
        let bad = load_module(directory.join("bad.xl"), BTreeMap::new(), 100_000).unwrap();
        let failure = bad.execute(100_000).unwrap_err();
        assert!(failure.message.contains("$.children[0].value"));
        assert!(failure.data_location().is_some());
        assert!(failure.rule_location().is_some());

        fs::write(
            directory.join("leak.xl"),
            r#"import Types from "./Types.xl";
               import json from "core:json";
               json.stringify(Types.Node)"#,
        )
        .unwrap();
        let leak = load_module(directory.join("leak.xl"), BTreeMap::new(), 100_000).unwrap();
        assert!(
            leak.execute(100_000)
                .unwrap_err()
                .message
                .contains("internal up-link")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn final_program_observes_only_presealed_recursive_type_roots() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               @struct type Forward = {next: Later};
               let premature = codec.decode(Forward, {next: 1});
               type Later = Int;
               premature"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "('Ok, {next: 1})"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn builtin_bool_option_and_result_are_normalized_enum_metadata() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import attributes from "core:attributes";
               type Maybe = Option(attributes.add(Int, { marker: "payload" }));
               type Outcome = Result(String, Int);
               let compared: Bool = 1 < 2;
               let none: Maybe = 'None;
               let some: Maybe = ('Some, 42);
               let ok: Outcome = ('Ok, "done");
               let err: Outcome = ('Err, 7);
               {
                   bool: Bool,
                   maybe: Maybe,
                   outcome: Outcome,
                   compared: validate(Bool, compared),
                   none: validate(Maybe, none),
                   some: validate(Maybe, some),
                   ok: validate(Outcome, ok),
                   err: validate(Outcome, err),
                   wrong_bool: validate(Bool, 'Other),
                   wrong_some: validate(Maybe, ('Some, "forty-two")),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected built-in type results")
        };
        for field in ["compared", "none", "some", "ok", "err"] {
            assert!(result.get(field).unwrap().to_string().starts_with("('Ok,"));
        }
        for field in ["wrong_bool", "wrong_some"] {
            assert!(result.get(field).unwrap().to_string().starts_with("('Err,"));
        }

        fn wrapper(value: &Value) -> &crate::Dict {
            let Value::Dict(wrapper) = value else {
                panic!("expected WithAttributes wrapper")
            };
            assert_eq!(wrapper.get("kind").unwrap().to_string(), "'WithAttributes");
            assert!(matches!(wrapper.get("attributes"), Some(Value::Dict(_))));
            wrapper
        }
        for field in ["bool", "maybe", "outcome"] {
            let root = wrapper(result.get(field).unwrap());
            let Value::Dict(metadata) = root.get("inner").unwrap() else {
                panic!("expected Enum metadata")
            };
            assert_eq!(metadata.get("kind").unwrap().to_string(), "'Enum");
            let Value::Dict(variants) = metadata.get("variants").unwrap() else {
                panic!("expected Enum variants")
            };
            for variant in variants.values() {
                wrapper(variant);
            }
        }
        let maybe = wrapper(result.get("maybe").unwrap());
        let Value::Dict(metadata) = maybe.get("inner").unwrap() else {
            panic!("expected Option metadata")
        };
        let Value::Dict(variants) = metadata.get("variants").unwrap() else {
            panic!("expected Option variants")
        };
        let some = wrapper(variants.get("Some").unwrap());
        assert_eq!(
            some.get("attributes").unwrap().to_string(),
            "{marker: \"payload\"}"
        );
        let none = wrapper(variants.get("None").unwrap());
        assert_eq!(none.get("inner").unwrap().to_string(), "'None");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn builtin_enum_type_constructors_validate_inputs_and_charge_quota() {
        let directory = fixture_dir();
        let invalid_path = directory.join("invalid.xl");
        fs::write(&invalid_path, "Option(1)").unwrap();
        let invalid = load_module(&invalid_path, BTreeMap::new(), 100_000)
            .unwrap()
            .execute(100_000)
            .unwrap_err();
        assert!(invalid.message.contains("Type metadata"));

        let quota_path = directory.join("quota.xl");
        fs::write(&quota_path, "Result(String, Int)").unwrap();
        let module = load_module(&quota_path, BTreeMap::new(), 100_000).unwrap();
        let mut account = QuotaAccount::new(Quota::new(10, 1_000, 0));
        let error = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut account,
            )
            .err()
            .expect("Result construction must exhaust allocation quota");
        assert_eq!(error.kind, crate::RuntimeErrorKind::AllocationQuotaExceeded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn normalized_model_constructors_reject_invalid_inputs_and_charge_quota() {
        let directory = fixture_dir();
        let run_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(&path, expression).unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            module.execute(100_000).unwrap_err()
        };
        assert!(
            run_error("context.xl", "struct('Bad, {x: Int})")
                .message
                .contains("model context")
        );
        assert!(
            run_error("empty.xl", "enum('None, {})")
                .message
                .contains("at least one variant")
        );
        assert!(
            run_error("field.xl", "struct('None, {x: 1})")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error("variant.xl", "enum('None, {Bad: 1})")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error("empty-union.xl", "union('None, [])")
                .message
                .contains("at least one variant")
        );
        assert!(
            run_error("union-variant.xl", "union('None, [1])")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error(
                "union-wrapper.xl",
                "union('None, [{kind: 'WithAttributes, inner: Int, attributes: []}])",
            )
            .message
            .contains("attributes must be a Dict")
        );

        for (name, source) in [
            ("uppercase-struct.xl", "Struct({x: Int})"),
            ("uppercase-union.xl", "Union([Int, String])"),
        ] {
            let path = directory.join(name);
            fs::write(&path, source).unwrap();
            let error = match load_module(path, BTreeMap::new(), 100_000) {
                Ok(_) => panic!("uppercase constructor must be absent"),
                Err(error) => error,
            };
            assert!(error.message.contains("unknown binding"));
        }

        let path = directory.join("quota.xl");
        fs::write(&path, "union('None, [Int, String])").unwrap();
        let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
        let mut account = QuotaAccount::new(Quota::new(10, 1_000, 0));
        let error = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut account,
            )
            .err()
            .expect("model normalization must exhaust allocation quota");
        assert_eq!(error.kind, crate::RuntimeErrorKind::AllocationQuotaExceeded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_attributes_rejects_malformed_wrappers_and_obeys_allocation_quota() {
        let directory = fixture_dir();
        let path = directory.join("main.xl");
        fs::write(
            &path,
            r#"import attributes from "core:attributes";
               attributes.normalize({kind: 'WithAttributes, inner: 1, attributes: []})"#,
        )
        .unwrap();
        let module = load_module(&path, BTreeMap::new(), 100_000).unwrap();
        let error = module.execute(100_000).unwrap_err();
        assert!(error.message.contains("attributes must be a Dict"));

        fs::write(
            &path,
            r#"import attributes from "core:attributes";
               attributes.normalize(1)"#,
        )
        .unwrap();
        let module = load_module(&path, BTreeMap::new(), 100_000).unwrap();
        let mut account = QuotaAccount::new(Quota::new(10, 1_000, 0));
        let error = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut account,
            )
            .err()
            .expect("normalization must exhaust allocation quota");
        assert_eq!(error.kind, crate::RuntimeErrorKind::AllocationQuotaExceeded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_json_decorators_build_flat_standard_attribute_metadata() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import json from "core:json";
               @json.rename_all('CamelCase)
               @struct
               type Model = {
                   @json.rename("outerName")
                   @json.rename("innerName")
                   @json.default(7)
                   @json.skip_serializing_if('None)
                   value_name: Option(Int),

                   @json.flatten
                   nested: struct('None, { child_value: String }),
               };
               Model"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(root) = module.execute(100_000).unwrap() else {
            panic!("expected attributed model")
        };
        let Value::Dict(root_attributes) = root.get("attributes").unwrap() else {
            panic!("expected root attributes")
        };
        assert_eq!(
            root_attributes
                .get("core:json.rename_all")
                .unwrap()
                .to_string(),
            "'CamelCase"
        );
        let Value::Dict(metadata) = root.get("inner").unwrap() else {
            panic!("expected Struct metadata")
        };
        let Value::Dict(fields) = metadata.get("fields").unwrap() else {
            panic!("expected fields")
        };
        let Value::Dict(value) = fields.get("value_name").unwrap() else {
            panic!("expected normalized field wrapper")
        };
        assert!(
            !matches!(value.get("inner"), Some(Value::Dict(inner)) if matches!(inner.get("kind"), Some(Value::Atom(kind)) if kind.name() == "WithAttributes"))
        );
        let Value::Dict(attributes) = value.get("attributes").unwrap() else {
            panic!("expected field attributes")
        };
        assert_eq!(
            attributes.get("core:json.rename").unwrap().to_string(),
            "\"outerName\""
        );
        assert_eq!(
            attributes.get("core:json.default").unwrap().to_string(),
            "7"
        );
        assert_eq!(
            attributes
                .get("core:json.skip_serializing_if")
                .unwrap()
                .to_string(),
            "'None"
        );
        let Value::Dict(nested) = fields.get("nested").unwrap() else {
            panic!("expected nested field wrapper")
        };
        let Value::Dict(nested_attributes) = nested.get("attributes").unwrap() else {
            panic!("expected nested attributes")
        };
        assert_eq!(
            nested_attributes
                .get("core:json.flatten")
                .unwrap()
                .to_string(),
            "'True"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn struct_json_codecs_apply_serde_style_attributes_bidirectionally() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               import result from "core:result";

               @struct type Coordinates = {
                   latitude: Int,
               };
               @struct type Address = {
                   city_name: String,
                   @json.flatten coordinates: Coordinates,
               };
               @json.rename_all('CamelCase)
               @struct type User = {
                   user_id: Int,
                   @json.rename("display") display_name: String,
                   @json.flatten address: Address,
                   @json.default('None)
                   @json.skip_serializing_if('None)
                   nickname: Option(String),
                   @json.skip_serializing_if('False) hidden: Any,
                   @json.skip_serializing_if('Empty) notes: String,
                   @json.skip_serializing_if('Empty) tags: Array(String),
                   @json.skip_serializing_if('Empty) extras: Any,
               };
               let decoded = codec.decode(User, {
                   userId: 7,
                   display: "Ada",
                   city_name: "London",
                   latitude: 51,
                   hidden: 'False,
                   notes: "",
                   tags: [],
                   extras: {},
               }) |> result.unwrap;
               { decoded: decoded, encoded: codec.encode(User, decoded) |> result.unwrap }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected codec results")
        };
        assert_eq!(
            output.get("decoded").unwrap().to_string(),
            "{address: {city_name: \"London\", coordinates: {latitude: 51}}, display_name: \"Ada\", extras: {}, hidden: 'False, nickname: 'None, notes: \"\", tags: [], user_id: 7}"
        );
        assert_eq!(
            output.get("encoded").unwrap().to_string(),
            "{city_name: \"London\", display: \"Ada\", latitude: 51, userId: 7}"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_skip_serializing_if_calls_promoted_bytecode_and_native_predicates() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import debug from "core:debug";
               import json from "core:json";
               let zero = 0;
               fn is_zero(value) { value == zero }
               @struct type Model = {
                   @json.skip_serializing_if(is_zero) omitted: Int,
                   @json.skip_serializing_if(is_zero) retained: Int,
                   @json.skip_serializing_if(debug.dbg) native_omitted: Bool,
               };
               codec.encode(Model, {
                   omitted: 0,
                   retained: 7,
                   native_omitted: 'True,
               })"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        let module = load_module_with_quota_and_debug_sink(
            directory.join("main.xl"),
            BTreeMap::new(),
            Quota::with_fuel(100_000),
            sink.clone(),
        )
        .unwrap();
        let failure = module
            .execute_with_quota_and_debug_sink(Quota::with_fuel(3), sink.clone())
            .unwrap_err();
        assert_eq!(failure.kind, crate::RuntimeErrorKind::FuelExhausted);
        sink.events.lock().unwrap().clear();
        let value = module
            .execute_with_quota_and_debug_sink(Quota::with_fuel(4), sink.clone())
            .unwrap();
        assert_eq!(value.to_string(), "('Ok, {retained: 7})");
        assert_eq!(sink.events.lock().unwrap().len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_skip_serializing_if_rejects_invalid_function_contracts() {
        let directory = fixture_dir();
        fs::write(
            directory.join("arity.xl"),
            r#"import json from "core:json";
               fn wrong(left, right) { 'False }
               @struct type Model = {
                   @json.skip_serializing_if(wrong) value: Int,
               };
               0"#,
        )
        .unwrap();
        let arity = load_module(directory.join("arity.xl"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(arity.message().contains("unary Func"), "{arity}");

        fs::write(
            directory.join("result.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               fn identity(value) { value }
               @struct type Model = {
                   @json.skip_serializing_if(identity) value: Int,
               };
               codec.encode(Model, {value: 1})"#,
        )
        .unwrap();
        let module = load_module(directory.join("result.xl"), BTreeMap::new(), 100_000).unwrap();
        let result = module.execute(100_000).unwrap_err();
        assert_eq!(result.kind, crate::RuntimeErrorKind::TypeMismatch);
        assert!(result.message.contains("must return 'True or 'False"));
        assert!(
            result
                .trace
                .iter()
                .any(|frame| frame.function == "core:codec.encode")
        );

        fs::write(
            directory.join("callback.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               fn fails(value) { 1 / 0 }
               @struct type Model = {
                   @json.skip_serializing_if(fails) value: Int,
               };
               codec.encode(Model, {value: 1})"#,
        )
        .unwrap();
        let callback =
            load_module(directory.join("callback.xl"), BTreeMap::new(), 100_000).unwrap();
        let failure = callback.execute(100_000).unwrap_err();
        assert_eq!(failure.kind, crate::RuntimeErrorKind::DivisionByZero);
        assert!(
            failure
                .trace
                .iter()
                .any(|frame| frame.function.contains("closure"))
        );
        assert!(
            failure
                .trace
                .iter()
                .any(|frame| frame.function == "core:codec.encode")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_skip_predicates_resume_at_nested_paths_and_before_flattening() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.xl"),
            r#"import codec from "core:codec";
               import json from "core:json";
               fn is_zero(value) { value == 0 }
               fn always(value) { 'True }
               @struct type Item = {
                   @json.skip_serializing_if(is_zero) value: Int,
               };
               @struct type Nested = {required: String};
               @struct type Model = {
                   items: Array(Item),
                   @json.skip_serializing_if(always)
                   @json.flatten nested: Nested,
               };
               codec.encode(Model, {
                   items: [{value: 0}, {value: 2}],
                   nested: 42,
               })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.xl"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "('Ok, {items: [{}, {value: 2}]})"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn struct_json_codecs_reject_attribute_conflicts_and_invalid_defaults() {
        let directory = fixture_dir();
        let cases = [
            (
                "collision.xl",
                r#"import codec from "core:codec";
                   import json from "core:json";
                   @struct type T = {
                       @json.rename("same") first: Int,
                       @json.rename("same") second: Int,
                   };
                   codec.decode(T, {same: 1})"#,
                "duplicate external field name",
            ),
            (
                "flatten-type.xl",
                r#"import codec from "core:codec";
                   import json from "core:json";
                   @struct type T = {@json.flatten value: Int};
                   codec.decode(T, {})"#,
                "flatten requires Struct metadata",
            ),
            (
                "flatten-rename.xl",
                r#"import codec from "core:codec";
                   import json from "core:json";
                   @struct type Inner = {value: Int};
                   @struct type T = {
                       @json.flatten @json.rename("x") inner: Inner,
                   };
                   codec.decode(T, {value: 1})"#,
                "flatten cannot be combined",
            ),
            (
                "default.xl",
                r#"import codec from "core:codec";
                   import json from "core:json";
                   @struct type T = {@json.default("wrong") value: Int};
                   codec.decode(T, {})"#,
                "expected Int",
            ),
        ];
        for (name, source, expected) in cases {
            let path = directory.join(name);
            fs::write(&path, source).unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            let result = module.execute(100_000).unwrap();
            assert!(result.to_string().contains("'Err"), "{result}");
            assert!(result.to_string().contains(expected), "{result}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_json_decorators_validate_policies_and_charge_allocations() {
        let directory = fixture_dir();
        let run_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(
                &path,
                format!("import json from \"core:json\"; {expression}"),
            )
            .unwrap();
            load_module(path, BTreeMap::new(), 100_000)
                .unwrap()
                .execute(100_000)
                .unwrap_err()
        };
        assert!(
            run_error("rename.xl", "json.rename(1)")
                .message
                .contains("expects a String")
        );
        assert!(
            run_error("case.xl", "json.rename_all('SnakeCase)")
                .message
                .contains("CamelCase")
        );
        assert!(
            run_error("skip.xl", "json.skip_serializing_if('Zero)")
                .message
                .contains("'Empty")
        );

        let path = directory.join("quota.xl");
        fs::write(
            &path,
            "import json from \"core:json\"; json.rename(\"name\")",
        )
        .unwrap();
        let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
        let mut account = QuotaAccount::new(Quota::new(10, 1_000, 0));
        let error = Vm::new()
            .execute_in_work(
                &module.runtime.main.heap,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut account,
            )
            .err()
            .expect("decorator factory must exhaust allocation quota");
        assert_eq!(error.kind, crate::RuntimeErrorKind::AllocationQuotaExceeded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_dict_rejects_invalid_arguments_pairs_and_duplicates() {
        let directory = fixture_dir();
        let run_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(
                &path,
                format!("import dicts from \"core:dict\"; {expression}"),
            )
            .unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            module.execute(100_000).unwrap_err()
        };

        assert!(
            run_error("keys.xl", "dicts.keys([])")
                .message
                .contains("Dict")
        );
        assert!(
            run_error("merge.xl", "dicts.merge({}, [])")
                .message
                .contains("right Dict")
        );
        assert!(
            run_error("pairs-array.xl", "dicts.from_pairs({})")
                .message
                .contains("Array")
        );
        assert!(
            run_error("pair-shape.xl", "dicts.from_pairs([(\"a\", 1, 2)])")
                .message
                .contains("two-element Tuple")
        );
        assert!(
            run_error("pair-key.xl", "dicts.from_pairs([('a, 1)])")
                .message
                .contains("key must be a String")
        );
        let duplicate = run_error("duplicate.xl", "dicts.from_pairs([(\"a\", 1), (\"a\", 2)])");
        assert!(duplicate.message.contains("duplicate field"));
        assert!(duplicate.to_string().contains("duplicate.xl:1:"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diamond_dependencies_reuse_the_same_persistent_root() {
        let directory = fixture_dir();
        let c = directory.join("c.xl");
        let a = directory.join("a.xl");
        let b = directory.join("b.xl");
        fs::write(&c, r#"{value: [1, 2, 3]}"#).unwrap();
        fs::write(&a, r#"import c from "./c.xl"; c"#).unwrap();
        fs::write(&b, r#"import c from "./c.xl"; c"#).unwrap();
        let mut loader = ModuleLoader {
            cache: HashMap::new(),
            core_modules: HashMap::new(),
            main: MainWorld::building(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
        };

        loader.load_value(&a).unwrap();
        let counts_after_a = loader.main.heap.counts();
        loader.load_value(&b).unwrap();
        let root = |path: &Path| match loader.cache.get(&canonicalize(path).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };

        assert_eq!(root(&a), root(&c));
        assert_eq!(root(&b), root(&c));
        assert_eq!(counts_after_a, loader.main.heap.counts());
        fs::remove_dir_all(directory).unwrap();
    }
}
