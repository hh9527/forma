use crate::ast::{BindingKind, Expr, Program};
use crate::compiler::compile_program_analyzed_in;
use crate::json::{Provenance, SourcedValue, parse_json_registered};
use crate::parser::parse_registered;
use crate::source::SourceDatabase;
use crate::types::{Analysis, analyze_program_with_bindings};
use crate::{BytecodeFunction, Value, Vm};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl LoadedModule {
    pub fn execute(&self, instruction_budget: usize) -> Result<Value, crate::RuntimeError> {
        Vm::new().execute(&self.function, instruction_budget)
    }
}

pub fn load_module(
    path: impl AsRef<Path>,
    external_bindings: BTreeMap<String, Value>,
    instruction_budget: usize,
) -> Result<LoadedModule, ModuleError> {
    let root = canonicalize(path.as_ref())?;
    if root.extension().and_then(|extension| extension.to_str()) != Some("xl") {
        return Err(ModuleError::new("root module must have an .xl extension"));
    }
    let mut loader = ModuleLoader {
        cache: HashMap::new(),
        visiting: Vec::new(),
        dependencies: BTreeSet::new(),
        instruction_budget,
        sources: SourceDatabase::default(),
    };
    loader.load_root(root, external_bindings)
}

struct ModuleLoader {
    cache: HashMap<PathBuf, SourcedValue>,
    visiting: Vec<PathBuf>,
    dependencies: BTreeSet<PathBuf>,
    instruction_budget: usize,
    sources: SourceDatabase,
}

impl ModuleLoader {
    fn load_root(
        &mut self,
        path: PathBuf,
        external_bindings: BTreeMap<String, Value>,
    ) -> Result<LoadedModule, ModuleError> {
        self.enter(&path)?;
        let result = self.compile_xl(&path, external_bindings, true);
        self.leave(&path);
        let (analysis, function) = result?;
        Ok(LoadedModule {
            path,
            dependencies: self.dependencies.iter().cloned().collect(),
            analysis,
            function,
            sources: self.sources.clone(),
        })
    }

    fn load_value(&mut self, path: &Path) -> Result<SourcedValue, ModuleError> {
        let path = canonicalize(path)?;
        if let Some(value) = self.cache.get(&path) {
            return Ok(value.clone());
        }
        self.enter(&path)?;
        let result = match path.extension().and_then(|extension| extension.to_str()) {
            Some("json") => {
                let source = read(&path)?;
                let source_id = self.sources.add(path.display().to_string(), source);
                let parsed = parse_json_registered(&self.sources, source_id);
                parsed.value.ok_or_else(|| {
                    ModuleError::new(
                        parsed
                            .diagnostics
                            .iter()
                            .map(|diagnostic| self.sources.render(diagnostic))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                })
            }
            Some("xl") => {
                self.compile_xl(&path, BTreeMap::new(), false)
                    .and_then(|(_, function)| {
                        Vm::new()
                            .execute(&function, self.instruction_budget)
                            .map(|value| SourcedValue {
                                value,
                                provenance: Provenance::default(),
                            })
                            .map_err(|error| ModuleError::new(error.to_string()))
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
        let value = result?;
        self.cache.insert(path, value.clone());
        Ok(value)
    }

    fn compile_xl(
        &mut self,
        path: &Path,
        mut external_bindings: BTreeMap<String, Value>,
        is_root: bool,
    ) -> Result<(Analysis, BytecodeFunction), ModuleError> {
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

        for binding in &program.body.bindings {
            if binding.kind != BindingKind::Import {
                continue;
            }
            if external_bindings.contains_key(&binding.name) {
                return Err(ModuleError::new(format!(
                    "duplicate module binding {:?} in {source_name}",
                    binding.name
                )));
            }
            let Expr::String(relative) = binding.value.unspanned() else {
                return Err(ModuleError::new("import path must be a string"));
            };
            let imported = path
                .parent()
                .expect("canonical module path has a parent")
                .join(relative);
            let sourced = self.load_value(&imported)?;
            if !sourced.provenance.values.is_empty() {
                external_provenance.insert(binding.name.clone(), sourced.provenance);
            }
            external_bindings.insert(binding.name.clone(), sourced.value);
        }

        let dynamic_bindings = if is_root && external_bindings.contains_key("input") {
            HashSet::from(["input".to_owned()])
        } else {
            HashSet::new()
        };
        let analysis = analyze_program_with_bindings(
            &source_name,
            &program,
            self.instruction_budget,
            &external_bindings,
            &dynamic_bindings,
            Some(&self.sources),
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
        Ok((analysis, function))
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
    for binding in &program.body.bindings {
        if binding.kind == BindingKind::Let && expression_has_import(&binding.value) {
            return Err(ModuleError::new(format!(
                "{source_name}: imports are only allowed at module top level"
            )));
        }
    }
    if expression_has_import(&program.body.result) {
        return Err(ModuleError::new(format!(
            "{source_name}: imports are only allowed at module top level"
        )));
    }
    Ok(())
}

fn expression_has_import(expression: &Expr) -> bool {
    match expression {
        Expr::Spanned { expression, .. } => expression_has_import(expression),
        Expr::Block(block) => {
            block
                .bindings
                .iter()
                .any(|binding| binding.kind == BindingKind::Import)
                || block
                    .bindings
                    .iter()
                    .any(|binding| expression_has_import(&binding.value))
                || expression_has_import(&block.result)
        }
        Expr::Array(items) | Expr::Tuple(items) => items.iter().any(expression_has_import),
        Expr::Dict(fields) => fields.iter().any(|(_, value)| expression_has_import(value)),
        Expr::Unary { operand, .. } => expression_has_import(operand),
        Expr::Binary { left, right, .. } => {
            expression_has_import(left) || expression_has_import(right)
        }
        Expr::Field { receiver, .. } => expression_has_import(receiver),
        Expr::Call { callee, arguments } => {
            expression_has_import(callee) || arguments.iter().any(expression_has_import)
        }
        Expr::Closure { body, .. } => {
            body.bindings
                .iter()
                .any(|binding| binding.kind == BindingKind::Import)
                || body
                    .bindings
                    .iter()
                    .any(|binding| expression_has_import(&binding.value))
                || expression_has_import(&body.result)
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression_has_import(condition)
                || then_branch
                    .bindings
                    .iter()
                    .chain(&else_branch.bindings)
                    .any(|binding| {
                        binding.kind == BindingKind::Import || expression_has_import(&binding.value)
                    })
                || expression_has_import(&then_branch.result)
                || expression_has_import(&else_branch.result)
        }
        Expr::Match { value, arms } => {
            expression_has_import(value) || arms.iter().any(|arm| expression_has_import(&arm.value))
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bytes(_)
        | Expr::Atom(_)
        | Expr::Variable(_) => false,
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
}
