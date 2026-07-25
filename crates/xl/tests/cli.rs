use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("xl-cli-test-{unique}"));
    fs::create_dir(&path).unwrap();
    path
}

fn xl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xl"))
}

#[test]
fn check_run_and_types_cover_the_closed_world_loop() {
    let directory = fixture_dir();
    fs::write(directory.join("data.json"), r#"{"name":"Ada","age":36}"#).unwrap();
    fs::write(
        directory.join("main.xl"),
        "import data from \"./data.json\";\
         @struct type User = {name: String, age: Int};\
         let user: User = data;\
         validate(User, user)",
    )
    .unwrap();

    let check = xl()
        .args(["check", directory.join("main.xl").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("2 dependencies"));

    let types = xl()
        .args(["types", directory.join("main.xl").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(types.status.success());
    assert!(String::from_utf8_lossy(&types.stdout).contains("type User ="));

    let run = xl()
        .args(["run", directory.join("main.xl").to_str().unwrap()])
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
    fs::write(directory.join("main.xl"), "input").unwrap();
    fs::write(directory.join("input.json"), "[1, 2, 3]").unwrap();

    let run = xl()
        .args([
            "run",
            directory.join("main.xl").to_str().unwrap(),
            "--input",
            directory.join("input.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "[1, 2, 3]");

    fs::write(directory.join("bad.xl"), "1 / 0").unwrap();
    let failure = xl()
        .args(["run", directory.join("bad.xl").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!failure.status.success());
    let stderr = String::from_utf8_lossy(&failure.stderr);
    assert!(stderr.contains("division by zero"));
    assert!(stderr.contains("bad.xl:1:1"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_evaluates_structured_string_interpolation() {
    let directory = fixture_dir();
    fs::write(
        directory.join("interpolation.xl"),
        r#"let name = "Ada"; let count = 2; "hi, \{name} x\{count}""#,
    )
    .unwrap();

    let run = xl()
        .args(["run", directory.join("interpolation.xl").to_str().unwrap()])
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
        directory.join("debug.xl"),
        r#"import debug from "core:debug";
           42 |> debug.dbg_with\("answer\nlabel", _)"#,
    )
    .unwrap();

    let run = xl()
        .args(["run", directory.join("debug.xl").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr).trim(),
        r#"[debug] "answer\nlabel": 42"#
    );
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn exec_dry_run_invokes_explicit_pure_entry() {
    let directory = fixture_dir();
    let cache = directory.join("cache");
    let main = directory.join("exec.xl");
    fs::write(
        &main,
        r#"#!/usr/bin/env -S xl exec --dry-run
import hash from "core:hash";
let captured = "python3";
fn(settings, request) {
    {
        command: captured,
        args: request.args,
        env_value: request.env.XL_EXEC_TEST,
        cwd: request.cwd,
        os: settings.platform.os,
        cache: "\{settings.cache_prefix}/\{hash.sha256("abc")}",
        install: settings.install_prefix,
    }
}"#,
    )
    .unwrap();

    let output = xl()
        .env("XL_CACHE_HOME", &cache)
        .env("XL_EXEC_TEST", "visible")
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
    assert!(stdout.contains(r#""args":["one","two words"]"#), "{stdout}");
    assert!(stdout.contains(r#""command":"python3""#), "{stdout}");
    assert!(stdout.contains(r#""env_value":"visible""#), "{stdout}");
    assert!(
        stdout.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            r#""install":"{}/xl/exec/installs""#,
            cache.display()
        )),
        "{stdout}"
    );
    assert!(!cache.exists());
    let repeated = xl()
        .env("XL_CACHE_HOME", &cache)
        .env("XL_EXEC_TEST", "visible")
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
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn exec_dry_run_rejects_invalid_cli_entry_and_result() {
    let directory = fixture_dir();
    let value = directory.join("value.xl");
    fs::write(&value, "42").unwrap();

    let rejected = xl()
        .args(["exec", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("currently requires --dry-run"));

    let short = xl()
        .args(["exec", "-n", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!short.status.success());

    let not_function = xl()
        .args(["exec", "--dry-run", value.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!not_function.status.success());
    assert!(String::from_utf8_lossy(&not_function.stderr).contains("must be a function"));

    let not_dict = directory.join("not-dict.xl");
    fs::write(&not_dict, "fn(settings, request) { 42 }").unwrap();
    let not_dict = xl()
        .args(["exec", "--dry-run", not_dict.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!not_dict.status.success());
    assert!(String::from_utf8_lossy(&not_dict.stderr).contains("must return a Dict"));

    let non_json = directory.join("non-json.xl");
    fs::write(&non_json, "fn(settings, request) { {bad: 'Named} }").unwrap();
    let non_json = xl()
        .args(["exec", "--dry-run", non_json.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!non_json.status.success());
    assert!(String::from_utf8_lossy(&non_json.stderr).contains("non-JSON Atom"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_observes_deterministic_workspace_and_position_queries() {
    let directory = fixture_dir();
    let main = directory.join("main.xl");
    fs::write(&main, "let answer = 42;\nanswer").unwrap();

    let first = xl()
        .args(["show", main.to_str().unwrap()])
        .output()
        .unwrap();
    let second = xl()
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

    let at = xl()
        .args([
            "show",
            main.to_str().unwrap(),
            "at",
            main.to_str().unwrap(),
            "2",
            "1",
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
fn types_projects_recursive_types_from_the_workspace_snapshot() {
    let directory = fixture_dir();
    let main = directory.join("main.xl");
    fs::write(
        &main,
        "@struct type Node = {children: Array(Node)}; {Node: Node}",
    )
    .unwrap();

    let types = xl()
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
    let main = directory.join("main.xl");
    fs::write(
        &main,
        "let before = 1; let broken = ; let after = missing; after",
    )
    .unwrap();

    let show = xl()
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

    let check = xl()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_continues_independent_type_metadata_after_tool_failure() {
    let directory = fixture_dir();
    let main = directory.join("partial.xl");
    fs::write(
        &main,
        "type A = broken(Int);\
         type B = String;\
         type C = Array(B);\
         type D = Array(A);\
         0",
    )
    .unwrap();

    let show = xl()
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

    let check = xl()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn show_recovers_types_and_causes_across_failed_modules() {
    let directory = fixture_dir();
    let model = directory.join("model.xl");
    let main = directory.join("main.xl");
    fs::write(
        &model,
        "type Broken = missing(Int); type Good = String; {Good: Good}",
    )
    .unwrap();
    fs::write(
        &main,
        "import model from \"./model.xl\";\
         type Local = String;\
         type Uses = model.Good;\
         type Down = Array(Uses);\
         0",
    )
    .unwrap();

    let show = xl()
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
    assert!(output.contains("model.xl"), "{output}");
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

    let check = xl()
        .args(["check", main.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!check.status.success());
    fs::remove_dir_all(directory).unwrap();
}
