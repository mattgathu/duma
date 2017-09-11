use std::process::Command;

static WITHOUT_ARGS_OUTPUT: &'static str = "error: The following required arguments were not provided:
    <URL>

USAGE:
    rget [FLAGS] [OPTIONS] <URL>

For more information try --help
";

static INVALID_URL_OUTPUT: &'static str = "Got error: failed to lookup address information: nodename nor servname provided, or not known
";
 
#[cfg(test)]
mod integration {
    use Command;
    use WITHOUT_ARGS_OUTPUT;
    use INVALID_URL_OUTPUT;

    #[test]
    fn calling_rget_without_args() {
        let output = Command::new("./target/debug/rget")
            .output()
            .expect("failed to execute process");
    
        assert_eq!(String::from_utf8_lossy(&output.stderr), WITHOUT_ARGS_OUTPUT);
    }
    
    #[test]
    fn calling_rget_with_invalid_url() {
        let output = Command::new("./target/debug/rget")
            .arg("wwww.shouldnotwork.com")
            .output()
            .expect("failed to execute process");
    
        assert_eq!(String::from_utf8_lossy(&output.stderr), INVALID_URL_OUTPUT);
    }
}
