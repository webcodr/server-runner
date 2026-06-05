use assert_cmd::Command;
use predicates::prelude::*;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

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
        .arg("tests/max_attempts.yaml")
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
        .arg("tests/max_attempts.yaml")
        .arg("-a")
        .arg("5")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Hello World after 5 attempts",
        ));
}

#[test]
fn fails_on_empty_server_list() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/empty_servers.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Configuration must include at least one server",
        ));
}

#[test]
fn fails_on_timeout_with_custom_timeout() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/timeout.yaml")
        .arg("-a")
        .arg("2")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Timeout Test Server after 2 attempts",
        ));
}

#[test]
fn fails_on_empty_command() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/empty_command.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Configuration must include a command to run",
        ));
}

#[test]
fn fails_on_invalid_yaml() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/invalid_yaml.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not find config file tests/invalid_yaml.yaml",
        ));
}

#[test]
fn fails_on_missing_required_fields() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/missing_fields.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not parse config file tests/missing_fields.yaml",
        ));
}

#[test]
fn fails_on_multiple_unreachable_servers() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/multiple_servers.yaml")
        .arg("-a")
        .arg("2")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Could not connect to server"));
}

#[test]
fn fails_on_zero_timeout() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/zero_timeout.yaml")
        .arg("-a")
        .arg("1")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Zero Timeout Server after 1 attempt",
        ));
}

#[test]
fn fails_on_one_attempt() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/timeout.yaml")
        .arg("-a")
        .arg("1")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Timeout Test Server after 1 attempt",
        ));
}

#[test]
fn fails_when_final_command_exits_non_zero() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/failing_command.yaml")
        .arg("-a")
        .arg("5")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Command false failed with exit status",
        ));

    assert_port_released("127.0.0.1:8123");
}

fn assert_port_released(addr: &str) {
    if TcpListener::bind(addr).is_ok() {
        return;
    }

    if let Ok(mut stream) = TcpStream::connect(addr) {
        let _ = stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
    }

    for _ in 0..10 {
        if TcpListener::bind(addr).is_ok() {
            return;
        }

        thread::sleep(Duration::from_millis(50));
    }

    panic!("server process still listening on {addr}");
}
