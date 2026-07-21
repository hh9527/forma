use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use xl::{Value, load_module, parse_json};

const BUDGET: usize = 1_000_000;

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
    let module = load_module(module_path, bindings, BUDGET).map_err(|error| error.to_string())?;
    let result = module.execute(BUDGET).map_err(|error| error.to_string())?;
    println!("{result}");
    Ok(())
}

fn check_command(arguments: &[String]) -> Result<(), String> {
    let [module_path] = arguments else {
        return Err(format!("check requires one module path\n{}", usage()));
    };
    let module =
        load_module(module_path, BTreeMap::new(), BUDGET).map_err(|error| error.to_string())?;
    println!("ok ({} dependencies)", module.dependencies.len());
    Ok(())
}

fn types_command(arguments: &[String]) -> Result<(), String> {
    let [module_path] = arguments else {
        return Err(format!("types requires one module path\n{}", usage()));
    };
    let module =
        load_module(module_path, BTreeMap::new(), BUDGET).map_err(|error| error.to_string())?;
    for (name, descriptor) in &module.analysis.declared_types {
        println!("type {name} = {}", descriptor.display_name());
    }
    for (name, descriptor) in &module.analysis.binding_types {
        println!("let {name}: {}", descriptor.display_name());
    }
    println!("result: {}", module.analysis.result_type.display_name());
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
