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
         type User = Struct({name: String, age: Int});\
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
    assert!(String::from_utf8_lossy(&run.stdout).contains("('Ok,"));
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
