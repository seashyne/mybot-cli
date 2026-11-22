use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_flag_includes_the_package_version() {
    Command::cargo_bin("mybot")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_command_reports_json() {
    let output = Command::cargo_bin("mybot")
        .unwrap()
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["name"], "mybot");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert!(value["target"].as_str().is_some_and(|target| !target.is_empty()));
    assert!(value["binary"].as_str().is_some());
}

#[test]
fn version_command_prints_the_target() {
    Command::cargo_bin("mybot")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("target:").and(predicate::str::contains("commit:")));
}

#[test]
fn update_help_lists_its_flags() {
    Command::cargo_bin("mybot")
        .unwrap()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--check")
                .and(predicate::str::contains("--force"))
                .and(predicate::str::contains("--tag")),
        );
}
