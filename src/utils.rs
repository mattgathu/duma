use failure::{bail, Fallible};
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use url::{ParseError, Url};

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

pub fn decode_percent_encoded_data(data: &str) -> Fallible<String> {
    let mut unescaped_bytes: Vec<u8> = Vec::new();
    let mut bytes = data.bytes();
    while let Some(b) = bytes.next() {
        match b as char {
            '%' => {
                let bytes_to_decode = &[bytes.next().unwrap(), bytes.next().unwrap()];
                let hex_str = std::str::from_utf8(bytes_to_decode).unwrap();
                unescaped_bytes.push(u8::from_str_radix(hex_str, 16).unwrap());
            }
            _ => {
                unescaped_bytes.push(b);
            }
        }
    }
    Ok(String::from_utf8(unescaped_bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_percent_encoded_data() {
        let x = "hello%20world";
        let y = decode_percent_encoded_data(x).unwrap();
        assert_eq!(&y, "hello world");
    }
}
