use assert_cmd::Command;
use predicates::prelude::*;

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
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Timeout for server Zero Timeout Server must be between 1 and 300 seconds",
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

#[test]
fn stops_servers_when_final_command_cannot_spawn() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/missing_final_command.yaml")
        .arg("-a")
        .arg("5")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not start process definitely-not-a-real-command-for-server-runner",
        ));

    assert_port_released("127.0.0.1:8124");
}

#[cfg(unix)]
#[test]
fn stops_descendant_processes_when_server_never_becomes_ready() {
    use std::fs;

    let suffix = std::process::id();
    let marker = format!("/tmp/server-runner-descendant-marker-{suffix}");
    let config = format!("/tmp/server-runner-descendant-{suffix}.yaml");
    let script = format!("/tmp/server-runner-descendant-{suffix}.sh");
    let _ = fs::remove_file(&marker);
    let _ = fs::remove_file(&config);
    let _ = fs::remove_file(&script);

    fs::write(
        &script,
        format!(
            "#!/bin/sh\ntrap \"\" HUP\nsleep 30 </dev/null >/dev/null 2>&1 &\necho $! > {marker}\nwait\n"
        ),
    )
    .unwrap();

    fs::write(
        &config,
        format!(
            "servers:\n  - name: \"Descendant Server\"\n    url: \"http://localhost:9997\"\n    command: \"sh {script}\"\n    timeout: 1\ncommand: \"echo done\"\n"
        ),
    )
    .unwrap();

    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg(&config)
        .arg("-a")
        .arg("2")
        .assert()
        .failure();

    thread::sleep(Duration::from_millis(250));

    let pid = fs::read_to_string(&marker).unwrap();
    let still_running = std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.trim())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    assert!(
        !still_running,
        "descendant process {} was still running",
        pid.trim()
    );

    let _ = fs::remove_file(&marker);
    let _ = fs::remove_file(&config);
    let _ = fs::remove_file(&script);
}

#[cfg(unix)]
#[test]
fn stops_servers_on_ctrl_c() {
    use std::fs;

    let suffix = std::process::id();
    let config = format!("/tmp/server-runner-ctrl-c-{suffix}.yaml");
    let _cleanup = RemoveFileOnDrop(config.clone());
    let port = 8126;
    let addr = format!("127.0.0.1:{port}");

    fs::write(
        &config,
        format!(
            "servers:\n  - name: \"Interrupt Server\"\n    url: \"http://127.0.0.1:{port}\"\n    command: \"python3 -m http.server {port} --bind 127.0.0.1\"\n    timeout: 1\ncommand: \"sleep 30\"\n"
        ),
    )
    .unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("server-runner"))
        .arg("-c")
        .arg(&config)
        .spawn()
        .unwrap();

    assert_port_opens(&addr);

    let interrupt = std::process::Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(interrupt.success());

    let status = child.wait().unwrap();
    assert!(status.success());

    assert_port_released(&addr);
}

#[test]
fn preserves_final_command_stdout_and_stderr() {
    use std::fs;

    let suffix = std::process::id();
    let config = format!("/tmp/server-runner-final-output-{suffix}.yaml");
    let _cleanup = RemoveFileOnDrop(config.clone());
    let port = 8127;

    fs::write(
        &config,
        format!(
            "servers:\n  - name: \"Output Server\"\n    url: \"http://127.0.0.1:{port}\"\n    command: \"python3 -m http.server {port} --bind 127.0.0.1\"\n    timeout: 1\ncommand: \"sh -c 'echo final-out; echo final-err >&2'\"\n"
        ),
    )
    .unwrap();

    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg(&config)
        .arg("-a")
        .arg("5")
        .assert()
        .success()
        .stdout(predicate::str::contains("final-out"))
        .stderr(predicate::str::contains("final-err"));

    assert_port_released(&format!("127.0.0.1:{port}"));
}

#[test]
fn rejects_non_http_readiness_urls() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/invalid_url.yaml")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Readiness URL for server Invalid URL Server must use http or https",
        ));
}

#[test]
fn does_not_follow_readiness_redirects() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/redirect_url.yaml")
        .arg("-a")
        .arg("5")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Redirect Server after 5 attempts",
        ));
}

#[test]
fn rejects_zero_attempts() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-a")
        .arg("0")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value '0'"));
}

#[test]
fn rejects_unreasonably_large_timeout() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg("tests/huge_timeout.yaml")
        .arg("-a")
        .arg("1")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Timeout for server Huge Timeout Server must be between 1 and 300 seconds",
        ));
}

#[cfg(unix)]
#[test]
fn blames_the_server_that_never_became_ready() {
    use std::fs;

    let suffix = std::process::id();
    let config = format!("/tmp/server-runner-blame-{suffix}.yaml");
    let _cleanup = RemoveFileOnDrop(config.clone());
    let port = 8128;

    fs::write(
        &config,
        format!(
            "servers:\n  - name: \"Ready Server\"\n    url: \"http://127.0.0.1:{port}\"\n    command: \"python3 -m http.server {port} --bind 127.0.0.1\"\n    timeout: 1\n  - name: \"Never Ready\"\n    url: \"http://127.0.0.1:9996\"\n    command: \"sleep 30\"\n    timeout: 1\ncommand: \"echo done\"\n"
        ),
    )
    .unwrap();

    // A ready server must not keep burning attempts. Otherwise it exhausts them
    // first and gets blamed for a run that a different server was holding up.
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command
        .arg("-c")
        .arg(&config)
        .arg("-a")
        .arg("8")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Could not connect to server Never Ready",
        ))
        .stderr(predicate::str::contains("Ready Server after").not());

    assert_port_released(&format!("127.0.0.1:{port}"));
}

#[cfg(unix)]
#[test]
fn stops_final_command_descendants_on_ctrl_c() {
    use std::fs;

    let suffix = std::process::id();
    let marker = format!("/tmp/server-runner-final-descendant-{suffix}");
    let config = format!("/tmp/server-runner-final-descendant-{suffix}.yaml");
    let script = format!("/tmp/server-runner-final-descendant-{suffix}.sh");
    let _ = fs::remove_file(&marker);
    let _cleanup_config = RemoveFileOnDrop(config.clone());
    let _cleanup_script = RemoveFileOnDrop(script.clone());
    let _cleanup_marker = RemoveFileOnDrop(marker.clone());
    let port = 8129;
    let addr = format!("127.0.0.1:{port}");

    // The descendant outlives the test by far, so if Ctrl+C fails to kill the
    // group it is still running when we check rather than having exited on its
    // own. The runner also blocks on its output readers until the group dies,
    // so an uncancelled group shows up as a runner that never exits.
    fs::write(
        &script,
        format!("#!/bin/sh\nsleep 300 </dev/null >/dev/null 2>&1 &\necho $! > {marker}\nwait\n"),
    )
    .unwrap();

    fs::write(
        &config,
        format!(
            "servers:\n  - name: \"Output Server\"\n    url: \"http://127.0.0.1:{port}\"\n    command: \"python3 -m http.server {port} --bind 127.0.0.1\"\n    timeout: 1\ncommand: \"sh {script}\"\n"
        ),
    )
    .unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("server-runner"))
        .arg("-c")
        .arg(&config)
        .arg("-a")
        .arg("20")
        .spawn()
        .unwrap();

    assert_port_opens(&addr);
    let pid = wait_for_marker(&marker);

    let interrupt = std::process::Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(interrupt.success());

    let status = wait_with_timeout(&mut child, Duration::from_secs(10))
        .expect("runner did not exit promptly after Ctrl+C");
    assert!(status.success());

    thread::sleep(Duration::from_millis(250));
    let orphaned = pid_alive(&pid);
    if orphaned {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(&pid)
            .status();
    }

    assert!(!orphaned, "final command descendant {pid} survived Ctrl+C");
    assert_port_released(&addr);
}

#[cfg(unix)]
#[test]
fn honours_ctrl_c_while_a_probe_is_in_flight() {
    let addr = "127.0.0.1:8130";

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("server-runner"))
        .arg("-c")
        .arg("tests/hanging_url.yaml")
        .arg("-a")
        .arg("20")
        .spawn()
        .unwrap();

    // The port accepts but never answers, so the runner's second probe blocks
    // for the full 10s timeout. Wait past the first one-second retry gap so the
    // signal lands squarely inside that probe rather than in the gap.
    assert_port_opens(addr);
    thread::sleep(Duration::from_secs(3));

    let interrupt = std::process::Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(interrupt.success());

    let status = wait_with_timeout(&mut child, Duration::from_secs(5))
        .expect("runner ignored Ctrl+C delivered during an in-flight probe");
    assert!(status.success());

    assert_port_released(addr);
}

#[cfg(unix)]
fn wait_for_marker(path: &str) -> String {
    for _ in 0..100 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let pid = contents.trim().to_string();
            if !pid.is_empty() {
                return pid;
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    panic!("final command never recorded its descendant pid in {path}");
}

#[cfg(unix)]
fn pid_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;

    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    None
}

fn assert_port_released(addr: &str) {
    if TcpListener::bind(addr).is_ok() {
        return;
    }

    for _ in 0..10 {
        if TcpListener::bind(addr).is_ok() {
            return;
        }

        thread::sleep(Duration::from_millis(50));
    }

    panic!("server process still listening on {addr}");
}

fn assert_port_opens(addr: &str) {
    for _ in 0..50 {
        if TcpStream::connect(addr).is_ok() {
            return;
        }

        thread::sleep(Duration::from_millis(50));
    }

    panic!("server process did not start listening on {addr}");
}

struct RemoveFileOnDrop(String);

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
