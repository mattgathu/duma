use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::fmt::Display;
use reqwest::Url;
use indicatif::{ProgressBar, ProgressStyle, HumanBytes};
use console::style;


use ftp::FtpStream;

static PBAR_FMT: &'static str = "{msg} {spinner:.green} {percent}% [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} eta: {eta}";

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

fn create_progress_bar(quiet_mode: bool, msg: &str, length: Option<usize>) -> ProgressBar {
    let progbar = if quiet_mode {
        ProgressBar::hidden()
    } else {
        match length {
            Some(len) => ProgressBar::new(len as u64),
            None => ProgressBar::new_spinner(),
        }
    };

    progbar.set_message(msg);
    if length.is_some() {
        progbar.set_style(ProgressStyle::default_bar().template(PBAR_FMT).progress_chars("=> "));
    } else {
        progbar.set_style(ProgressStyle::default_spinner());
    }

    progbar
}


pub fn download(url: Url,
                filename: Option<&str>,
                quiet_mode: bool)
                -> Result<(), Box<::std::error::Error>> {
    let ftp_server = format!("{}:{}",
                             url.host_str().unwrap(),
                             url.port_or_known_default().unwrap());
    let username = if url.username().is_empty() {
        "anonymous"
    } else {
        url.username()
    };
    let password = url.password().unwrap_or("anonymous");

    let mut path_segments: Vec<&str> = url.path_segments().unwrap().collect();
    let ftp_fname = path_segments.pop().unwrap();

    let mut conn = FtpStream::connect(ftp_server)?;
    conn.login(username, password)?;
    for path in &path_segments {
        conn.cwd(path)?;
    }
    let ct_len = conn.size(ftp_fname)?;
    let mut reader = conn.get(ftp_fname)?;

    match ct_len {
        Some(len) => {
            print(&format!("Length: {} ({})",
                           style(len).green(),
                           style(format!("{}", HumanBytes(len as u64))).red()),
                  quiet_mode,
                  false);
        }
        None => {
            print(&format!("Length: {}", style("unknown").red()),
                  quiet_mode,
                  false);
        }
    }

    let fname = match filename {
        Some(name) => name,
        None => {
            url.path()
                .split('/')
                .last()
                .unwrap()
        }
    };


    let chunk_size = 2048usize;
    let out_file = get_file_handle(fname, false)?;
    let mut writer = BufWriter::new(out_file);

    let pbar = create_progress_bar(quiet_mode, fname, ct_len);

    loop {
        let mut buffer = vec![0; chunk_size];
        let bcount = reader.read(&mut buffer[..]).unwrap();
        buffer.truncate(bcount);
        if !buffer.is_empty() {
            writer.write_all(buffer.as_slice()).unwrap();
            pbar.inc(bcount as u64);
        } else {
            break;
        }
    }

    pbar.finish();

    Ok(())

}
