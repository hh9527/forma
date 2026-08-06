use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("forma-test-{unique}"));
    fs::create_dir(&path).unwrap();
    path
}

fn forma() -> Command {
    Command::new(env!("CARGO_BIN_EXE_forma"))
}

#[test]
fn help_exposes_the_lsp_subcommand() {
    let output = forma().arg("help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("forma lsp"));
}

#[test]
fn check_run_and_types_cover_the_closed_world_loop() {
    let directory = fixture_dir();
    fs::write(directory.join("data.json"), r#"{"name":"Ada","age":36}"#).unwrap();
    fs::write(
        directory.join("main.forma"),
        "import \"./data.json\" as data;\
         @struct type User = {name: String, age: Int};\
         let user: User = data;\
         export let output = validate(User, user);",
    )
    .unwrap();

    let check = forma()
        .args(["check", directory.join("main.forma").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("2 dependencies"));

    let types = forma()
        .args(["types", directory.join("main.forma").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(types.status.success());
    assert!(String::from_utf8_lossy(&types.stdout).contains("type User ="));

    let run = forma()
        .args(["run", directory.join("main.forma").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("'Ok("));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_accepts_external_json_and_failures_are_nonzero() {
    let directory = fixture_dir();
    fs::write(directory.join("main.forma"), "export { input as output };").unwrap();
    fs::write(directory.join("input.json"), "[1, 2, 3]").unwrap();

    let run = forma()
        .args([
            "run",
            directory.join("main.forma").to_str().unwrap(),
            "--input",
            directory.join("input.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "[1, 2, 3]");

    fs::write(directory.join("bad.forma"), "export let output = 1 / 0;").unwrap();
    let failure = forma()
        .args(["run", directory.join("bad.forma").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!failure.status.success());
    let stderr = String::from_utf8_lossy(&failure.stderr);
    assert!(stderr.contains("division by zero"));
    assert!(stderr.contains("bad.forma:1:"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_selects_output_from_explicit_main_exports() {
    let directory = fixture_dir();
    let main = directory.join("explicit.forma");
    fs::write(
        &main,
        "let private = 1; let result = private + 41; export { result as output };",
    )
    .unwrap();
    let run = forma()
        .args(["run", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");

    fs::write(&main, "export let other = 1;").unwrap();
    let missing = forma()
        .args(["run", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("forma run requires the explicit export \"output\"")
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_reports_ordered_independent_runtime_failures_while_run_stays_strict() {
    let directory = fixture_dir();
    let main = directory.join("main.forma");
    fs::write(
        &main,
        "let first = 1 / 0;\nlet blocked = first + 1;\nlet second = 2 / 0;\nexport let output = 0;",
    )
    .unwrap();

    let show = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(show.status.success());
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert_eq!(stdout.matches("division by zero").count(), 2, "{stdout}");
    assert!(stdout.find(":1:").unwrap() < stdout.find(":3:").unwrap());

    let run = forma()
        .args(["run", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!stderr.trim().is_empty());
    assert_eq!(stderr.matches("division by zero").count(), 1, "{stderr}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_evaluates_structured_string_interpolation() {
    let directory = fixture_dir();
    fs::write(
        directory.join("interpolation.forma"),
        r#"let name = "Ada"; let count = 2; export let output = `hi, \{name} x\{count}`;"#,
    )
    .unwrap();

    let run = forma()
        .args([
            "run",
            directory.join("interpolation.forma").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "\"hi, Ada x2\""
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_writes_debug_events_only_to_stderr() {
    let directory = fixture_dir();
    fs::write(
        directory.join("debug.forma"),
        r#"import "std/debug" as debug;
           export let output: Int = 42 |> debug.dbg_with\("answer\nlabel", _);"#,
    )
    .unwrap();

    let run = forma()
        .args(["run", directory.join("debug.forma").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr
            .lines()
            .all(|line| line == r#"[debug] "answer\nlabel": 42"#),
        "{stderr}"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn exec_dry_run_invokes_explicit_pure_entry() {
    let directory = fixture_dir();
    let cache = directory.join("cache");
    let main = directory.join("exec.forma");
    fs::write(
        &main,
        r#"#!/usr/bin/env -S forma exec --dry-run
import "std/array" as arrays;
import "std/exec" as exec_types;
import "std/hash" as hash;
type ExecSettings = exec_types.ExecSettings;
type ExecRequest = exec_types.ExecRequest;
type ExecEnv = exec_types.ExecEnv;
export let exec: Fn(ExecSettings, ExecRequest) -> ExecEnv = fn(settings, request) {
    let platform = `\{settings.platform.os}-\{settings.platform.arch}`;
    let compiler_url = `https://example.invalid/gcc-\{platform}.tar.gz`;
    let sysroot_url = `https://example.invalid/sysroot-\{platform}.tar.gz`;
    let compiler = `\{settings.install_prefix}/\{hash.sha256(compiler_url)}`;
    let sysroot = `\{settings.install_prefix}/\{hash.sha256(sysroot_url)}`;
    {
        install: [
            'Unpack({dest: compiler, ty: 'TarGzip, src: compiler_url, strip: 1, digest: 'None}),
            'Unpack({dest: sysroot, ty: 'TarGzip, src: sysroot_url, strip: 1, digest: 'None}),
        ],
        cwd: 'Some(request.cwd),
        bin: `\{compiler}/bin/gcc`,
        args: arrays.flat_map([[`--sysroot=\{sysroot}`], request.args], fn(part) { part }),
        env: {VISIBLE: request.env.FORMA_EXEC_TEST},
    }
};"#,
    )
    .unwrap();

    let output = forma()
        .env("FORMA_CACHE_HOME", &cache)
        .env("FORMA_EXEC_TEST", "visible")
        .args([
            "exec",
            "--dry-run",
            main.to_str().unwrap(),
            "--",
            "one",
            "two words",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(r#""args":["--sysroot="#) && stdout.contains(r#"","one","two words"]"#),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#""bin":"#) && stdout.contains("/bin/gcc"),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#""env":{"VISIBLE":"visible"}"#),
        "{stdout}"
    );
    assert_eq!(stdout.matches(r#""Unpack""#).count(), 2, "{stdout}");
    assert!(stdout.contains(r#""ty":"TarGzip""#), "{stdout}");
    assert!(
        stdout.contains(&format!("{}/forma/exec/installs/", cache.display())),
        "{stdout}"
    );
    assert!(!cache.exists());
    let repeated = forma()
        .env("FORMA_CACHE_HOME", &cache)
        .env("FORMA_EXEC_TEST", "visible")
        .args([
            "exec",
            "--dry-run",
            main.to_str().unwrap(),
            "--",
            "one",
            "two words",
        ])
        .output()
        .unwrap();
    assert!(repeated.status.success());
    assert_eq!(repeated.stdout, stdout.as_bytes());
    assert!(!cache.exists());

    let shown = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert!(
        String::from_utf8_lossy(&shown.stdout).contains("Dict<String>"),
        "{}",
        String::from_utf8_lossy(&shown.stdout)
    );
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn exec_dry_run_rejects_invalid_cli_entry_and_result() {
    let directory = fixture_dir();
    let value = directory.join("value.forma");
    fs::write(&value, "export let exec = 42;").unwrap();

    let rejected = forma()
        .args(["exec", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("currently requires --dry-run"));

    let short = forma()
        .args(["exec", "-n", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!short.status.success());

    let not_function = forma()
        .args(["exec", "--dry-run", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!not_function.status.success());
    assert!(String::from_utf8_lossy(&not_function.stderr).contains("must be a function"));

    let not_dict = directory.join("not-dict.forma");
    fs::write(&not_dict, "export def exec = fn(settings, request) { 42 };").unwrap();
    let not_dict = forma()
        .args(["exec", "--dry-run", not_dict.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!not_dict.status.success());
    assert!(String::from_utf8_lossy(&not_dict.stderr).contains("ExecEnv must be a Dict"));

    let malformed = directory.join("malformed.forma");
    fs::write(
        &malformed,
        "export def exec = fn(settings, request) { {install: ['Copy({})], cwd: 'None, bin: \"x\", args: [], env: {}} };",
    )
    .unwrap();
    let malformed = forma()
        .args(["exec", "--dry-run", malformed.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stderr)
            .contains("ExecEnv.install[0] has unknown Install variant")
    );

    let bad_env = directory.join("bad-env.forma");
    fs::write(
        &bad_env,
        "export def exec = fn(settings, request) { {install: [], cwd: 'None, bin: \"x\", args: [], env: {BAD: 1}} };",
    )
    .unwrap();
    let bad_env = forma()
        .args(["exec", "--dry-run", bad_env.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!bad_env.status.success());
    assert!(bad_env.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad_env.stderr).contains("ExecEnv.env.BAD must be a String"));

    let bad_cwd = directory.join("bad-cwd.forma");
    fs::write(
        &bad_cwd,
        "export def exec = fn(settings, request) { {install: [], cwd: 'Some(1), bin: \"x\", args: [], env: {}} };",
    )
    .unwrap();
    let bad_cwd = forma()
        .args(["exec", "--dry-run", bad_cwd.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!bad_cwd.status.success());
    assert!(bad_cwd.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad_cwd.stderr).contains("ExecEnv.cwd must be a String"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn build_dry_run_validates_and_prints_text_artifacts_without_writing() {
    let directory = fixture_dir();
    let main = directory.join("build.forma");
    fs::write(
        &main,
        r####"import "std/build" as build_types;
import "std/string" as strings;
type OutputPlan = build_types.OutputPlan;
export def build: Fn() -> OutputPlan = fn() {
    {
        files: [
            'TextFile({
                path: "generated/app.conf",
                content: strings.trim_margin(r"|server {
                    |    listen 8080;
                    |}", "|") |> strings.ensure_trailing_newline,
            }),
            'TextFile({path: "generated/name.txt", content: `Forma\n`}),
        ],
    }
};"####,
    )
    .unwrap();

    let output = forma()
        .args(["build", "--dry-run", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("TextFile").count(), 2, "{stdout}");
    assert!(
        stdout.contains(r#""path":"generated/app.conf""#),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#"server {\n    listen 8080;\n}\n"#),
        "{stdout}"
    );
    assert!(!directory.join("generated").exists());

    let repeated = forma()
        .args(["build", "--dry-run", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(repeated.stdout, stdout.as_bytes());

    for (name, source, expected) in [
        (
            "escape.forma",
            "export def build = fn() { {files: ['TextFile({path: \"../outside\", content: \"x\"})]} };",
            "normalized relative path",
        ),
        (
            "duplicate.forma",
            "export def build = fn() { {files: ['TextFile({path: \"a\", content: \"x\"}), 'TextFile({path: \"a\", content: \"y\"})]} };",
            "duplicate path",
        ),
    ] {
        let path = directory.join(name);
        fs::write(&path, source).unwrap();
        let output = forma()
            .args(["build", "--dry-run", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_observes_deterministic_workspace_and_position_queries() {
    let directory = fixture_dir();
    let main = directory.join("main.forma");
    fs::write(&main, "let answer = 42;\nexport { answer as output };").unwrap();

    let first = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    let second = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    let report = String::from_utf8_lossy(&first.stdout);
    assert!(report.contains("definitions:"));
    assert!(report.contains("Let answer"));
    assert!(report.contains("references:"));
    assert!(report.contains("expressions:"));
    assert!(report.contains("answer"));
    assert!(report.contains(" = Int"));

    let at = forma()
        .args([
            "show",
            main.to_str().unwrap(),
            "at",
            main.to_str().unwrap(),
            "2",
            "10",
        ])
        .output()
        .unwrap();
    assert!(
        at.status.success(),
        "{}",
        String::from_utf8_lossy(&at.stderr)
    );
    let output = String::from_utf8_lossy(&at.stdout);
    assert!(output.contains("reference:"), "{output}");
    assert!(output.contains("expression:"), "{output}");
    assert!(output.contains("type:"), "{output}");
    assert!(output.contains(" = Int"), "{output}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_reports_inferred_local_type_schemes() {
    let directory = fixture_dir();
    let main = directory.join("main.forma");
    fs::write(
        &main,
        "let identity = fn(value) { value };\nlet result = identity(1); export { result as output };",
    )
    .unwrap();

    let show = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let output = String::from_utf8_lossy(&show.stdout);
    assert!(
        output.contains("Let identity") && output.contains("for(A) Fn(A) -> A"),
        "{output}"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn types_projects_recursive_types_from_the_workspace_snapshot() {
    let directory = fixture_dir();
    let main = directory.join("main.forma");
    fs::write(
        &main,
        "@struct type Node = {children: Array(Node)}; export { Node };",
    )
    .unwrap();

    let types = forma()
        .args(["types", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        types.status.success(),
        "{}",
        String::from_utf8_lossy(&types.stderr)
    );
    let output = String::from_utf8_lossy(&types.stdout);
    assert_eq!(output.matches("type Node =").count(), 1, "{output}");
    assert!(!output.contains("let Node:"), "{output}");
    assert!(output.contains("children: Array<"), "{output}");
    assert!(output.contains("Node"), "{output}");
    assert!(output.contains("result:"), "{output}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_recovers_semantics_from_damaged_source_while_check_remains_strict() {
    let directory = fixture_dir();
    let main = directory.join("main.forma");
    fs::write(
        &main,
        "let before = 1; let broken = ; let after = missing; after",
    )
    .unwrap();

    let show = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let output = String::from_utf8_lossy(&show.stdout);
    assert!(output.contains("diagnostics:"), "{output}");
    assert!(output.contains("before"), "{output}");
    assert!(output.contains("after"), "{output}");
    assert!(output.contains("Unknown(UnresolvedName)"), "{output}");

    let check = forma()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_continues_independent_type_metadata_after_tool_failure() {
    let directory = fixture_dir();
    let main = directory.join("partial.forma");
    fs::write(
        &main,
        "type A = broken(Int);\
         type B = String;\
         type C = Array(B);\
         type D = Array(A);\
         0",
    )
    .unwrap();

    let show = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let output = String::from_utf8_lossy(&show.stdout);
    assert!(
        output.contains("A") && output.contains("Incomputable"),
        "{output}"
    );
    assert!(
        output.contains("B") && output.contains(" = String"),
        "{output}"
    );
    assert!(
        output.contains("C") && output.contains(" = Array<String>"),
        "{output}"
    );
    assert!(
        output.contains("D") && output.contains("BlockedBy"),
        "{output}"
    );

    let check = forma()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_recovers_types_and_causes_across_failed_modules() {
    let directory = fixture_dir();
    let model = directory.join("model.forma");
    let main = directory.join("main.forma");
    fs::write(
        &model,
        "type Broken = missing(Int); type Good = String; export { Good };",
    )
    .unwrap();
    fs::write(
        &main,
        "import \"./model.forma\" as model;\
         type Local = String;\
         type Uses = model.Good;\
         type Down = Array(Uses);\
         export { Local as output };",
    )
    .unwrap();

    let show = forma()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let output = String::from_utf8_lossy(&show.stdout);
    assert!(output.contains("Partial"), "{output}");
    assert!(output.contains("model.forma"), "{output}");
    assert!(
        output.contains("Local") && output.contains(" = String"),
        "{output}"
    );
    assert!(
        output.contains("Uses") && output.contains("BlockedBy"),
        "{output}"
    );
    assert!(
        output.contains("Good") && output.contains(" = String"),
        "{output}"
    );

    let check = forma()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}
