use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use clap::ArgMatches;
use console::style;
use failure::Fallible;
use indicatif::{HumanBytes, ProgressBar};
use reqwest::header::{ByteRangeSpec, ContentLength, ContentType, Headers, Range, UserAgent};
use reqwest::{Client, StatusCode, Url};

use crate::bar::create_progress_bar;
use crate::core::{Config, EventsHandler, FtpDownload, HttpDownload};
use crate::utils::get_file_handle;

fn request_headers_from_server(url: &Url) -> Fallible<Headers> {
    let client = Client::new()?;
    let head_resp = client.head(url.clone())?.send()?;

    Ok(head_resp.headers().clone())
}

fn print_headers(headers: Headers) {
    for hdr in headers.iter() {
        println!(
            "{}: {}",
            style(hdr.name()).red(),
            style(hdr.value_string()).green()
        );
    }
}

fn get_resume_chunk_sizes(fname: &str, ct_len: u64) -> Fallible<Vec<(u64, u64)>> {
    let st_fname = format!("{}.st", fname);
    let input = fs::File::open(st_fname)?;
    let buf = BufReader::new(input);
    let mut downloaded = vec![];
    let mut lines = buf.lines().filter_map(|l| l.ok()).collect::<Vec<String>>();
    lines.pop(); // throw last line away to avoid partial written line
    for line in lines {
        let l = line
            .split(':')
            .map(|x| x.parse::<u64>().unwrap())
            .collect::<Vec<u64>>();
        downloaded.push((l[0], l[1]));
    }
    downloaded.sort_by_key(|a| a.1);
    let mut chunks = vec![];

    let mut i: u64 = 0;
    for (bc, idx) in downloaded {
        if i == idx {
            i = idx + bc;
        } else {
            chunks.push((i, idx));
            i = idx + bc;
        }
    }
    chunks.push((i, ct_len));

    Ok(chunks)
}

fn gen_filename(url: &Url, fname: Option<&str>) -> String {
    match fname {
        Some(name) => name.to_owned(),
        None => {
            let name = &url.path().split('/').last().unwrap();
            if !name.is_empty() {
                name.to_string()
            } else {
                "index.html".to_owned()
            }
        }
    }
}

fn calc_bytes_on_disk(fname: &str) -> Option<u64> {
    // use state file if present
    let st_fname = format!("{}.st", fname);
    if Path::new(&st_fname).exists() {
        let input = fs::File::open(st_fname).unwrap();
        let buf = BufReader::new(input);
        let mut lines = buf.lines().filter_map(|l| l.ok()).collect::<Vec<String>>();
        lines.pop();
        let byte_count: u64 = lines
            .iter()
            .map(|line| line.split(':').nth(0).unwrap())
            .map(|part| part.parse::<u64>().unwrap())
            .sum();
        return Some(byte_count);
    }
    match fs::metadata(fname) {
        Ok(metadata) => Some(metadata.len()),
        _ => None,
    }
}

fn prep_headers(fname: &str, resume: bool, user_agent: &str) -> Headers {
    let bytes_on_disk = calc_bytes_on_disk(fname);
    let mut headers = Headers::new();
    if resume && bytes_on_disk.is_some() {
        let range_hdr = Range::Bytes(vec![ByteRangeSpec::AllFrom(bytes_on_disk.unwrap())]);
        headers.set(range_hdr);
    }

    headers.set(UserAgent::new(user_agent.to_string()));

    headers
}

fn get_http_proxies() -> Option<HashMap<String, String>> {
    let mut proxies = HashMap::new();
    if let Ok(proxy) = env::var("http_proxy") {
        proxies.insert("http_proxy".to_owned(), proxy);
    };
    if let Ok(proxy) = env::var("https_proxy") {
        proxies.insert("https_proxy".to_owned(), proxy);
    };

    if !proxies.is_empty() {
        Some(proxies)
    } else {
        None
    }
}

pub fn ftp_download(url: Url, quiet_mode: bool, filename: Option<&str>) -> Fallible<()> {
    let fname = gen_filename(&url, filename);

    let mut client = FtpDownload::new(url.clone());
    let events_handler = DefaultEventsHandler::new(&fname, false, false, quiet_mode);
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
    let headers = request_headers_from_server(&url)?;

    // early exit if headers flag is present
    if args.is_present("headers") {
        print_headers(headers);
        return Ok(());
    }
    let ct_len = headers
        .get::<ContentLength>()
        .map(|ct_len| **ct_len)
        .unwrap_or(0);

    let fname = gen_filename(&url, args.value_of("FILE"));
    let headers = prep_headers(&fname, resume_download, &user_agent);
    let timeout = if let Some(secs) = args.value_of("SECONDS") {
        Some(Duration::new(secs.parse::<u64>()?, 0))
    } else {
        None
    };
    let proxies = get_http_proxies();
    let state_file_exists = Path::new(&format!("{}.st", fname)).exists();

    let chunk_sizes = if state_file_exists && resume_download && concurrent_download && ct_len != 0
    {
        Some(get_resume_chunk_sizes(&fname, ct_len)?)
    } else {
        None
    };

    let bytes_on_disk = if resume_download {
        calc_bytes_on_disk(&fname)
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
        proxies,
        max_retries: 100,
        bytes_on_disk,
        chunk_sizes,
        chunk_sz: 512_000,
    };

    let mut client = HttpDownload::new(url.clone(), conf.clone());
    let quiet_mode = args.is_present("quiet");
    let events_handler =
        DefaultEventsHandler::new(&fname, resume_download, concurrent_download, quiet_mode);
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
    ) -> DefaultEventsHandler {
        let st_file = if concurrent {
            Some(BufWriter::new(
                get_file_handle(&format!("{}.st", fname), resume).unwrap(),
            ))
        } else {
            None
        };
        DefaultEventsHandler {
            prog_bar: None,
            bytes_on_disk: calc_bytes_on_disk(fname),
            fname: fname.to_owned(),
            file: BufWriter::new(get_file_handle(fname, resume).unwrap()),
            st_file,
            server_supports_resume: false,
            quiet_mode,
        }
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
        if byte_count.is_some() {
            prog_bar.inc(byte_count.unwrap());
        }
        self.prog_bar = Some(prog_bar);
    }
}

impl EventsHandler for DefaultEventsHandler {
    fn on_headers(&mut self, headers: Headers) {
        if self.quiet_mode {
            return;
        }
        let ct_type = headers.get::<ContentType>().unwrap();
        println!("Type: {}", style(ct_type).green());

        println!("Saving to: {}", style(&self.fname).green());
        let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);

        self.create_prog_bar(ct_len);
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
        if let Some(ref mut b) = self.prog_bar {
            b.inc(byte_count);
        }
        if let Some(ref mut file) = self.st_file {
            writeln!(file, "{}:{}", byte_count, offset)?;
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
        self.file.flush().unwrap();
        if let Some(ref mut file) = self.st_file {
            file.flush().unwrap()
        }
        ::std::process::exit(0);
    }

    fn on_failure_status(&self, status: StatusCode) {
        if self.quiet_mode {
            return;
        }
        if status.as_u16() == 416 {
            println!(
                "{}",
                &style("\nThe file is already fully retrieved; nothing to do.\n").red()
            );
        }
    }
}
