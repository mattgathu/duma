use failure::{bail, Fallible};
use url::{Url, ParseError};
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

pub fn parse_url(url: &str) -> Result<Url, ParseError> {
    match Url::parse(url) {
        Ok(url) => Ok(url),
        Err(error) if error == ParseError::RelativeUrlWithoutBase => {
            let url_with_base = format!("{}{}", "http://", url);
            Url::parse(url_with_base.as_str())
        }
        Err(error) => Err(error),
    }
}

pub fn gen_error(msg: String) -> Fallible<()> {
    bail!(msg)
}

pub fn get_file_handle(fname: &str, resume_download: bool, append: bool) -> io::Result<File> {
    if resume_download && Path::new(fname).exists() {
        if append {
            match OpenOptions::new().append(true).open(fname) {
                Ok(file) => Ok(file),
                Err(error) => Err(error),
            }
        } else {
            match OpenOptions::new().write(true).open(fname) {
                Ok(file) => Ok(file),
                Err(error) => Err(error),
            }
        }
    } else {
        match OpenOptions::new().write(true).create(true).open(fname) {
            Ok(file) => Ok(file),
            Err(error) => Err(error),
        }
    }
}
