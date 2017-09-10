use std::process::Command;

#[test]
fn calling_rget_without_args() {
    let output = Command::new("sh")
        .arg("-c")
        .arg("./target/debug/rget")
        .output()
        .expect("failed to execute process");

    assert!(String::from_utf8_lossy(&output.stderr).contains("error: The following required arguments were not provided:"));
}
