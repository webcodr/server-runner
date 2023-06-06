use assert_cmd::Command;

#[test]
fn runs() {
    let mut command = Command::cargo_bin("server-runner").unwrap();

    command.assert().success();
}
