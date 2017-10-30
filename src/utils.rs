use std::io;
use std::path::Path;
use std::fmt::Display;
use std::fs::OpenOptions;
use std::io::{Error, ErrorKind, Write};
use reqwest::{Url, UrlError};

pub fn parse_url(url: &str) -> Result<Url, UrlError> {
    match Url::parse(url) {
        Ok(url) => Ok(url),
        Err(error) if error == UrlError::RelativeUrlWithoutBase => {
            let url_with_base = format!("{}{}", "http://", url);
            Url::parse(url_with_base.as_str())
        }
        Err(error) => Err(error),
    }

}

pub fn gen_error(msg: String) -> Result<(), Box<::std::error::Error>> {
    Err(Box::new(Error::new(ErrorKind::Other, msg)))
}

pub fn get_file_handle(fname: &str, resume_download: bool) -> io::Result<Box<Write>> {
    if fname == "-" {
        return Ok(Box::new(io::stdout()));
    }
    if resume_download && Path::new(fname).exists() {
        match OpenOptions::new().append(true).open(fname) {
            Ok(file) => Ok(Box::new(file)),
            Err(error) => Err(error),
        }
    } else {
        match OpenOptions::new().write(true).create(true).open(fname) {
            Ok(file) => Ok(Box::new(file)),
            Err(error) => Err(error),
        }
    }
}

pub fn print<T: Display>(var: &T, quiet_mode: bool, is_error: bool) {
    // print if not in quiet mode
    if !quiet_mode {
        if is_error {
            eprintln!("{}", var);
        } else {
            println!("{}", var);
        }
    }
}
