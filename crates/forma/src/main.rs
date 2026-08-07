use forma_core::{
    DebugEvent, DebugSink, DefinitionKind, Diagnostic, Engine, EngineConfig, LoadedModule,
    Location, Quota, TextRange, Value, Vm, WorkspaceSnapshot, WorkspaceTypeId, parse_json,
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

fn engine_config() -> EngineConfig {
    EngineConfig {
        module_quota: Quota::new(EVALUATION_FUEL, STACK_SLOTS, ALLOCATION_BYTES),
        session_quota: Quota::new(EVALUATION_FUEL, STACK_SLOTS, ALLOCATION_BYTES),
    }
}

fn engine() -> Engine {
    Engine::new(engine_config()).with_debug_sink(Arc::new(StderrDebugSink))
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
        "build" => build_command(&arguments[1..]),
        "check" => check_command(&arguments[1..]),
        "types" => types_command(&arguments[1..]),
        "show" => show_command(&arguments[1..]),
        "lsp" => lsp_command(&arguments[1..]),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n{}", usage())),
    }
}

fn lsp_command(arguments: &[String]) -> Result<(), String> {
    if !arguments.is_empty() {
        return Err(format!("lsp does not accept arguments\n{}", usage()));
    }
    let root = env::current_dir()
        .map_err(|error| format!("cannot determine current directory: {error}"))?;
    forma::lsp::run_stdio(root, engine_config()).map_err(|error| error.to_string())
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
    let exports = engine.execute(&module).map_err(|error| error.to_string())?;
    select_host_entry(&module, exports, "run", "output")
}

fn select_host_entry(
    module: &LoadedModule,
    module_value: Value,
    mode: &str,
    name: &str,
) -> Result<Value, String> {
    if !module.uses_explicit_exports() {
        return Ok(module_value);
    }
    let Value::Dict(exports) = module_value else {
        return Err(format!(
            "forma {mode} expected an explicit export record, found {}",
            module_value.type_name()
        ));
    };
    exports
        .get(name)
        .cloned()
        .ok_or_else(|| format!("forma {mode} requires the explicit export {name:?} in @main"))
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
    let main = engine
        .load_module(module_path, BTreeMap::new())
        .map_err(|error| error.to_string())?;
    let mut values = Vm::new();
    let settings = exec_settings(&mut values)?;
    let captures = exec_environment_captures(&main)?;
    let request = exec_request(&mut values, request_args, &captures)?;
    let injected = exec_injected_modules(&mut values, &main, settings, request)?;
    let entry = engine
        .load_entry_with_modules(module_path, "entry/exec.forma", BTreeMap::new(), injected)
        .map_err(|error| exec_entry_error(&main, error.to_string()))?;
    let exports = engine.execute(&entry).map_err(|error| error.to_string())?;
    let Value::Dict(exports) = exports else {
        return Err("exec entry must export a record".into());
    };
    if exports.shape().fields() != ["exec_opts", "install"] {
        return Err("exec entry must export exactly exec_opts and install".into());
    }
    let install = expect_string_export(&exports, "install")?;
    let exec_opts = expect_string_export(&exports, "exec_opts")?;
    println!(r#"{{"install":{install},"exec_opts":{exec_opts}}}"#);
    Ok(())
}

fn exec_entry_error(main: &LoadedModule, detail: String) -> String {
    let Some(definition) = main
        .analysis
        .hir
        .definitions()
        .iter()
        .find(|definition| definition.top_level && definition.name == "exec")
    else {
        return detail;
    };
    let diagnostic = Diagnostic::error(
        "exported exec does not satisfy the forma exec entry contract",
        definition.location,
    );
    format!(
        "{}\nentry contract detail:\n{detail}",
        main.sources.render(&diagnostic)
    )
}

fn expect_string_export<'a>(exports: &'a forma_core::Dict, name: &str) -> Result<&'a str, String> {
    match exports.get(name) {
        Some(Value::String(value)) => Ok(value),
        Some(value) => Err(format!(
            "exec entry export {name:?} must be a String, found {}",
            value.type_name()
        )),
        None => Err(format!("exec entry omitted export {name:?}")),
    }
}

fn exec_injected_modules(
    vm: &mut Vm,
    main: &LoadedModule,
    settings: Value,
    request: Value,
) -> Result<BTreeMap<String, String>, String> {
    let Value::Dict(request) = request else {
        return Err("internal ExecRequest snapshot is not a Dict".into());
    };
    let runtime_source = format!(
        "import \"std/rt-types/exec.forma\" {{ ExecSettings }};\n\
         export let settings: ExecSettings = {};\n\
         export let args: Array(String) = {};\n\
         export let env: Dict(String) = {};\n\
         export let cwd: String = {};\n",
        settings.to_forma_literal()?,
        request
            .get("args")
            .expect("ExecRequest args")
            .to_forma_literal()?,
        request
            .get("env")
            .expect("ExecRequest env")
            .to_forma_literal()?,
        request
            .get("cwd")
            .expect("ExecRequest cwd")
            .to_forma_literal()?,
    );
    let actions = main
        .option_actions()
        .iter()
        .map(|action| {
            vm.make_dict(vec![
                ("key".into(), Value::string(action.key.clone())),
                ("value".into(), action.value.clone()),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let option_source = format!(
        "export let actions: Array(Any) = {};\n",
        Value::Array(actions.into()).to_forma_literal()?
    );
    Ok(BTreeMap::from([
        ("entry/opts.priv.forma".into(), option_source),
        ("entry/rt.priv.forma".into(), runtime_source),
    ]))
}

fn build_command(arguments: &[String]) -> Result<(), String> {
    let [dry_run, module_path] = arguments else {
        return Err(format!("build currently requires --dry-run\n{}", usage()));
    };
    if dry_run != "--dry-run" {
        return Err(format!("build currently requires --dry-run\n{}", usage()));
    }
    let engine = engine();
    let module = engine
        .load_module(module_path, BTreeMap::new())
        .map_err(|error| error.to_string())?;
    let exports = engine.execute(&module).map_err(|error| error.to_string())?;
    let entry = select_host_entry(&module, exports, "build", "build")?;
    let plan = engine
        .invoke(&module, &entry, &[])
        .map_err(|error| error.to_string())?;
    println!("{}", canonical_build_json(&plan)?);
    Ok(())
}

fn canonical_build_json(value: &Value) -> Result<String, String> {
    fn expect_dict<'a>(
        value: &'a Value,
        path: &str,
        fields: &[&str],
    ) -> Result<&'a forma_core::Dict, String> {
        let Value::Dict(dict) = value else {
            return Err(format!(
                "{path} must be a Dict, found {}",
                value.type_name()
            ));
        };
        if !dict
            .shape()
            .fields()
            .iter()
            .map(String::as_str)
            .eq(fields.iter().copied())
        {
            return Err(format!("{path} has an invalid field shape"));
        }
        Ok(dict)
    }

    fn expect_string<'a>(value: &'a Value, path: &str) -> Result<&'a str, String> {
        let Value::String(value) = value else {
            return Err(format!(
                "{path} must be a String, found {}",
                value.type_name()
            ));
        };
        Ok(value)
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

    fn validate_path(path: &str, location: &str) -> Result<(), String> {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|component| matches!(component, "" | "." | ".."))
        {
            return Err(format!(
                "{location} must be a normalized relative path using / separators"
            ));
        }
        Ok(())
    }

    let plan = expect_dict(value, "OutputPlan", &["files"])?;
    let Value::Array(files) = plan.get("files").expect("field checked") else {
        return Err("OutputPlan.files must be an Array".into());
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut output = String::from("{\"files\":[");
    for (index, artifact) in files.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        let Value::Tagged { tag, payload } = artifact else {
            return Err(format!(
                "OutputPlan.files[{index}] must be an Artifact Tagged value"
            ));
        };
        if tag.name() != "TextFile" {
            return Err(format!(
                "OutputPlan.files[{index}] has unknown Artifact variant {:?}",
                tag.name()
            ));
        }
        let location = format!("OutputPlan.files[{index}].TextFile");
        let file = expect_dict(payload, &location, &["content", "path"])?;
        let content = expect_string(
            file.get("content").expect("field checked"),
            &format!("{location}.content"),
        )?;
        let path = expect_string(
            file.get("path").expect("field checked"),
            &format!("{location}.path"),
        )?;
        validate_path(path, &format!("{location}.path"))?;
        if !seen.insert(path) {
            return Err(format!("OutputPlan contains duplicate path {path:?}"));
        }
        output.push_str("{\"TextFile\":{\"content\":");
        write_string(&mut output, content);
        output.push_str(",\"path\":");
        write_string(&mut output, path);
        output.push_str("}}");
    }
    output.push_str("]}");
    Ok(output)
}

fn exec_settings(vm: &mut Vm) -> Result<Value, String> {
    let cache = cache_root().join("forma").join("exec");
    let platform = vm.make_dict(vec![
        ("arch".into(), Value::string(env::consts::ARCH)),
        ("os".into(), Value::string(env::consts::OS)),
    ])?;
    vm.make_dict(vec![
        (
            "download_prefix".into(),
            Value::string(path_string(&cache.join("downloads"))?),
        ),
        (
            "install_prefix".into(),
            Value::string(path_string(&cache.join("installs"))?),
        ),
        ("platform".into(), platform),
    ])
}

fn exec_environment_captures(module: &LoadedModule) -> Result<Vec<String>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut captures = Vec::new();
    for value in module.options("exec.capture-envs") {
        let Value::Array(names) = value else {
            return Err("option \"exec.capture-envs\" must contain an Array(String)".into());
        };
        for name in names.iter() {
            let Value::String(name) = name else {
                return Err("option \"exec.capture-envs\" must contain an Array(String)".into());
            };
            if seen.insert(name.to_string()) {
                captures.push(name.to_string());
            }
        }
    }
    Ok(captures)
}

fn exec_request(vm: &mut Vm, arguments: &[String], captures: &[String]) -> Result<Value, String> {
    let mut environment = Vec::new();
    for name in captures {
        let Some(value) = env::var_os(name) else {
            continue;
        };
        let value = value
            .into_string()
            .map_err(|_| format!("exec environment variable {name:?} is not UTF-8"))?;
        environment.push((name.clone(), Value::string(value)));
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
        let ty = definition.scheme.as_ref().map_or_else(
            || {
                definition.ty.value.map_or_else(
                    || format!(" {:?}", definition.ty.state),
                    |ty| {
                        format!(
                            " t{} = {}",
                            ty.index(),
                            workspace.types().display(ty).unwrap_or_else(|| "?".into())
                        )
                    },
                )
            },
            |scheme| format!(" = {scheme}"),
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
            "definition: d{} {:?} {}{}",
            definition.id.index(),
            definition.kind,
            definition.name,
            definition
                .scheme
                .as_ref()
                .map_or_else(String::new, |scheme| format!(": {scheme}"))
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
    "usage:\n  forma run <module.forma> [--input <file|->]\n  forma exec --dry-run <module.forma> [-- <arguments>...]\n  forma build --dry-run <module.forma>\n  forma check <module.forma>\n  forma types <module.forma>\n  forma show <module.forma> [at <source> <line> <column>]\n  forma lsp".into()
}
