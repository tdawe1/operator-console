use std::process::Command;

#[test]
fn help_command_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_operator-console"))
        .arg("--help")
        .output()
        .expect("failed to execute operator-console");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("operator-console"));
}
