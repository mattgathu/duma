
static WITHOUT_ARGS_OUTPUT: &'static str = "error: The following required arguments were not provided:
    <URL>

USAGE:
    rget [FLAGS] [OPTIONS] <URL>

For more information try --help
";

static INVALID_URL_OUTPUT: &'static str = "Got error: failed to lookup address information:";
 
#[cfg(test)]
mod integration {
    use std::process::Command;
    use WITHOUT_ARGS_OUTPUT;
    use INVALID_URL_OUTPUT;

    #[cfg(not(windows))]
    fn get_cmd() -> Command {
        Command::new("./target/debug/rget")
    }

    #[cfg(windows)]
    fn get_cmd() -> Command {
        Command::new("./target/debug/rget.exe")
    }

    #[test]
    fn calling_rget_without_args() {
        let output = get_cmd()
            .output()
            .expect("failed to execute process");
    
        assert_eq!(String::from_utf8_lossy(&output.stderr), WITHOUT_ARGS_OUTPUT);
    }
    
    #[test]
    fn calling_rget_with_invalid_url() {
        let output = get_cmd()
            .arg("wwww.shouldnotwork.com")
            .output()
            .expect("failed to execute process");
    
        assert!(String::from_utf8_lossy(&output.stderr).contains(INVALID_URL_OUTPUT));
    }
}
