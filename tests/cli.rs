use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn runs() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command.assert().success();
}

#[test]
fn fails_on_missing_config_file() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("foobar.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not find config file foobar.yaml",
        ));
}

#[test]
fn fails_on_too_many_attempts() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("max_attempts.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Hello World after 10 attempts",
        ));
}

#[test]
fn fails_on_too_many_attempts_custom() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("max_attempts.yaml")
        .arg("-a")
        .arg("5")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Hello World after 5 attempts",
        ));
}
