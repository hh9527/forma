use crate::ast::{BindingKind, Expr, ExprKind, Program, StringPartKind};
use crate::compiler::compile_program_analyzed_in;
use crate::core::{
    ARRAY_MODULE, CODEC_MODULE, DEBUG_MODULE, DICT_MODULE, JSON_MODULE, RESULT_MODULE,
    array_module_value, codec_module_value, debug_module_value, dict_module_value,
    json_module_value, result_module_value,
};
use crate::heap::{Heap, PersistentValue, publish_root, publish_value};
use crate::json::{Provenance, SourcedValue, parse_json_registered};
use crate::parser::parse_registered;
use crate::source::SourceDatabase;
use crate::types::{Analysis, analyze_program_with_bindings_observed};
use crate::{BytecodeFunction, DebugSink, DiscardDebugSink, Quota, QuotaAccount, Value, Vm};
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
    world: Heap,
    externals: HashMap<String, PersistentValue>,
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
            .execute_in_background(
                &self.runtime.world,
                &self.runtime.externals,
                &self.function,
                &[],
                &mut account,
            )
            .map_err(|error| error.with_sources(&self.sources))?;
        arena
            .export(&self.runtime.world)
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
    let mut loader = ModuleLoader {
        cache: HashMap::new(),
        core_modules: HashMap::new(),
        world: Heap::persistent(),
        visiting: Vec::new(),
        dependencies: BTreeSet::new(),
        module_quota,
        debug_sink,
        sources: SourceDatabase::default(),
    };
    loader.load_root(root, external_bindings)
}

struct ModuleLoader {
    cache: HashMap<PathBuf, ModuleState>,
    core_modules: HashMap<&'static str, (Value, PersistentValue)>,
    world: Heap,
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
        let world = std::mem::replace(&mut self.world, Heap::persistent());
        Ok(LoadedModule {
            path,
            dependencies: self.dependencies.iter().cloned().collect(),
            analysis,
            function,
            sources: self.sources.clone(),
            runtime: Arc::new(ModuleRuntime { world, externals }),
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
                            let mut local = Heap::local();
                            let local_root = local
                                .import_sourced_value(Some(&self.world), &sourced)
                                .map_err(|error| ModuleError::new(error.to_string()))?;
                            let root = publish_root(&mut self.world, &local, local_root)
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
                                .execute_in_background(
                                    &self.world,
                                    &externals,
                                    &function,
                                    &[],
                                    &mut account,
                                )
                                .map_err(|error| {
                                    ModuleError::new(error.with_sources(&self.sources).to_string())
                                })?;
                            let (value, opaque) = match arena.export(&self.world) {
                                Ok(value) => (value, false),
                                Err(error) if error.is_legacy_cycle() => (Value::none(), true),
                                Err(error) => return Err(ModuleError::new(error.to_string())),
                            };
                            let root = arena
                                .publish(&mut self.world)
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

        let mut dynamic_bindings = opaque_bindings;
        if is_root && external_bindings.contains_key("input") {
            dynamic_bindings.insert("input".to_owned());
        }
        let analysis = analyze_program_with_bindings_observed(
            &source_name,
            &program,
            account,
            &external_bindings,
            &dynamic_bindings,
            &self.sources,
            &external_provenance,
            &self.debug_sink,
        )
        .map_err(|error| {
            error.diagnostic.as_ref().map_or_else(
                || ModuleError::new(error.to_string()),
                |diagnostic| ModuleError::new(self.sources.render(diagnostic)),
            )
        })?;
        let function =
            compile_program_analyzed_in(self.sources.get(source_id), &program, &analysis)
                .map_err(|error| ModuleError::new(error.to_string()))?;
        Ok((analysis, function, external_roots))
    }

    fn load_core_module(&mut self, name: &str) -> Result<(Value, PersistentValue), ModuleError> {
        if let Some((value, root)) = self.core_modules.get(name) {
            return Ok((value.clone(), *root));
        }
        let (identity, value) = match name {
            ARRAY_MODULE => (ARRAY_MODULE, array_module_value()),
            DICT_MODULE => (DICT_MODULE, dict_module_value()),
            DEBUG_MODULE => (DEBUG_MODULE, debug_module_value()),
            CODEC_MODULE => (CODEC_MODULE, codec_module_value()),
            RESULT_MODULE => (RESULT_MODULE, result_module_value()),
            JSON_MODULE => (JSON_MODULE, json_module_value()),
            _ => return Err(ModuleError::new(format!("unknown core module {name:?}"))),
        };
        let root = publish_value(&mut self.world, &value)
            .map_err(|error| ModuleError::new(error.to_string()))?;
        self.core_modules.insert(identity, (value.clone(), root));
        Ok((value, root))
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
                "{source_name}: imports are only allowed at module top level"
            )));
        }
    }
    if expression_has_import(&program.value.body.value.result) {
        return Err(ModuleError::new(format!(
            "{source_name}: imports are only allowed at module top level"
        )));
    }
    Ok(())
}

fn expression_has_import(expression: &Expr) -> bool {
    match &expression.value {
        ExprKind::Block(block) => {
            block
                .value
                .bindings
                .iter()
                .any(|binding| binding.value.kind == BindingKind::Import)
                || block
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
            .execute_in_background(
                &module.runtime.world,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut exact,
            )
            .unwrap();
        assert_eq!(exact.requested_allocation_bytes(), 0);
        assert_eq!(
            arena.export(&module.runtime.world).unwrap().to_string(),
            "{value: 42}"
        );
        assert_eq!(sink.events.lock().unwrap().len(), 3);

        let mut no_fuel = QuotaAccount::new(Quota::new(0, 1_000, 0));
        assert_eq!(
            Vm::new()
                .with_debug_sink(sink)
                .execute_in_background(
                    &module.runtime.world,
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
    fn derived_codec_normalizes_options_and_pretty_prints_json() {
        let directory = fixture_dir();
        fs::write(
            directory.join("User.xl"),
            r#"import codec from "core:codec";
               import result from "core:result";
               fn Optional(item) {
                   Union([Atom('None), Tuple([Atom('Some), item])])
               }
               type Type = Struct({v: Optional(String)});
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
        assert!(rendered.contains("User.xl:6:48:"), "{rendered}");

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
                   type T = Struct({name: String});
                   codec.decode(T, {}) |> result.unwrap"#,
                "$.name: missing required field",
            ),
            (
                r#"import codec from "core:codec";
                   import result from "core:result";
                   type T = Struct({name: String});
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
             type User = Struct({name: String, age: Int});\
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
            module.analysis.binding_types["input"],
            crate::TypeDescriptor::Any
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
             type User = Struct({name: String, age: Int});\n\
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
            world: Heap::persistent(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
        };

        let first = loader.load_value(&data).unwrap();
        let counts = loader.world.counts();
        let first_root = match loader.cache.get(&canonicalize(&data).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };
        let second = loader.load_value(&data).unwrap();
        let second_root = match loader.cache.get(&canonicalize(&data).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };

        assert_eq!(first.value.to_string(), second.value.to_string());
        assert_eq!(first_root, second_root);
        assert_eq!(counts, loader.world.counts());
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
            .execute_in_background(
                &module.runtime.world,
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
        let Value::Array(mapped) = arena.export(&module.runtime.world).unwrap() else {
            panic!("expected mapped Array")
        };
        assert_eq!(mapped.len(), item_count);

        let mut fuel_short = QuotaAccount::new(Quota::new(1_500, 1_000, u64::MAX));
        assert_eq!(
            Vm::new()
                .execute_in_background(
                    &module.runtime.world,
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
                .execute_in_background(
                    &module.runtime.world,
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
            .execute_in_background(
                &module.runtime.world,
                &module.runtime.externals,
                &module.function,
                &[],
                &mut exact,
            )
            .unwrap();
        assert_eq!(exact.requested_allocation_bytes(), requested);
        assert_eq!(
            arena.export(&module.runtime.world).unwrap().to_string(),
            "[\"a\", \"long\"]"
        );

        let mut short = QuotaAccount::new(Quota::new(1, 1_000, requested - 1));
        assert_eq!(
            Vm::new()
                .execute_in_background(
                    &module.runtime.world,
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
            world: Heap::persistent(),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
            debug_sink: Arc::new(DiscardDebugSink),
            sources: SourceDatabase::default(),
        };

        loader.load_value(&a).unwrap();
        let counts_after_a = loader.world.counts();
        loader.load_value(&b).unwrap();
        let root = |path: &Path| match loader.cache.get(&canonicalize(path).unwrap()).unwrap() {
            ModuleState::Ready { root, .. } => *root,
        };

        assert_eq!(root(&a), root(&c));
        assert_eq!(root(&b), root(&c));
        assert_eq!(counts_after_a, loader.world.counts());
        fs::remove_dir_all(directory).unwrap();
    }
}
