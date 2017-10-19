extern crate assert_cli;

#[cfg(test)]
mod integration {
    use assert_cli;

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
            .prints_error("error:")
            .unwrap();
    }
}
