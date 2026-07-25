use forma::{
    DebugEvent, DebugSink, DefinitionKind, Engine, EngineConfig, Location, Quota, TextRange, Value,
    Vm, WorkspaceSnapshot, WorkspaceTypeId, parse_json,
};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const EVALUATION_FUEL: usize = 1_000_000;
const STACK_SLOTS: usize = 65_536;
const ALLOCATION_BYTES: u64 = 256 * 1024 * 1024;

fn engine() -> Engine {
    Engine::new(EngineConfig {
        module_quota: Quota::new(EVALUATION_FUEL, STACK_SLOTS, ALLOCATION_BYTES),
        session_quota: Quota::new(EVALUATION_FUEL, STACK_SLOTS, ALLOCATION_BYTES),
    })
    .with_debug_sink(Arc::new(StderrDebugSink))
}

struct StderrDebugSink;

impl DebugSink for StderrDebugSink {
    fn emit(&self, event: DebugEvent) {
        match event.label {
            Some(label) => eprintln!("[debug] {label:?}: {}", event.value),
            None => eprintln!("[debug] {}", event.value),
        }
    }
}

fn main() {
    if let Err(error) = run_cli(env::args().skip(1).collect()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run_cli(arguments: Vec<String>) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage());
    };
    match command {
        "run" => run_command(&arguments[1..]),
        "exec" => exec_command(&arguments[1..]),
        "check" => check_command(&arguments[1..]),
        "types" => types_command(&arguments[1..]),
        "show" => show_command(&arguments[1..]),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n{}", usage())),
    }
}

fn run_command(arguments: &[String]) -> Result<(), String> {
    let Some(module_path) = arguments.first() else {
        return Err(format!("run requires a module path\n{}", usage()));
    };
    let mut bindings = BTreeMap::new();
    match &arguments[1..] {
        [] => {}
        [flag, input] if flag == "--input" => {
            bindings.insert("input".into(), read_input(input)?);
        }
        _ => return Err(format!("invalid run arguments\n{}", usage())),
    }
    let result = evaluate_module(module_path, bindings)?;
    println!("{result}");
    Ok(())
}

fn evaluate_module(module_path: &str, bindings: BTreeMap<String, Value>) -> Result<Value, String> {
    let engine = engine();
    let module = engine
        .load_module(module_path, bindings)
        .map_err(|error| error.to_string())?;
    engine.execute(&module).map_err(|error| error.to_string())
}

fn exec_command(arguments: &[String]) -> Result<(), String> {
    let (module_path, request_args) = match arguments {
        [dry_run, module_path] if dry_run == "--dry-run" => (module_path, &[][..]),
        [dry_run, module_path, separator, request_args @ ..]
            if dry_run == "--dry-run" && separator == "--" =>
        {
            (module_path, request_args)
        }
        _ => {
            return Err(format!("exec currently requires --dry-run\n{}", usage()));
        }
    };
    let engine = engine();
    let module = engine
        .load_module(module_path, BTreeMap::new())
        .map_err(|error| error.to_string())?;
    let entry = engine.execute(&module).map_err(|error| error.to_string())?;
    let mut values = Vm::new();
    let settings = exec_settings(&mut values)?;
    let request = exec_request(&mut values, request_args)?;
    let plan = engine
        .invoke(&module, &entry, &[settings, request])
        .map_err(|error| error.to_string())?;
    let Value::Dict(_) = plan else {
        return Err(format!(
            "exec entry must return a Dict, found {}",
            plan.type_name()
        ));
    };
    println!("{}", canonical_json(&plan)?);
    Ok(())
}

fn exec_settings(vm: &mut Vm) -> Result<Value, String> {
    let cache = cache_root().join("forma").join("exec");
    let platform = vm.make_dict(vec![
        ("arch".into(), Value::string(env::consts::ARCH)),
        ("os".into(), Value::string(env::consts::OS)),
    ])?;
    vm.make_dict(vec![
        (
            "cache_prefix".into(),
            Value::string(path_string(&cache.join("downloads"))?),
        ),
        (
            "install_prefix".into(),
            Value::string(path_string(&cache.join("installs"))?),
        ),
        ("platform".into(), platform),
    ])
}

fn exec_request(vm: &mut Vm, arguments: &[String]) -> Result<Value, String> {
    let mut environment = Vec::new();
    for (name, value) in env::vars_os() {
        let name = name
            .into_string()
            .map_err(|_| "exec environment contains a non-UTF-8 name".to_owned())?;
        let value = value
            .into_string()
            .map_err(|_| format!("exec environment variable {name:?} is not UTF-8"))?;
        environment.push((name, Value::string(value)));
    }
    let environment = vm.make_dict(environment)?;
    vm.make_dict(vec![
        (
            "args".into(),
            Value::Array(arguments.iter().cloned().map(Value::string).collect()),
        ),
        (
            "cwd".into(),
            Value::string(path_string(
                &env::current_dir().map_err(|error| error.to_string())?,
            )?),
        ),
        ("env".into(), environment),
    ])
}

fn cache_root() -> PathBuf {
    env::var_os("FORMA_CACHE_HOME")
        .or_else(|| env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(env::temp_dir)
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))
}

fn canonical_json(value: &Value) -> Result<String, String> {
    fn write_value(output: &mut String, value: &Value) -> Result<(), String> {
        match value {
            Value::Int(value) => write!(output, "{value}").unwrap(),
            Value::Float(value) if value.is_finite() => write!(output, "{value}").unwrap(),
            Value::Float(_) => return Err("Exec contains a non-finite Float".into()),
            Value::String(value) => write_string(output, value),
            Value::Atom(atom) if atom.name() == "None" => output.push_str("null"),
            Value::Atom(atom) if atom.name() == "True" => output.push_str("true"),
            Value::Atom(atom) if atom.name() == "False" => output.push_str("false"),
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    write_value(output, value)?;
                }
                output.push(']');
            }
            Value::Dict(value) => {
                output.push('{');
                for (index, (name, value)) in value
                    .shape()
                    .fields()
                    .iter()
                    .zip(value.values())
                    .enumerate()
                {
                    if index > 0 {
                        output.push(',');
                    }
                    write_string(output, name);
                    output.push(':');
                    write_value(output, value)?;
                }
                output.push('}');
            }
            _ => {
                return Err(format!(
                    "Exec contains a non-JSON {} value",
                    value.type_name()
                ));
            }
        }
        Ok(())
    }

    fn write_string(output: &mut String, value: &str) {
        output.push('"');
        for character in value.chars() {
            match character {
                '"' => output.push_str("\\\""),
                '\\' => output.push_str("\\\\"),
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                character if character.is_control() => {
                    write!(output, "\\u{:04x}", u32::from(character)).unwrap();
                }
                character => output.push(character),
            }
        }
        output.push('"');
    }

    let mut output = String::new();
    write_value(&mut output, value)?;
    Ok(output)
}

fn check_command(arguments: &[String]) -> Result<(), String> {
    let [module_path] = arguments else {
        return Err(format!("check requires one module path\n{}", usage()));
    };
    let module = engine()
        .load_module(module_path, BTreeMap::new())
        .map_err(|error| error.to_string())?;
    println!("ok ({} dependencies)", module.dependencies.len());
    Ok(())
}

fn types_command(arguments: &[String]) -> Result<(), String> {
    let [module_path] = arguments else {
        return Err(format!("types requires one module path\n{}", usage()));
    };
    let module = engine()
        .load_module(module_path, BTreeMap::new())
        .map_err(|error| error.to_string())?;
    let root = module
        .workspace
        .module_by_path(&module.path)
        .ok_or_else(|| "loaded root is absent from the workspace snapshot".to_owned())?;
    let mut definitions = module
        .workspace
        .definitions()
        .iter()
        .filter(|definition| definition.module == root.id)
        .filter_map(|definition| definition.ty.value.map(|ty| (definition, ty)))
        .collect::<Vec<_>>();
    definitions.sort_by(|(left, _), (right, _)| left.name.cmp(&right.name));
    for (definition, ty) in definitions
        .iter()
        .filter(|(definition, _)| definition.kind == DefinitionKind::Type)
    {
        println!(
            "type {} = {}",
            definition.name,
            display_workspace_type(&module.workspace, *ty)?
        );
    }
    for (definition, ty) in definitions
        .iter()
        .filter(|(definition, _)| definition.kind != DefinitionKind::Type)
    {
        println!(
            "let {}: {}",
            definition.name,
            display_workspace_type(&module.workspace, *ty)?
        );
    }
    println!(
        "result: {}",
        display_workspace_type(
            &module.workspace,
            root.result_type
                .ok_or_else(|| "loaded root has no workspace result type".to_owned())?,
        )?
    );
    Ok(())
}

fn display_workspace_type(
    workspace: &WorkspaceSnapshot,
    ty: WorkspaceTypeId,
) -> Result<String, String> {
    workspace
        .types()
        .display(ty)
        .ok_or_else(|| format!("workspace type t{} is absent", ty.index()))
}

fn show_command(arguments: &[String]) -> Result<(), String> {
    let Some(module_path) = arguments.first() else {
        return Err(format!("show requires a module path\n{}", usage()));
    };
    let engine = engine();
    let workspace = engine
        .recover_workspace(module_path)
        .map_err(|error| error.to_string())?;
    show_query(&workspace, &arguments[1..])
}

fn show_query(workspace: &WorkspaceSnapshot, arguments: &[String]) -> Result<(), String> {
    match arguments {
        [] => show_workspace(workspace),
        [mode, source_path, line, column] if mode == "at" => {
            let line = line
                .parse::<usize>()
                .map_err(|_| format!("invalid line {line:?}"))?;
            let column = column
                .parse::<usize>()
                .map_err(|_| format!("invalid column {column:?}"))?;
            show_at(workspace, source_path, line, column)
        }
        _ => Err(format!("invalid show arguments\n{}", usage())),
    }
}

fn show_workspace(workspace: &WorkspaceSnapshot) -> Result<(), String> {
    println!("diagnostics:");
    for diagnostic in workspace.diagnostics() {
        println!(
            "  {}",
            workspace.sources().render(diagnostic).replace('\n', "\n  ")
        );
    }
    println!("modules:");
    for module in workspace.modules() {
        println!(
            "  m{} {:?} {:?} {}",
            module.id.index(),
            module.kind,
            module.state,
            module.name
        );
        for import in &module.imports {
            println!("    import {} -> m{}", import.name, import.target.index());
        }
        if let Some(ty) = module.result_type {
            println!(
                "    result t{} = {}",
                ty.index(),
                workspace.types().display(ty).unwrap_or_else(|| "?".into())
            );
        }
        for export in workspace.exports_of(module.id) {
            println!(
                "    export {}: t{} = {}",
                export.name,
                export.ty.index(),
                workspace
                    .types()
                    .display(export.ty)
                    .unwrap_or_else(|| "?".into())
            );
        }
    }
    println!("definitions:");
    for definition in workspace.definitions() {
        let location = show_location(workspace, definition.location);
        let ty = definition.ty.value.map_or_else(
            || format!(" {:?}", definition.ty.state),
            |ty| {
                format!(
                    " t{} = {}",
                    ty.index(),
                    workspace.types().display(ty).unwrap_or_else(|| "?".into())
                )
            },
        );
        let target = definition
            .import_target
            .map_or_else(String::new, |module| format!(" -> m{}", module.index()));
        println!(
            "  d{} {:?} {} {}{}{}",
            definition.id.index(),
            definition.kind,
            definition.name,
            location,
            ty,
            target
        );
    }
    println!("references:");
    for reference in workspace.references() {
        let target = reference.definition.map_or_else(
            || {
                if reference.external {
                    "external".into()
                } else {
                    "unresolved".into()
                }
            },
            |id| format!("d{}", id.index()),
        );
        println!(
            "  r{} {} {} -> {}",
            reference.id.index(),
            reference.name,
            show_location(workspace, reference.location),
            target
        );
    }
    println!("expressions:");
    for expression in workspace.expressions() {
        let ty = expression.ty.value.map_or_else(
            || format!("{:?}", expression.ty.state),
            |ty| {
                format!(
                    "t{} = {}",
                    ty.index(),
                    workspace.types().display(ty).unwrap_or_else(|| "?".into())
                )
            },
        );
        println!(
            "  e{} {} {}",
            expression.id.index(),
            show_location(workspace, expression.location),
            ty
        );
    }
    println!("types:");
    for (id, node) in workspace.types().nodes() {
        println!("  t{} {node:?}", id.index());
    }
    Ok(())
}

fn show_at(
    workspace: &WorkspaceSnapshot,
    source_path: &str,
    line: usize,
    column: usize,
) -> Result<(), String> {
    let canonical = fs::canonicalize(source_path).ok();
    let source = workspace
        .sources()
        .files()
        .find(|source| {
            source.name.as_ref() == source_path
                || canonical
                    .as_ref()
                    .is_some_and(|path| source.name.as_ref() == path.to_string_lossy())
        })
        .ok_or_else(|| format!("source {source_path:?} is not in the workspace"))?;
    let offset = source
        .offset(line, column)
        .ok_or_else(|| format!("position {line}:{column} is outside {source_path}"))?;
    let location = Location::new(source.id(), TextRange::at(offset));
    println!("position: {}:{line}:{column}", source.name);
    if let Some(definition) = workspace.definition_at(location) {
        println!(
            "definition: d{} {:?} {}",
            definition.id.index(),
            definition.kind,
            definition.name
        );
    }
    if let Some(reference) = workspace.reference_at(location) {
        println!(
            "reference: r{} {} -> {}",
            reference.id.index(),
            reference.name,
            reference.definition.map_or_else(
                || {
                    if reference.external {
                        "external".into()
                    } else {
                        "unresolved".into()
                    }
                },
                |id| format!("d{}", id.index()),
            )
        );
    }
    if let Some(expression) = workspace.expression_at(location) {
        println!(
            "expression: e{} {}",
            expression.id.index(),
            expression.ty.value.map_or_else(
                || format!("{:?}", expression.ty.state),
                |ty| format!(
                    "t{} = {}",
                    ty.index(),
                    workspace.types().display(ty).unwrap_or_else(|| "?".into())
                )
            )
        );
    }
    if let Some(ty) = workspace.type_at(location) {
        println!(
            "type: t{} = {}",
            ty.index(),
            workspace.types().display(ty).unwrap_or_else(|| "?".into())
        );
    }
    Ok(())
}

fn show_location(workspace: &WorkspaceSnapshot, location: Location) -> String {
    let source = workspace.sources().get(location.source);
    let position = source.position(location.start);
    format!("{}:{}:{}", source.name, position.line, position.column)
}

fn read_input(path: &str) -> Result<Value, String> {
    let (source_name, source) = if path == "-" {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| format!("cannot read standard input: {error}"))?;
        ("<stdin>".to_owned(), source)
    } else {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("cannot read input {}: {error}", Path::new(path).display()))?;
        (path.to_owned(), source)
    };
    parse_json(&source_name, &source).map_err(|error| error.to_string())
}

fn usage() -> String {
    "usage:\n  forma run <module.forma> [--input <file|->]\n  forma exec --dry-run <module.forma> [-- <arguments>...]\n  forma check <module.forma>\n  forma types <module.forma>\n  forma show <module.forma> [at <source> <line> <column>]".into()
}
