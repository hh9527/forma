use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use xl::{DebugEvent, DebugSink, Engine, EngineConfig, Quota, Value, parse_json};

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
    for (name, type_id) in &module.analysis.declared_types {
        println!("type {name} = {}", module.analysis.display(*type_id));
    }
    for (name, type_id) in &module.analysis.binding_types {
        println!("let {name}: {}", module.analysis.display(*type_id));
    }
    println!(
        "result: {}",
        module.analysis.display(module.analysis.result_type)
    );
    Ok(())
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
    "usage:\n  xl run <module.xl> [--input <file|->]\n  xl check <module.xl>\n  xl types <module.xl>".into()
}
