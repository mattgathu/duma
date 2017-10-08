use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::fmt::Display;
use reqwest::{Client, Url, UrlError};
use reqwest::header::{Range, ByteRangeSpec, ContentLength, ContentType, AcceptRanges, RangeUnit};
use indicatif::{ProgressBar, ProgressStyle, HumanBytes};
use console::style;



fn parse_url(url: &str) -> Result<Url, UrlError> {
    match Url::parse(url) {
        Ok(url) => Ok(url),
        Err(error) if error == UrlError::RelativeUrlWithoutBase => {
            let url_with_base = format!("{}{}", "http://", url);
            Url::parse(url_with_base.as_str())
        }
        Err(error) => Err(error),
    }

}


fn get_file_handle(fname: &str, resume_download: bool) -> io::Result<File> {
    if resume_download {
        match OpenOptions::new().append(true).open(fname) {
            Ok(file) => Ok(file),
            Err(ref error) if error.kind() == ErrorKind::NotFound => {
                OpenOptions::new().write(true).create(true).open(fname)
            }
            Err(error) => Err(error),
        }
    } else {
        OpenOptions::new().write(true).create(true).open(fname)
    }
}

fn create_progress_bar(quiet_mode: bool, msg: &str, length: Option<u64>) -> ProgressBar {
    let progbar = if quiet_mode {
        ProgressBar::hidden()
    } else {
        match length {
            Some(len) => ProgressBar::new(len),
            None => ProgressBar::new_spinner(),
        }
    };

    progbar.set_message(msg);
    if length.is_some() {
        progbar.set_style(ProgressStyle::default_bar()
                .template("{msg} {spinner:.green} {percent}% [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} eta: {eta}")
                .progress_chars("=> "));
    } else {
        progbar.set_style(ProgressStyle::default_spinner());
    }

    progbar
}


pub fn download(target: &str,
            quiet_mode: bool,
            filename: Option<&str>,
            resume_download: bool)
            -> Result<(), Box<::std::error::Error>> {

    let fname = match filename {
        Some(name) => name,
        None => target.split('/').last().unwrap(),
    };

    // parse url
    let url = parse_url(target)?;
    let client = Client::new().unwrap();
    let mut resp = if resume_download {
        let req_headers = client.head(parse_url(target)?)?
            .send()?
            .headers()
            .clone();
        match req_headers.get::<AcceptRanges>() {
            Some(header) => {
                if header.contains(&RangeUnit::Bytes) {
                    let byte_count = match fs::metadata(fname) {
                        Ok(metadata) => metadata.len(),
                        Err(_) => 0u64,
                    };
                    // if byte_count is zero don't pass range header
                    match byte_count {
                        0 => {
                            client.get(url)?
                                .send()?
                        }
                        _ => {
                            let byte_range = Range::Bytes(vec![ByteRangeSpec::AllFrom(byte_count)]);
                            client.get(url)?
                                .header(byte_range)
                                .send()?
                        }
                    }
                } else {
                    client.get(url)?
                        .send()?
                }
            }
            None => {
                client.get(url)?
                    .send()?
            }
        }

        //client.get(url)?.send()?

    } else {
        client.get(url)?
            .send()?
    };
    print(&format!("HTTP request sent... {}",
                   style(format!("{}", resp.status())).green()),
          quiet_mode,
          false);
    if resp.status().is_success() {

        let headers = if resume_download {
            resp.headers().clone()
        } else {
            client.get(parse_url(target)?)?
                .send()?
                .headers()
                .clone()
        };
        let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);

        let ct_type = headers.get::<ContentType>().unwrap();

        match ct_len {
            Some(len) => {
                print(&format!("Length: {} ({})",
                               style(len).green(),
                               style(format!("{}", HumanBytes(len))).red()),
                      quiet_mode,
                      false);
            }
            None => {
                print(&format!("Length: {}", style("unknown").red()),
                      quiet_mode,
                      false);
            }
        }

        print(&format!("Type: {}", style(ct_type).green()),
              quiet_mode,
              false);

        print(&format!("Saving to: {}", style(fname).green()),
              quiet_mode,
              false);

        let chunk_size = match ct_len {
            Some(x) => x as usize / 99,
            None => 1024usize, // default chunk size
        };

        let out_file = get_file_handle(fname, resume_download)?;
        let mut writer = BufWriter::new(out_file);

        let pbar = create_progress_bar(quiet_mode, fname, ct_len);

        // if resuming download, update progress bar
        if resume_download {
            let metadata = fs::metadata(fname)?;
            pbar.inc(metadata.len());
        }

        loop {
            let mut buffer = vec![0; chunk_size];
            let bcount = resp.read(&mut buffer[..]).unwrap();
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                writer.write_all(buffer.as_slice()).unwrap();
                pbar.inc(bcount as u64);
            } else {
                break;
            }
        }

        pbar.finish();

    } else if resp.status().as_u16() == 416 {
        print(&style("\nThe file is already fully retrieved; nothing to do.\n").red(),
              quiet_mode,
              false);
    }

    Ok(())

}

fn print<T: Display>(var: &T, quiet_mode: bool, is_error: bool) {
    // print if not in quiet mode
    if !quiet_mode {
        if is_error {
            eprintln!("{}", var);
        } else {
            println!("{}", var);
        }
    }
}

