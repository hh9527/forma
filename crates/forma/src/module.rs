use crate::ast::{BindingKind, Expr, ExprKind, Program, StringPartKind, TypeArgumentKind};
use crate::compiler::{
    compile_metadata_initializer, compile_program_analyzed_in, compile_program_with_promoted_types,
    function_contract_arity, type_link_key,
};
use crate::core::module_specs;
use crate::heap::{Heap, PersistentValue, publish_root, publish_value};
use crate::json::{Provenance, SourcedValue, parse_json_registered};
use crate::module_id::{ModuleFormat, ModuleId, ModuleResolver, ResolvedModule};
use crate::parser::parse_registered;
use crate::semantic::{
    SemanticImport, SemanticModuleInput, WorkspaceModuleKind, WorkspaceModuleState,
    WorkspaceSnapshot,
};
use crate::source::{Diagnostic, SourceDatabase};
use crate::toml::parse_toml_registered;
use crate::types::{
    Analysis, ModuleInterface, PartialAnalysisControl, analyze_partial_types_recovered_with_query,
    analyze_program_with_bindings_observed,
};
use crate::yaml::parse_yaml_registered;
use crate::{
    BytecodeFunction, Closure, DebugSink, DiscardDebugSink, Prototype, Quota, QuotaAccount, Value,
    Vm,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct StaticDataParse {
    value: Option<SourcedValue>,
    diagnostics: Vec<Diagnostic>,
    kind: WorkspaceModuleKind,
}

fn static_data_kind(format: ModuleFormat) -> Option<WorkspaceModuleKind> {
    match format {
        ModuleFormat::Json => Some(WorkspaceModuleKind::Json),
        ModuleFormat::Toml => Some(WorkspaceModuleKind::Toml),
        ModuleFormat::Yaml => Some(WorkspaceModuleKind::Yaml),
        _ => None,
    }
}

fn parse_static_data_registered(
    format: ModuleFormat,
    sources: &SourceDatabase,
    source_id: crate::SourceId,
) -> Option<StaticDataParse> {
    let kind = static_data_kind(format)?;
    let (value, diagnostics) = match format {
        ModuleFormat::Json => {
            let parsed = parse_json_registered(sources, source_id);
            (parsed.value, parsed.diagnostics)
        }
        ModuleFormat::Toml => {
            let parsed = parse_toml_registered(sources, source_id);
            (parsed.value, parsed.diagnostics)
        }
        ModuleFormat::Yaml => {
            let parsed = parse_yaml_registered(sources, source_id);
            (parsed.value, parsed.diagnostics)
        }
        _ => unreachable!("kind exists only for static data formats"),
    };
    Some(StaticDataParse {
        value,
        diagnostics,
        kind,
    })
}

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
    pub workspace: WorkspaceSnapshot,
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
) -> Result<HashMap<&'static str, (Value, PersistentValue, ModuleInterface)>, ModuleError> {
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
        modules.insert(spec.name, (value, root, analysis.module_interface));
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

    pub fn invoke_with_quota_and_debug_sink(
        &self,
        callee: &Value,
        arguments: &[Value],
        quota: Quota,
        debug_sink: Arc<dyn DebugSink>,
    ) -> Result<Value, ModuleError> {
        let Value::Func(closure) = callee else {
            return Err(ModuleError::new(format!(
                "module result must be a function, found {}",
                callee.type_name()
            )));
        };
        let Prototype::Bytecode(function) = closure.prototype() else {
            return Err(ModuleError::new(
                "module result must be an XL function, found a native function",
            ));
        };
        let mut account = QuotaAccount::new(quota);
        let arena = Vm::new()
            .with_debug_sink(debug_sink)
            .execute_function_with_captures_in_work(
                &self.runtime.main.heap,
                &self.runtime.externals,
                function,
                closure.upvalues(),
                arguments,
                &mut account,
            )
            .map_err(|error| ModuleError::new(error.with_sources(&self.sources).to_string()))?;
        arena
            .export(&self.runtime.main.heap)
            .map_err(|error| ModuleError::new(error.to_string()))
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

    pub fn invoke(
        &self,
        module: &LoadedModule,
        callee: &Value,
        arguments: &[Value],
    ) -> Result<Value, ModuleError> {
        module.invoke_with_quota_and_debug_sink(
            callee,
            arguments,
            self.config.session_quota,
            Arc::clone(&self.debug_sink),
        )
    }

    pub fn recover_workspace(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<WorkspaceSnapshot, ModuleError> {
        let resolver = ModuleResolver::for_root(path.as_ref())
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let root_module = resolver
            .resolve_root(path.as_ref())
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let root = root_module
            .path()
            .expect("local root has a path")
            .to_owned();
        if let Ok(module) = self.load_module(&root, BTreeMap::new()) {
            return Ok(module.workspace);
        }
        let mut main = MainWorld::building();
        let mut sources = SourceDatabase::default();
        let core_modules = install_core_modules(&mut main, &mut sources, &self.debug_sink)?
            .into_iter()
            .map(|(name, (value, _, interface))| (name.to_owned(), (value, interface)))
            .collect();
        let mut builder = RecoverableWorkspaceBuilder {
            engine: self,
            resolver,
            overlays: &BTreeMap::new(),
            query: None,
            sources,
            core_modules,
            inputs: BTreeMap::new(),
            values: HashMap::new(),
            interfaces: HashMap::new(),
            visiting: Vec::new(),
            cycle_members: HashSet::new(),
            cycle_reported: false,
        };
        block_on_recovery(builder.load_xl(root_module));
        Ok(WorkspaceSnapshot::build(
            builder.sources,
            builder.inputs.into_values().collect(),
        ))
    }

    pub async fn recover_workspace_async(
        &self,
        path: impl AsRef<Path>,
        overlays: &BTreeMap<PathBuf, crate::document::DocumentText>,
        context: &crate::query::QueryContext,
    ) -> Result<WorkspaceSnapshot, ModuleError> {
        context
            .checkpoint()
            .await
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let root_path = path.as_ref();
        let resolver = overlays
            .get(root_path)
            .map_or_else(
                || ModuleResolver::for_root(root_path),
                |source| ModuleResolver::for_root_with_source(root_path, source),
            )
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let root_module = resolver
            .resolve_root(path.as_ref())
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let mut main = MainWorld::building();
        let mut sources = SourceDatabase::default();
        let core_modules = install_core_modules(&mut main, &mut sources, &self.debug_sink)?
            .into_iter()
            .map(|(name, (value, _, interface))| (name.to_owned(), (value, interface)))
            .collect();
        let mut builder = RecoverableWorkspaceBuilder {
            engine: self,
            resolver,
            overlays,
            query: Some(context),
            sources,
            core_modules,
            inputs: BTreeMap::new(),
            values: HashMap::new(),
            interfaces: HashMap::new(),
            visiting: Vec::new(),
            cycle_members: HashSet::new(),
            cycle_reported: false,
        };
        builder.load_xl(root_module).await;
        context
            .checkpoint()
            .await
            .map_err(|error| ModuleError::new(error.to_string()))?;
        Ok(WorkspaceSnapshot::build(
            builder.sources,
            builder.inputs.into_values().collect(),
        ))
    }
}

struct RecoverableWorkspaceBuilder<'a> {
    engine: &'a Engine,
    resolver: ModuleResolver,
    overlays: &'a BTreeMap<PathBuf, crate::document::DocumentText>,
    query: Option<&'a crate::query::QueryContext>,
    sources: SourceDatabase,
    core_modules: HashMap<String, (Value, ModuleInterface)>,
    inputs: BTreeMap<String, SemanticModuleInput>,
    values: HashMap<ModuleId, Value>,
    interfaces: HashMap<ModuleId, ModuleInterface>,
    visiting: Vec<ModuleId>,
    cycle_members: HashSet<ModuleId>,
    cycle_reported: bool,
}

impl RecoverableWorkspaceBuilder<'_> {
    fn load_xl<'a>(
        &'a mut self,
        module: ResolvedModule,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Value>> + 'a>> {
        Box::pin(async move {
            if let Some(context) = self.query
                && context.checkpoint().await.is_err()
            {
                return None;
            }
            let path = module.path()?.to_owned();
            let module_id = module.id;
            if let Some(value) = self.values.get(&module_id) {
                return Some(value.clone());
            }
            let key = module_id.to_string();
            if self.inputs.contains_key(&key) {
                return None;
            }
            if let Some(index) = self
                .visiting
                .iter()
                .position(|candidate| candidate == &module_id)
            {
                self.cycle_members
                    .extend(self.visiting[index..].iter().cloned());
                self.cycle_members.insert(module_id.clone());
                return None;
            }
            let source = match self.overlays.get(&path).cloned() {
                Some(source) => source,
                None => match fs::read_to_string(&path) {
                    Ok(source) => crate::document::DocumentText::new(source),
                    Err(error) => {
                        self.inputs.insert(
                            key.clone(),
                            unavailable_input(key, path.clone(), WorkspaceModuleKind::Forma),
                        );
                        let _ = error;
                        return None;
                    }
                },
            };
            let source_id = self
                .sources
                .add_document(path.display().to_string(), source);
            let parsed = parse_registered(&self.sources, source_id);
            let has_manifest = parsed.manifest.is_some();
            let program = parsed.program.clone();
            let imports = parsed
                .recovered
                .bindings
                .iter()
                .filter(|binding| binding.value.kind == BindingKind::Import)
                .filter_map(|binding| match &binding.value.value.value {
                    ExprKind::String(target) => Some((
                        binding.value.name.value.clone(),
                        binding.value.name.location,
                        target.clone(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();

            self.visiting.push(module_id.clone());
            let mut semantic_imports = Vec::new();
            let mut external_values = BTreeMap::new();
            let mut external_interfaces = BTreeMap::new();
            let mut unavailable_imports = HashSet::new();
            let mut diagnostics = Vec::new();
            if has_manifest && module_id != ModuleId::Main {
                diagnostics.push(Diagnostic::error(
                    "@@manifest is only allowed in @main",
                    parsed.manifest.as_ref().expect("checked manifest").location,
                ));
            }
            for (name, location, target) in imports {
                if target.starts_with("@bim/") {
                    semantic_imports.push(SemanticImport {
                        name: name.clone(),
                        location,
                        target: ModuleId::builtin(target.clone()),
                    });
                    if let Some((value, interface)) = self.core_modules.get(&target) {
                        external_values.insert(name.clone(), value.clone());
                        external_interfaces.insert(name, interface.clone());
                    } else {
                        unavailable_imports.insert(name);
                        diagnostics.push(Diagnostic::error(
                            format!("unknown built-in module {target:?}"),
                            location,
                        ));
                        self.inputs
                            .entry(target.clone())
                            .or_insert_with(|| SemanticModuleInput {
                                key: target.clone(),
                                path: None,
                                kind: WorkspaceModuleKind::Core,
                                source: None,
                                program: None,
                                analysis: None,
                                partial: None,
                                state: WorkspaceModuleState::Unavailable,
                                imports: Vec::new(),
                                diagnostics: Vec::new(),
                            });
                    }
                    continue;
                }

                let target_module = match self.resolver.resolve_import(&module_id, &target) {
                    Ok(target) => target,
                    Err(error) => {
                        unavailable_imports.insert(name.clone());
                        diagnostics.push(Diagnostic::error(error.to_string(), location));
                        continue;
                    }
                };
                let target_path = target_module
                    .path()
                    .expect("local import resolves to a local source")
                    .to_owned();
                semantic_imports.push(SemanticImport {
                    name: name.clone(),
                    location,
                    target: target_module.id.clone(),
                });
                let value = match target_module.format {
                    ModuleFormat::Forma => self.load_xl(target_module.clone()).await,
                    ModuleFormat::Json | ModuleFormat::Toml | ModuleFormat::Yaml => {
                        self.load_static_data(target_module.clone()).await
                    }
                };
                if let Some(value) = value {
                    if let Some(interface) = self.interfaces.get(&target_module.id) {
                        external_interfaces.insert(name.clone(), interface.clone());
                    }
                    external_values.insert(name, value);
                } else {
                    unavailable_imports.insert(name);
                    if self.cycle_members.contains(&target_module.id) {
                        if !self.cycle_reported {
                            diagnostics.push(Diagnostic::error(
                                format!("module cycle reaches {}", target_path.display()),
                                location,
                            ));
                            self.cycle_reported = true;
                        }
                    } else {
                        diagnostics.push(Diagnostic::error(
                            format!("module {} is unavailable", target_path.display()),
                            location,
                        ));
                    }
                }
            }
            self.visiting.pop();

            if let Some(context) = self.query
                && context.checkpoint().await.is_err()
            {
                return None;
            }

            let partial = analyze_partial_types_recovered_with_query(
                &self.sources,
                source_id,
                &parsed.recovered,
                parsed.diagnostics,
                self.engine.config.module_quota,
                &external_values,
                PartialAnalysisControl {
                    unavailable_imports: &unavailable_imports,
                    query: self.query,
                },
            );
            let strict = if self.cycle_members.contains(&module_id)
                || has_manifest && module_id != ModuleId::Main
            {
                None
            } else {
                program.as_ref().and_then(|program| {
                    self.analyze_and_evaluate(
                        source_id,
                        program,
                        &external_values,
                        &external_interfaces,
                    )
                    .ok()
                })
            };
            let strict_value = strict.as_ref().map(|(_, value)| value);
            let state = if strict_value.is_some() {
                WorkspaceModuleState::Known
            } else if self.cycle_members.contains(&module_id)
                || partial.hir.definitions().is_empty() && partial.hir.expressions().is_empty()
            {
                WorkspaceModuleState::Unavailable
            } else {
                WorkspaceModuleState::Partial
            };
            self.inputs.insert(
                key.clone(),
                SemanticModuleInput {
                    key: key.clone(),
                    path: Some(path.clone()),
                    kind: WorkspaceModuleKind::Forma,
                    source: Some(source_id),
                    program,
                    analysis: strict.as_ref().map(|(analysis, _)| analysis.clone()),
                    partial: Some(partial),
                    state,
                    imports: semantic_imports,
                    diagnostics,
                },
            );
            if let Some((_, value)) = strict {
                let interface = self.inputs[&key]
                    .analysis
                    .as_ref()
                    .expect("strict module has analysis")
                    .module_interface
                    .clone();
                self.interfaces.insert(module_id.clone(), interface);
                self.values.insert(module_id, value.clone());
                Some(value)
            } else {
                None
            }
        })
    }

    async fn load_static_data(&mut self, module: ResolvedModule) -> Option<Value> {
        if let Some(context) = self.query
            && context.checkpoint().await.is_err()
        {
            return None;
        }
        let path = module.path()?.to_owned();
        let module_id = module.id;
        if let Some(value) = self.values.get(&module_id) {
            return Some(value.clone());
        }
        let key = module_id.to_string();
        if self.inputs.contains_key(&key) {
            return None;
        }
        let source = match self.overlays.get(&path).cloned() {
            Some(source) => source,
            None => match fs::read_to_string(&path) {
                Ok(source) => crate::document::DocumentText::new(source),
                Err(_) => {
                    let kind = static_data_kind(module.format)?;
                    self.inputs
                        .insert(key.clone(), unavailable_input(key, path.clone(), kind));
                    return None;
                }
            },
        };
        let source_id = self
            .sources
            .add_document(path.display().to_string(), source);
        let parsed = parse_static_data_registered(module.format, &self.sources, source_id)?;
        let value = parsed.value.map(|sourced| sourced.value);
        self.inputs.insert(
            key.clone(),
            SemanticModuleInput {
                key,
                path: Some(path),
                kind: parsed.kind,
                source: Some(source_id),
                program: None,
                analysis: None,
                partial: None,
                state: if value.is_some() {
                    WorkspaceModuleState::Known
                } else {
                    WorkspaceModuleState::Unavailable
                },
                imports: Vec::new(),
                diagnostics: parsed.diagnostics,
            },
        );
        if let Some(value) = &value {
            self.values.insert(module_id, value.clone());
        }
        value
    }

    fn analyze_and_evaluate(
        &self,
        source_id: crate::SourceId,
        program: &Program,
        external_values: &BTreeMap<String, Value>,
        external_interfaces: &BTreeMap<String, ModuleInterface>,
    ) -> Result<(crate::Analysis, Value), ModuleError> {
        let mut account = QuotaAccount::new(self.engine.config.module_quota);
        if let Some(query) = self.query {
            account = account.with_query(query.clone());
        }
        let source = self.sources.get(source_id);
        let analysis = analyze_program_with_bindings_observed(
            &source.name,
            program,
            &mut account,
            external_values,
            &HashSet::new(),
            &self.sources,
            &BTreeMap::new(),
            external_interfaces,
            &self.engine.debug_sink,
        )
        .map_err(|error| ModuleError::new(error.to_string()))?;
        let function = compile_program_analyzed_in(source, program, &analysis)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        let mut main = MainWorld::building();
        let mut external_roots = HashMap::new();
        for (name, value) in external_values {
            external_roots.insert(
                name.clone(),
                publish_value(&mut main.heap, value)
                    .map_err(|error| ModuleError::new(error.to_string()))?,
            );
        }
        let arena = Vm::new()
            .with_debug_sink(Arc::clone(&self.engine.debug_sink))
            .execute_in_work(&main.heap, &external_roots, &function, &[], &mut account)
            .map_err(|error| ModuleError::new(error.with_sources(&self.sources).to_string()))?;
        let value = arena
            .export(&main.heap)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        Ok((analysis, value))
    }
}

fn block_on_recovery<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

fn unavailable_input(key: String, path: PathBuf, kind: WorkspaceModuleKind) -> SemanticModuleInput {
    SemanticModuleInput {
        key,
        path: Some(path),
        kind,
        source: None,
        program: None,
        analysis: None,
        partial: None,
        state: WorkspaceModuleState::Unavailable,
        imports: Vec::new(),
        diagnostics: Vec::new(),
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
    let resolver = ModuleResolver::for_root(path.as_ref())
        .map_err(|error| ModuleError::new(error.to_string()))?;
    let root_module = resolver
        .resolve_root(path.as_ref())
        .map_err(|error| ModuleError::new(error.to_string()))?;
    if root_module.format != ModuleFormat::Forma {
        return Err(ModuleError::new("root module must have a .forma extension"));
    }
    let mut main = MainWorld::building();
    let mut sources = SourceDatabase::default();
    let core_modules = install_core_modules(&mut main, &mut sources, &debug_sink)?;
    let mut loader = ModuleLoader {
        resolver,
        cache: HashMap::new(),
        core_modules,
        main,
        visiting: Vec::new(),
        dependencies: BTreeSet::new(),
        module_quota,
        debug_sink,
        sources,
        semantic_inputs: BTreeMap::new(),
    };
    loader.load_root(root_module, external_bindings)
}

struct ModuleLoader {
    resolver: ModuleResolver,
    cache: HashMap<ModuleId, ModuleState>,
    core_modules: HashMap<&'static str, (Value, PersistentValue, ModuleInterface)>,
    main: MainWorld,
    visiting: Vec<ModuleId>,
    dependencies: BTreeSet<PathBuf>,
    module_quota: Quota,
    debug_sink: Arc<dyn DebugSink>,
    sources: SourceDatabase,
    semantic_inputs: BTreeMap<String, SemanticModuleInput>,
}

#[derive(Clone)]
enum ModuleState {
    Ready {
        root: PersistentValue,
        sourced: SourcedValue,
        opaque: bool,
        interface: ModuleInterface,
    },
}

impl ModuleLoader {
    fn load_root(
        &mut self,
        module: ResolvedModule,
        external_bindings: BTreeMap<String, Value>,
    ) -> Result<LoadedModule, ModuleError> {
        let path = module
            .path()
            .expect("root module has a physical path")
            .to_owned();
        let module_id = module.id;
        self.dependencies.insert(path.clone());
        self.enter(&module_id)?;
        let mut account = QuotaAccount::new(self.module_quota);
        let result = self.compile_xl(&module_id, &path, external_bindings, true, &mut account);
        self.leave(&module_id);
        let (analysis, function, externals) = result?;
        let workspace = WorkspaceSnapshot::build(
            self.sources.clone(),
            self.semantic_inputs.values().cloned().collect(),
        );
        let main = std::mem::replace(&mut self.main, MainWorld::building()).seal();
        Ok(LoadedModule {
            path,
            dependencies: self.dependencies.iter().cloned().collect(),
            analysis,
            function,
            sources: self.sources.clone(),
            workspace,
            runtime: Arc::new(ModuleRuntime { main, externals }),
        })
    }

    #[cfg(test)]
    fn load_value(&mut self, path: &Path) -> Result<SourcedValue, ModuleError> {
        let module = self
            .resolver
            .resolve_root(path)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        self.load_resolved_value(module)
    }

    fn load_resolved_value(&mut self, module: ResolvedModule) -> Result<SourcedValue, ModuleError> {
        let format = module.format;
        let path = module
            .path()
            .expect("source module has a physical path")
            .to_owned();
        let module_id = module.id;
        if let Some(ModuleState::Ready { root, sourced, .. }) = self.cache.get(&module_id) {
            let _persistent_root = root;
            return Ok(sourced.clone());
        }
        self.enter(&module_id)?;
        self.dependencies.insert(path.clone());
        let result: Result<(SourcedValue, PersistentValue, bool, ModuleInterface), ModuleError> =
            match format {
                ModuleFormat::Json | ModuleFormat::Toml | ModuleFormat::Yaml => {
                    let source = read(&path)?;
                    let source_id = self.sources.add(path.display().to_string(), source);
                    let StaticDataParse {
                        value,
                        diagnostics,
                        kind,
                    } = parse_static_data_registered(format, &self.sources, source_id)
                        .expect("static data format has a frontend");
                    value
                        .ok_or_else(|| {
                            ModuleError::new(
                                diagnostics
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
                            let key = module_id.to_string();
                            self.semantic_inputs.insert(
                                key.clone(),
                                SemanticModuleInput {
                                    key,
                                    path: Some(path.clone()),
                                    kind,
                                    source: Some(source_id),
                                    program: None,
                                    analysis: None,
                                    partial: None,
                                    state: crate::semantic::WorkspaceModuleState::Known,
                                    imports: Vec::new(),
                                    diagnostics: Vec::new(),
                                },
                            );
                            Ok((sourced, root, false, ModuleInterface::default()))
                        })
                }
                ModuleFormat::Forma => {
                    let mut account = QuotaAccount::new(self.module_quota);
                    self.compile_xl(&module_id, &path, BTreeMap::new(), false, &mut account)
                        .and_then(|(analysis, function, externals)| {
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
                                analysis.module_interface,
                            ))
                        })
                }
            };
        self.leave(&module_id);
        let (sourced, root, opaque, interface) = result?;
        self.cache.insert(
            module_id,
            ModuleState::Ready {
                root,
                sourced: sourced.clone(),
                opaque,
                interface,
            },
        );
        Ok(sourced)
    }

    fn compile_xl(
        &mut self,
        module_id: &ModuleId,
        path: &Path,
        mut external_bindings: BTreeMap<String, Value>,
        is_root: bool,
        account: &mut QuotaAccount,
    ) -> Result<(Analysis, BytecodeFunction, HashMap<String, PersistentValue>), ModuleError> {
        let source = read(path)?;
        let source_name = path.display().to_string();
        let source_id = self.sources.add(source_name.clone(), source);
        let parsed = parse_registered(&self.sources, source_id);
        if parsed.manifest.is_some() && *module_id != ModuleId::Main {
            return Err(ModuleError::new(format!(
                "{source_name}: @@manifest is only allowed in @main"
            )));
        }
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
        let mut semantic_imports = Vec::new();
        let mut external_interfaces = BTreeMap::new();

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
            if relative.starts_with("@bim/") {
                let (value, root, interface) = self.load_core_module(relative)?;
                semantic_imports.push(SemanticImport {
                    name: binding.value.name.value.clone(),
                    location: binding.value.name.location,
                    target: ModuleId::builtin(relative.clone()),
                });
                external_roots.insert(binding.value.name.value.clone(), root);
                external_interfaces.insert(binding.value.name.value.clone(), interface);
                external_bindings.insert(binding.value.name.value.clone(), value);
                continue;
            }
            let imported = self
                .resolver
                .resolve_import(module_id, relative)
                .map_err(|error| ModuleError::new(error.to_string()))?;
            let imported_id = imported.id.clone();
            let sourced = self.load_resolved_value(imported)?;
            semantic_imports.push(SemanticImport {
                name: binding.value.name.value.clone(),
                location: binding.value.name.location,
                target: imported_id.clone(),
            });
            let ModuleState::Ready {
                root,
                opaque,
                interface,
                ..
            } = self
                .cache
                .get(&imported_id)
                .expect("loaded module has a ready cache entry");
            external_roots.insert(binding.value.name.value.clone(), *root);
            external_interfaces.insert(binding.value.name.value.clone(), interface.clone());
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
            &external_interfaces,
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
        let key = module_id.to_string();
        self.semantic_inputs.insert(
            key.clone(),
            SemanticModuleInput {
                key,
                path: Some(path.to_owned()),
                kind: WorkspaceModuleKind::Forma,
                source: Some(source_id),
                program: Some(program),
                analysis: Some(analysis.clone()),
                partial: None,
                state: crate::semantic::WorkspaceModuleState::Known,
                imports: semantic_imports,
                diagnostics: Vec::new(),
            },
        );
        Ok((analysis, function, external_roots))
    }

    fn load_core_module(
        &mut self,
        name: &str,
    ) -> Result<(Value, PersistentValue, ModuleInterface), ModuleError> {
        self.core_modules
            .get(name)
            .map(|(value, root, interface)| (value.clone(), *root, interface.clone()))
            .ok_or_else(|| ModuleError::new(format!("unknown core module {name:?}")))
    }

    fn enter(&mut self, module_id: &ModuleId) -> Result<(), ModuleError> {
        if let Some(index) = self
            .visiting
            .iter()
            .position(|candidate| candidate == module_id)
        {
            let mut cycle = self.visiting[index..]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            cycle.push(module_id.to_string());
            return Err(ModuleError::new(format!(
                "module import cycle: {}",
                cycle.join(" -> ")
            )));
        }
        self.visiting.push(module_id.clone());
        Ok(())
    }

    fn leave(&mut self, module_id: &ModuleId) {
        let popped = self.visiting.pop();
        debug_assert_eq!(popped.as_ref(), Some(module_id));
    }
}

fn reject_nested_imports(program: &Program, source_name: &str) -> Result<(), ModuleError> {
    for binding in &program.value.body.value.bindings {
        if matches!(binding.value.kind, BindingKind::Let | BindingKind::Def)
            && expression_has_import(&binding.value.value)
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
        ExprKind::TypeApply { callee, arguments } => {
            expression_has_import(callee)
                || arguments.iter().any(|argument| match &argument.value {
                    TypeArgumentKind::Explicit(argument) => expression_has_import(argument),
                    TypeArgumentKind::Infer => false,
                })
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

fn read(path: &Path) -> Result<String, ModuleError> {
    fs::read_to_string(path).map_err(|error| {
        ModuleError::new(format!("cannot read module {}: {error}", path.display()))
    })
}

#[cfg(test)]
fn canonicalize(path: &Path) -> Result<PathBuf, ModuleError> {
    fs::canonicalize(path).map_err(|error| {
        ModuleError::new(format!("cannot resolve module {}: {error}", path.display()))
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
        let path = std::env::temp_dir().join(format!("forma-module-test-{unique}"));
        fs::create_dir(&path).unwrap();
        path
    }

    fn recovery_engine() -> Engine {
        Engine::new(EngineConfig {
            module_quota: Quota::with_fuel(1_000_000),
            session_quota: Quota::with_fuel(1_000_000),
        })
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
            directory.join("main.forma"),
            r#"import debug from "@bim/std/debug";
               let identity: Fn(Any) -> Any = fn(value) { value };
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
            .load_module(directory.join("main.forma"), BTreeMap::new())
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
            directory.join("bad-label.forma"),
            r#"import debug from "@bim/std/debug"; debug.dbg_with(1, 42)"#,
        )
        .unwrap();
        let bad = engine
            .load_module(directory.join("bad-label.forma"), BTreeMap::new())
            .unwrap_err();
        assert!(bad.to_string().contains("String"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_debug_uses_one_fuel_no_xl_allocation_and_observes_module_init() {
        let directory = fixture_dir();
        fs::write(directory.join("data.json"), r#"{"value":42}"#).unwrap();
        fs::write(
            directory.join("dependency.forma"),
            r#"import debug from "@bim/std/debug"; debug.dbg_with("tool", 41)"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import debug from "@bim/std/debug";
               import dependency from "./dependency.forma";
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
            .load_module(directory.join("main.forma"), BTreeMap::new())
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
            directory.join("erased.forma"),
            r#"import debug from "@bim/std/debug";
               def observe: Fn(Any) -> Any = fn(value) { debug.dbg_with("metadata", value) };
               type Observed = observe(Int);
               0"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        let erased = load_module_with_quota_and_debug_sink(
            directory.join("erased.forma"),
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
            directory.join("retained.forma"),
            r#"import debug from "@bim/std/debug";
               def observe: Fn(Any) -> Any = fn(value) { debug.dbg_with("observed", value) };
               type Observed = observe(Int);
               observe(1)"#,
        )
        .unwrap();
        let retained = load_module_with_quota_and_debug_sink(
            directory.join("retained.forma"),
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
            directory.join("main.forma"),
            r#"import debug from "@bim/std/debug";
               type Observed = debug.dbg(Int);
               0"#,
        )
        .unwrap();
        let sink = Arc::new(CapturingDebugSink::default());
        load_module_with_quota_and_debug_sink(
            directory.join("main.forma"),
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
            directory.join("User.forma"),
            r#"import codec from "@bim/std/codec";
               import result from "@bim/std/result";
               @struct type User = {v: Option(String)};
               let decode = fn(value) { codec.decode(User, value) };
               let encode = fn(value) {
                   codec.encode(User, value) |> result.unwrap
               };
               {Type: User, decode: decode, encode: encode}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import data from "./abc.json";
               import User from "./User.forma";
               import result from "@bim/std/result";
               import json from "@bim/std/json";
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
            let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000)
                .unwrap_or_else(|error| panic!("failed to load {source}: {error}"));
            assert_eq!(
                module.execute(100_000).unwrap().to_string(),
                format!("{output:?}")
            );
        }

        fs::write(directory.join("abc.json"), r#"{"v":1}"#).unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
                .slice(data_location)
                .as_deref(),
            Some("1")
        );
        let rendered = failure.to_string();
        assert!(rendered.contains("abc.json:1:6:"), "{rendered}");
        assert!(
            rendered.contains("contract rule declared here"),
            "{rendered}"
        );
        assert!(rendered.contains("User.forma:3:47:"), "{rendered}");

        fs::write(
            directory.join("inspect.forma"),
            r#"import data from "./abc.json";
               import User from "./User.forma";
               data |> User.decode"#,
        )
        .unwrap();
        let inspected = load_module(directory.join("inspect.forma"), BTreeMap::new(), 100_000)
            .unwrap()
            .execute(100_000)
            .unwrap();
        let Value::Tagged { tag, payload } = inspected else {
            panic!("codec must return a tagged Result")
        };
        assert_eq!(tag.name(), "Err");
        let Value::Dict(payload) = payload.as_ref() else {
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
            directory.join("main.forma"),
            r#"import data from "./data.json";
               import codec from "@bim/std/codec";
               import result from "@bim/std/result";
               type StringRule = {kind: 'String};
               type UserRule = {kind: 'Struct, fields: {v: StringRule}};
               codec.decode(UserRule, data) |> result.unwrap"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "{v: \"plain\"}"
        );

        fs::write(
            directory.join("legacy.forma"),
            r#"import result from "@bim/std/result"; result.unwrap('Err("legacy"))"#,
        )
        .unwrap();
        let legacy = load_module(directory.join("legacy.forma"), BTreeMap::new(), 100_000)
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
                r#"import codec from "@bim/std/codec";
                   import result from "@bim/std/result";
                   @struct type T = {name: String};
                   codec.decode(T, {}) |> result.unwrap"#,
                "$.name: missing required field",
            ),
            (
                r#"import codec from "@bim/std/codec";
                   import result from "@bim/std/result";
                   @struct type T = {name: String};
                   codec.decode(T, {name: "Ada", extra: 1}) |> result.unwrap"#,
                "$.extra: unknown field",
            ),
            (
                r#"import json from "@bim/std/json"; json.stringify((1, 2))"#,
                "JSON cannot encode Tuple",
            ),
            (
                r#"import json from "@bim/std/json"; json.stringify_pretty(17)"#,
                "indent must be between 0 and 16",
            ),
        ];
        for (index, (source, expected)) in cases.into_iter().enumerate() {
            let path = directory.join(format!("case-{index}.forma"));
            fs::write(&path, source).unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            let failure = module.execute(100_000).unwrap_err();
            assert!(failure.message.contains(expected), "{}", failure.message);
        }

        let path = directory.join("compact.forma");
        fs::write(
            &path,
            r#"import json from "@bim/std/json";
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
        fs::write(directory.join("answer.forma"), "40 + 2").unwrap();
        fs::write(
            directory.join("main.forma"),
            "import user from \"./user.json\";\
             import answer from \"./answer.forma\";\
             @struct type User = {name: String, age: Int};\
             let checked: User = user;\
             (checked.name, answer)",
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(module.dependencies.len(), 3);
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "(\"Ada\", 42)"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_toml_modules_with_temporal_tags_and_reuses_resolved_identity() {
        let directory = fixture_dir();
        fs::write(
            directory.join("config.toml"),
            r#"title = "Forma"
released = 2026-08-04
[environment]
PATH = "/bin"
[[tools]]
name = "forma"
[[tools]]
name = "rustc"
"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import config from "./config.toml";
               import same from "./sub/../config.toml";
               import toml from "@bim/std/toml";
               type TomlDate = toml.DateTime;
               @struct type Tool = {name: String};
               @struct type Config = {
                   title: String,
                   released: TomlDate,
                   environment: Dict(String),
                   tools: Array(Tool),
               };
               let checked: Config = config;
               (checked.released, checked.tools, same.title)"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(module.dependencies.len(), 2);
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "('LocalDate(\"2026-08-04\"), [{name: \"forma\"}, {name: \"rustc\"}], \"Forma\")"
        );
        let toml = module
            .workspace
            .module_by_path(&canonicalize(&directory.join("config.toml")).unwrap())
            .unwrap();
        assert_eq!(toml.kind, WorkspaceModuleKind::Toml);
        assert_eq!(toml.state, WorkspaceModuleState::Known);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn toml_annotation_errors_label_data_and_type_declaration() {
        let directory = fixture_dir();
        fs::write(
            directory.join("user.toml"),
            "name = \"Ada\"\nage = \"old\"\n",
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            "import user from \"./user.toml\";\n\
             @struct type User = {name: String, age: Int};\n\
             let checked: User = user;\n\
             checked",
        )
        .unwrap();
        let error =
            load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap_err();
        let message = error.message();
        assert!(
            message.contains("user.toml:2:7: binding checked has type"),
            "{message}"
        );
        assert!(
            message.contains("main.forma:2:1: type requirement declared here"),
            "{message}"
        );

        fs::write(
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import result from "@bim/std/result";
               import user from "./user.toml";
               @struct type User = {name: String, age: Int};
               codec.decode(User, user) |> result.unwrap"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let error = module.execute(100_000).unwrap_err();
        let rendered = error.with_sources(&module.sources).to_string();
        assert!(rendered.contains("user.toml:2:7:"), "{rendered}");
        assert!(rendered.contains("main.forma:4:"), "{rendered}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recoverable_workspace_retains_invalid_toml_source_and_diagnostics() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        let config = directory.join("config.toml");
        fs::write(&config, "name = \"first\"\nname = \"second\"\n").unwrap();
        fs::write(&main, "import config from \"./config.toml\"; config").unwrap();

        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        let config = snapshot
            .module_by_path(&canonicalize(&config).unwrap())
            .unwrap();
        assert_eq!(config.kind, WorkspaceModuleKind::Toml);
        assert_eq!(config.state, WorkspaceModuleState::Unavailable);
        let source = config.source.expect("invalid TOML source is retained");
        assert!(snapshot.diagnostics().iter().any(|diagnostic| {
            diagnostic.message.contains("duplicate TOML key")
                && diagnostic
                    .labels
                    .first()
                    .is_some_and(|label| label.location.source == source)
        }));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_yaml_modules_and_retains_invalid_workspace_source() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        let config = directory.join("config.yaml");
        fs::write(
            &config,
            "name: Forma\nfeatures:\n  - static data\n  - provenance\nlegacy: yes\n",
        )
        .unwrap();
        fs::write(
            &main,
            "import config from \"./config.yaml\"; (config.name, config.features, config.legacy)",
        )
        .unwrap();

        let module = load_module(&main, BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "(\"Forma\", [\"static data\", \"provenance\"], \"yes\")"
        );
        let yaml = module
            .workspace
            .module_by_path(&canonicalize(&config).unwrap())
            .unwrap();
        assert_eq!(yaml.kind, WorkspaceModuleKind::Yaml);
        assert_eq!(yaml.state, WorkspaceModuleState::Known);

        fs::write(&config, "name: first\nname: second\n").unwrap();
        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        let yaml = snapshot
            .module_by_path(&canonicalize(&config).unwrap())
            .unwrap();
        assert_eq!(yaml.kind, WorkspaceModuleKind::Yaml);
        assert_eq!(yaml.state, WorkspaceModuleState::Unavailable);
        assert!(
            snapshot
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("duplicate YAML key"))
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_module_cycles() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            "import a from \"./a.forma\"; a",
        )
        .unwrap();
        fs::write(directory.join("a.forma"), "import b from \"./b.forma\"; b").unwrap();
        fs::write(directory.join("b.forma"), "import a from \"./a.forma\"; a").unwrap();
        let error =
            load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.message().contains("cycle"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unregistered_and_nested_native_declarations_with_locations() {
        let directory = fixture_dir();
        fs::write(
            directory.join("missing-native.forma"),
            "native missing: Fn(Int) -> Int; missing(1)",
        )
        .unwrap();
        let missing = load_module(
            directory.join("missing-native.forma"),
            BTreeMap::new(),
            100_000,
        )
        .unwrap_err();
        assert!(missing.message().contains("not registered"));
        assert!(missing.to_string().contains("missing-native.forma:1:1"));

        fs::write(
            directory.join("nested-native.forma"),
            "let value = { native hidden: Fn(Int) -> Int; 1 }; value",
        )
        .unwrap();
        let nested = load_module(
            directory.join("nested-native.forma"),
            BTreeMap::new(),
            100_000,
        )
        .unwrap_err();
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
            directory.join("countdown.forma"),
            "def countdown: Fn(Int) -> Int = fn(n) { if n < 1 { 0 } else { countdown(n - 1) } }; countdown",
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            "import countdown from \"./countdown.forma\"; countdown(4)",
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(module.execute(100_000).unwrap().to_string(), "0");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn external_input_is_any_and_available_at_runtime() {
        let directory = fixture_dir();
        fs::write(directory.join("main.forma"), "input").unwrap();
        let input = parse_json("input", r#"{"value":42}"#).unwrap();
        let module = load_module(
            directory.join("main.forma"),
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
            directory.join("main.forma"),
            "import user from \"./user.json\";\n\
             @struct type User = {name: String, age: Int};\n\
             let checked: User = user;\n\
             checked",
        )
        .unwrap();
        let error =
            load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap_err();
        let message = error.message();
        assert!(
            message.contains("user.json:1:21: binding checked has type"),
            "{message}"
        );
        assert!(
            message.contains("main.forma:2:1: type requirement declared here"),
            "{message}"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn module_execution_uses_evaluation_fuel_semantics() {
        let directory = fixture_dir();
        fs::write(directory.join("straight.forma"), "40 + 2").unwrap();
        let straight = load_module(directory.join("straight.forma"), BTreeMap::new(), 0).unwrap();
        assert_eq!(straight.execute(0).unwrap().to_string(), "42");

        fs::write(
            directory.join("call.forma"),
            "let identity = fn(value) { value }; identity(42)",
        )
        .unwrap();
        let call = load_module(directory.join("call.forma"), BTreeMap::new(), 0).unwrap();
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
            directory.join("typed.forma"),
            "type First = Array(Int); type Second = Array(Int); 0",
        )
        .unwrap();
        let module_limited = Engine::new(EngineConfig {
            module_quota: Quota::new(1, 1_000, u64::MAX),
            session_quota: Quota::new(100, 1_000, u64::MAX),
        });
        let error = module_limited
            .load_module(directory.join("typed.forma"), BTreeMap::new())
            .unwrap_err();
        assert!(error.message().contains("fuel"));

        fs::write(directory.join("value.forma"), "[1]").unwrap();
        let session_limited = Engine::new(EngineConfig {
            module_quota: Quota::new(100, 1_000, u64::MAX),
            session_quota: Quota::new(100, 1_000, 0),
        });
        let module = session_limited
            .load_module(directory.join("value.forma"), BTreeMap::new())
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
            resolver: ModuleResolver::for_root(&data).unwrap(),
            cache: HashMap::new(),
            core_modules: HashMap::new(),
            main: MainWorld::building(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
            semantic_inputs: BTreeMap::new(),
        };

        let first = loader.load_value(&data).unwrap();
        let counts = loader.main.heap.counts();
        let data_id = loader.resolver.resolve_root(&data).unwrap().id;
        let first_root = match loader.cache.get(&data_id).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };
        let second = loader.load_value(&data).unwrap();
        let second_root = match loader.cache.get(&data_id).unwrap() {
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
            directory.join("main.forma"),
            r#"import arrays from "@bim/std/array"; arrays.map([1, 2], fn(x) { x + 1 })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("main.forma"),
            r#"import arrays from "@bim/std/array";
               let values = [1, 2, 3];
               let empty: Array(Int) = [];
               {
                   length: arrays.length(values),
                   mapped: arrays.map(values, fn(value) { value + 10 }),
                   filtered: arrays.filter(values, fn(value) { 1 < value }),
                   flattened: arrays.flat_map(values, fn(value) { [value, value] }),
                   folded: arrays.fold(values, 0, fn(total, value) { total + value }),
                   empty_map: arrays.map(empty, fn(value) { value / 0 }),
                   empty_filter: arrays.filter(empty, fn(unused) { 'True }),
                   empty_flat_map: arrays.flat_map(empty, fn(value) { [value] }),
                   empty_fold: arrays.fold(empty, 42, fn(total, value) { total + value }),
                   nested: arrays.map(values, fn(value) {
                       arrays.fold([value, value], 0, fn(total, item) { total + item })
                   }),
                   pipelined: values |> arrays.map\(_, fn(value) { value + 20 }),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deterministic_array_string_and_path_modules_cover_plan_composition() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import arrays from "@bim/std/array";
               import paths from "@bim/std/path";
               import strings from "@bim/std/string";
               {
                   concat: arrays.concat([[1, 2], [], [3]]),
                   any: arrays.any([1, 0], fn(value) {
                       if 0 < value { 'True } else { value / 0 < 1 }
                   }),
                   all: arrays.all([0, 1], fn(value) {
                       if value < 1 { 'False } else { value / 0 < 1 }
                   }),
                   found: arrays.find([1, 2, 3], fn(value) { 1 < value }),
                   missing: arrays.find([1], fn(value) { value < 0 }),
                   empty_any: arrays.any([], fn(value) { value / 0 < 1 }),
                   empty_all: arrays.all([], fn(value) { value / 0 < 1 }),
                   chars: strings.length("形态a"),
                   joined: strings.join(["a", "形", "c"], ":"),
                   split: strings.split("a::形", ":"),
                   scalar_split: strings.split("a形", ""),
                   starts: strings.starts_with("形态", "形"),
                   ends: strings.ends_with("forma", "ma"),
                   contains: strings.contains("forma", "orm"),
                   replaced: strings.replace("a-b-a", "a", "xy"),
                   lines: strings.lines("a\r\nb\n"),
                   joined_lines: strings.join_lines(["a", "形", "c"]),
                   indented: strings.indent("a\n\nb", 2),
                   trailing: strings.ensure_trailing_newline("a"),
                   margin: strings.trim_margin(r"  |a
    |b
unchanged", "|"),
                   normalized: paths.normalize("/a/./b/../../../../c"),
                   relative: paths.normalize("../../a/../b"),
                   joined_path: paths.join(["/tool", "bin", "../lib", "gcc"]),
                   restarted: paths.join(["ignored", "/absolute", "file"]),
                   parent: paths.parent("/a/b/../c"),
                   root_parent: paths.parent("/"),
                   file_name: paths.file_name("a/b/../c"),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert_eq!(result.get("concat").unwrap().to_string(), "[1, 2, 3]");
        assert_eq!(result.get("any").unwrap().to_string(), "'True");
        assert_eq!(result.get("all").unwrap().to_string(), "'False");
        assert_eq!(result.get("found").unwrap().to_string(), "'Some(2)");
        assert_eq!(result.get("missing").unwrap().to_string(), "'None");
        assert_eq!(result.get("empty_any").unwrap().to_string(), "'False");
        assert_eq!(result.get("empty_all").unwrap().to_string(), "'True");
        assert_eq!(result.get("chars").unwrap().to_string(), "3");
        assert_eq!(result.get("joined").unwrap().to_string(), r#""a:形:c""#);
        assert_eq!(
            result.get("split").unwrap().to_string(),
            r#"["a", "", "形"]"#
        );
        assert_eq!(
            result.get("scalar_split").unwrap().to_string(),
            r#"["", "a", "形", ""]"#
        );
        assert_eq!(result.get("starts").unwrap().to_string(), "'True");
        assert_eq!(result.get("ends").unwrap().to_string(), "'True");
        assert_eq!(result.get("contains").unwrap().to_string(), "'True");
        assert_eq!(result.get("replaced").unwrap().to_string(), r#""xy-b-xy""#);
        assert_eq!(
            result.get("lines").unwrap().to_string(),
            r#"["a", "b", ""]"#
        );
        assert_eq!(
            result.get("joined_lines").unwrap().to_string(),
            "\"a\\n形\\nc\""
        );
        assert_eq!(
            result.get("indented").unwrap().to_string(),
            "\"  a\\n\\n  b\""
        );
        assert_eq!(result.get("trailing").unwrap().to_string(), "\"a\\n\"");
        assert_eq!(
            result.get("margin").unwrap().to_string(),
            "\"a\\nb\\nunchanged\""
        );
        assert_eq!(result.get("normalized").unwrap().to_string(), r#""/c""#);
        assert_eq!(result.get("relative").unwrap().to_string(), r#""../../b""#);
        assert_eq!(
            result.get("joined_path").unwrap().to_string(),
            r#""/tool/lib/gcc""#
        );
        assert_eq!(
            result.get("restarted").unwrap().to_string(),
            r#""/absolute/file""#
        );
        assert_eq!(result.get("parent").unwrap().to_string(), r#"'Some("/a")"#);
        assert_eq!(result.get("root_parent").unwrap().to_string(), "'None");
        assert_eq!(
            result.get("file_name").unwrap().to_string(),
            r#"'Some("c")"#
        );

        for (source, expected) in [
            (
                "import strings from \"@bim/std/string\"; strings.indent(\"x\", -1)",
                "indentation width must be non-negative",
            ),
            (
                "import strings from \"@bim/std/string\"; strings.trim_margin(\"x\", \"\")",
                "margin marker must not be empty",
            ),
        ] {
            fs::write(directory.join("invalid.forma"), source).unwrap();
            let module =
                load_module(directory.join("invalid.forma"), BTreeMap::new(), 100_000).unwrap();
            let error = module.execute(100_000).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn homogeneous_dict_metadata_preserves_types_through_core_codecs_and_schema() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        fs::write(
            &main,
            r#"import codec from "@bim/std/codec";
               import dicts from "@bim/std/dict";
               import json from "@bim/std/json";
               import result from "@bim/std/result";
               type Env = Dict(String);
               let env: Env = {PATH: "/bin", HOME: "/tmp"};
               let decoded = codec.decode(Env, {SHELL: "/bin/sh"}) |> result.unwrap;
               {
                   env: env,
                   decoded: decoded,
                   values: dicts.values(env),
                   built: dicts.from_pairs([("A", "one"), ("B", "two")]),
                   encoded: codec.encode(Env, decoded) |> result.unwrap,
                   schema: json.schema(Env),
               }"#,
        )
        .unwrap();

        let module = load_module(&main, BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "{built: Any, decoded: Dict<String>, encoded: Any, env: Dict<String>, schema: Any, values: Any}"
        );
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected Dict output")
        };
        assert_eq!(
            output.get("values").unwrap().to_string(),
            "[\"/tmp\", \"/bin\"]"
        );
        assert_eq!(
            output.get("built").unwrap().to_string(),
            "{A: \"one\", B: \"two\"}"
        );
        assert_eq!(
            output.get("encoded").unwrap().to_string(),
            "{SHELL: \"/bin/sh\"}"
        );
        let Value::Dict(schema) = output.get("schema").unwrap() else {
            panic!("expected schema Dict")
        };
        assert_eq!(schema.get("type").unwrap().to_string(), "\"object\"");
        assert_eq!(
            schema.get("additionalProperties").unwrap().to_string(),
            "{type: \"string\"}"
        );

        fs::write(
            &main,
            r#"type Env = Dict(String);
               let env: Env = {GOOD: "yes", BAD: 1};
               env"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.to_string().contains("BAD"), "{error}");
        assert!(error.to_string().contains("Int"), "{error}");
        assert!(error.to_string().contains("String"), "{error}");

        fs::write(
            &main,
            r#"@struct type Fixed = {a: String};
               let dynamic: Dict(String) = {a: "value"};
               let fixed: Fixed = dynamic;
               fixed"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.to_string().contains("not assignable"), "{error}");
        assert!(error.to_string().contains("Dict<String>"), "{error}");

        fs::write(
            &main,
            r#"@struct type Fixed = {a: String};
               let read: Fn(Fixed) -> String = fn(value) { value.a };
               let dynamic: Dict(String) = {a: "value"};
               read(dynamic)"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(
            error.to_string().contains("cannot unify Dict<String>"),
            "{error}"
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recursive_dict_metadata_reuses_existing_schema_links() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import json from "@bim/std/json";
               @struct type Node = {children: Dict(Node)};
               json.schema(Node)"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let schema = module.execute(100_000).unwrap().to_string();
        assert!(schema.contains("additionalProperties"), "{schema}");
        assert!(schema.contains("$defs"), "{schema}");
        assert!(schema.contains("$ref"), "{schema}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generic_core_exports_instantiate_per_member_access_but_not_per_local_use() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        fs::write(
            &main,
            r#"import arrays from "@bim/std/array";
               {
                   ints: arrays.map([1, 2], fn(value) { value + 1 }),
                   strings: arrays.map(["a"], fn(value) { value }),
               }"#,
        )
        .unwrap();
        let module = load_module(&main, BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "{ints: Array<Int>, strings: Array<String>}"
        );

        fs::write(
            &main,
            r#"import arrays from "@bim/std/array";
               let map = arrays.map;
               (map([1], fn(value) { value }), map(["a"], fn(value) { value }))"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.to_string().contains("cannot unify String with Int"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generic_definition_exports_instantiate_per_member_access() {
        let directory = fixture_dir();
        fs::write(
            directory.join("identity.forma"),
            r#"decl identity: for(A) Fn(A) -> A;
               def identity = fn(value) { value };
               {identity: identity}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import generic from "./identity.forma";
               (generic.identity(1), generic.identity("x"), generic.identity[_](2))"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "(Int, String, Int)"
        );
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "(1, \"x\", 2)"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn inferred_generic_let_exports_instantiate_per_member_access() {
        let directory = fixture_dir();
        fs::write(
            directory.join("identity.forma"),
            r#"let identity = fn(value) { value };
               {identity: identity}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import generic from "./identity.forma";
               (generic.identity(1), generic.identity("x"), generic.identity[_](2))"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "(Int, String, Int)"
        );
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "(1, \"x\", 2)"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn acyclic_generic_def_exports_instantiate_per_member_access() {
        let directory = fixture_dir();
        fs::write(
            directory.join("identity.forma"),
            r#"def identity = fn(value) { value };
               {identity: identity}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import generic from "./identity.forma";
               (generic.identity(1), generic.identity("x"))"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "(Int, String)"
        );
        assert_eq!(module.execute(100_000).unwrap().to_string(), "(1, \"x\")");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn typed_metadata_constructors_cross_module_interfaces() {
        let directory = fixture_dir();
        fs::write(
            directory.join("constructors.forma"),
            r#"def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) { Option(Item) };
               {Maybe: Maybe}"#,
        )
        .unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import constructors from "./constructors.forma";
               type MaybeInt = constructors.Maybe(Int);
               let value: MaybeInt = 'None;
               value"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "enum {None, Some(Int)}"
        );
        assert_eq!(module.execute(100_000).unwrap().to_string(), "'None");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_array_callbacks_share_fuel_allocation_and_tool_stage_execution() {
        let directory = fixture_dir();
        let item_count = 1_500usize;
        let data = format!("[{}]", vec!["1"; item_count].join(","));
        fs::write(directory.join("values.json"), data).unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import arrays from "@bim/std/array";
               import values from "./values.json";
               arrays.map(values, fn(value) { value + 1 })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();

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
            directory.join("types.forma"),
            r#"import arrays from "@bim/std/array";
               type Pair = Tuple(arrays.map([Int, String], fn(item) { item }));
               let pair: Pair = (1, "one");
               pair"#,
        )
        .unwrap();
        let types = load_module(directory.join("types.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(types.execute(100_000).unwrap().to_string(), "(1, \"one\")");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_array_reports_boundary_and_callback_result_errors() {
        let directory = fixture_dir();
        let analysis_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(
                &path,
                format!("import arrays from \"@bim/std/array\"; {expression}"),
            )
            .unwrap();
            load_module(path, BTreeMap::new(), 100_000).unwrap_err()
        };
        let run_error = |name: &str, expression: &str| {
            let path = directory.join(name);
            fs::write(
                &path,
                format!("import arrays from \"@bim/std/array\"; {expression}"),
            )
            .unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            module.execute(100_000).unwrap_err()
        };

        assert!(
            analysis_error("length.forma", "arrays.length(1)")
                .to_string()
                .contains("cannot unify Int with Array")
        );
        assert!(
            analysis_error("arity.forma", "arrays.map([1], fn(a, b) { a + b })")
                .to_string()
                .contains("cannot unify")
        );
        assert!(
            analysis_error("filter.forma", "arrays.filter([1], fn(value) { value })")
                .to_string()
                .contains("cannot unify Int with enum {False, True}")
        );
        assert!(
            analysis_error(
                "flat-map.forma",
                "arrays.flat_map([1], fn(value) { value })"
            )
            .to_string()
            .contains("cannot unify Int with Array")
        );
        let callback = run_error("callback.forma", "arrays.map([1], fn(value) { value / 0 })");
        assert!(callback.to_string().contains("callback.forma:1:"));
        assert!(
            callback
                .trace
                .iter()
                .any(|frame| frame.function == "@bim/std/array.map")
        );

        let nested_depth = run_error(
            "nested-depth.forma",
            "decl nest: Fn(Int) -> Int;
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

        let unknown_path = directory.join("unknown-core.forma");
        fs::write(
            &unknown_path,
            "import unknown from \"@bim/std/unknown\"; unknown",
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
    fn core_option_and_result_combinators_are_generic_xl_definitions() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import options from "@bim/std/option";
               import results from "@bim/std/result";
               let ok: Result(Int, String) = 'Ok(3);
               let err: Result(Int, String) = 'Err("bad");
               {
                   option_map: options.map('Some(2), fn(value) { value + 1 }),
                   option_map_none: options.map('None, fn(value) { value / 0 }),
                   option_flat_map: options.flat_map('Some(2), fn(value) { 'Some(value + 2) }),
                   option_flat_none: options.flat_map('None, fn(value) { 'Some(value / 0) }),
                   option_some_or: options.unwrap_or('Some(4), 9),
                   option_none_or: options.unwrap_or('None, 9),
                   option_is_some: options.is_some('Some("x")),
                   option_is_none: options.is_some('None),
                   result_map: results.map(ok, fn(value) { value + 1 }),
                   result_map_err: results.map(err, fn(value) { value / 0 }),
                   result_err_map: results.map_err(err, fn(error) { error }),
                   result_err_map_ok: results.map_err(ok, fn(error) { error }),
                   result_ok_or: results.unwrap_or(ok, 9),
                   result_err_or: results.unwrap_or(err, 9),
                   result_is_ok: results.is_ok(ok),
                   result_is_err: results.is_ok(err),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected combinator results")
        };
        let expected = [
            ("option_map", "'Some(3)"),
            ("option_map_none", "'None"),
            ("option_flat_map", "'Some(4)"),
            ("option_flat_none", "'None"),
            ("option_some_or", "4"),
            ("option_none_or", "9"),
            ("option_is_some", "'True"),
            ("option_is_none", "'False"),
            ("result_map", "'Ok(4)"),
            ("result_map_err", "'Err(\"bad\")"),
            ("result_err_map", "'Err(\"bad\")"),
            ("result_err_map_ok", "'Ok(3)"),
            ("result_ok_or", "3"),
            ("result_err_or", "9"),
            ("result_is_ok", "'True"),
            ("result_is_err", "'False"),
        ];
        for (name, expected) in expected {
            assert_eq!(result.get(name).unwrap().to_string(), expected, "{name}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn typed_metadata_witnesses_flow_through_codec_and_validation_boundaries() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import result from "@bim/std/result";
               @struct type User = {name: String};
               let decoded = codec.decode(User, {name: "Ada"});
               let encoded = codec.encode(User, {name: "Lin"});
               let checked = validate(User, {name: "Grace"});
               let invalid = validate(User, {name: 1});
               let formatted = result.map_err(
                   codec.decode(User, {name: 1}),
                   codec.format_error,
               );
               let chained = result.flat_map(
                   codec.decode(User, {name: "Mira"}),
                   fn(user) { validate(User, user) },
               );
               let name = result.unwrap(result.map(
                   codec.decode(User, {name: "Kai"}),
                   fn(user) { user.name },
               ));
               let blame_error = BlameError;
               {
                   decoded: decoded,
                   encoded: encoded,
                   checked: checked,
                   invalid: invalid,
                   formatted: formatted,
                   chained: chained,
                   name: name,
                   BlameError: blame_error,
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["decoded"]),
            "enum {Err({data: Any, message: String, rule: Any}), Ok({name: String})}"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["checked"]),
            "enum {Err({data: Any, message: String, rule: Any}), Ok({name: String})}"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["encoded"]),
            "enum {Err({data: Any, message: String, rule: Any}), Ok(Any)}"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["formatted"]),
            "enum {Err(String), Ok({name: String})}"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["chained"]),
            "enum {Err({data: Any, message: String, rule: Any}), Ok({name: String})}"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["name"]),
            "String"
        );
        assert_eq!(
            module
                .analysis
                .display(module.analysis.binding_types["blame_error"]),
            "TypeOf({data: Any, message: String, rule: Any})"
        );
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("typed boundary test must return a Dict")
        };
        assert_eq!(output.get("name").unwrap().to_string(), "\"Kai\"");
        assert!(
            output
                .get("formatted")
                .unwrap()
                .to_string()
                .contains("expected String")
        );
        let Value::Tagged { tag, payload } = output.get("invalid").unwrap() else {
            panic!("invalid validation must return a Result")
        };
        assert_eq!(tag.name(), "Err");
        let Value::Dict(error) = payload.as_ref() else {
            panic!("validation failure must be a structured error")
        };
        assert!(
            error
                .get("message")
                .unwrap()
                .to_string()
                .contains("must be String")
        );
        assert_eq!(error.get("data").unwrap().to_string(), "{name: 1}");
        assert!(error.get("rule").unwrap().to_string().contains("'Struct"));
        assert!(
            output
                .get("BlameError")
                .unwrap()
                .to_string()
                .contains("'Struct")
        );

        fs::write(
            directory.join("wrong-encode.forma"),
            r#"import codec from "@bim/std/codec";
               @struct type User = {name: String};
               codec.encode(User, {name: 1})"#,
        )
        .unwrap();
        let error = load_module(
            directory.join("wrong-encode.forma"),
            BTreeMap::new(),
            100_000,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot unify Int with String"));

        fs::write(
            directory.join("erased.forma"),
            "let metadata: Type = Int; validate(metadata, 1)",
        )
        .unwrap();
        let error =
            load_module(directory.join("erased.forma"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.to_string().contains("TypeOf"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_dict_enumerates_constructs_and_merges_in_canonical_order() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import dicts from "@bim/std/dict";
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

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
    fn homogeneous_dict_combinators_preserve_types_and_canonical_order() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import dicts from "@bim/std/dict";
               let source: Dict(Int) = {z: 3, a: 1, middle: 2};
               let empty: Dict(Int) = {};
               {
                   mapped: dicts.map_values(source, fn(value) { `v\{value}` }),
                   filtered: dicts.filter(source, fn(value) { 1 < value }),
                   folded: dicts.fold(source, "", fn(total, key, value) {
                       `\{total}\{key}=\{value};`
                   }),
                   empty_mapped: dicts.map_values(empty, fn(value) { `v\{value}` }),
                   empty_filtered: dicts.filter(empty, fn(value) { 0 < value }),
                   empty_folded: dicts.fold(empty, 42, fn(total, key, value) {
                       total + value
                   }),
               }"#,
        )
        .unwrap();

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.analysis.display(module.analysis.result_type),
            "{empty_filtered: Dict<Int>, empty_folded: Int, empty_mapped: Dict<String>, filtered: Dict<Int>, folded: String, mapped: Dict<String>}"
        );
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert_eq!(
            result.get("mapped").unwrap().to_string(),
            r#"{a: "v1", middle: "v2", z: "v3"}"#
        );
        assert_eq!(
            result.get("filtered").unwrap().to_string(),
            "{middle: 2, z: 3}"
        );
        assert_eq!(
            result.get("folded").unwrap().to_string(),
            r#""a=1;middle=2;z=3;""#
        );
        assert_eq!(result.get("empty_mapped").unwrap().to_string(), "{}");
        assert_eq!(result.get("empty_filtered").unwrap().to_string(), "{}");
        assert_eq!(result.get("empty_folded").unwrap().to_string(), "42");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn homogeneous_dict_combinators_reject_invalid_contracts_and_trace_callbacks() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        fs::write(
            &main,
            r#"import dicts from "@bim/std/dict";
               dicts.filter({a: 1}, fn(value) { value })"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot unify Int with enum {False, True}")
        );

        fs::write(
            &main,
            r#"import dicts from "@bim/std/dict";
               let mixed = {number: 1, text: "two"};
               dicts.map_values(mixed, fn(value) { value })"#,
        )
        .unwrap();
        let error = load_module(&main, BTreeMap::new(), 100_000).unwrap_err();
        assert!(error.to_string().contains("cannot unify"));

        fs::write(
            &main,
            r#"import dicts from "@bim/std/dict";
               let source: Dict(Int) = {a: 1};
               dicts.map_values(source, fn(value) { value / 0 })"#,
        )
        .unwrap();
        let module = load_module(&main, BTreeMap::new(), 100_000).unwrap();
        let error = module.execute(100_000).unwrap_err();
        assert!(error.to_string().contains("main.forma:3:"));
        assert!(
            error
                .trace
                .iter()
                .any(|frame| frame.function == "@bim/std/dict.map_values")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_dict_supports_tool_stage_and_exact_output_quota() {
        let directory = fixture_dir();
        fs::write(directory.join("data.json"), r#"{"a":1,"long":2}"#).unwrap();
        fs::write(
            directory.join("main.forma"),
            r#"import dicts from "@bim/std/dict";
               import data from "./data.json";
               dicts.keys(data)"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("types.forma"),
            r#"import dicts from "@bim/std/dict";
               type Pair = Tuple(dicts.values({ first: Int, second: String }));
               let pair: Pair = (1, "one");
               pair"#,
        )
        .unwrap();
        let types = load_module(directory.join("types.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(types.execute(100_000).unwrap().to_string(), "(1, \"one\")");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_attributes_normalizes_flattens_and_inspects_arbitrary_values() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import attributes from "@bim/std/attributes";
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

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert_eq!(
            result.get("all").unwrap().to_string(),
            "{only_inner: 1, only_outer: 2, shared: \"addition\", vendor:acme.flag: 'True}"
        );
        assert_eq!(
            result.get("shared").unwrap().to_string(),
            "'Some(\"addition\")"
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
            directory.join("main.forma"),
            r#"import attributes from "@bim/std/attributes";
               import codec from "@bim/std/codec";
               let rename = fn(name) {
                   let decorate: Fn(Any, Any) -> Any = fn(ctx, value) {
                       attributes.add(value, { "@bim/std/json.rename": name })
                   }; decorate
               };
               let model: Fn(Any, Any) -> Any = fn(ctx, value) {
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

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected Dict result")
        };
        assert!(
            result
                .get("checked")
                .unwrap()
                .to_string()
                .starts_with("'Ok(")
        );
        assert!(
            result
                .get("decoded")
                .unwrap()
                .to_string()
                .starts_with("'Ok(")
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
                .get("@bim/std/json.rename")
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
            directory.join("main.forma"),
            r#"import attributes from "@bim/std/attributes";
               let annotate = fn(key, payload) {
                   let decorate: Fn(Any, Any) -> Any = fn(ctx, value) { attributes.add(value, { marker: (key, payload) }) };
                   decorate
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
               let payload: Choice = 'User({ name: "Ada", role: "admin" });
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

        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected model result")
        };
        assert!(result.get("unit").unwrap().to_string().starts_with("'Ok("));
        assert!(
            result
                .get("payload")
                .unwrap()
                .to_string()
                .starts_with("'Ok(")
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
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               @enum
               type Choice = { None: 'None, Number: Int };
               {
                   unknown: validate(Choice, 'Other),
                   missing: validate(Choice, 'Number),
                   unexpected: validate(Choice, 'None(1)),
                   wrong: validate(Choice, 'Number("one")),
                   codec: codec.decode(Choice, "None"),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected validation results")
        };
        for field in ["unknown", "missing", "unexpected", "wrong"] {
            assert!(result.get(field).unwrap().to_string().starts_with("'Err("));
        }
        assert_eq!(result.get("codec").unwrap().to_string(), "'Ok('None)");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn enum_json_codecs_round_trip_external_and_untagged_representations() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               import result from "@bim/std/result";
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
                   fatal: codec.encode(Event, 'FatalError("boom")) |> result.unwrap,
                   nested: codec.encode(Envelope, {event: 'UserJoined({name: "Lin"})}) |> result.unwrap,
                   text: codec.decode(Scalar, "hello") |> result.unwrap,
                   count: codec.encode(Scalar, 'Count(3)) |> result.unwrap,
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(output) = module.execute(100_000).unwrap() else {
            panic!("expected Enum codec results")
        };
        assert_eq!(output.get("idle").unwrap().to_string(), "'Idle");
        assert_eq!(
            output.get("joined").unwrap().to_string(),
            "'UserJoined({name: \"Ada\"})"
        );
        assert_eq!(
            output.get("fatal").unwrap().to_string(),
            "{fatal: \"boom\"}"
        );
        assert_eq!(
            output.get("nested").unwrap().to_string(),
            "{event: {userJoined: {name: \"Lin\"}}}"
        );
        assert_eq!(output.get("text").unwrap().to_string(), "'Text(\"hello\")");
        assert_eq!(output.get("count").unwrap().to_string(), "3");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn untagged_enum_json_codec_rejects_no_match_and_ambiguity() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               @json.untagged @enum type Scalar = {Text: String, Count: Int};
               @json.untagged @enum type Ambiguous = {Anything: Any, Text: String};
               {
                   no_match: codec.decode(Scalar, []),
                   ambiguous: codec.decode(Ambiguous, "text"),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("main.forma"),
            r#"import data from "./data.json";
               import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               import result from "@bim/std/result";
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
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let failure = module.execute(100_000).unwrap_err();
        assert!(failure.message.contains("$.userId"));
        assert!(failure.data_location().is_some());
        assert!(failure.rule_location().is_some());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_schema_maps_composites_and_obeys_allocation_quota() {
        let directory = fixture_dir();
        let path = directory.join("main.forma");
        fs::write(
            &path,
            r#"import json from "@bim/std/json";
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
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               import result from "@bim/std/result";
               {
                   boolean: codec.decode(Bool, 'True) |> result.unwrap,
                   none: codec.decode(Option(Int), 'None) |> result.unwrap,
                   some: codec.decode(Option(Int), 3) |> result.unwrap,
                   encoded: codec.encode(Option(Int), 'Some(4)) |> result.unwrap,
                   bool_schema: json.schema(Bool),
                   option_schema: json.schema(Option(Int)),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let output = module.execute(100_000).unwrap().to_string();
        assert!(output.contains("boolean: 'True"), "{output}");
        assert!(output.contains("none: 'None"), "{output}");
        assert!(output.contains("some: 'Some(3)"), "{output}");
        assert!(output.contains("encoded: 4"), "{output}");
        assert!(output.contains("type: \"boolean\""), "{output}");
        assert!(output.contains("type: \"null\""), "{output}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recursive_type_metadata_publishes_and_drives_codecs_and_schema_refs() {
        let directory = fixture_dir();
        fs::write(
            directory.join("Types.forma"),
            r#"import json from "@bim/std/json";
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
            load_module(directory.join("Types.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("main.forma"),
            r#"import Types from "./Types.forma";
               import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               import result from "@bim/std/result";
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
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let output = module.execute(100_000).unwrap().to_string();
        assert!(
            output.contains("children: [{children: [], value: 2}]"),
            "{output}"
        );
        assert!(
            output.contains("pair: {right: 'Some({left: 'None})}"),
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
            directory.join("bad.forma"),
            r#"import data from "./bad.json";
               import Types from "./Types.forma";
               import codec from "@bim/std/codec";
               import result from "@bim/std/result";
               codec.decode(Types.Node, data) |> result.unwrap"#,
        )
        .unwrap();
        let bad = load_module(directory.join("bad.forma"), BTreeMap::new(), 100_000).unwrap();
        let failure = bad.execute(100_000).unwrap_err();
        assert!(failure.message.contains("$.children[0].value"));
        assert!(failure.data_location().is_some());
        assert!(failure.rule_location().is_some());

        fs::write(
            directory.join("leak.forma"),
            r#"import Types from "./Types.forma";
               import json from "@bim/std/json";
               json.stringify(Types.Node)"#,
        )
        .unwrap();
        let leak = load_module(directory.join("leak.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               @struct type Forward = {next: Later};
               let premature = codec.decode(Forward, {next: 1});
               type Later = Int;
               premature"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "'Ok({next: 1})"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn builtin_bool_option_and_result_are_normalized_enum_metadata() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import attributes from "@bim/std/attributes";
               type Maybe = Option(attributes.add(Int, { marker: "payload" }));
               type Outcome = Result(String, Int);
               let compared: Bool = 1 < 2;
               let none: Maybe = 'None;
               let some: Maybe = 'Some(42);
               let ok: Outcome = 'Ok("done");
               let err: Outcome = 'Err(7);
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
                   wrong_some: validate(Maybe, 'Some("forty-two")),
               }"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(result) = module.execute(100_000).unwrap() else {
            panic!("expected built-in type results")
        };
        for field in ["compared", "none", "some", "ok", "err"] {
            assert!(result.get(field).unwrap().to_string().starts_with("'Ok("));
        }
        for field in ["wrong_bool", "wrong_some"] {
            assert!(result.get(field).unwrap().to_string().starts_with("'Err("));
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
        let invalid_path = directory.join("invalid.forma");
        fs::write(&invalid_path, "Option(1)").unwrap();
        let invalid = load_module(&invalid_path, BTreeMap::new(), 100_000).unwrap_err();
        assert!(invalid.message.contains("cannot unify Int with Type"));

        let quota_path = directory.join("quota.forma");
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
            run_error("context.forma", "struct('Bad, {x: Int})")
                .message
                .contains("model context")
        );
        assert!(
            run_error("empty.forma", "enum('None, {})")
                .message
                .contains("at least one variant")
        );
        assert!(
            run_error("field.forma", "struct('None, {x: 1})")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error("variant.forma", "enum('None, {Bad: 1})")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error("empty-union.forma", "union('None, [])")
                .message
                .contains("at least one variant")
        );
        assert!(
            run_error("union-variant.forma", "union('None, [1])")
                .message
                .contains("Type metadata")
        );
        assert!(
            run_error(
                "union-wrapper.forma",
                "union('None, [{kind: 'WithAttributes, inner: Int, attributes: []}])",
            )
            .message
            .contains("attributes must be a Dict")
        );

        for (name, source) in [
            ("uppercase-struct.forma", "Struct({x: Int})"),
            ("uppercase-union.forma", "Union([Int, String])"),
        ] {
            let path = directory.join(name);
            fs::write(&path, source).unwrap();
            let error = match load_module(path, BTreeMap::new(), 100_000) {
                Ok(_) => panic!("uppercase constructor must be absent"),
                Err(error) => error,
            };
            assert!(error.message.contains("unknown binding"));
        }

        let path = directory.join("quota.forma");
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
        let path = directory.join("main.forma");
        fs::write(
            &path,
            r#"import attributes from "@bim/std/attributes";
               attributes.normalize({kind: 'WithAttributes, inner: 1, attributes: []})"#,
        )
        .unwrap();
        let module = load_module(&path, BTreeMap::new(), 100_000).unwrap();
        let error = module.execute(100_000).unwrap_err();
        assert!(error.message.contains("attributes must be a Dict"));

        fs::write(
            &path,
            r#"import attributes from "@bim/std/attributes";
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
            directory.join("main.forma"),
            r#"import json from "@bim/std/json";
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
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        let Value::Dict(root) = module.execute(100_000).unwrap() else {
            panic!("expected attributed model")
        };
        let Value::Dict(root_attributes) = root.get("attributes").unwrap() else {
            panic!("expected root attributes")
        };
        assert_eq!(
            root_attributes
                .get("@bim/std/json.rename_all")
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
            attributes.get("@bim/std/json.rename").unwrap().to_string(),
            "\"outerName\""
        );
        assert_eq!(
            attributes.get("@bim/std/json.default").unwrap().to_string(),
            "7"
        );
        assert_eq!(
            attributes
                .get("@bim/std/json.skip_serializing_if")
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
                .get("@bim/std/json.flatten")
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
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               import result from "@bim/std/result";

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
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
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
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import debug from "@bim/std/debug";
               import json from "@bim/std/json";
               let zero = 0;
               def is_zero: Fn(Int) -> Bool = fn(value) { value == zero };
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
            directory.join("main.forma"),
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
        assert_eq!(value.to_string(), "'Ok({retained: 7})");
        assert_eq!(sink.events.lock().unwrap().len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_skip_serializing_if_rejects_invalid_function_contracts() {
        let directory = fixture_dir();
        fs::write(
            directory.join("arity.forma"),
            r#"import json from "@bim/std/json";
               def wrong: Fn(Any, Any) -> Bool = fn(left, right) { 'False };
               @struct type Model = {
                   @json.skip_serializing_if(wrong) value: Int,
               };
               0"#,
        )
        .unwrap();
        let arity =
            load_module(directory.join("arity.forma"), BTreeMap::new(), 100_000).unwrap_err();
        assert!(arity.message().contains("unary Func"), "{arity}");

        fs::write(
            directory.join("result.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               def identity: Fn(Any) -> Any = fn(value) { value };
               @struct type Model = {
                   @json.skip_serializing_if(identity) value: Int,
               };
               codec.encode(Model, {value: 1})"#,
        )
        .unwrap();
        let module = load_module(directory.join("result.forma"), BTreeMap::new(), 100_000).unwrap();
        let result = module.execute(100_000).unwrap_err();
        assert_eq!(result.kind, crate::RuntimeErrorKind::TypeMismatch);
        assert!(result.message.contains("must return 'True or 'False"));
        assert!(
            result
                .trace
                .iter()
                .any(|frame| frame.function == "@bim/std/codec.encode")
        );

        fs::write(
            directory.join("callback.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               def fails: Fn(Any) -> Int = fn(value) { 1 / 0 };
               @struct type Model = {
                   @json.skip_serializing_if(fails) value: Int,
               };
               codec.encode(Model, {value: 1})"#,
        )
        .unwrap();
        let callback =
            load_module(directory.join("callback.forma"), BTreeMap::new(), 100_000).unwrap();
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
                .any(|frame| frame.function == "@bim/std/codec.encode")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_skip_predicates_resume_at_nested_paths_and_before_flattening() {
        let directory = fixture_dir();
        fs::write(
            directory.join("main.forma"),
            r#"import codec from "@bim/std/codec";
               import json from "@bim/std/json";
               def is_zero: Fn(Int) -> Bool = fn(value) { value == 0 };
               def always: Fn(Any) -> Bool = fn(value) { 'True };
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
                   nested: {required: "present"},
               })"#,
        )
        .unwrap();
        let module = load_module(directory.join("main.forma"), BTreeMap::new(), 100_000).unwrap();
        assert_eq!(
            module.execute(100_000).unwrap().to_string(),
            "'Ok({items: [{}, {value: 2}]})"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn struct_json_codecs_reject_attribute_conflicts_and_invalid_defaults() {
        let directory = fixture_dir();
        let cases = [
            (
                "collision.forma",
                r#"import codec from "@bim/std/codec";
                   import json from "@bim/std/json";
                   @struct type T = {
                       @json.rename("same") first: Int,
                       @json.rename("same") second: Int,
                   };
                   codec.decode(T, {same: 1})"#,
                "duplicate external field name",
            ),
            (
                "flatten-type.forma",
                r#"import codec from "@bim/std/codec";
                   import json from "@bim/std/json";
                   @struct type T = {@json.flatten value: Int};
                   codec.decode(T, {})"#,
                "flatten requires Struct metadata",
            ),
            (
                "flatten-rename.forma",
                r#"import codec from "@bim/std/codec";
                   import json from "@bim/std/json";
                   @struct type Inner = {value: Int};
                   @struct type T = {
                       @json.flatten @json.rename("x") inner: Inner,
                   };
                   codec.decode(T, {value: 1})"#,
                "flatten cannot be combined",
            ),
            (
                "default.forma",
                r#"import codec from "@bim/std/codec";
                   import json from "@bim/std/json";
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
                format!("import json from \"@bim/std/json\"; {expression}"),
            )
            .unwrap();
            load_module(path, BTreeMap::new(), 100_000)
                .unwrap()
                .execute(100_000)
                .unwrap_err()
        };
        assert!(
            run_error("rename.forma", "json.rename(1)")
                .message
                .contains("expects a String")
        );
        assert!(
            run_error("case.forma", "json.rename_all('SnakeCase)")
                .message
                .contains("CamelCase")
        );
        assert!(
            run_error("skip.forma", "json.skip_serializing_if('Zero)")
                .message
                .contains("'Empty")
        );

        let path = directory.join("quota.forma");
        fs::write(
            &path,
            "import json from \"@bim/std/json\"; json.rename(\"name\")",
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
                format!("import dicts from \"@bim/std/dict\"; {expression}"),
            )
            .unwrap();
            let module = load_module(path, BTreeMap::new(), 100_000).unwrap();
            module.execute(100_000).unwrap_err()
        };

        assert!(
            run_error("keys.forma", "dicts.keys([])")
                .message
                .contains("Dict")
        );
        assert!(
            run_error("merge.forma", "dicts.merge({}, [])")
                .message
                .contains("right Dict")
        );
        assert!(
            run_error("pairs-array.forma", "dicts.from_pairs({})")
                .message
                .contains("Array")
        );
        assert!(
            run_error("pair-shape.forma", "dicts.from_pairs([(\"a\", 1, 2)])")
                .message
                .contains("two-element Tuple")
        );
        assert!(
            run_error("pair-key.forma", "dicts.from_pairs([('a, 1)])")
                .message
                .contains("key must be a String")
        );
        let duplicate = run_error(
            "duplicate.forma",
            "dicts.from_pairs([(\"a\", 1), (\"a\", 2)])",
        );
        assert!(duplicate.message.contains("duplicate field"));
        assert!(duplicate.to_string().contains("duplicate.forma:1:"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diamond_dependencies_reuse_the_same_persistent_root() {
        let directory = fixture_dir();
        let c = directory.join("c.forma");
        let a = directory.join("a.forma");
        let b = directory.join("b.forma");
        fs::write(&c, r#"{value: [1, 2, 3]}"#).unwrap();
        fs::write(&a, r#"import c from "./c.forma"; c"#).unwrap();
        fs::write(&b, r#"import c from "./c.forma"; c"#).unwrap();
        let mut loader = ModuleLoader {
            resolver: ModuleResolver::for_root(&a).unwrap(),
            cache: HashMap::new(),
            core_modules: HashMap::new(),
            main: MainWorld::building(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
            semantic_inputs: BTreeMap::new(),
        };

        loader.load_value(&a).unwrap();
        let counts_after_a = loader.main.heap.counts();
        loader.load_value(&b).unwrap();
        let root = |path: &Path| match loader
            .cache
            .get(&loader.resolver.resolve_root(path).unwrap().id)
            .unwrap()
        {
            ModuleState::Ready { root, .. } => *root,
        };

        assert_eq!(root(&a), root(&c));
        assert_eq!(root(&b), root(&c));
        assert_eq!(counts_after_a, loader.main.heap.counts());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recoverable_workspace_blocks_failed_imports_and_keeps_independent_facts() {
        let directory = fixture_dir();
        let model = directory.join("model.forma");
        let main = directory.join("main.forma");
        fs::write(
            &model,
            "type Broken = missing(Int); type Good = String; {Good: Good}",
        )
        .unwrap();
        fs::write(
            &main,
            "import model from \"./model.forma\";\
             type Local = String;\
             type Uses = model.Good;\
             type Down = Array(Uses);\
             0",
        )
        .unwrap();
        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        let main = snapshot
            .module_by_path(&canonicalize(&main).unwrap())
            .unwrap();
        let model = snapshot
            .module_by_path(&canonicalize(&model).unwrap())
            .unwrap();
        assert_eq!(main.state, WorkspaceModuleState::Partial);
        assert_eq!(model.state, WorkspaceModuleState::Partial);
        let fact = |module, name: &str| {
            &snapshot
                .definitions()
                .iter()
                .find(|definition| definition.module == module && definition.name == name)
                .unwrap()
                .ty
        };
        assert_eq!(fact(main.id, "Local").state, crate::FactState::Known);
        assert!(matches!(
            fact(main.id, "Uses").state,
            crate::FactState::Unknown(crate::UnknownReason::BlockedBy(_))
        ));
        assert!(matches!(
            fact(main.id, "Down").state,
            crate::FactState::Unknown(crate::UnknownReason::BlockedBy(_))
        ));
        assert_eq!(fact(model.id, "Good").state, crate::FactState::Known);
        let broken = fact(model.id, "Broken");
        let diagnostic = broken.diagnostics[0];
        assert!(
            snapshot.diagnostics()[diagnostic.index()]
                .message
                .contains("unknown binding")
        );
        assert!(main.imports.iter().any(|import| import.target == model.id));
        assert_ne!(main.source, model.source);
        let model_path = model.path.as_ref().unwrap();
        assert_eq!(model.name, "@src/model.forma");
        assert_eq!(
            snapshot.sources().get(model.source.unwrap()).name.as_ref(),
            model_path.to_string_lossy()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recoverable_workspace_prefers_complete_analysis() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        fs::write(&main, "type Item = String; {Item: Item}").unwrap();
        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        let root = snapshot
            .module_by_path(&canonicalize(&main).unwrap())
            .unwrap();
        assert_eq!(root.state, WorkspaceModuleState::Known);
        let item = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.module == root.id && definition.name == "Item")
            .unwrap();
        assert_eq!(item.ty.state, crate::FactState::Known);
        assert!(snapshot.diagnostics().is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recoverable_workspace_links_json_and_core_values() {
        let directory = fixture_dir();
        let data = directory.join("data.json");
        let model = directory.join("model.forma");
        let main = directory.join("main.forma");
        fs::write(&data, r#"{"kind":"int"}"#).unwrap();
        fs::write(&model, "type Shared = String; {Shared: Shared}").unwrap();
        fs::write(
            &main,
            "import data from \"./data.json\";\
             import model from \"./model.forma\";\
             import attributes from \"@bim/std/attributes\";\
             type FromData = if data.kind == \"int\" { Int } else { String };\
             type FromForma = model.Shared;\
             type FromCore = attributes.strip(String);\
             type Broken = missing(Int);\
             0",
        )
        .unwrap();
        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        let root = snapshot
            .module_by_path(&canonicalize(&main).unwrap())
            .unwrap();
        let fact = |name: &str| {
            &snapshot
                .definitions()
                .iter()
                .find(|definition| definition.module == root.id && definition.name == name)
                .unwrap()
                .ty
        };
        for (name, expected) in [
            ("FromData", "Int"),
            ("FromForma", "String"),
            ("FromCore", "String"),
        ] {
            assert_eq!(fact(name).state, crate::FactState::Known, "{name}");
            assert_eq!(
                snapshot.types().display(fact(name).value.unwrap()).unwrap(),
                expected
            );
        }
        assert!(snapshot.modules().iter().any(|module| {
            module.kind == WorkspaceModuleKind::Json && module.state == WorkspaceModuleState::Known
        }));
        assert!(snapshot.modules().iter().any(|module| {
            module.kind == WorkspaceModuleKind::Core && module.state == WorkspaceModuleState::Known
        }));
        assert_eq!(
            snapshot
                .module_by_path(&canonicalize(&model).unwrap())
                .unwrap()
                .state,
            WorkspaceModuleState::Known
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recoverable_workspace_retains_module_cycles_once() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        let a = directory.join("a.forma");
        let b = directory.join("b.forma");
        fs::write(&main, "import a from \"./a.forma\"; a").unwrap();
        fs::write(&a, "import b from \"./b.forma\"; type A = String; 0").unwrap();
        fs::write(&b, "import a from \"./a.forma\"; type B = String; 0").unwrap();
        let snapshot = recovery_engine().recover_workspace(&main).unwrap();
        assert_eq!(
            snapshot
                .modules()
                .iter()
                .filter(|module| module.kind == WorkspaceModuleKind::Forma)
                .filter(|module| module.state == WorkspaceModuleState::Unavailable)
                .count(),
            2
        );
        assert_eq!(
            snapshot
                .diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.message.contains("module cycle"))
                .count(),
            1
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn core_json_parses_and_decodes_strings_with_blame_results() {
        let directory = fixture_dir();
        let main = directory.join("main.forma");
        fs::write(
            &main,
            r#"import json from "@bim/std/json";
               import result from "@bim/std/result";
               {
                   parsed: result.unwrap(json.parse("{\"answer\": 42}")),
                   decoded: result.unwrap(json.decode(Int, "42")),
                   failed: json.parse("{")
               }"#,
        )
        .unwrap();
        let engine = recovery_engine();
        let module = engine.load_module(&main, BTreeMap::new()).unwrap();
        let output = engine.execute(&module).unwrap().to_string();
        assert!(output.contains("parsed: {answer: 42}"), "{output}");
        assert!(output.contains("decoded: 42"), "{output}");
        assert!(output.contains("failed: 'Err("), "{output}");
        assert!(output.contains("data: \"{\""), "{output}");
        assert!(output.contains("rule: 'Json"), "{output}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dependency_imports_preserve_identity_across_relative_edges() {
        let directory = fixture_dir();
        let app = directory.join("app");
        let models = directory.join("models");
        fs::create_dir(&app).unwrap();
        fs::create_dir(&models).unwrap();
        fs::write(
            directory.join("forma-deps.json"),
            r#"{"dependencies":{"models":{"path":"models"}}}"#,
        )
        .unwrap();
        fs::write(models.join("base.forma"), "{answer: 42}").unwrap();
        fs::write(
            models.join("user.forma"),
            "import base from \"./base.forma\"; base",
        )
        .unwrap();
        let main = app.join("main.forma");
        fs::write(&main, "import user from \"models/user.forma\"; user.answer").unwrap();

        let engine = recovery_engine();
        let loaded = engine.load_module(&main, BTreeMap::new()).unwrap();
        assert_eq!(engine.execute(&loaded).unwrap().to_string(), "42");
        let names = loaded
            .workspace
            .modules()
            .iter()
            .map(|module| module.name.as_str())
            .collect::<HashSet<_>>();
        assert!(names.contains("models/user.forma"));
        assert!(names.contains("models/base.forma"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn embedded_manifest_resolves_before_imports_and_is_root_only() {
        let directory = fixture_dir();
        let app = directory.join("app");
        let dependency = directory.join("dependency");
        fs::create_dir_all(app.join("bin-src")).unwrap();
        fs::create_dir_all(app.join("src")).unwrap();
        fs::create_dir_all(dependency.join("src")).unwrap();
        let main = app.join("bin-src/tool.forma");
        fs::write(
            &main,
            r#"@@manifest {name: "tool", dependencies: {dep: {path: "../dependency"}}};
               import answer from "dep/answer.forma";
               answer"#,
        )
        .unwrap();
        fs::write(dependency.join("src/answer.forma"), "42").unwrap();

        let engine = recovery_engine();
        let loaded = engine.load_module(&main, BTreeMap::new()).unwrap();
        assert_eq!(engine.execute(&loaded).unwrap().to_string(), "42");

        fs::write(
            app.join("src/helper.forma"),
            "@@manifest {name: \"nested\", dependencies: {}}; 1",
        )
        .unwrap();
        fs::write(&main, "import helper from \"@src/helper.forma\"; helper").unwrap();
        let error = engine.load_module(&main, BTreeMap::new()).unwrap_err();
        assert!(error.message().contains("only allowed in @main"));
        fs::remove_dir_all(directory).unwrap();
    }
}
