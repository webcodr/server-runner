use assert_cmd::Command;

#[test]
fn runs() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command.assert().success();
}

#[test]
fn fails_on_missing_config_file() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command.arg("-c").arg("foobar.yaml").assert().failure();
}
