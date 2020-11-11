use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use clap::ArgMatches;
use console::style;
use failure::{format_err, Fallible};
use indicatif::{HumanBytes, ProgressBar};
use reqwest::blocking::Client;
use reqwest::header::{self, HeaderMap, HeaderValue};

use url::Url;

use crate::bar::create_progress_bar;
use crate::core::{Config, EventsHandler, FtpDownload, HttpDownload};
use crate::utils::{decode_percent_encoded_data, get_file_handle};

fn request_headers_from_server(url: &Url, timeout: u64, ua: &str) -> Fallible<HeaderMap> {
    let resp = Client::new()
        .get(url.as_ref())
        .timeout(Duration::from_secs(timeout))
        .header(header::USER_AGENT, HeaderValue::from_str(ua)?)
        .header(header::ACCEPT, HeaderValue::from_str("*/*")?)
        .send()?;
    Ok(resp.headers().clone())
}

fn print_headers(headers: HeaderMap) {
    for (hdr, val) in headers.iter() {
        println!(
            "{}: {}",
            style(hdr.as_str()).red(),
            style(val.to_str().unwrap_or("<..>")).green()
        );
    }
}

fn get_resume_chunk_offsets(
    fname: &str,
    ct_len: u64,
    chunk_size: u64,
) -> Fallible<Vec<(u64, u64)>> {
    let st_fname = format!("{}.st", fname);
    let input = fs::File::open(st_fname)?;
    let buf = BufReader::new(input);
    let mut downloaded = vec![];
    for line in buf.lines() {
        let l = line?;
        let l = l.split(':').collect::<Vec<_>>();
        let n = (l[0].parse::<u64>()?, l[1].parse::<u64>()?);
        downloaded.push(n);
    }
    downloaded.sort_by_key(|a| a.1);
    let mut chunks = vec![];

    let mut i: u64 = 0;
    for (bc, offset) in downloaded {
        if i == offset {
            i = offset + bc;
        } else {
            chunks.push((i, offset - 1));
            i = offset + bc;
        }
    }

    while (ct_len - i) > chunk_size {
        chunks.push((i, i + chunk_size - 1));
        i += chunk_size;
    }
    chunks.push((i, ct_len));

    Ok(chunks)
}

fn gen_filename(url: &Url, fname: Option<&str>, headers: Option<&HeaderMap>) -> String {
    let content_disposition = headers
        .and_then(|hdrs| hdrs.get(header::CONTENT_DISPOSITION))
        .and_then(|val| {
            let val = val.to_str().unwrap_or("");
            if val.contains("filename=") {
                Some(val)
            } else {
                None
            }
        })
        .and_then(|val| {
            let x = val
                .rsplit(';')
                .nth(0)
                .unwrap_or("")
                .rsplit('=')
                .nth(0)
                .unwrap_or("")
                .trim_start_matches('"')
                .trim_end_matches('"');
            if !x.is_empty() {
                Some(x.to_string())
            } else {
                None
            }
        });
    match fname {
        Some(name) => name.to_owned(),
        None => match content_disposition {
            Some(val) => val,
            None => {
                let name = &url.path().split('/').last().unwrap_or("");
                if !name.is_empty() {
                    match decode_percent_encoded_data(name) {
                        Ok(val) => val,
                        _ => name.to_string(),
                    }
                } else {
                    "index.html".to_owned()
                }
            }
        },
    }
}

fn calc_bytes_on_disk(fname: &str) -> Fallible<Option<u64>> {
    // use state file if present
    let st_fname = format!("{}.st", fname);
    if Path::new(&st_fname).exists() {
        let input = fs::File::open(st_fname)?;
        let buf = BufReader::new(input);
        let mut byte_count: u64 = 0;
        for line in buf.lines() {
            let num_of_bytes = line?
                .split(':')
                .nth(0)
                .ok_or_else(|| format_err!("failed to split state file line"))?
                .parse::<u64>()?;
            byte_count += num_of_bytes;
        }
        return Ok(Some(byte_count));
    }
    match fs::metadata(fname) {
        Ok(metadata) => Ok(Some(metadata.len())),
        _ => Ok(None),
    }
}

fn prep_headers(fname: &str, resume: bool, user_agent: &str) -> Fallible<HeaderMap> {
    let bytes_on_disk = calc_bytes_on_disk(fname)?;
    let mut headers = HeaderMap::new();
    if let Some(bcount) = bytes_on_disk {
        if resume {
            let byte_range = format!("bytes={}-", bcount);
            headers.insert(header::RANGE, byte_range.parse()?);
        }
    }

    headers.insert(header::USER_AGENT, user_agent.parse()?);

    Ok(headers)
}

pub fn ftp_download(url: Url, quiet_mode: bool, filename: Option<&str>) -> Fallible<()> {
    let fname = gen_filename(&url, filename, None);

    let mut client = FtpDownload::new(url.clone());
    let events_handler = DefaultEventsHandler::new(&fname, false, false, quiet_mode)?;
    client.events_hook(events_handler).download()?;
    Ok(())
}

pub fn http_download(url: Url, args: &ArgMatches, version: &str) -> Fallible<()> {
    let resume_download = args.is_present("continue");
    let concurrent_download = !args.is_present("singlethread");
    let user_agent = args
        .value_of("AGENT")
        .unwrap_or(&format!("Duma/{}", version))
        .to_owned();
    let timeout = if let Some(secs) = args.value_of("SECONDS") {
        secs.parse::<u64>()?
    } else {
        30u64
    };
    let num_workers = if let Some(num) = args.value_of("NUM_CONNECTIONS") {
        num.parse::<usize>()?
    } else {
        8usize
    };
    let headers = request_headers_from_server(&url, timeout, &user_agent)?;
    let fname = gen_filename(&url, args.value_of("FILE"), Some(&headers));

    // early exit if headers flag is present
    if args.is_present("headers") {
        print_headers(headers);
        return Ok(());
    }
    let ct_len = if let Some(val) = headers.get("Content-Length") {
        val.to_str()?.parse::<u64>().unwrap_or(0)
    } else {
        0u64
    };

    let headers = prep_headers(&fname, resume_download, &user_agent)?;

    let state_file_exists = Path::new(&format!("{}.st", fname)).exists();
    let chunk_size = 512_000u64;

    let chunk_offsets =
        if state_file_exists && resume_download && concurrent_download && ct_len != 0 {
            Some(get_resume_chunk_offsets(&fname, ct_len, chunk_size)?)
        } else {
            None
        };

    let bytes_on_disk = if resume_download {
        calc_bytes_on_disk(&fname)?
    } else {
        None
    };

    let conf = Config {
        user_agent: user_agent.clone(),
        resume: resume_download,
        headers,
        file: fname.clone(),
        timeout,
        concurrent: concurrent_download,
        max_retries: 100,
        num_workers,
        bytes_on_disk,
        chunk_offsets,
        chunk_size,
    };

    let mut client = HttpDownload::new(url.clone(), conf.clone());
    let quiet_mode = args.is_present("quiet");
    let events_handler =
        DefaultEventsHandler::new(&fname, resume_download, concurrent_download, quiet_mode)?;
    client.events_hook(events_handler).download()?;
    Ok(())
}

pub struct DefaultEventsHandler {
    prog_bar: Option<ProgressBar>,
    bytes_on_disk: Option<u64>,
    fname: String,
    file: BufWriter<fs::File>,
    st_file: Option<BufWriter<fs::File>>,
    server_supports_resume: bool,
    quiet_mode: bool,
}

impl DefaultEventsHandler {
    pub fn new(
        fname: &str,
        resume: bool,
        concurrent: bool,
        quiet_mode: bool,
    ) -> Fallible<DefaultEventsHandler> {
        let st_file = if concurrent {
            Some(BufWriter::new(get_file_handle(
                &format!("{}.st", fname),
                resume,
                true,
            )?))
        } else {
            None
        };
        Ok(DefaultEventsHandler {
            prog_bar: None,
            bytes_on_disk: calc_bytes_on_disk(fname)?,
            fname: fname.to_owned(),
            file: BufWriter::new(get_file_handle(fname, resume, !concurrent)?),
            st_file,
            server_supports_resume: false,
            quiet_mode,
        })
    }

    fn create_prog_bar(&mut self, length: Option<u64>) {
        let byte_count = if self.server_supports_resume {
            self.bytes_on_disk
        } else {
            None
        };
        if let Some(len) = length {
            let exact = style(len).green();
            let human_readable = style(format!("{}", HumanBytes(len))).red();

            println!("Length: {} ({})", exact, human_readable);
        } else {
            println!("Length: {}", style("unknown").red());
        }

        let prog_bar = create_progress_bar(&self.fname, length);
        if let Some(count) = byte_count {
            prog_bar.inc(count);
        }
        self.prog_bar = Some(prog_bar);
    }
}

impl EventsHandler for DefaultEventsHandler {
    fn on_headers(&mut self, headers: HeaderMap) {
        if self.quiet_mode {
            return;
        }
        let ct_type = if let Some(val) = headers.get(header::CONTENT_TYPE) {
            val.to_str().unwrap_or("")
        } else {
            ""
        };
        println!("Type: {}", style(ct_type).green());

        println!("Saving to: {}", style(&self.fname).green());
        if let Some(val) = headers.get(header::CONTENT_LENGTH) {
            self.create_prog_bar(val.to_str().unwrap_or("").parse::<u64>().ok());
        } else {
            println!(
                "{}",
                style("Got no content-length. Progress bar skipped.").red()
            );
        }
    }

    fn on_ftp_content_length(&mut self, ct_len: Option<u64>) {
        if !self.quiet_mode {
            self.create_prog_bar(ct_len);
        }
    }

    fn on_server_supports_resume(&mut self) {
        self.server_supports_resume = true;
    }

    fn on_content(&mut self, content: &[u8]) -> Fallible<()> {
        let byte_count = content.len() as u64;
        self.file.write_all(content)?;
        if let Some(ref mut b) = self.prog_bar {
            b.inc(byte_count);
        }

        Ok(())
    }

    fn on_concurrent_content(&mut self, content: (u64, u64, &[u8])) -> Fallible<()> {
        let (byte_count, offset, buf) = content;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(buf)?;
        self.file.flush()?;
        if let Some(ref mut b) = self.prog_bar {
            b.inc(byte_count);
        }
        if let Some(ref mut file) = self.st_file {
            writeln!(file, "{}:{}", byte_count, offset)?;
            file.flush()?;
        }
        Ok(())
    }

    fn on_resume_download(&mut self, bytes_on_disk: u64) {
        self.bytes_on_disk = Some(bytes_on_disk);
    }

    fn on_finish(&mut self) {
        if let Some(ref mut b) = self.prog_bar {
            b.finish();
        }
        match fs::remove_file(&format!("{}.st", self.fname)) {
            _ => {}
        }
    }

    fn on_max_retries(&mut self) {
        if !self.quiet_mode {
            eprintln!("{}", style("max retries exceeded. Quitting!").red());
        }
        match self.file.flush() {
            _ => {}
        }
        if let Some(ref mut file) = self.st_file {
            match file.flush() {
                _ => {}
            }
        }
        ::std::process::exit(0);
    }

    fn on_failure_status(&self, status: i32) {
        if self.quiet_mode {
            return;
        }
        if status == 416 {
            println!(
                "{}",
                &style("\nThe file is already fully retrieved; nothing to do.\n").red()
            );
        }
    }
}
