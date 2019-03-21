#[cfg(test)]
mod integration {
    use assert_cmd::prelude::*;
    use std::process::Command;

    #[test]
    fn calling_duma_without_args() {
        let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
        cmd.assert().failure();
    }

    #[test]
    fn calling_duma_with_invalid_url() {
        let mut cmd: Command = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
        cmd.args(&["wwww.shouldnotwork.com"]).assert().failure();
    }

    #[test]
    fn test_request_timeout() {
        let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
        cmd.args(&["-T", "3", "https://httpbin.org/delay/60"])
            .assert()
            .failure();
    }
}
