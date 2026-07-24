use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use xl::{
    DebugEvent, DebugSink, DefinitionKind, Engine, EngineConfig, Location, Quota, TextRange, Value,
    WorkspaceSnapshot, WorkspaceTypeId, parse_json,
};

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
    let engine = engine();
    let module = engine
        .load_module(module_path, bindings)
        .map_err(|error| error.to_string())?;
    let result = engine.execute(&module).map_err(|error| error.to_string())?;
    println!("{result}");
    Ok(())
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
    let source = fs::read_to_string(module_path)
        .map_err(|error| format!("cannot read {}: {error}", Path::new(module_path).display()))?;
    match engine().load_module(module_path, BTreeMap::new()) {
        Ok(module) => show_query(&module.workspace, &arguments[1..]),
        Err(error) => {
            let recovered = WorkspaceSnapshot::recover_source(module_path.clone(), source);
            if recovered.diagnostics().is_empty() {
                return Err(error.to_string());
            }
            show_query(&recovered, &arguments[1..])
        }
    }
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
        println!("  m{} {:?} {}", module.id.index(), module.kind, module.name);
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
    "usage:\n  xl run <module.xl> [--input <file|->]\n  xl check <module.xl>\n  xl types <module.xl>\n  xl show <module.xl> [at <source> <line> <column>]".into()
}
