extern crate assert_cli;

static INVALID_URL_OUTPUT: &'static str = "Got error: failed to lookup address information:";

#[cfg(test)]
mod integration {
    use assert_cli;
    use INVALID_URL_OUTPUT;

    #[test]
    fn calling_rget_without_args() {
        assert_cli::Assert::main_binary()
            .fails()
            .and()
            .prints_error("error: The following required arguments were not provided:")
            .unwrap();

    }

    #[test]
    fn calling_rget_with_invalid_url() {
        assert_cli::Assert::main_binary()
            .with_args(&["wwww.shouldnotwork.com"])
            .fails()
            .and()
            .prints_error(INVALID_URL_OUTPUT)
            .unwrap();
    }
}
