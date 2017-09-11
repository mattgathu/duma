use std::process::Command;

#[test]
fn calling_rget_without_args() {
    let expected = r#"error: The following required arguments were not provided:
    <URL>

USAGE:
    rget [FLAGS] [OPTIONS] <URL>

For more information try --help
"#;
    let output = Command::new("./target/debug/rget")
        .output()
        .expect("failed to execute process");

    assert_eq!(String::from_utf8_lossy(&output.stderr), expected);
}
