use crate::ast::{BindingKind, Expr, ExprKind, Program, StringPartKind};
use crate::compiler::compile_program_analyzed_in;
use crate::heap::{Heap, RuntimeValue, promote_roots};
use crate::json::{Provenance, SourcedValue, parse_json_registered};
use crate::parser::parse_registered;
use crate::source::SourceDatabase;
use crate::types::{Analysis, analyze_program_with_bindings};
use crate::{BytecodeFunction, Quota, QuotaAccount, Value, Vm};
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
    externals: HashMap<String, RuntimeValue>,
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
        let mut account = QuotaAccount::new(quota);
        let arena = Vm::new()
            .execute_in_background(
                &self.runtime.world,
                &self.runtime.externals,
                &self.function,
                &[],
                &mut account,
            )
            .map_err(|error| error.with_sources(&self.sources))?;
        crate::heap::HeapView {
            current: &arena.heap,
            background: Some(&self.runtime.world),
        }
        .export_value(arena.root)
        .map_err(|error| crate::RuntimeError::from_heap_error(&self.function, error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineConfig {
    pub module_quota: Quota,
    pub session_quota: Quota,
}

#[derive(Debug)]
pub struct Engine {
    config: EngineConfig,
}

impl Engine {
    pub const fn new(config: EngineConfig) -> Self {
        Self { config }
    }

    pub const fn config(&self) -> EngineConfig {
        self.config
    }

    pub fn load_module(
        &self,
        path: impl AsRef<Path>,
        external_bindings: BTreeMap<String, Value>,
    ) -> Result<LoadedModule, ModuleError> {
        load_module_with_quota(path, external_bindings, self.config.module_quota)
    }

    pub fn execute(&self, module: &LoadedModule) -> Result<Value, crate::RuntimeError> {
        module.execute_with_quota(self.config.session_quota)
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
    let root = canonicalize(path.as_ref())?;
    if root.extension().and_then(|extension| extension.to_str()) != Some("xl") {
        return Err(ModuleError::new("root module must have an .xl extension"));
    }
    let mut loader = ModuleLoader {
        cache: HashMap::new(),
        world: Heap::new(0),
        visiting: Vec::new(),
        dependencies: BTreeSet::new(),
        module_quota,
        sources: SourceDatabase::default(),
    };
    loader.load_root(root, external_bindings)
}

struct ModuleLoader {
    cache: HashMap<PathBuf, ModuleState>,
    world: Heap,
    visiting: Vec<PathBuf>,
    dependencies: BTreeSet<PathBuf>,
    module_quota: Quota,
    sources: SourceDatabase,
}

#[derive(Clone)]
enum ModuleState {
    Ready {
        root: RuntimeValue,
        sourced: SourcedValue,
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
        let world = std::mem::replace(&mut self.world, Heap::new(0));
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
        if let Some(ModuleState::Ready { root, sourced }) = self.cache.get(&path) {
            let _persistent_root = root;
            return Ok(sourced.clone());
        }
        self.enter(&path)?;
        let result: Result<(SourcedValue, RuntimeValue), ModuleError> =
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
                            let mut current = Heap::new(1);
                            let root = current
                                .import_value(Some(&self.world), &sourced.value)
                                .map_err(|error| ModuleError::new(error.to_string()))?;
                            let root = promote_roots(&mut self.world, &current, &[root])
                                .map_err(|error| ModuleError::new(error.to_string()))?[0];
                            Ok((sourced, root))
                        })
                }
                Some("xl") => {
                    let mut account = QuotaAccount::new(self.module_quota);
                    self.compile_xl(&path, BTreeMap::new(), false, &mut account)
                        .and_then(|(_, function, externals)| {
                            let arena = Vm::new()
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
                            let value = crate::heap::HeapView {
                                current: &arena.heap,
                                background: Some(&self.world),
                            }
                            .export_value(arena.root)
                            .map_err(|error| ModuleError::new(error.to_string()))?;
                            let root = promote_roots(&mut self.world, &arena.heap, &[arena.root])
                                .map_err(|error| ModuleError::new(error.to_string()))?[0];
                            Ok((
                                SourcedValue {
                                    value,
                                    provenance: Provenance::default(),
                                },
                                root,
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
        let (sourced, root) = result?;
        self.cache.insert(
            path,
            ModuleState::Ready {
                root,
                sourced: sourced.clone(),
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
    ) -> Result<(Analysis, BytecodeFunction, HashMap<String, RuntimeValue>), ModuleError> {
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
            let imported = path
                .parent()
                .expect("canonical module path has a parent")
                .join(relative);
            let sourced = self.load_value(&imported)?;
            let imported = canonicalize(&imported)?;
            let ModuleState::Ready { root, .. } = self
                .cache
                .get(&imported)
                .expect("loaded module has a ready cache entry");
            external_roots.insert(binding.value.name.value.clone(), *root);
            if !sourced.provenance.values.is_empty() {
                external_provenance.insert(binding.value.name.value.clone(), sourced.provenance);
            }
            external_bindings.insert(binding.value.name.value.clone(), sourced.value);
        }

        let dynamic_bindings = if is_root && external_bindings.contains_key("input") {
            HashSet::from(["input".to_owned()])
        } else {
            HashSet::new()
        };
        let analysis = analyze_program_with_bindings(
            &source_name,
            &program,
            account,
            &external_bindings,
            &dynamic_bindings,
            &self.sources,
            &external_provenance,
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
        if binding.value.kind == BindingKind::Let && expression_has_import(&binding.value.value) {
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
            world: Heap::new(0),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
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
            world: Heap::new(0),
            visiting: Vec::new(),
            dependencies: BTreeSet::new(),
            module_quota: Quota::with_fuel(100_000),
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
