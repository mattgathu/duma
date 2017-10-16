use std::io::Read;
use std::io::Write;
use std::io::BufWriter;
use reqwest::Url;
use indicatif::HumanBytes;
use console::style;
use ftp::FtpStream;

use utils::{get_file_handle, print};
use bar::create_progress_bar;


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

    let bar_len = match ct_len {
        Some(len) => {
            let len_u64 = len as u64;
            Some(len_u64)
        }
        None => None
    };


    let chunk_size = 2048usize;
    let out_file = get_file_handle(fname, false)?;
    let mut writer = BufWriter::new(out_file);

    let pbar = create_progress_bar(quiet_mode, fname, bar_len);

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
