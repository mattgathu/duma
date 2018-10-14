extern crate assert_cli;

#[cfg(test)]
mod integration {
    use assert_cli;

    #[test]
    fn calling_duma_without_args() {
        assert_cli::Assert::main_binary()
            .fails()
            .and()
            .prints_error("error: The following required arguments were not provided:")
            .unwrap();

    }

    #[test]
    fn calling_duma_with_invalid_url() {
        assert_cli::Assert::main_binary()
            .with_args(&["wwww.shouldnotworkanddoesnot.com"])
            .fails()
            .and()
            .prints_error("error:")
            .unwrap();
    }

    #[test]
    fn test_request_timeout() {
        assert_cli::Assert::main_binary()
            .with_args(&["-T", "3", "https://httpbin.org/delay/60"])
            .fails()
            .and()
            .prints_error("timed out")
            .unwrap();
    }
}
